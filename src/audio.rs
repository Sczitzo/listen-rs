//! Microphone capture and the little sound cues.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

pub fn input_devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Silence detection threshold (peak 100 ms window RMS) for normal dictation.
pub const NORMAL_SILENCE_RMS: f32 = 0.004;
/// Lower silence detection threshold used in Quiet / Whisper Mode.
pub const QUIET_SILENCE_RMS: f32 = 0.0012;

const QUIET_GAIN: f32 = 3.5;
const QUIET_NOISE_FLOOR: f32 = 0.0004;
const QUIET_WHISPER_START: f32 = 0.0010;
const QUIET_BLOCK_SAMPLES: usize = 160; // 10 ms at 16 kHz
const SOFT_LIMIT_KNEE: f32 = 0.60;
const SOFT_LIMIT_CEIL: f32 = 0.95;

/// Silence detection threshold for the active recording mode.
pub fn silence_rms(quiet_mode: bool) -> f32 {
    if quiet_mode {
        QUIET_SILENCE_RMS
    } else {
        NORMAL_SILENCE_RMS
    }
}

/// C1-smooth soft-knee limiter: unity transfer below `0.60`, smooth tanh compression
/// asymptotically bounded within `[-0.95, 0.95]`.
#[inline]
fn soft_limit(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let ax = x.abs();
    if ax <= SOFT_LIMIT_KNEE {
        return x;
    }
    let headroom = SOFT_LIMIT_CEIL - SOFT_LIMIT_KNEE;
    let compressed = SOFT_LIMIT_KNEE + headroom * ((ax - SOFT_LIMIT_KNEE) / headroom).tanh();
    x.signum() * compressed.min(SOFT_LIMIT_CEIL)
}

#[inline]
fn block_quiet_gain(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 1.0;
    }
    let sum_sq: f64 = block
        .iter()
        .map(|&s| {
            let v = if s.is_finite() { s as f64 } else { 0.0 };
            v * v
        })
        .sum();
    let rms = (sum_sq / block.len() as f64).sqrt() as f32;
    let t = ((rms - QUIET_NOISE_FLOOR) / (QUIET_WHISPER_START - QUIET_NOISE_FLOOR)).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    1.0 + (QUIET_GAIN - 1.0) * smooth
}

/// Apply gentle 3.5x whisper boost with noise-floor protection and smooth soft-knee limiting.
/// Output samples are always finite and bounded within `[-0.95, 0.95]`.
pub fn apply_quiet_gain(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    let mut prev_gain: Option<f32> = None;
    for block in samples.chunks_mut(QUIET_BLOCK_SAMPLES) {
        let target_gain = block_quiet_gain(block);
        let start_gain = prev_gain.unwrap_or(target_gain);
        let len = block.len();
        for (i, s) in block.iter_mut().enumerate() {
            let v = if s.is_finite() { *s } else { 0.0 };
            let frac = if len <= 1 { 1.0 } else { (i + 1) as f32 / len as f32 };
            let gain = start_gain + (target_gain - start_gain) * frac;
            *s = soft_limit(v * gain);
        }
        prev_gain = Some(target_gain);
    }
}

enum Cmd {
    Start(String, bool, mpsc::Sender<Result<String, String>>),
    Stop(mpsc::Sender<Vec<f32>>),
    Cancel,
}

struct LiveTapState {
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    quiet_mode: bool,
}

/// A read-only window onto the recording in progress, for the live preview.
/// Holds the capture buffer (at the device rate) while a recording runs.
#[derive(Clone, Default)]
pub struct LiveTap(Arc<Mutex<Option<LiveTapState>>>);

impl LiveTap {
    /// How many 16 kHz samples have been captured so far (0 when not recording).
    pub fn len_16k(&self) -> usize {
        let g = self.0.lock().unwrap();
        match g.as_ref() {
            Some(state) => {
                let n = state.buf.lock().unwrap().len();
                (n as u64 * crate::speech::SAMPLE_RATE as u64 / state.rate.max(1) as u64) as usize
            }
            None => 0,
        }
    }

    /// 16 kHz samples `start..end` of the recording so far, without stopping it.
    /// Only the needed part of the buffer is copied, and the lock is held only for the copy.
    pub fn range_16k(&self, start: usize, end: usize) -> Vec<f32> {
        let (buf, rate, quiet_mode) = match self.0.lock().unwrap().as_ref() {
            Some(state) => (state.buf.clone(), state.rate, state.quiet_mode),
            None => return Vec::new(),
        };
        let to = crate::speech::SAMPLE_RATE as u64;
        let a = (start as u64 * rate as u64 / to) as usize;
        let b = (end as u64 * rate as u64 / to) as usize;
        let part: Vec<f32> = {
            let g = buf.lock().unwrap();
            let b = b.min(g.len());
            if a >= b {
                return Vec::new();
            }
            g[a..b].to_vec()
        };
        let mut out = resample(&part, rate, to as u32);
        if quiet_mode {
            apply_quiet_gain(&mut out);
        }
        out
    }

    fn set(&self, v: Option<LiveTapState>) {
        *self.0.lock().unwrap() = v;
    }
}

/// Owns the microphone on its own thread (audio streams can't move between threads).
pub struct Recorder {
    tx: mpsc::Sender<Cmd>,
    tap: LiveTap,
}

impl Recorder {
    pub fn spawn(level: Arc<AtomicU32>) -> Self {
        let (tx, rx) = mpsc::channel();
        let tap = LiveTap::default();
        let t = tap.clone();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || audio_thread(rx, level, t))
            .expect("audio thread");
        Recorder { tx, tap }
    }

    /// A handle the live-preview worker reads the growing recording through.
    pub fn tap(&self) -> LiveTap {
        self.tap.clone()
    }

    pub fn start_with_options(&self, device: &str, quiet_mode: bool) -> Result<String, String> {
        let (t, r) = mpsc::channel();
        let _ = self.tx.send(Cmd::Start(device.to_string(), quiet_mode, t));
        r.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| Err("the microphone didn't respond".into()))
    }

    /// Stop and return 16 kHz mono samples.
    pub fn stop(&self) -> Vec<f32> {
        let (t, r) = mpsc::channel();
        let _ = self.tx.send(Cmd::Stop(t));
        r.recv_timeout(Duration::from_secs(5)).unwrap_or_default()
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(Cmd::Cancel);
    }
}

struct Capture {
    stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    quiet_mode: bool,
}

fn audio_thread(rx: mpsc::Receiver<Cmd>, level: Arc<AtomicU32>, tap: LiveTap) {
    let mut current: Option<Capture> = None;
    for cmd in rx {
        match cmd {
            Cmd::Start(name, quiet_mode, reply) => {
                current = None;
                tap.set(None);
                match open(&name, level.clone(), quiet_mode) {
                    Ok((stream, buf, rate, label)) => {
                        tap.set(Some(LiveTapState {
                            buf: buf.clone(),
                            rate,
                            quiet_mode,
                        }));
                        current = Some(Capture {
                            stream,
                            buf,
                            rate,
                            quiet_mode,
                        });
                        let _ = reply.send(Ok(label));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Cmd::Stop(reply) => {
                tap.set(None);
                let out = if let Some(cap) = current.take() {
                    drop(cap.stream);
                    let data = std::mem::take(&mut *cap.buf.lock().unwrap());
                    let mut resampled = resample_recording(&data, cap.rate);
                    if cap.quiet_mode {
                        apply_quiet_gain(&mut resampled);
                    }
                    resampled
                } else {
                    Vec::new()
                };
                level.store(0, Ordering::Relaxed);
                let _ = reply.send(out);
            }
            Cmd::Cancel => {
                tap.set(None);
                current = None;
                level.store(0, Ordering::Relaxed);
            }
        }
    }
}

fn open(
    name: &str,
    level: Arc<AtomicU32>,
    quiet_mode: bool,
) -> Result<(cpal::Stream, Arc<Mutex<Vec<f32>>>, u32, String), String> {
    let host = cpal::default_host();
    let device = select_input(name, || host.default_input_device(), |name| {
        host.input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false)))
    })?;
    let label = device.name().unwrap_or_else(|_| "Microphone".into());
    let cfg = device.default_input_config().map_err(|e| format!("microphone: {e}"))?;
    let fmt = cfg.sample_format();
    let config: cpal::StreamConfig = cfg.into();
    let rate = config.sample_rate.0;
    let buf = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 30)));
    let stream = match fmt {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, buf.clone(), level, quiet_mode),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, buf.clone(), level, quiet_mode),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, buf.clone(), level, quiet_mode),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, buf.clone(), level, quiet_mode),
        other => Err(format!("this microphone's format ({other:?}) isn't supported")),
    }?;
    stream.play().map_err(|e| format!("microphone: {e}"))?;
    Ok((stream, buf, rate, label))
}

fn select_input<T>(name: &str, default: impl FnOnce() -> Option<T>, named: impl FnOnce(&str) -> Option<T>) -> Result<T, &'static str> {
    if name.is_empty() {
        default().ok_or("no Windows default microphone was found")
    } else {
        named(name).ok_or("saved microphone not connected; reconnect it or choose another in Settings")
    }
}

#[cfg(test)]
#[test]
fn missing_saved_input_never_uses_the_default() {
    assert!(select_input("Unplugged microphone", || panic!("opened default"), |_| None::<u8>).is_err());
    assert_eq!(select_input("", || Some(1), |_| panic!("looked for a named input")), Ok(1));
    assert_eq!(select_input("Desk microphone", || panic!("opened default"), |name| (name == "Desk microphone").then_some(2)), Ok(2));
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buf: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
    quiet_mode: bool,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let ch = config.channels.max(1) as usize;
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut sum = 0f32;
                let mut n = 0usize;
                let mut b = buf.lock().unwrap();
                for frame in data.chunks(ch) {
                    let s = frame.iter().map(|x| <f32 as cpal::FromSample<T>>::from_sample_(*x)).sum::<f32>() / ch as f32;
                    b.push(s);
                    sum += s * s;
                    n += 1;
                }
                if n > 0 {
                    let rms = (sum / n as f32).sqrt();
                    let display = if quiet_mode && rms >= QUIET_NOISE_FLOOR {
                        let t = ((rms - QUIET_NOISE_FLOOR) / (QUIET_WHISPER_START - QUIET_NOISE_FLOOR)).clamp(0.0, 1.0);
                        let gain = 1.0 + (QUIET_GAIN - 1.0) * (t * t * (3.0 - 2.0 * t));
                        (rms * gain).min(SOFT_LIMIT_CEIL)
                    } else {
                        rms
                    };
                    level.store(display.to_bits(), Ordering::Relaxed);
                }
            },
            |e| eprintln!("audio error: {e}"),
            None,
        )
        .map_err(|e| format!("microphone: {e}"))
}

/// Hard safety cap on undrained meeting audio in RAM (60 minutes at 16 kHz mono ≈ 220 MB).
/// When callers drain chunks incrementally every 25–30 s via `MeetingRecorder::take_chunk_16k`,
/// live audio RAM stays under ~2 MB for meetings of any length.
pub const MEETING_MAX_SECS: usize = 60 * 60;
pub const MEETING_MAX_SAMPLES_16K: usize = MEETING_MAX_SECS * crate::speech::SAMPLE_RATE;

const MEETING_SYNC_LAG_16K: usize = crate::speech::SAMPLE_RATE / 4; // 250 ms alignment buffer
const MEETING_RESYNC_DRIFT_16K: usize = crate::speech::SAMPLE_RATE / 10; // 100 ms resync threshold

/// Status returned when starting a dual-capture meeting recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingStartInfo {
    pub mic_label: String,
    pub loopback_active: bool,
    pub loopback_label: Option<String>,
    pub loopback_warning: Option<String>,
}

/// Mix microphone and system-loopback 16 kHz mono streams with smooth soft-knee limiting
/// so neither side clips even when local and remote voices overlap loudly.
#[allow(dead_code)]
pub fn mix_streams(mic: &[f32], loopback: &[f32]) -> Vec<f32> {
    let len = mic.len().max(loopback.len());
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let m = mic.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0);
        let l = loopback.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0);
        out.push(soft_limit(m + l));
    }
    out
}

struct StreamResampler {
    from_rate: u32,
    to_rate: u32,
    carry: Vec<f32>,
    phase: f64,
}

impl StreamResampler {
    fn new(from_rate: u32, to_rate: u32) -> Self {
        Self {
            from_rate,
            to_rate,
            carry: Vec::with_capacity(1024),
            phase: 0.0,
        }
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if self.from_rate == self.to_rate || self.from_rate == 0 || self.to_rate == 0 {
            out.extend(input.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
            return;
        }
        self.carry.extend_from_slice(input);
        let ratio = self.from_rate as f64 / self.to_rate as f64;
        let mut pos = self.phase;
        if ratio > 1.0 {
            while pos + ratio <= self.carry.len() as f64 {
                let end = pos + ratio;
                let first = pos.floor() as usize;
                let last = (end.ceil() as usize).min(self.carry.len());
                let mut acc = 0.0f64;
                for idx in first..last {
                    let lo = (idx as f64).max(pos);
                    let hi = ((idx + 1) as f64).min(end);
                    let w = (hi - lo).max(0.0);
                    let s = self.carry[idx];
                    let v = if s.is_finite() { s as f64 } else { 0.0 };
                    acc += v * w;
                }
                out.push((acc / ratio) as f32);
                pos = end;
            }
        } else {
            while (pos.floor() as usize) + 1 < self.carry.len() {
                let a = pos.floor() as usize;
                let t = (pos - a as f64) as f32;
                let x0 = if self.carry[a].is_finite() { self.carry[a] } else { 0.0 };
                let x1 = if self.carry[a + 1].is_finite() { self.carry[a + 1] } else { 0.0 };
                out.push(x0 + (x1 - x0) * t);
                pos += ratio;
            }
        }
        let consumed = (pos.floor() as usize).min(self.carry.len());
        if consumed > 0 {
            self.carry.drain(..consumed);
            pos -= consumed as f64;
        }
        self.phase = pos;
    }
}

struct MeetingMixBuffer {
    mic_pending: Vec<f32>,
    loop_pending: Vec<f32>,
    mixed: Vec<f32>,
    total_samples_16k: u64,
    dropped_samples: u64,
    max_buffered_samples: usize,
}

impl MeetingMixBuffer {
    fn new(max_buffered_samples: usize) -> Self {
        Self {
            mic_pending: Vec::with_capacity(MEETING_SYNC_LAG_16K * 2),
            loop_pending: Vec::with_capacity(MEETING_SYNC_LAG_16K * 2),
            mixed: Vec::with_capacity(crate::speech::SAMPLE_RATE * 30),
            total_samples_16k: 0,
            dropped_samples: 0,
            max_buffered_samples,
        }
    }

    fn reset(&mut self) {
        self.mic_pending.clear();
        self.loop_pending.clear();
        self.mixed.clear();
        self.mixed.shrink_to(crate::speech::SAMPLE_RATE * 30);
        self.total_samples_16k = 0;
        self.dropped_samples = 0;
    }

    fn push_mic(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        self.mic_pending.extend_from_slice(samples);
        self.flush_ready();
    }

    fn push_loopback(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let mic_len = self.mic_pending.len();
        let loop_len = self.loop_pending.len();
        // WASAPI loopback stops delivering packets during system audio silence.
        // When playback resumes, pad zeros so remote speech aligns onto the live mic timeline.
        if loop_len + samples.len() + MEETING_RESYNC_DRIFT_16K < mic_len {
            let target_start = mic_len.saturating_sub(samples.len());
            if target_start > loop_len {
                self.loop_pending.resize(target_start, 0.0);
            }
        } else if loop_len > mic_len + MEETING_SYNC_LAG_16K * 2 {
            // Prevent unbounded growth if mic ever pauses while loopback continues.
            let excess = loop_len - (mic_len + MEETING_SYNC_LAG_16K);
            self.loop_pending.drain(..excess);
        }
        self.loop_pending.extend_from_slice(samples);
    }

    fn flush_ready(&mut self) {
        let ready = self.mic_pending.len().saturating_sub(MEETING_SYNC_LAG_16K);
        if ready == 0 {
            return;
        }
        self.mix_prefix(ready);
    }

    fn mix_prefix(&mut self, count: usize) {
        let room = self.max_buffered_samples.saturating_sub(self.mixed.len());
        let to_store = count.min(room);
        for i in 0..to_store {
            let m = self.mic_pending.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0);
            let l = self.loop_pending.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0);
            self.mixed.push(soft_limit(m + l));
        }
        if count > to_store {
            self.dropped_samples += (count - to_store) as u64;
        }
        let mic_drain = count.min(self.mic_pending.len());
        self.mic_pending.drain(..mic_drain);
        let loop_drain = count.min(self.loop_pending.len());
        self.loop_pending.drain(..loop_drain);
        self.total_samples_16k += count as u64;
    }

    fn take_chunk(&mut self) -> Vec<f32> {
        self.flush_ready();
        std::mem::take(&mut self.mixed)
    }

    fn finish(&mut self) -> Vec<f32> {
        let remaining = self.mic_pending.len().max(self.loop_pending.len());
        if remaining > 0 {
            self.mix_prefix(remaining);
        }
        self.mic_pending.clear();
        self.loop_pending.clear();
        std::mem::take(&mut self.mixed)
    }

    fn buffered_len(&self) -> usize {
        self.mixed.len() + self.mic_pending.len().saturating_sub(MEETING_SYNC_LAG_16K)
    }

    fn total_duration_secs(&self) -> u64 {
        let total = self.total_samples_16k + self.mic_pending.len().max(self.loop_pending.len()) as u64;
        total / crate::speech::SAMPLE_RATE as u64
    }
}

enum MeetingCmd {
    Start(String, mpsc::Sender<Result<MeetingStartInfo, String>>),
    Stop(mpsc::Sender<Vec<f32>>),
    Cancel,
}

struct ActiveMeeting {
    _mic_stream: cpal::Stream,
    _loopback_stream: Option<cpal::Stream>,
}

/// Captures microphone + Windows WASAPI system-output loopback simultaneously on a dedicated thread,
/// mixing both into 16 kHz mono with incremental chunk draining so long meetings stay low-memory.
pub struct MeetingRecorder {
    tx: mpsc::Sender<MeetingCmd>,
    mix: Arc<Mutex<MeetingMixBuffer>>,
}

impl MeetingRecorder {
    pub fn spawn(level: Arc<AtomicU32>) -> Self {
        let (tx, rx) = mpsc::channel();
        let mix = Arc::new(Mutex::new(MeetingMixBuffer::new(MEETING_MAX_SAMPLES_16K)));
        let mix_worker = mix.clone();
        std::thread::Builder::new()
            .name("meeting-audio".into())
            .spawn(move || meeting_audio_thread(rx, level, mix_worker))
            .expect("meeting-audio thread");
        Self { tx, mix }
    }

    /// Start capturing both the selected microphone and default system output loopback.
    /// If loopback cannot be opened, falls back cleanly to mic-only with `loopback_active: false`.
    pub fn start_meeting(&self, device: &str) -> Result<MeetingStartInfo, String> {
        let (t, r) = mpsc::channel();
        let _ = self.tx.send(MeetingCmd::Start(device.to_string(), t));
        r.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| Err("the meeting recorder didn't respond".into()))
    }

    /// Drain all ready mixed 16 kHz mono samples accumulated since the last call without stopping.
    pub fn take_chunk_16k(&self) -> Vec<f32> {
        self.mix.lock().unwrap().take_chunk()
    }

    /// Number of mixed 16 kHz samples currently ready to drain via `take_chunk_16k`.
    pub fn buffered_len_16k(&self) -> usize {
        self.mix.lock().unwrap().buffered_len()
    }

    /// Total elapsed meeting audio duration in whole seconds (including already-drained chunks).
    pub fn total_duration_secs(&self) -> u64 {
        self.mix.lock().unwrap().total_duration_secs()
    }

    /// Stop both capture streams and return any remaining undrained 16 kHz mono samples.
    pub fn stop(&self) -> Vec<f32> {
        let (t, r) = mpsc::channel();
        let _ = self.tx.send(MeetingCmd::Stop(t));
        r.recv_timeout(Duration::from_secs(5)).unwrap_or_default()
    }

    /// Stop both capture streams and discard any buffered audio.
    pub fn cancel(&self) {
        let _ = self.tx.send(MeetingCmd::Cancel);
    }
}

fn meeting_audio_thread(
    rx: mpsc::Receiver<MeetingCmd>,
    level: Arc<AtomicU32>,
    mix: Arc<Mutex<MeetingMixBuffer>>,
) {
    let mut current: Option<ActiveMeeting> = None;
    let mic_level = Arc::new(AtomicU32::new(0));
    let loop_level = Arc::new(AtomicU32::new(0));

    for cmd in rx {
        match cmd {
            MeetingCmd::Start(device_name, reply) => {
                current = None;
                mic_level.store(0, Ordering::Relaxed);
                loop_level.store(0, Ordering::Relaxed);
                level.store(0, Ordering::Relaxed);
                mix.lock().unwrap().reset();

                match open_meeting_streams(
                    &device_name,
                    mix.clone(),
                    level.clone(),
                    mic_level.clone(),
                    loop_level.clone(),
                ) {
                    Ok((active, info)) => {
                        current = Some(active);
                        let _ = reply.send(Ok(info));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            MeetingCmd::Stop(reply) => {
                // Drop both streams before draining the mix buffer so no callback runs after flush.
                drop(current.take());
                mic_level.store(0, Ordering::Relaxed);
                loop_level.store(0, Ordering::Relaxed);
                level.store(0, Ordering::Relaxed);
                let tail = mix.lock().unwrap().finish();
                let _ = reply.send(tail);
            }
            MeetingCmd::Cancel => {
                drop(current.take());
                mic_level.store(0, Ordering::Relaxed);
                loop_level.store(0, Ordering::Relaxed);
                level.store(0, Ordering::Relaxed);
                mix.lock().unwrap().reset();
            }
        }
    }
}

fn open_meeting_streams(
    mic_name: &str,
    mix: Arc<Mutex<MeetingMixBuffer>>,
    combined_level: Arc<AtomicU32>,
    mic_level: Arc<AtomicU32>,
    loop_level: Arc<AtomicU32>,
) -> Result<(ActiveMeeting, MeetingStartInfo), String> {
    let host = cpal::default_host();
    let mic_device = select_input(
        mic_name,
        || host.default_input_device(),
        |name| {
            host.input_devices()
                .ok()
                .and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false)))
        },
    )?;
    let mic_label = mic_device.name().unwrap_or_else(|_| "Microphone".into());
    let mic_cfg = mic_device
        .default_input_config()
        .map_err(|e| format!("microphone: {e}"))?;
    let mic_fmt = mic_cfg.sample_format();
    let mic_config: cpal::StreamConfig = mic_cfg.into();
    let mic_stream = build_meeting_stream_any(
        &mic_device,
        &mic_config,
        mic_fmt,
        mix.clone(),
        false,
        mic_level.clone(),
        loop_level.clone(),
        combined_level.clone(),
    )?;
    mic_stream.play().map_err(|e| format!("microphone: {e}"))?;

    let (loopback_stream, loopback_label, loopback_warning) =
        match open_loopback_stream(&host, mix, loop_level, mic_level, combined_level) {
            Ok((stream, label)) => (Some(stream), Some(label), None),
            Err(reason) => (None, None, Some(reason)),
        };

    let info = MeetingStartInfo {
        mic_label,
        loopback_active: loopback_stream.is_some(),
        loopback_label,
        loopback_warning,
    };
    Ok((
        ActiveMeeting {
            _mic_stream: mic_stream,
            _loopback_stream: loopback_stream,
        },
        info,
    ))
}

fn open_loopback_stream(
    host: &cpal::Host,
    mix: Arc<Mutex<MeetingMixBuffer>>,
    loop_level: Arc<AtomicU32>,
    mic_level: Arc<AtomicU32>,
    combined_level: Arc<AtomicU32>,
) -> Result<(cpal::Stream, String), String> {
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default speaker was found for system audio loopback".to_string())?;
    let label = device.name().unwrap_or_else(|_| "System audio".into());
    // On cpal 0.15 WASAPI, render endpoints expose their shared mix format via
    // `default_output_config()`, and `build_input_stream` sets `AUDCLNT_STREAMFLAGS_LOOPBACK`.
    let cfg = device
        .default_output_config()
        .map_err(|e| format!("system audio loopback unavailable: {e}"))?;
    let fmt = cfg.sample_format();
    let config: cpal::StreamConfig = cfg.into();
    let stream = build_meeting_stream_any(
        &device,
        &config,
        fmt,
        mix,
        true,
        loop_level,
        mic_level,
        combined_level,
    )?;
    stream
        .play()
        .map_err(|e| format!("system audio loopback couldn't start: {e}"))?;
    Ok((stream, label))
}

#[allow(clippy::too_many_arguments)]
fn build_meeting_stream_any(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    fmt: cpal::SampleFormat,
    mix: Arc<Mutex<MeetingMixBuffer>>,
    is_loopback: bool,
    own_level: Arc<AtomicU32>,
    peer_level: Arc<AtomicU32>,
    combined_level: Arc<AtomicU32>,
) -> Result<cpal::Stream, String> {
    match fmt {
        cpal::SampleFormat::F32 => build_meeting_stream::<f32>(
            device, config, mix, is_loopback, own_level, peer_level, combined_level,
        ),
        cpal::SampleFormat::I16 => build_meeting_stream::<i16>(
            device, config, mix, is_loopback, own_level, peer_level, combined_level,
        ),
        cpal::SampleFormat::U16 => build_meeting_stream::<u16>(
            device, config, mix, is_loopback, own_level, peer_level, combined_level,
        ),
        cpal::SampleFormat::I32 => build_meeting_stream::<i32>(
            device, config, mix, is_loopback, own_level, peer_level, combined_level,
        ),
        other => Err(format!(
            "{} format ({other:?}) isn't supported",
            if is_loopback { "system audio" } else { "microphone" }
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_meeting_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mix: Arc<Mutex<MeetingMixBuffer>>,
    is_loopback: bool,
    own_level: Arc<AtomicU32>,
    peer_level: Arc<AtomicU32>,
    combined_level: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let ch = config.channels.max(1) as usize;
    let rate = config.sample_rate.0;
    let mut resampler = StreamResampler::new(rate, crate::speech::SAMPLE_RATE as u32);
    let mut mono_scratch = Vec::with_capacity(1024);
    let mut out_16k = Vec::with_capacity(512);
    let source_name = if is_loopback { "system audio loopback" } else { "microphone" };

    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                mono_scratch.clear();
                let mut sum_sq = 0f32;
                for frame in data.chunks(ch) {
                    let s = frame
                        .iter()
                        .map(|x| <f32 as cpal::FromSample<T>>::from_sample_(*x))
                        .sum::<f32>()
                        / ch as f32;
                    let s = if s.is_finite() { s } else { 0.0 };
                    mono_scratch.push(s);
                    sum_sq += s * s;
                }
                if !mono_scratch.is_empty() {
                    let rms = (sum_sq / mono_scratch.len() as f32).sqrt();
                    own_level.store(rms.to_bits(), Ordering::Relaxed);
                    let peer = f32::from_bits(peer_level.load(Ordering::Relaxed));
                    combined_level.store(rms.max(peer).to_bits(), Ordering::Relaxed);

                    out_16k.clear();
                    resampler.push(&mono_scratch, &mut out_16k);
                    if !out_16k.is_empty() {
                        let mut guard = mix.lock().unwrap();
                        if is_loopback {
                            guard.push_loopback(&out_16k);
                        } else {
                            guard.push_mic(&out_16k);
                        }
                    }
                }
            },
            move |e| eprintln!("meeting {source_name} error: {e}"),
            None,
        )
        .map_err(|e| format!("{source_name}: {e}"))
}

pub(crate) fn resample_recording(data: &[f32], rate: u32) -> Vec<f32> {
    resample(data, rate, crate::speech::SAMPLE_RATE as u32)
}

pub fn resample(data: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || data.is_empty() {
        return data.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = (data.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    if ratio > 1.0 {
        // Downsampling: average the source samples that fall into each output slot.
        for i in 0..out_len {
            let a = (i as f64 * ratio) as usize;
            let b = (((i + 1) as f64 * ratio) as usize).min(data.len()).max(a + 1);
            out.push(data[a..b].iter().sum::<f32>() / (b - a) as f32);
        }
    } else {
        for i in 0..out_len {
            let pos = i as f64 * ratio;
            let a = pos as usize;
            let t = (pos - a as f64) as f32;
            let x0 = data[a];
            let x1 = *data.get(a + 1).unwrap_or(&x0);
            out.push(x0 + (x1 - x0) * t);
        }
    }
    out
}

#[derive(Clone, Copy)]
pub enum Cue {
    Start,
    Stop,
    Cancel,
    Error,
    Copied,
    DeliveryMissed,
}

pub fn play_cue(cue: Cue) {
    std::thread::spawn(move || {
        let _ = play(cue);
    });
}

fn play(cue: Cue) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no speaker")?;
    let cfg = device.default_output_config().map_err(|e| e.to_string())?;
    let fmt = cfg.sample_format();
    let config: cpal::StreamConfig = cfg.into();
    let rate = config.sample_rate.0 as f32;
    let ch = config.channels.max(1) as usize;
    let (total, mut next) = cue_samples(cue, rate);
    let stream = match fmt {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(ch) {
                    let v = next();
                    frame.fill(v);
                }
            },
            |_| {},
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(ch) {
                    let v = (next() * 32767.0) as i16;
                    frame.fill(v);
                }
            },
            |_| {},
            None,
        ),
        _ => return Err("unsupported speaker format".into()),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs_f32(total as f32 / rate + 0.12));
    Ok(())
}

pub(crate) fn cue_samples(cue: Cue, rate: f32) -> (usize, impl FnMut() -> f32) {
    let notes: &[f32] = match cue {
        Cue::Start => &[660.0, 880.0],
        Cue::Stop => &[880.0, 660.0],
        Cue::Cancel => &[440.0],
        Cue::Error => &[330.0, 330.0],
        Cue::Copied => &[880.0],
        Cue::DeliveryMissed => &[523.0, 392.0, 523.0],
    };
    let samples_per_note = (rate * 0.09) as usize;
    let total = samples_per_note * notes.len();
    let mut i = 0usize;
    let mut phase = 0f32;
    let next = move || {
        if i >= total {
            return 0.0;
        }
        let p = (i % samples_per_note) as f32 / samples_per_note as f32;
        let f = notes[i / samples_per_note];
        phase += std::f32::consts::TAU * f / rate;
        i += 1;
        0.16 * (std::f32::consts::PI * p).sin() * phase.sin()
    };
    (total, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(xs: &[f32]) -> f32 {
        if xs.is_empty() {
            return 0.0;
        }
        (xs.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / xs.len() as f64).sqrt() as f32
    }

    #[test]
    fn silence_rms_returns_quiet_and_normal_thresholds() {
        assert_eq!(silence_rms(false), 0.004);
        assert_eq!(silence_rms(true), QUIET_SILENCE_RMS);
        assert_eq!(QUIET_SILENCE_RMS, 0.0012);
    }

    #[test]
    fn quiet_gain_boosts_whisper_limits_peaks_and_preserves_silence() {
        // 1. Whisper-level sine wave (~0.015 peak) gets clean ~3.5x amplification.
        let mut whisper: Vec<f32> = (0..1600)
            .map(|i| 0.015 * (std::f32::consts::TAU * 300.0 * i as f32 / 16_000.0).sin())
            .collect();
        let before = rms(&whisper);
        apply_quiet_gain(&mut whisper);
        let after = rms(&whisper);
        let ratio = after / before;
        assert!(
            (ratio - 3.5).abs() < 0.05,
            "expected ~3.5x whisper boost, got {ratio:.3}x ({before:.5} -> {after:.5})"
        );

        // 2. Loud speech & extreme peaks stay smoothly bounded within [-0.95, 0.95].
        let mut loud = vec![0.4, -0.4, 0.85, -0.92, 1.5, -3.0, 10.0, -10.0];
        apply_quiet_gain(&mut loud);
        for &s in &loud {
            assert!(s.is_finite(), "output must be finite");
            assert!(
                (-0.95..=0.95).contains(&s),
                "soft limiter exceeded [-0.95, 0.95]: got {s}"
            );
        }
        assert!(loud[2] > 0.90 && loud[2] <= 0.95);
        assert!(loud[3] < -0.90 && loud[3] >= -0.95);

        // 3. Near-zero silence / noise floor is NOT amplified recklessly.
        let mut silence = vec![0.0001f32; 1600];
        apply_quiet_gain(&mut silence);
        assert!(
            rms(&silence) < 0.0002,
            "noise floor should remain quiet, got {}",
            rms(&silence)
        );
        let mut zeros = vec![0.0f32; 320];
        apply_quiet_gain(&mut zeros);
        assert!(zeros.iter().all(|&s| s == 0.0));

        // 4. Non-finite samples are sanitized to 0.0.
        let mut bad = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.02];
        apply_quiet_gain(&mut bad);
        assert!(bad.iter().all(|s| s.is_finite() && (-0.95..=0.95).contains(s)));
        assert_eq!(bad[0], 0.0);
        assert_eq!(bad[1], 0.0);
        assert_eq!(bad[2], 0.0);
    }

    #[test]
    fn live_tap_applies_quiet_gain_only_when_quiet_mode_enabled() {
        let raw_buf = Arc::new(Mutex::new(vec![0.02f32; 1600]));
        let tap = LiveTap::default();

        tap.set(Some(LiveTapState {
            buf: raw_buf.clone(),
            rate: 16_000,
            quiet_mode: false,
        }));
        let normal = tap.range_16k(0, 1600);
        assert!((rms(&normal) - 0.02).abs() < 0.001);

        tap.set(Some(LiveTapState {
            buf: raw_buf,
            rate: 16_000,
            quiet_mode: true,
        }));
        let boosted = tap.range_16k(0, 1600);
        assert!(
            (rms(&boosted) - 0.07).abs() < 0.002,
            "expected boosted LiveTap RMS ~0.07, got {}",
            rms(&boosted)
        );
    }

    #[test]
    fn stream_resampler_preserves_length_and_dc_across_chunk_boundaries() {
        let mut r44 = StreamResampler::new(44_100, 16_000);
        let mut out = Vec::new();
        for _ in 0..100 {
            let chunk = vec![0.25f32; 512];
            r44.push(&chunk, &mut out);
        }
        let expected = ((51_200f64) / (44_100.0 / 16_000.0)).floor() as usize;
        assert!(
            out.len().abs_diff(expected) <= 1,
            "expected ~{expected} samples, got {}",
            out.len()
        );
        assert!(out.iter().all(|&s| (s - 0.25).abs() < 1e-4));

        let mut r48 = StreamResampler::new(48_000, 16_000);
        let mut out48 = Vec::new();
        r48.push(&vec![0.5f32; 480], &mut out48);
        assert_eq!(out48.len(), 160);
        assert!(out48.iter().all(|&s| (s - 0.5).abs() < 1e-5));
    }

    #[test]
    fn mix_streams_balances_mic_and_loopback_with_soft_limiting() {
        let mic = vec![0.2, 0.8, -0.9, f32::NAN];
        let loopback = vec![0.0, 0.8, -0.9, 0.3];
        let mixed = mix_streams(&mic, &loopback);
        assert_eq!(mixed.len(), 4);
        assert!((mixed[0] - 0.2).abs() < 1e-5);
        assert!(mixed[1] > 0.90 && mixed[1] <= 0.95);
        assert!(mixed[2] < -0.90 && mixed[2] >= -0.95);
        assert!((mixed[3] - 0.3).abs() < 1e-5);
    }

    #[test]
    fn meeting_mix_buffer_drains_chunks_resyncs_loopback_and_enforces_cap() {
        let mut buf = MeetingMixBuffer::new(MEETING_MAX_SAMPLES_16K);

        // Push 1 second of mic audio only (simulating silent speakers).
        buf.push_mic(&vec![0.1f32; 16_000]);
        let chunk1 = buf.take_chunk();
        assert_eq!(chunk1.len(), 16_000 - MEETING_SYNC_LAG_16K);
        assert!(chunk1.iter().all(|&s| (s - 0.1).abs() < 1e-5));
        assert_eq!(buf.buffered_len(), 0);

        // Now push 160 samples of loopback audio after silence: should align near the live edge.
        buf.push_loopback(&vec![0.2f32; 160]);
        // Advance mic by 0.5s so the aligned loopback packet flushes out of the 250ms sync window.
        buf.push_mic(&vec![0.1f32; 8_000]);
        let chunk2 = buf.take_chunk();
        assert_eq!(chunk2.len(), 8_000);
        let mixed_count = chunk2.iter().filter(|&&s| (s - 0.3).abs() < 1e-4).count();
        assert_eq!(mixed_count, 160, "expected 160 aligned mic+loopback samples");

        let tail = buf.finish();
        assert_eq!(tail.len(), MEETING_SYNC_LAG_16K);
        assert_eq!(chunk1.len() + chunk2.len() + tail.len(), 24_000);
        assert_eq!(buf.total_duration_secs(), 1);

        // Verify hard buffer cap prevents unbounded memory growth if never drained.
        let mut capped = MeetingMixBuffer::new(1_000);
        capped.push_mic(&vec![0.1f32; 10_000]);
        let out = capped.finish();
        assert_eq!(out.len(), 1_000);
        assert!(capped.dropped_samples >= 9_000);
    }
}
