//! The dictation pipeline: record → recognise → clean up → deliver, run on a
//! background "controller" thread so the window never freezes.

use crate::audio::{self, Cue, Recorder};
use crate::cleanup::{self, LabAction};
use crate::speech::{self, Speech};
use crate::storage::{self, Destination, HistoryEntry, Settings};
use crate::winput::Target;
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A press shorter than this locks recording on (hands-free); tap again to finish.
pub const LOCK_TAP_S: f32 = 0.3;
/// Hold this long before the start cue plays and the microphone opens.
pub const ANNOUNCE_S: f32 = 0.15;
/// Longest single recording before it finishes on its own.
pub const MAX_REC_S: f32 = 600.0;
/// If no 100 ms window is louder than this, the recording counts as silent (normal mode).
#[cfg(test)]
pub const SILENCE_RMS: f32 = audio::NORMAL_SILENCE_RMS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Recording,
    Working,
}

#[derive(Clone, Debug)]
pub enum EngineState {
    Loading,
    Ready(String),
    Unavailable(String),
}

pub enum Cmd {
    /// `target` records the original window and its focused child at shortcut activation.
    Start { target: Option<Target>, hands_free: bool },
    /// Start Command Mode ("Highlight & Edit Anywhere") targeting the focused app.
    StartCommand { target: Option<Target>, selected_text: Option<String>, hands_free: bool },
    MarkHandsFree,
    Finish,
    Cancel,
    CancelLantern { generation: u64 },
    StartMeeting { include_system_audio: bool, reply: mpsc::SyncSender<Result<audio::MeetingStartInfo, String>> },
    StopMeeting { reply: mpsc::SyncSender<Result<(), String>> },
    CancelMeeting,
    Lab { action: LabAction, text: String, instruction: String },
    Deliver { text: String, target: Option<Target> },
    ReloadEngine,
    RefreshModels,
}

pub enum UiEvent {
    History(HistoryEntry),
    Retained { text: String, reason: String },
    ToWorkspace(String, Option<(u64, u64)>),
    LabDone(Result<String, String>),
    LocalSuggestion { id: u64, result: Result<String, String>, model: String, draft_stamp: u64, settings: Box<Settings> },
    Models(Vec<String>),
}

pub struct Shared {
    pub phase: Phase,
    pub started: Option<Instant>,
    pub engine: EngineState,
    pub status: String,
    pub lab_busy: bool,
    pub last_target: Option<Target>,
    /// What the live preview has heard so far in the current recording.
    pub live_text: String,
    /// A short note next to "Listening" (e.g. "Preview paused"); empty when all is well.
    pub live_note: String,
    /// How the last recording ended, shown on the floating card until the instant passes.
    pub outcome: Option<(String, Instant)>,
    /// Bumped on every new recording so an old preview worker knows to stop.
    pub rec_gen: u64,
    pub hands_free: bool,
    pub recording_error: String,
    pub shortcut_error: String,
    pub paste_waiting: bool,
    pub active_draft: Option<(u64, u64)>,
    captured_draft: Option<(u64, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PieceClean {
    Pending,
    Skipped,
    Done(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommittedPiece {
    raw: String,
    cleaned: PieceClean,
}

struct LiveCache {
    gen: u64,
    vocab: String,
    cleanup: bool,
    style: crate::storage::Style,
    tone: crate::app_tone::TonePreset,
    quiet_mode: bool,
    local_model: String,
    ollama_url: String,
    committed_upto: usize,
    committed: Vec<CommittedPiece>,
    tail_end: usize,
    tail_text: String,
    failed: bool,
    sealed: bool,
    abandoned: bool,
    preclean_active: bool,
}

impl Default for LiveCache {
    fn default() -> Self {
        Self {
            gen: 0,
            vocab: String::new(),
            cleanup: false,
            style: crate::storage::Style::Tighten,
            tone: crate::app_tone::TonePreset::Default,
            quiet_mode: false,
            local_model: String::new(),
            ollama_url: String::new(),
            committed_upto: 0,
            committed: Vec::new(),
            tail_end: 0,
            tail_text: String::new(),
            failed: false,
            sealed: false,
            abandoned: false,
            preclean_active: false,
        }
    }
}

pub struct MeetingState {
    pub active: bool,
    pub transcribing: bool,
    pub started: Option<Instant>,
    pub elapsed_secs: u64,
    pub loopback_active: bool,
    pub loopback_warning: Option<String>,
    pub status: String,
    pub notes: Vec<crate::meeting::MeetingNote>,
    gen: u64,
}

fn sample_meeting_notes() -> Vec<crate::meeting::MeetingNote> {
    vec![crate::meeting::MeetingNote {
        id: 1,
        ts: "2026-11-25 10:00".into(),
        title: "Weekly Product Sync (Sample)".into(),
        duration_secs: 185,
        loopback_used: true,
        transcript: "We reviewed the release checklist for Listen, confirmed local speech latency is under half a second, and agreed to test the new Meetings view across all themes.".into(),
        summary: "Reviewed the upcoming release checklist and verified local speech performance across themes.".into(),
        action_items: vec![
            "Verify dark and light theme contrast in Meetings".into(),
            "Run full offline verification suite before tagging".into(),
        ],
    }]
}

pub struct Core {
    pub shared: Mutex<Shared>,
    pub settings: Mutex<Settings>,
    pub level: Arc<AtomicU32>,
    pub engine: Mutex<Option<Speech>>,
    pub meeting: Mutex<MeetingState>,
    live_cache: (Mutex<LiveCache>, Condvar),
    pub preview: bool,
    pub capture_paused: std::sync::atomic::AtomicBool,
    pub draft_delivery_pending: AtomicBool,
    #[cfg(windows)]
    pub controller_capture: Mutex<Option<crate::controller::Capture>>,
    tx: Sender<Cmd>,
    ui_tx: Sender<UiEvent>,
    ctx: Mutex<Option<egui::Context>>,
}

impl Core {
    pub fn new(settings: Settings, preview: bool) -> (Arc<Core>, Receiver<UiEvent>) {
        let (tx, rx) = mpsc::channel();
        let (ui_tx, ui_rx) = mpsc::channel();
        let initial_notes = if preview {
            sample_meeting_notes()
        } else {
            crate::meeting::load_meetings()
        };
        let core = Arc::new(Core {
            shared: Mutex::new(Shared {
                phase: Phase::Idle,
                started: None,
                engine: EngineState::Loading,
                status: "Starting up…".into(),
                lab_busy: false,
                last_target: None,
                live_text: String::new(),
                live_note: String::new(),
                outcome: None,
                rec_gen: 0,
                hands_free: false,
                recording_error: String::new(),
                shortcut_error: String::new(),
                paste_waiting: false,
                active_draft: None,
                captured_draft: None,
            }),
            settings: Mutex::new(settings),
            level: Arc::new(AtomicU32::new(0)),
            engine: Mutex::new(None),
            meeting: Mutex::new(MeetingState {
                active: false,
                transcribing: false,
                started: None,
                elapsed_secs: 0,
                loopback_active: false,
                loopback_warning: None,
                status: String::new(),
                notes: initial_notes,
                gen: 0,
            }),
            live_cache: (Mutex::new(LiveCache::default()), Condvar::new()),
            preview,
            capture_paused: std::sync::atomic::AtomicBool::new(false),
            draft_delivery_pending: AtomicBool::new(false),
            #[cfg(windows)]
            controller_capture: Mutex::new(None),
            tx,
            ui_tx,
            ctx: Mutex::new(None),
        });
        if preview {
            core.shared.lock().unwrap().engine = EngineState::Ready("Sample · no speech engine".into());
        } else {
            let c = core.clone();
            std::thread::Builder::new().name("controller".into()).spawn(move || controller(c, rx)).expect("controller");
        }
        (core, ui_rx)
    }

    pub fn meeting_active(&self) -> bool {
        let m = self.meeting.lock().unwrap();
        m.active || m.transcribing
    }

    pub fn meeting_snapshot(&self) -> serde_json::Value {
        let m = self.meeting.lock().unwrap();
        let elapsed_secs = if m.active {
            m.started.map_or(m.elapsed_secs, |t| t.elapsed().as_secs())
        } else {
            m.elapsed_secs
        };
        serde_json::json!({
            "active": m.active,
            "transcribing": m.transcribing,
            "elapsed_secs": elapsed_secs,
            "loopback_active": m.loopback_active,
            "loopback_warning": m.loopback_warning,
            "status": m.status,
            "notes": m.notes,
        })
    }

    pub fn start_meeting(&self) -> Result<audio::MeetingStartInfo, String> {
        self.start_meeting_with_loopback(true)
    }

    pub fn start_meeting_with_loopback(&self, include_system_audio: bool) -> Result<audio::MeetingStartInfo, String> {
        if self.preview {
            return Err("Meeting recording is unavailable in sample mode.".into());
        }
        if self.phase() != Phase::Idle {
            return Err("Finish the current dictation before starting a meeting.".into());
        }
        if self.meeting_active() {
            return Err("A meeting recording is already in progress.".into());
        }
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Cmd::StartMeeting { include_system_audio, reply })
            .map_err(|_| "Listen background controller is unavailable.".to_owned())?;
        self.repaint();
        rx.recv_timeout(Duration::from_secs(6))
            .unwrap_or_else(|_| Err("Timed out starting meeting recording.".into()))
    }

    pub fn stop_meeting(&self) -> Result<(), String> {
        if self.preview {
            return Err("No meeting recording is active.".into());
        }
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Cmd::StopMeeting { reply })
            .map_err(|_| "Listen background controller is unavailable.".to_owned())?;
        self.repaint();
        rx.recv_timeout(Duration::from_secs(6))
            .unwrap_or_else(|_| Err("Timed out stopping meeting recording.".into()))
    }

    pub fn cancel_meeting(&self) {
        if self.preview {
            return;
        }
        let _ = self.tx.send(Cmd::CancelMeeting);
        self.repaint();
    }

    pub fn delete_meeting(&self, id: u64) -> bool {
        let mut m = self.meeting.lock().unwrap();
        let before = m.notes.len();
        m.notes.retain(|note| note.id != id);
        if m.notes.len() == before {
            return false;
        }
        if !self.preview && crate::meeting::save_meetings(&m.notes).is_err() {
            return false;
        }
        true
    }

    pub fn set_ctx(&self, ctx: egui::Context) {
        *self.ctx.lock().unwrap() = Some(ctx);
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
        self.repaint();
    }

    pub fn queue_draft_delivery(&self, text: String, target: Target) -> bool {
        if text.trim().is_empty() || self.preview || self.phase() != Phase::Idle
            || self.draft_delivery_pending.swap(true, Ordering::AcqRel) {
            return false;
        }
        if self.tx.send(Cmd::Deliver { text, target: Some(target) }).is_err() {
            self.draft_delivery_pending.store(false, Ordering::Release);
            return false;
        }
        self.repaint();
        true
    }

    pub fn repaint(&self) {
        if let Some(c) = self.ctx.lock().unwrap().as_ref() {
            c.request_repaint();
        }
    }

    pub fn emit(&self, ev: UiEvent) {
        let _ = self.ui_tx.send(ev);
        self.repaint();
    }

    pub fn phase(&self) -> Phase {
        self.shared.lock().unwrap().phase
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn set_status(&self, s: impl Into<String>) {
        self.shared.lock().unwrap().status = s.into();
        self.repaint();
    }

    pub fn set_shortcut_error(&self, message: &str) {
        self.shared.lock().unwrap().shortcut_error = message.to_owned();
        self.repaint();
    }

    fn set_phase(&self, p: Phase) {
        let mut sh = self.shared.lock().unwrap();
        sh.phase = p;
        if p == Phase::Recording {
            sh.started = Some(Instant::now());
        } else if p == Phase::Idle {
            sh.started = None;
        }
        drop(sh);
        self.repaint();
    }

    pub fn cue(&self, c: Cue) {
        if self.settings.lock().unwrap().sounds && !self.preview {
            audio::play_cue(c);
        }
    }

    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Show how the recording ended on the floating card for `secs` seconds.
    fn set_outcome(&self, text: &str, secs: f32) {
        let mut sh = self.shared.lock().unwrap();
        sh.outcome = Some((text.to_string(), Instant::now() + Duration::from_secs_f32(secs)));
        sh.live_note.clear();
        drop(sh);
        self.repaint();
    }
}

const PREVIEW_WIN: usize = 1600; // 100 ms at 16 kHz
const PAUSE_COMMIT_MIN: usize = 4 * speech::SAMPLE_RATE;
const PAUSE_LOOKAHEAD: usize = 5600; // 350 ms lookahead margin from live edge
const PAUSE_MIN_CHUNK: usize = 2 * speech::SAMPLE_RATE;
const TRAILING_SILENCE_WIN: usize = 5; // 500 ms quiet at live edge -> commit tail
const MAX_LIVE_CHUNK: usize = 18 * speech::SAMPLE_RATE;
const MAX_LIVE_SEARCH_MIN: usize = 15 * speech::SAMPLE_RATE;

/// Find a safe quiet boundary in an uncommitted 16 kHz audio slice where `live_preview`
/// can commit a finalized piece without splitting a word.
fn find_commit_cut(chunk: &[f32], silence_rms: f32) -> Option<usize> {
    let n = chunk.len();
    let trailing_samples = TRAILING_SILENCE_WIN * PREVIEW_WIN;
    if n >= speech::SAMPLE_RATE + trailing_samples {
        let tail_start = n - trailing_samples;
        if speech::peak_window_rms(&chunk[tail_start..]) < silence_rms
            && speech::peak_window_rms(&chunk[..tail_start]) >= silence_rms
        {
            return Some(n - 2 * PREVIEW_WIN);
        }
    }
    if n >= PAUSE_COMMIT_MIN + PAUSE_LOOKAHEAD {
        let peak = speech::peak_window_rms(chunk);
        if peak < silence_rms {
            // Pure leading/middle silence: advance past it without spending inference.
            return Some(n - PAUSE_LOOKAHEAD);
        }
        let pause_threshold = silence_rms.max((peak * 0.06).min(0.007));
        // Scan backwards in 25 ms (400-sample) steps for a 160 ms (2560-sample) quiet window.
        // Any >= 200 ms pause at any arbitrary sample offset fits completely inside a 160 ms grid
        // slot, while brief <= 100 ms stop-consonant closures cannot fill it.
        const PAUSE_WIN: usize = 2560; // 160 ms
        const PAUSE_STEP: usize = 400; // 25 ms
        let search_end = n - PAUSE_LOOKAHEAD;
        let mut pos = search_end.saturating_sub(PAUSE_WIN);
        while pos >= PAUSE_MIN_CHUNK {
            if speech::rms(&chunk[pos..pos + PAUSE_WIN]) < pause_threshold {
                return Some(pos + PAUSE_WIN / 2);
            }
            if pos < PAUSE_STEP {
                break;
            }
            pos -= PAUSE_STEP;
        }
    }
    if n >= MAX_LIVE_CHUNK {
        let lo = MAX_LIVE_SEARCH_MIN.min(n);
        let hi = MAX_LIVE_CHUNK.min(n);
        let (mut best, mut best_e) = (lo, f64::MAX);
        let mut at = lo;
        while at < hi {
            let w = &chunk[at..(at + PREVIEW_WIN).min(hi)];
            let e = w.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / w.len().max(1) as f64;
            if e < best_e {
                best_e = e;
                best = at;
            }
            at += PREVIEW_WIN;
        }
        return Some((best + PREVIEW_WIN / 2).min(n));
    }
    None
}

fn join_non_empty(parts: &[String], tail: &str) -> String {
    let mut out = String::new();
    for part in parts {
        let part = part.trim();
        if !part.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(part);
        }
    }
    let tail = tail.trim();
    if !tail.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(tail);
    }
    out
}

fn has_cross_chunk_command(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    ["scratch that", "delete that", "new paragraph", "new line"]
        .iter()
        .any(|phrase| lower.contains(phrase))
}

fn ends_sentence(raw: &str) -> bool {
    let trimmed = raw
        .trim_end()
        .trim_end_matches(['"', '\'', '”', '’', ')', ']']);
    trimmed.ends_with(['.', '!', '?'])
}

fn abandon_live_cache(core: &Core, gen: u64) {
    let (lock, cvar) = &core.live_cache;
    let mut cache = lock.lock().unwrap();
    if cache.gen == gen {
        cache.sealed = true;
        cache.abandoned = true;
        cvar.notify_all();
    }
}

fn preclean_worker(core: Arc<Core>, gen: u64, settings: Settings, tone: crate::app_tone::TonePreset) {
    struct PrecleanGuard {
        core: Arc<Core>,
        gen: u64,
    }
    impl Drop for PrecleanGuard {
        fn drop(&mut self) {
            let (lock, cvar) = &self.core.live_cache;
            if let Ok(mut cache) = lock.lock() {
                if cache.gen == self.gen {
                    cache.preclean_active = false;
                    cvar.notify_all();
                }
            }
        }
    }
    let _guard = PrecleanGuard { core: core.clone(), gen };

    // Verify the local model and warm Ollama while the user speaks so Finish pays 0 ms cold load
    // and 0 ms `/api/show` overhead.
    cleanup::prepare_local_model(&settings.ollama_url, &settings.local_model);
    if !settings.live_preview {
        return;
    }
    let (lock, cvar) = &core.live_cache;
    loop {
        let next_job = {
            let mut cache = lock.lock().unwrap();
            loop {
                if cache.gen != gen || cache.abandoned {
                    return;
                }
                if let Some((idx, piece)) = cache
                    .committed
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.cleaned == PieceClean::Pending)
                {
                    break Some((idx, piece.raw.clone()));
                }
                if cache.sealed {
                    return;
                }
                cache = cvar.wait(cache).unwrap();
            }
        };
        let Some((idx, raw)) = next_job else { return };
        if !core.settings.lock().unwrap().cleanup {
            return;
        }
        // Only pre-clean chunks that already end at a sentence boundary so mid-sentence thinking
        // pauses never cause Ollama to insert bogus periods across the seam.
        let outcome = if !ends_sentence(&raw) || has_cross_chunk_command(&raw) {
            PieceClean::Skipped
        } else {
            match clean_transcript_outcome(&settings, &raw, tone, |text| {
                cleanup::clean_dictation_with_tone(&settings, text, tone)
            }) {
                TranscriptResult::Discarded => PieceClean::Skipped,
                TranscriptResult::Deliver { cleaned, note } => PieceClean::Done(cleaned, note),
            }
        };
        let mut cache = lock.lock().unwrap();
        if cache.gen != gen || cache.abandoned {
            return;
        }
        if let Some(piece) = cache.committed.get_mut(idx) {
            piece.cleaned = outcome;
            cvar.notify_all();
        }
    }
}

/// Live preview: incrementally recognise speech while recording, committing finalized pieces at
/// quiet pauses so `Finish` only has to transcribe the short remaining tail (or 0 ms if trailing
/// audio is silent) and can reuse background pre-cleaned chunks.
fn live_preview(core: Arc<Core>, tap: audio::LiveTap, gen: u64, quiet_mode: bool) {
    const SR: usize = speech::SAMPLE_RATE as usize;
    const MIN_AUDIO: usize = SR;
    const MIN_NEW: usize = SR / 2;
    const BASE_S: f32 = 0.85;
    let vocab = core.settings().vocab;
    let silence_rms = audio::silence_rms(quiet_mode);
    let mut committed_raws: Vec<String> = Vec::new();
    let mut committed_upto = 0usize;
    let mut last_len = 0usize;
    let mut next_at = Instant::now() + Duration::from_secs_f32(BASE_S);
    let alive = |core: &Core| {
        let sh = core.shared.lock().unwrap();
        sh.phase == Phase::Recording && sh.rec_gen == gen
    };
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if !alive(&core) {
            return;
        }
        if !core.settings.lock().unwrap().live_preview {
            continue;
        }
        if Instant::now() < next_at {
            continue;
        }
        let n = tap.len_16k();
        if n < MIN_AUDIO || n < last_len + MIN_NEW {
            continue;
        }
        // Never wait for the engine: if it's busy, skip this run.
        let mut guard = match core.engine.try_lock() {
            Ok(g) => g,
            Err(_) => continue,
        };
        let Some(eng) = guard.as_mut() else { return };
        if !matches!(eng, Speech::Dll(_)) {
            return;
        }
        let t0 = Instant::now();
        let mut failed = false;
        let uncommitted = tap.range_16k(committed_upto, n);
        let actual_end = committed_upto + uncommitted.len();
        let mut local_off = 0usize;
        while alive(&core) {
            let Some(rel_cut) = find_commit_cut(&uncommitted[local_off..], silence_rms) else {
                break;
            };
            let piece = &uncommitted[local_off..local_off + rel_cut];
            let mut piece_text = String::new();
            if speech::peak_window_rms(piece) >= silence_rms {
                match eng.transcribe(piece, &vocab) {
                    Ok(t) => piece_text = t.trim().to_string(),
                    Err(_) => failed = true,
                }
            }
            if failed {
                break;
            }
            local_off += rel_cut;
            committed_upto += rel_cut;
            let (lock, cvar) = &core.live_cache;
            let mut cache = lock.lock().unwrap();
            if cache.gen == gen {
                cache.committed_upto = committed_upto;
                if !piece_text.is_empty() {
                    committed_raws.push(piece_text.clone());
                    cache.committed.push(CommittedPiece {
                        raw: piece_text,
                        cleaned: PieceClean::Pending,
                    });
                }
                cache.tail_end = committed_upto;
                cache.tail_text.clear();
                cvar.notify_all();
            }
        }
        let mut tail_text = String::new();
        let remaining = &uncommitted[local_off..];
        if !failed && remaining.len() > SR / 2 && alive(&core) {
            if speech::peak_window_rms(remaining) >= silence_rms {
                match eng.transcribe(remaining, &vocab) {
                    Ok(t) => tail_text = t.trim().to_string(),
                    Err(_) => failed = true,
                }
            }
            // Always publish the finished tail BEFORE dropping the engine lock, even if Finish
            // switched phase to Working while `eng.transcribe` was running, so Finish reuses it!
            let (lock, _) = &core.live_cache;
            let mut cache = lock.lock().unwrap();
            if cache.gen == gen {
                if failed {
                    cache.failed = true;
                } else {
                    cache.tail_end = actual_end;
                    cache.tail_text = tail_text.clone();
                }
            }
        } else if failed {
            let (lock, _) = &core.live_cache;
            let mut cache = lock.lock().unwrap();
            if cache.gen == gen {
                cache.failed = true;
            }
        }
        drop(guard);
        let took = t0.elapsed().as_secs_f32();
        last_len = actual_end;
        next_at = Instant::now() + Duration::from_secs_f32(BASE_S.max(1.5 * took));
        let text = join_non_empty(&committed_raws, &tail_text);
        if !core.settings.lock().unwrap().live_preview {
            continue;
        }
        let mut sh = core.shared.lock().unwrap();
        if sh.rec_gen != gen || sh.phase != Phase::Recording {
            return;
        }
        sh.live_text = text;
        sh.live_note = if failed { "Preview paused".into() } else { String::new() };
        drop(sh);
        core.repaint();
    }
}

fn load_engine(core: &Arc<Core>) {
    let s = core.settings();
    {
        let mut sh = core.shared.lock().unwrap();
        sh.engine = EngineState::Loading;
        sh.status = "Loading the speech model…".into();
    }
    core.repaint();
    *core.engine.lock().unwrap() = None; // free the old one first
    if core.preview {
        // Preview runs (screenshots) only check that the files are there.
        let state = match speech::probe(&s) {
            Ok(l) => EngineState::Ready(l),
            Err(e) => EngineState::Unavailable(e),
        };
        let mut sh = core.shared.lock().unwrap();
        sh.status = match &state {
            EngineState::Ready(_) => "Ready.".into(),
            EngineState::Unavailable(e) => format!("Speech isn't available: {e}."),
            EngineState::Loading => String::new(),
        };
        sh.engine = state;
        drop(sh);
        core.repaint();
        return;
    }
    let t0 = Instant::now();
    let result = speech::load(&s);
    match result {
        Ok(e) => {
            let label = e.label();
            // Always lock `engine` before `shared` to keep lock ordering consistent.
            *core.engine.lock().unwrap() = Some(e);
            let mut sh = core.shared.lock().unwrap();
            sh.status = format!("Ready. Speech model loaded in {:.1} s.", t0.elapsed().as_secs_f32());
            sh.engine = EngineState::Ready(label);
        }
        Err(e) => {
            let mut sh = core.shared.lock().unwrap();
            sh.status = format!("Speech isn't available: {e}.");
            sh.engine = EngineState::Unavailable(e);
        }
    }
    core.repaint();
}

struct ActiveCommand {
    selected_text: Option<String>,
    deferred_copy: bool,
}

enum SelectionCaptureErr {
    ModifiersHeld,
    Other(String),
}

fn capture_command_selection(restore_clipboard: bool) -> Result<Option<String>, SelectionCaptureErr> {
    if crate::winput::clipboard_recovery_pending() {
        return Err(SelectionCaptureErr::Other(
            "Resolve clipboard recovery in Listen before using Command Mode.".into(),
        ));
    }
    let snap = crate::winput::snapshot_clipboard_for_command(restore_clipboard)
        .map_err(SelectionCaptureErr::Other)?;
    let seq_before = crate::winput::clipboard_sequence();
    #[cfg(windows)]
    let copy_result = crate::command_mode::copy_selection_with_ctrl_c();
    #[cfg(not(windows))]
    let copy_result: Result<Option<String>, String> = Ok(None);
    let seq_after = crate::winput::clipboard_sequence();
    if seq_after != seq_before {
        crate::winput::restore_clipboard_after_command(snap, seq_after);
    }
    match copy_result {
        Ok(text) => Ok(text),
        Err(msg) if msg.contains("Release Shift, Alt, or Win") => {
            Err(SelectionCaptureErr::ModifiersHeld)
        }
        Err(msg) => Err(SelectionCaptureErr::Other(msg)),
    }
}

fn resolve_target_tone(settings: &Settings, target: Option<Target>) -> crate::app_tone::TonePreset {
    if settings.destination == Destination::Workspace || target.is_some_and(Target::is_ours) {
        return crate::app_tone::TonePreset::Default;
    }
    let dest_app = target.and_then(Target::app_name).unwrap_or_default();
    crate::app_tone::resolve_tone(&dest_app, settings.auto_app_tone, &settings.app_tone_rules)
}

fn standalone_snippet_expansion(raw: &str, snippets: &[crate::snippets::Snippet]) -> Option<String> {
    if snippets.is_empty() {
        return None;
    }
    let res = crate::snippets::expand_snippets_detailed(raw, snippets);
    res.standalone.then_some(res.text)
}

fn matched_snippet_triggers<'a>(
    text: &str,
    snippets: &'a [crate::snippets::Snippet],
) -> Vec<&'a str> {
    if text.is_empty() || snippets.is_empty() {
        return Vec::new();
    }
    snippets
        .iter()
        .filter(|sn| {
            !sn.trigger.trim().is_empty()
                && crate::snippets::expand_snippets_detailed(text, std::slice::from_ref(sn)).matched
        })
        .map(|sn| sn.trigger.as_str())
        .collect()
}

fn transcribe_meeting_audio(core: &Core, samples: &[f32], vocab: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for chunk in crate::meeting::chunk_audio_16k(samples, 25) {
        if speech::peak_window_rms(chunk) < audio::NORMAL_SILENCE_RMS {
            continue;
        }
        let mut eng_guard = core.engine.lock().unwrap();
        if let Some(eng) = eng_guard.as_mut() {
            if let Ok(text) = eng.transcribe(chunk, vocab) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_owned());
                }
            }
        }
    }
    parts
}

fn controller(core: Arc<Core>, rx: Receiver<Cmd>) {
    {
        let c = core.clone();
        std::thread::spawn(move || load_engine(&c));
    }
    if !core.preview {
        crate::winput::prewarm_clipboard();
        let s = core.settings();
        let c = core.clone();
        std::thread::spawn(move || {
            if let Ok(m) = cleanup::ollama_models(&s.ollama_url) {
                c.emit(UiEvent::Models(m));
                if s.cleanup {
                    cleanup::ollama_warm(&s.ollama_url, &s.local_model);
                }
            }
        });
    }
    let recorder = Recorder::spawn(core.level.clone());
    let meeting_recorder = Arc::new(Mutex::new(audio::MeetingRecorder::spawn(core.level.clone())));
    let mut meeting_drain_handle: Option<std::thread::JoinHandle<Vec<String>>> = None;
    let mut recording_clipboard = None;
    let mut active_command: Option<ActiveCommand> = None;

    for cmd in rx {
        match cmd {
            Cmd::Start { target, hands_free } => {
                if core.phase() != Phase::Idle || core.capture_paused.load(Ordering::Acquire)
                    || core.draft_delivery_pending.load(Ordering::Acquire) {
                    continue;
                }
                if core.meeting_active() {
                    core.set_status("Stop the active meeting recording before dictating.");
                    core.set_outcome("Meeting recording active", 2.5);
                    core.cue(Cue::Error);
                    continue;
                }
                core.shared.lock().unwrap().recording_error.clear();
                let engine = core.shared.lock().unwrap().engine.clone();
                match engine {
                    EngineState::Ready(_) => {}
                    EngineState::Loading => {
                        core.set_status("The speech model is still loading — try again in a moment.");
                        core.cue(Cue::Error);
                        continue;
                    }
                    EngineState::Unavailable(e) => {
                        core.set_status(format!("Can't record: speech isn't available ({e})."));
                        core.cue(Cue::Error);
                        continue;
                    }
                }
                let s = core.settings();
                let tone = resolve_target_tone(&s, target);
                let clipboard = crate::winput::clipboard_sequence();
                match recorder.start_with_options(&s.microphone, s.quiet_mode) {
                    Ok(label) => {
                        active_command = None;
                        recording_clipboard = clipboard;
                        let gen = {
                            let mut sh = core.shared.lock().unwrap();
                            sh.last_target = target;
                            sh.captured_draft = sh.active_draft;
                            sh.hands_free = hands_free;
                            sh.rec_gen += 1;
                            sh.live_text.clear();
                            sh.live_note.clear();
                            sh.outcome = None;
                            sh.rec_gen
                        };
                        {
                            let (lock, cvar) = &core.live_cache;
                            *lock.lock().unwrap() = LiveCache {
                                gen,
                                vocab: s.vocab.clone(),
                                cleanup: s.cleanup,
                                style: s.style,
                                tone,
                                quiet_mode: s.quiet_mode,
                                local_model: s.local_model.clone(),
                                ollama_url: s.ollama_url.clone(),
                                preclean_active: !core.preview && s.cleanup && s.live_preview,
                                ..LiveCache::default()
                            };
                            cvar.notify_all();
                        }
                        core.set_phase(Phase::Recording);
                        core.set_status(format!("Listening on {label}."));
                        core.cue(Cue::Start);
                        if !core.preview {
                            let (c, tap) = (core.clone(), recorder.tap());
                            let quiet = s.quiet_mode;
                            let _ = std::thread::Builder::new()
                                .name("live-preview".into())
                                .spawn(move || live_preview(c, tap, gen, quiet));
                            if s.cleanup {
                                let c = core.clone();
                                if std::thread::Builder::new()
                                    .name("live-preclean".into())
                                    .spawn(move || preclean_worker(c, gen, s, tone))
                                    .is_err()
                                {
                                    let (lock, cvar) = &core.live_cache;
                                    let mut cache = lock.lock().unwrap();
                                    if cache.gen == gen {
                                        cache.preclean_active = false;
                                        cvar.notify_all();
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let message = format!("Couldn't start recording: {e}.");
                        core.shared.lock().unwrap().recording_error = message.clone();
                        core.set_status(message);
                        core.cue(Cue::Error);
                    }
                }
            }
            Cmd::StartCommand { target, selected_text, hands_free } => {
                if core.phase() != Phase::Idle || core.capture_paused.load(Ordering::Acquire)
                    || core.draft_delivery_pending.load(Ordering::Acquire) {
                    continue;
                }
                if core.meeting_active() {
                    core.set_status("Stop the active meeting recording before using Command Mode.");
                    core.set_outcome("Meeting recording active", 2.5);
                    core.cue(Cue::Error);
                    continue;
                }
                core.shared.lock().unwrap().recording_error.clear();
                let engine = core.shared.lock().unwrap().engine.clone();
                match engine {
                    EngineState::Ready(_) => {}
                    EngineState::Loading => {
                        core.set_status("The speech model is still loading — try again in a moment.");
                        core.cue(Cue::Error);
                        continue;
                    }
                    EngineState::Unavailable(e) => {
                        core.set_status(format!("Can't run Command Mode: speech isn't available ({e})."));
                        core.cue(Cue::Error);
                        continue;
                    }
                }
                let s = core.settings();
                let (captured_selection, deferred_copy) = if let Some(text) = selected_text {
                    if text.trim().is_empty() {
                        core.set_status("Highlight text first to use Command Mode.");
                        core.set_outcome("Highlight text first", 2.5);
                        core.cue(Cue::Error);
                        continue;
                    }
                    (Some(text), false)
                } else if target.is_none() || target.is_some_and(Target::is_ours) {
                    core.set_status("Highlight text first to use Command Mode.");
                    core.set_outcome("Highlight text first", 2.5);
                    core.cue(Cue::Error);
                    continue;
                } else {
                    match capture_command_selection(s.restore_clipboard) {
                        Ok(Some(text)) => (Some(text), false),
                        Ok(None) => {
                            core.set_status("Highlight text first to use Command Mode.");
                            core.set_outcome("Highlight text first", 2.5);
                            core.cue(Cue::Error);
                            continue;
                        }
                        Err(SelectionCaptureErr::ModifiersHeld) if !hands_free => (None, true),
                        Err(SelectionCaptureErr::ModifiersHeld) => {
                            core.set_status("Release Shift, Alt, or Win before using Command Mode.");
                            core.set_outcome("Release modifier keys", 2.5);
                            core.cue(Cue::Error);
                            continue;
                        }
                        Err(SelectionCaptureErr::Other(msg)) => {
                            core.set_status(msg);
                            core.set_outcome("Command Mode unavailable", 2.5);
                            core.cue(Cue::Error);
                            continue;
                        }
                    }
                };

                let clipboard = crate::winput::clipboard_sequence();
                match recorder.start_with_options(&s.microphone, s.quiet_mode) {
                    Ok(label) => {
                        recording_clipboard = clipboard;
                        active_command = Some(ActiveCommand {
                            selected_text: captured_selection,
                            deferred_copy,
                        });
                        let gen = {
                            let mut sh = core.shared.lock().unwrap();
                            sh.last_target = target;
                            sh.captured_draft = None;
                            sh.hands_free = hands_free;
                            sh.rec_gen += 1;
                            sh.live_text.clear();
                            sh.live_note = "Command Mode".into();
                            sh.outcome = None;
                            sh.rec_gen
                        };
                        {
                            let (lock, cvar) = &core.live_cache;
                            *lock.lock().unwrap() = LiveCache {
                                gen,
                                vocab: s.vocab.clone(),
                                quiet_mode: s.quiet_mode,
                                ..LiveCache::default()
                            };
                            cvar.notify_all();
                        }
                        core.set_phase(Phase::Recording);
                        core.set_status(format!("Command Mode — listening on {label}."));
                        core.cue(Cue::Start);
                        if !core.preview {
                            let (c, tap) = (core.clone(), recorder.tap());
                            let quiet = s.quiet_mode;
                            let _ = std::thread::Builder::new()
                                .name("live-preview".into())
                                .spawn(move || live_preview(c, tap, gen, quiet));
                            let (url, model) = (s.ollama_url.clone(), s.local_model.clone());
                            let _ = std::thread::Builder::new()
                                .name("command-warm".into())
                                .spawn(move || cleanup::prepare_local_model(&url, &model));
                        }
                    }
                    Err(e) => {
                        let message = format!("Couldn't start Command Mode recording: {e}.");
                        core.shared.lock().unwrap().recording_error = message.clone();
                        core.set_status(message);
                        core.cue(Cue::Error);
                    }
                }
            }
            Cmd::MarkHandsFree => {
                let mut state = core.shared.lock().unwrap();
                if state.phase == Phase::Recording {
                    state.hands_free = true;
                    core.repaint();
                }
            }
            Cmd::Finish => {
                if core.phase() != Phase::Recording {
                    continue;
                }
                let (held, target) = {
                    let mut state = core.shared.lock().unwrap();
                    let held = state.started.map(|started| started.elapsed()).unwrap_or_default();
                    let target = state.last_target;
                    // Transition to Working BEFORE stopping the mic so live_preview won't start
                    // another transcription pass while audio is flushing.
                    state.phase = Phase::Working;
                    (held, target)
                };
                core.repaint();
                let samples = recorder.stop();
                core.cue(Cue::Stop);
                if let Some(cmd_ctx) = active_command.take() {
                    process_command(&core, samples, held, target, recording_clipboard.take(), cmd_ctx);
                } else {
                    process(&core, samples, held, target, recording_clipboard.take());
                }
                core.shared.lock().unwrap().live_text.clear();
                core.set_phase(Phase::Idle);
            }
            action @ (Cmd::Cancel | Cmd::CancelLantern { .. }) => {
                let may_cancel = {
                    let state = core.shared.lock().unwrap();
                    state.phase == Phase::Recording && match action {
                        Cmd::CancelLantern { generation } => generation == state.rec_gen,
                        _ => true,
                    }
                };
                if may_cancel {
                    active_command = None;
                    recorder.cancel();
                    {
                        let (lock, cvar) = &core.live_cache;
                        let mut cache = lock.lock().unwrap();
                        cache.sealed = true;
                        cache.abandoned = true;
                        cache.preclean_active = false;
                        cache.gen = cache.gen.wrapping_add(1);
                        cvar.notify_all();
                    }
                    core.shared.lock().unwrap().live_text.clear();
                    core.set_phase(Phase::Idle);
                    core.set_status("Recording discarded.");
                    core.set_outcome("Recording discarded", 1.5);
                    core.cue(Cue::Cancel);
                }
            }
            Cmd::StartMeeting { include_system_audio, reply } => {
                if core.phase() != Phase::Idle {
                    let _ = reply.send(Err("Finish the current dictation before starting a meeting.".into()));
                    continue;
                }
                if core.meeting_active() {
                    let _ = reply.send(Err("A meeting recording is already in progress.".into()));
                    continue;
                }
                let engine = core.shared.lock().unwrap().engine.clone();
                match engine {
                    EngineState::Ready(_) => {}
                    EngineState::Loading => {
                        let _ = reply.send(Err("The speech model is still loading — try again in a moment.".into()));
                        continue;
                    }
                    EngineState::Unavailable(e) => {
                        let _ = reply.send(Err(format!("Speech isn't available: {e}.")));
                        continue;
                    }
                }
                let s = core.settings();
                let start_res = meeting_recorder.lock().unwrap().start_meeting(&s.microphone, include_system_audio);
                match start_res {
                    Ok(info) => {
                        let my_gen = {
                            let mut m = core.meeting.lock().unwrap();
                            m.gen = m.gen.wrapping_add(1);
                            m.active = true;
                            m.transcribing = false;
                            m.started = Some(Instant::now());
                            m.elapsed_secs = 0;
                            m.loopback_active = info.loopback_active;
                            m.loopback_warning = info.loopback_warning.clone();
                            m.status = match &info.loopback_label {
                                Some(lb) => format!("Recording meeting ({} + {lb})…", info.mic_label),
                                None => format!("Recording meeting ({} · mic only)…", info.mic_label),
                            };
                            m.gen
                        };
                        let drain_core = core.clone();
                        let drain_rec = meeting_recorder.clone();
                        let vocab = s.vocab.clone();
                        meeting_drain_handle = std::thread::Builder::new()
                            .name("meeting-drain".into())
                            .spawn(move || {
                                let mut chunks_text: Vec<String> = Vec::new();
                                loop {
                                    std::thread::sleep(Duration::from_millis(500));
                                    let keep_going = {
                                        let m = drain_core.meeting.lock().unwrap();
                                        m.active && m.gen == my_gen
                                    };
                                    if !keep_going {
                                        break;
                                    }
                                    let ready = drain_rec.lock().unwrap().buffered_len_16k();
                                    if ready >= 25 * speech::SAMPLE_RATE {
                                        let pcm = drain_rec.lock().unwrap().take_chunk_16k();
                                        if !pcm.is_empty() {
                                            let parts = transcribe_meeting_audio(&drain_core, &pcm, &vocab);
                                            chunks_text.extend(parts);
                                        }
                                    }
                                }
                                chunks_text
                            })
                            .ok();
                        core.cue(Cue::Start);
                        core.repaint();
                        let _ = reply.send(Ok(info));
                    }
                    Err(e) => {
                        core.cue(Cue::Error);
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Cmd::StopMeeting { reply } => {
                let stop_info = {
                    let mut m = core.meeting.lock().unwrap();
                    if !m.active {
                        None
                    } else {
                        let elapsed_wall = m.started.map_or(0, |t| t.elapsed().as_secs());
                        m.active = false;
                        m.transcribing = true;
                        m.elapsed_secs = elapsed_wall;
                        m.status = "Transcribing remaining audio…".into();
                        Some((m.gen, m.loopback_active, elapsed_wall))
                    }
                };
                let Some((my_gen, loopback_used, elapsed_wall)) = stop_info else {
                    let _ = reply.send(Err("No meeting recording is active.".into()));
                    continue;
                };
                let (audio_secs, tail) = {
                    let rec = meeting_recorder.lock().unwrap();
                    let secs = rec.total_duration_secs();
                    let tail = rec.stop();
                    (secs, tail)
                };
                let duration_secs = audio_secs.max(elapsed_wall);
                let drain_handle = meeting_drain_handle.take();
                core.cue(Cue::Stop);
                core.repaint();
                let _ = reply.send(Ok(()));

                let fin_core = core.clone();
                let _ = std::thread::Builder::new()
                    .name("meeting-finalize".into())
                    .spawn(move || {
                        let s = fin_core.settings();
                        let mut parts = drain_handle
                            .and_then(|h| h.join().ok())
                            .unwrap_or_default();
                        if !tail.is_empty() {
                            parts.extend(transcribe_meeting_audio(&fin_core, &tail, &s.vocab));
                        }
                        if fin_core.meeting.lock().unwrap().gen != my_gen {
                            return;
                        }
                        let transcript = join_non_empty(&parts, "");
                        if transcript.is_empty() {
                            let mut m = fin_core.meeting.lock().unwrap();
                            if m.gen == my_gen {
                                m.transcribing = false;
                                m.started = None;
                                m.status = "No speech was detected in the meeting.".into();
                            }
                            fin_core.repaint();
                            return;
                        }
                        {
                            let mut m = fin_core.meeting.lock().unwrap();
                            if m.gen != my_gen {
                                return;
                            }
                            m.status = "Summarizing meeting…".into();
                        }
                        fin_core.repaint();
                        let (ts, unix) = storage::now_stamp();
                        let (title, summary, action_items, done_status) =
                            match cleanup::summarize_meeting(&s, &transcript) {
                                Ok((t, sum, items)) => {
                                    let title = if t.trim().is_empty() {
                                        format!("Meeting — {ts}")
                                    } else {
                                        t
                                    };
                                    (title, sum, items, "Meeting note saved.".to_owned())
                                }
                                Err(_) => (
                                    format!("Meeting — {ts}"),
                                    String::new(),
                                    Vec::new(),
                                    "Meeting saved (raw transcript — Ollama unavailable).".to_owned(),
                                ),
                            };
                        let mut m = fin_core.meeting.lock().unwrap();
                        if m.gen != my_gen {
                            return;
                        }
                        let max_existing = m.notes.iter().map(|n| n.id).max().unwrap_or(0);
                        let id = unix.max(max_existing.saturating_add(1));
                        m.notes.insert(
                            0,
                            crate::meeting::MeetingNote {
                                id,
                                ts,
                                title,
                                duration_secs,
                                loopback_used,
                                transcript,
                                summary,
                                action_items,
                            },
                        );
                        m.notes.truncate(crate::meeting::MAX_MEETINGS);
                        if !fin_core.preview {
                            let _ = crate::meeting::save_meetings(&m.notes);
                        }
                        m.transcribing = false;
                        m.started = None;
                        m.status = done_status;
                        drop(m);
                        fin_core.repaint();
                    });
            }
            Cmd::CancelMeeting => {
                {
                    let mut m = core.meeting.lock().unwrap();
                    m.gen = m.gen.wrapping_add(1);
                    m.active = false;
                    m.transcribing = false;
                    m.started = None;
                    m.elapsed_secs = 0;
                    m.loopback_active = false;
                    m.loopback_warning = None;
                    m.status = "Meeting recording discarded.".into();
                }
                meeting_recorder.lock().unwrap().cancel();
                let _ = meeting_drain_handle.take();
                core.cue(Cue::Cancel);
                core.repaint();
            }
            Cmd::Lab { action, text, instruction } => {
                let c = core.clone();
                c.shared.lock().unwrap().lab_busy = true;
                c.repaint();
                std::thread::spawn(move || {
                    let s = c.settings();
                    let r = cleanup::run_lab(&s, action, &text, &instruction);
                    c.shared.lock().unwrap().lab_busy = false;
                    c.emit(UiEvent::LabDone(r));
                });
            }
            Cmd::Deliver { text, target } => {
                if text.is_empty() || core.phase() != Phase::Idle {
                    core.draft_delivery_pending.store(false, Ordering::Release);
                    continue;
                }
                core.set_phase(Phase::Working);
                // Allow Windows to finish minimizing Listen and returning focus to the previous app.
                std::thread::sleep(Duration::from_millis(160));
                if let Some(expected) = target {
                    let wait_until = Instant::now() + Duration::from_millis(340);
                    while Instant::now() < wait_until && crate::winput::capture_target() != Some(expected) {
                        std::thread::sleep(Duration::from_millis(15));
                    }
                }
                let settings = core.settings();
                let allow_ctrl_v = settings.registered_hotkey.as_deref() != Some("ctrl+v");
                match crate::winput::deliver_draft(&text, target, allow_ctrl_v, settings.restore_clipboard) {
                    Ok(crate::winput::Delivery::Confirmed) => core.set_status("Draft paste confirmed."),
                    Ok(_) => core.set_status("Draft sent to the original app."),
                    Err(
                        crate::winput::DeliveryError::Uncertain(reason)
                        | crate::winput::DeliveryError::NotSent(reason)
                        | crate::winput::DeliveryError::NotSentKeepClipboard(reason),
                    ) => {
                        core.emit(UiEvent::Retained { text, reason });
                        core.set_status("Open Listen to recover the saved draft.");
                        core.cue(Cue::DeliveryMissed);
                    }
                }
                core.set_phase(Phase::Idle);
                core.draft_delivery_pending.store(false, Ordering::Release);
            }
            Cmd::ReloadEngine => {
                let c = core.clone();
                std::thread::spawn(move || load_engine(&c));
            }
            Cmd::RefreshModels => {
                let c = core.clone();
                std::thread::spawn(move || {
                    let url = c.settings().ollama_url;
                    match cleanup::ollama_models(&url) {
                        Ok(m) => {
                            c.set_status(format!("Found {} local model(s).", m.len()));
                            c.emit(UiEvent::Models(m));
                        }
                        Err(e) => c.set_status(format!("Couldn't list local models: {e}.")),
                    }
                });
            }
        }
    }
}

/// Conservative fast-skip for already-clean short dictations under `Style::Clean`.
/// Never skips when any spoken command, list/structure trigger, filler word, repeated word,
/// capitalization/punctuation issue, or custom vocabulary setting is present.
fn is_already_clean_short(settings: &Settings, raw: &str) -> bool {
    if settings.style != crate::storage::Style::Clean || !settings.vocab.trim().is_empty() {
        return false;
    }
    let text = raw.trim();
    if text.is_empty() || text.len() > 160 || text.contains(['\r', '\n']) {
        return false;
    }
    let Some(first) = text.chars().next() else {
        return false;
    };
    if !first.is_uppercase() || !ends_sentence(text) {
        return false;
    }
    if text.contains("  ") || text.contains(" ,") || text.contains(" .") || text.contains(" ?") || text.contains(" !") {
        return false;
    }
    if has_cross_chunk_command(text) || crate::formatting::spoken_list(text, &settings.vocab).is_some() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    for phrase in [
        "you know",
        "i mean",
        "sort of",
        "kind of",
        "question mark",
        "exclamation mark",
        "exclamation point",
    ] {
        if lower.contains(phrase) {
            return false;
        }
    }
    // Check that every internal sentence boundary is followed by uppercase.
    let mut after_stop = false;
    for ch in text.chars() {
        if matches!(ch, '.' | '!' | '?') {
            after_stop = true;
        } else if ch.is_alphabetic() {
            if after_stop && ch.is_lowercase() {
                return false;
            }
            after_stop = false;
        } else if !ch.is_whitespace() && !matches!(ch, '"' | '\'' | '”' | '’' | ')' | ']') {
            after_stop = false;
        }
    }

    let mut count = 0usize;
    let mut prev_lower = String::new();
    for raw_tok in text.split_whitespace() {
        let tok = raw_tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’' && c != '-');
        if tok.is_empty() {
            continue;
        }
        count += 1;
        if count > 25 {
            return false;
        }
        // Catch lowercase standalone "i" or "i'm"/"i'll"/"i've"/"i'd".
        if tok == "i" || tok.starts_with("i'") || tok.starts_with("i’") {
            return false;
        }
        let tok_lower = tok.to_ascii_lowercase().replace('’', "'");
        if tok_lower == prev_lower {
            return false;
        }
        if matches!(
            tok_lower.as_str(),
            // Spoken fillers
            "um" | "umm" | "uh" | "uhh" | "uh-huh" | "er" | "erm" | "hmm"
                | "like" | "basically" | "actually" | "literally"
                // Spoken edit / formatting / list / table / punctuation triggers
                | "scratch" | "list" | "bullet" | "bullets" | "bulleted"
                | "numbered" | "lettered" | "table" | "heading" | "headings"
                | "section" | "paragraph" | "colon" | "semicolon" | "comma"
                | "period" | "dash" | "hyphen" | "slash" | "backslash"
                | "quote" | "quotes" | "first" | "second" | "third" | "fourth"
                | "fifth" | "one" | "two" | "three" | "step" | "steps"
                | "item" | "items" | "option" | "options"
        ) {
            return false;
        }
        prev_lower = tok_lower;
    }
    count > 0
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TranscriptResult {
    Discarded,
    Deliver { cleaned: String, note: String },
}

#[cfg(test)]
fn clean_transcript(
    settings: &Settings,
    raw: &str,
    clean: impl FnOnce(&str) -> Result<String, String>,
) -> (String, String) {
    clean_transcript_with_tone(settings, raw, crate::app_tone::TonePreset::Default, clean)
}

#[cfg(test)]
fn clean_transcript_with_tone(
    settings: &Settings,
    raw: &str,
    tone: crate::app_tone::TonePreset,
    clean: impl FnOnce(&str) -> Result<String, String>,
) -> (String, String) {
    match clean_transcript_outcome(settings, raw, tone, clean) {
        TranscriptResult::Discarded => (String::new(), String::new()),
        TranscriptResult::Deliver { cleaned, note } => (cleaned, note),
    }
}

fn clean_transcript_outcome(
    settings: &Settings,
    raw: &str,
    tone: crate::app_tone::TonePreset,
    clean: impl FnOnce(&str) -> Result<String, String>,
) -> TranscriptResult {
    let prepared = crate::formatting::prepare_dictation(raw, &settings.vocab);
    if prepared.discarded {
        return TranscriptResult::Discarded;
    }
    if !settings.cleanup {
        let cleaned = if prepared.locally_edited {
            prepared.corrected
        } else {
            String::new()
        };
        return TranscriptResult::Deliver {
            cleaned,
            note: String::new(),
        };
    }
    if tone == crate::app_tone::TonePreset::Default
        && is_already_clean_short(settings, &prepared.corrected)
    {
        let cleaned = prepared.local_fallback().unwrap_or_default().to_owned();
        return TranscriptResult::Deliver {
            cleaned,
            note: String::new(),
        };
    }
    let input = prepared.model_input();
    let preserved_triggers = matched_snippet_triggers(input, &settings.snippets);
    let note = match clean(input) {
        Ok(text)
            if prepared.is_safe_output_for_style(&text, settings.style, &preserved_triggers) =>
        {
            return TranscriptResult::Deliver {
                cleaned: text,
                note: String::new(),
            };
        }
        Ok(_) => " Formatting changed too much, so your exact words were used.".into(),
        Err(error) => format!(" Formatting skipped ({error})."),
    };
    match prepared.local_fallback() {
        Some(text) => {
            let fallback_note = if prepared.local_list.is_some() {
                " List formatted on this computer.".into()
            } else {
                note
            };
            TranscriptResult::Deliver {
                cleaned: text.to_owned(),
                note: fallback_note,
            }
        }
        None => TranscriptResult::Deliver {
            cleaned: String::new(),
            note,
        },
    }
}

fn resolve_dictation_outcome(
    settings: &Settings,
    raw: &str,
    tone: crate::app_tone::TonePreset,
    precleaned: Option<(String, String)>,
    clean: impl FnOnce(&str) -> Result<String, String>,
) -> TranscriptResult {
    if let Some(expanded) = standalone_snippet_expansion(raw, &settings.snippets) {
        return TranscriptResult::Deliver {
            cleaned: expanded,
            note: String::new(),
        };
    }
    let (mut cleaned, note) = match precleaned {
        Some(result) => result,
        None => match clean_transcript_outcome(settings, raw, tone, clean) {
            TranscriptResult::Discarded => return TranscriptResult::Discarded,
            TranscriptResult::Deliver { cleaned, note } => (cleaned, note),
        },
    };
    if !settings.snippets.is_empty() {
        let base = if cleaned.is_empty() {
            raw
        } else {
            cleaned.as_str()
        };
        let (expanded, did_expand) = crate::snippets::expand_snippets(base, &settings.snippets);
        if did_expand {
            cleaned = expanded;
        }
    }
    TranscriptResult::Deliver { cleaned, note }
}

#[allow(clippy::too_many_arguments)]
fn apply_dictation_outcome(
    core: &Core,
    raw: String,
    outcome: TranscriptResult,
    secs: f32,
    speech_secs: f32,
    total_secs: f32,
    target: Option<Target>,
    deliver: impl FnOnce(
        &str,
        Option<Target>,
    ) -> Result<crate::winput::Delivery, crate::winput::DeliveryError>,
    copy_fallback: impl FnOnce(&str) -> bool,
) -> Option<String> {
    let TranscriptResult::Deliver { cleaned, note } = outcome else {
        core.set_status("Dictation discarded.");
        core.set_outcome("Discarded", 2.0);
        core.cue(Cue::Cancel);
        return None;
    };
    let delivered = save_and_deliver_using(
        core, raw, cleaned, secs, total_secs, target, deliver, copy_fallback,
    )?;
    core.set_status(format!(
        "Done — {delivered}. {secs:.1} s of speech recognised in {speech_secs:.1} s, total {total_secs:.1} s.{note}"
    ));
    Some(delivered)
}

fn recording_failed(core: &Core, message: String) {
    core.shared.lock().unwrap().recording_error = message.clone();
    core.set_status(message);
    core.set_outcome("Processing failed", 3.0);
    core.cue(Cue::Error);
}

pub(crate) fn sample_recording_outcome(samples: Vec<f32>, held: Duration) -> String {
    let (core, _) = Core::new(Settings::default(), true);
    process(&core, samples, held, None, None);
    let mut state = core.shared.lock().unwrap();
    state.outcome.take().map_or_else(String::new, |(message, _)| message)
}

struct IncrementalTranscript {
    raw: String,
    tail_raw: String,
    committed_count: usize,
    preclean_eligible: bool,
}

fn resolve_incremental_transcript(
    samples: &[f32],
    cache: &LiveCache,
    gen: u64,
    vocab: &str,
    silence_rms: f32,
    can_reuse_engine: bool,
    mut transcribe: impl FnMut(&[f32]) -> Result<String, String>,
) -> Result<IncrementalTranscript, String> {
    let can_reuse = can_reuse_engine
        && cache.gen == gen
        && !cache.failed
        && cache.vocab == vocab
        && cache.committed_upto <= samples.len();

    let clean_tail_boundary = cache.tail_end == cache.committed_upto
        || cache.tail_end == samples.len()
        || (cache.tail_end >= PREVIEW_WIN
            && speech::rms(&samples[cache.tail_end - PREVIEW_WIN..cache.tail_end]) < silence_rms);

    if can_reuse
        && cache.tail_end >= cache.committed_upto
        && cache.tail_end <= samples.len()
        && clean_tail_boundary
        && (cache.committed_upto > 0 || !cache.tail_text.is_empty())
        && speech::peak_window_rms(&samples[cache.tail_end..]) < silence_rms
    {
        let committed_raws: Vec<String> = cache.committed.iter().map(|p| p.raw.clone()).collect();
        let tail_raw = cache.tail_text.trim().to_string();
        let raw = join_non_empty(&committed_raws, &tail_raw);
        return Ok(IncrementalTranscript {
            raw,
            tail_raw,
            committed_count: committed_raws.len(),
            preclean_eligible: true,
        });
    }

    if can_reuse && cache.committed_upto > 0 {
        let tail_slice = &samples[cache.committed_upto..];
        let tail_raw = if speech::peak_window_rms(tail_slice) >= silence_rms {
            transcribe(tail_slice)?.trim().to_string()
        } else {
            String::new()
        };
        let committed_raws: Vec<String> = cache.committed.iter().map(|p| p.raw.clone()).collect();
        let raw = join_non_empty(&committed_raws, &tail_raw);
        return Ok(IncrementalTranscript {
            raw,
            tail_raw,
            committed_count: committed_raws.len(),
            preclean_eligible: true,
        });
    }

    let raw = transcribe(samples)?.trim().to_string();
    Ok(IncrementalTranscript {
        raw: raw.clone(),
        tail_raw: raw,
        committed_count: 0,
        preclean_eligible: false,
    })
}

#[cfg(test)]
fn assemble_precleaned(
    settings: &Settings,
    raw: &str,
    committed: &[CommittedPiece],
    tail_raw: &str,
    clean_tail: impl FnOnce(&str) -> Result<String, String>,
) -> Option<(String, String)> {
    assemble_precleaned_with_tone(
        settings,
        raw,
        committed,
        tail_raw,
        crate::app_tone::TonePreset::Default,
        clean_tail,
    )
}

fn assemble_precleaned_with_tone(
    settings: &Settings,
    raw: &str,
    committed: &[CommittedPiece],
    tail_raw: &str,
    tone: crate::app_tone::TonePreset,
    clean_tail: impl FnOnce(&str) -> Result<String, String>,
) -> Option<(String, String)> {
    let prepared = crate::formatting::prepare_dictation(raw, &settings.vocab);
    if !settings.cleanup
        || committed.is_empty()
        || prepared.discarded
        || prepared.self_corrected
        || prepared.local_list.is_some()
        || has_cross_chunk_command(raw)
        || committed.iter().any(|p| !matches!(p.cleaned, PieceClean::Done(_, _)))
    {
        return None;
    }
    let mut parts: Vec<String> = Vec::with_capacity(committed.len());
    let mut note = String::new();
    for piece in committed {
        let PieceClean::Done(cleaned, piece_note) = &piece.cleaned else {
            return None;
        };
        if cleaned.contains('\n') {
            return None;
        }
        if cleaned.is_empty() {
            parts.push(piece.raw.clone());
            if note.is_empty() && !piece_note.is_empty() {
                note = piece_note.clone();
            }
        } else {
            parts.push(cleaned.clone());
        }
    }
    let tail_part = if tail_raw.is_empty() {
        String::new()
    } else {
        let (tail_cleaned, tail_note) =
            match clean_transcript_outcome(settings, tail_raw, tone, clean_tail) {
                TranscriptResult::Discarded => return None,
                TranscriptResult::Deliver { cleaned, note } => (cleaned, note),
            };
        if tail_cleaned.contains('\n') {
            return None;
        }
        if tail_cleaned.is_empty() {
            if note.is_empty() && !tail_note.is_empty() {
                note = tail_note;
            }
            tail_raw.to_owned()
        } else {
            tail_cleaned
        }
    };
    let combined = join_non_empty(&parts, &tail_part);
    if combined == raw {
        return Some((String::new(), note));
    }
    let preserved_triggers = matched_snippet_triggers(prepared.model_input(), &settings.snippets);
    if prepared.is_safe_output_for_style(&combined, settings.style, &preserved_triggers) {
        Some((combined, note))
    } else {
        None
    }
}

fn process_command(
    core: &Arc<Core>,
    samples: Vec<f32>,
    held: Duration,
    target: Option<Target>,
    clipboard: Option<u32>,
    cmd_ctx: ActiveCommand,
) {
    let gen = core.shared.lock().unwrap().rec_gen;
    abandon_live_cache(core, gen);

    if samples.is_empty() && held > Duration::from_millis(500) {
        recording_failed(core, "No audio arrived from the microphone.".into());
        return;
    }
    let s = core.settings();
    let silence_rms = audio::silence_rms(s.quiet_mode);
    let secs = samples.len() as f32 / speech::SAMPLE_RATE as f32;
    if secs < 0.25 || speech::peak_window_rms(&samples) < silence_rms {
        core.set_status("Nothing heard — Command Mode cancelled.");
        core.set_outcome("No speech heard", 2.5);
        return;
    }

    let selected = if cmd_ctx.deferred_copy {
        // User held a Shift/Alt Command Mode key while speaking; now that it is released,
        // wait briefly for keys to settle and copy the highlighted text.
        std::thread::sleep(Duration::from_millis(40));
        match capture_command_selection(s.restore_clipboard) {
            Ok(Some(text)) => text,
            Ok(None) => {
                core.set_status("Highlight text first to use Command Mode.");
                core.set_outcome("Highlight text first", 2.5);
                core.cue(Cue::Error);
                return;
            }
            Err(SelectionCaptureErr::ModifiersHeld) => {
                core.set_status("Release Shift, Alt, or Win before using Command Mode.");
                core.set_outcome("Release modifier keys", 2.5);
                core.cue(Cue::Error);
                return;
            }
            Err(SelectionCaptureErr::Other(msg)) => {
                recording_failed(core, msg);
                return;
            }
        }
    } else {
        cmd_ctx.selected_text.unwrap_or_default()
    };

    core.set_status("Recognising voice command…");
    core.shared.lock().unwrap().live_note = "Finishing".into();
    let t0 = Instant::now();
    let voice_command = {
        let mut g = core.engine.lock().unwrap();
        match g.as_mut() {
            Some(e) => e.transcribe(&samples, &s.vocab),
            None => Err("the speech engine isn't loaded".into()),
        }
    };
    let speech_secs = t0.elapsed().as_secs_f32();
    let voice_command = match voice_command {
        Ok(t) => t.trim().to_owned(),
        Err(e) => {
            recording_failed(core, format!("Recognition failed: {e}."));
            return;
        }
    };
    if !crate::command_mode::should_use_command_mode(Some(&selected), &voice_command) {
        core.set_status("Highlight text first to use Command Mode.");
        core.set_outcome("Highlight text first", 2.5);
        core.cue(Cue::Error);
        return;
    }
    core.shared.lock().unwrap().live_text = voice_command.clone();
    core.set_status("Editing highlighted text…");
    core.shared.lock().unwrap().live_note = "Editing".into();
    core.repaint();

    let replacement = match cleanup::run_command_mode(&s, &selected, &voice_command) {
        Ok(text) => text,
        Err(e) => {
            recording_failed(core, format!("Command Mode failed: {e}."));
            return;
        }
    };

    let copy = |text: &str| {
        !core.preview
            && !crate::winput::clipboard_recovery_pending()
            && crate::winput::copy_if_clipboard_unchanged(text, clipboard)
    };
    let allow_ctrl_v = s.registered_hotkey.as_deref() != Some("ctrl+v");
    let restore_clipboard = s.restore_clipboard;
    if let Some(outcome) = save_and_deliver_command(
        core,
        voice_command,
        replacement,
        secs,
        t0.elapsed().as_secs_f32(),
        target,
        |text, dest| crate::winput::deliver_dictation(text, dest, allow_ctrl_v, restore_clipboard),
        copy,
    ) {
        core.set_status(format!(
            "Command Mode — {outcome}. {secs:.1} s recognised in {speech_secs:.1} s, total {:.1} s.",
            t0.elapsed().as_secs_f32()
        ));
    }
}

fn process(core: &Arc<Core>, samples: Vec<f32>, held: Duration, target: Option<Target>, clipboard: Option<u32>) {
    let gen = core.shared.lock().unwrap().rec_gen;
    struct ProcessCacheGuard<'a> {
        core: &'a Core,
        gen: u64,
    }
    impl Drop for ProcessCacheGuard<'_> {
        fn drop(&mut self) {
            abandon_live_cache(self.core, self.gen);
        }
    }
    let _cache_guard = ProcessCacheGuard { core, gen };

    if samples.is_empty() && held > Duration::from_millis(500) {
        recording_failed(core, "No audio arrived from the microphone.".into());
        return;
    }
    let s = core.settings();
    let silence_rms = audio::silence_rms(s.quiet_mode);
    let secs = samples.len() as f32 / speech::SAMPLE_RATE as f32;
    if secs < 0.25 || speech::peak_window_rms(&samples) < silence_rms {
        core.set_status("Nothing heard — the recording was silent.");
        core.set_outcome("No speech heard", 3.0);
        return;
    }
    core.set_status(format!("Recognising {secs:.1} s of speech…"));
    core.shared.lock().unwrap().live_note = "Finishing".into();
    let t0 = Instant::now();
    let transcription = {
        // Locking `core.engine` waits if `live_preview` is finishing a short chunk, and
        // `live_preview` publishes that finished result to `live_cache` before dropping its lock.
        let mut g = core.engine.lock().unwrap();
        let cache_snapshot = {
            let (lock, cvar) = &core.live_cache;
            let mut cache = lock.lock().unwrap();
            if cache.gen == gen {
                cache.sealed = true;
                cvar.notify_all();
            }
            LiveCache {
                gen: cache.gen,
                vocab: cache.vocab.clone(),
                cleanup: cache.cleanup,
                style: cache.style,
                tone: cache.tone,
                quiet_mode: cache.quiet_mode,
                local_model: cache.local_model.clone(),
                ollama_url: cache.ollama_url.clone(),
                committed_upto: cache.committed_upto,
                committed: cache.committed.clone(),
                tail_end: cache.tail_end,
                tail_text: cache.tail_text.clone(),
                failed: cache.failed,
                sealed: cache.sealed,
                abandoned: cache.abandoned,
                preclean_active: cache.preclean_active,
            }
        };
        let can_reuse_engine = matches!(g.as_ref(), Some(Speech::Dll(_)));
        resolve_incremental_transcript(
            &samples,
            &cache_snapshot,
            gen,
            &s.vocab,
            silence_rms,
            can_reuse_engine,
            |slice| match g.as_mut() {
                Some(e) => e.transcribe(slice, &s.vocab),
                None => Err("the speech engine isn't loaded".into()),
            },
        )
    };
    let speech_secs = t0.elapsed().as_secs_f32();
    let transcript = match transcription {
        Ok(t) => t,
        Err(e) => {
            recording_failed(core, format!("Recognition failed: {e}."));
            return;
        }
    };
    let raw = transcript.raw;
    if raw.is_empty() {
        core.set_status("No words were recognised.");
        core.set_outcome("No speech heard", 3.0);
        return;
    }
    core.shared.lock().unwrap().live_text = raw.clone();

    let tone = resolve_target_tone(&s, target);
    let outcome = if let Some(expanded) = standalone_snippet_expansion(&raw, &s.snippets) {
        abandon_live_cache(core, gen);
        TranscriptResult::Deliver {
            cleaned: expanded,
            note: String::new(),
        }
    } else {
        let prepared = crate::formatting::prepare_dictation(&raw, &s.vocab);
        if prepared.discarded {
            abandon_live_cache(core, gen);
            TranscriptResult::Discarded
        } else {
            let mut precleaned_result = None;
            if s.cleanup
                && transcript.preclean_eligible
                && transcript.committed_count > 0
                && !prepared.self_corrected
                && prepared.local_list.is_none()
                && !has_cross_chunk_command(&raw)
            {
                let committed_pieces = {
                    let (lock, cvar) = &core.live_cache;
                    let mut cache = lock.lock().unwrap();
                    let same_cleanup = cache.gen == gen
                        && cache.cleanup == s.cleanup
                        && cache.style == s.style
                        && cache.tone == tone
                        && cache.quiet_mode == s.quiet_mode
                        && cache.local_model == s.local_model
                        && cache.ollama_url == s.ollama_url
                        && cache.vocab == s.vocab;
                    if same_cleanup {
                        let deadline = Instant::now() + cleanup::CLEANUP_TIMEOUT;
                        while cache.gen == gen
                            && cache.preclean_active
                            && !cache.abandoned
                            && cache.committed.len() >= transcript.committed_count
                            && cache.committed[..transcript.committed_count]
                                .iter()
                                .any(|p| p.cleaned == PieceClean::Pending)
                        {
                            let Some(rem) = deadline.checked_duration_since(Instant::now()) else {
                                break;
                            };
                            let (guard, timeout) = cvar.wait_timeout(cache, rem).unwrap();
                            cache = guard;
                            if timeout.timed_out() {
                                break;
                            }
                        }
                        if cache.gen == gen && cache.committed.len() >= transcript.committed_count {
                            Some(cache.committed[..transcript.committed_count].to_vec())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                // Stop `preclean_worker` before running any tail or fallback Ollama call so it never
                // contends with Finish on the GPU.
                abandon_live_cache(core, gen);
                if let Some(pieces) = committed_pieces {
                    precleaned_result = assemble_precleaned_with_tone(
                        &s,
                        &raw,
                        &pieces,
                        &transcript.tail_raw,
                        tone,
                        |text| {
                            core.set_status("Formatting…");
                            core.shared.lock().unwrap().live_note = "Formatting".into();
                            core.repaint();
                            cleanup::clean_dictation_with_tone(&s, text, tone)
                        },
                    );
                }
            } else {
                abandon_live_cache(core, gen);
            }
            resolve_dictation_outcome(&s, &raw, tone, precleaned_result, |text| {
                core.set_status("Formatting…");
                core.shared.lock().unwrap().live_note = "Formatting".into();
                core.repaint();
                cleanup::clean_dictation_with_tone(&s, text, tone)
            })
        }
    };
    let copy = |text: &str| {
        !core.preview
            && !crate::winput::clipboard_recovery_pending()
            && crate::winput::copy_if_clipboard_unchanged(text, clipboard)
    };
    let allow_ctrl_v = s.registered_hotkey.as_deref() != Some("ctrl+v");
    let restore_clipboard = s.restore_clipboard;
    // Deliver to whichever text box has focus when processing finishes (unless Destination is Workspace).
    let final_target = if matches!(outcome, TranscriptResult::Discarded)
        || core.preview
        || s.destination == Destination::Workspace
    {
        target
    } else {
        let live = crate::winput::capture_target();
        if live.is_some() { live } else { target }
    };
    apply_dictation_outcome(
        core,
        raw,
        outcome,
        secs,
        speech_secs,
        t0.elapsed().as_secs_f32(),
        final_target,
        |text, target| {
            // Trailing space on single-line dictation keeps back-to-back dictations separated,
            // matching original Listen (`dictate.py`), while History and clipboard recovery keep
            // the clean trimmed transcript.
            let spoken = if !text.contains(['\r', '\n']) && !text.ends_with(char::is_whitespace) {
                format!("{text} ")
            } else {
                text.to_owned()
            };
            crate::winput::deliver_dictation(&spoken, target, allow_ctrl_v, restore_clipboard)
        },
        copy,
    );
}

fn save_and_deliver_command(
    core: &Core,
    voice_command: String,
    replacement: String,
    secs: f32,
    processing_secs: f32,
    target: Option<Target>,
    deliver: impl FnOnce(&str, Option<Target>) -> Result<crate::winput::Delivery, crate::winput::DeliveryError>,
    copy_fallback: impl FnOnce(&str) -> bool,
) -> Option<String> {
    save_and_deliver_inner(core, voice_command, replacement, secs, processing_secs, target, true, deliver, copy_fallback)
}

pub(crate) fn save_and_deliver_using(
    core: &Core, raw: String, cleaned: String, secs: f32, processing_secs: f32, target: Option<Target>,
    deliver: impl FnOnce(&str, Option<Target>) -> Result<crate::winput::Delivery, crate::winput::DeliveryError>,
    copy_fallback: impl FnOnce(&str) -> bool,
) -> Option<String> {
    save_and_deliver_inner(core, raw, cleaned, secs, processing_secs, target, false, deliver, copy_fallback)
}

#[allow(clippy::too_many_arguments)]
fn save_and_deliver_inner(
    core: &Core, raw: String, cleaned: String, secs: f32, processing_secs: f32, target: Option<Target>,
    force_external_target: bool,
    deliver: impl FnOnce(&str, Option<Target>) -> Result<crate::winput::Delivery, crate::winput::DeliveryError>,
    copy_fallback: impl FnOnce(&str) -> bool,
) -> Option<String> {
    use crate::winput::Delivery;
    let to_workspace = !force_external_target
        && (core.settings.lock().unwrap().destination == Destination::Workspace
            || target.is_some_and(Target::is_ours));
    let (app_name, app_icon) = if to_workspace { Default::default() }
        else { target.and_then(Target::history_app).unwrap_or_default() };
    let (when, unix) = storage::now_stamp();
    let mut entry = HistoryEntry {
        when,
        unix,
        raw,
        cleaned,
        seconds: secs,
        processing_secs,
        engine: core.shared.lock().unwrap().engine_label(),
        outcome: if to_workspace { "saved for Prompt Lab" } else { "saved · delivery not confirmed" }.to_string(),
        app_name,
        app_icon,
        ..Default::default()
    };
    let text = entry.text().to_owned();
    if let Err(error) = storage::append_history(&entry) {
        core.emit(UiEvent::Retained { text, reason: "History could not be saved. Copy your text before closing Listen.".into() });
        core.set_outcome("Open Listen to recover text", 3.0);
        core.set_status(format!("Couldn't save dictation. No text was sent ({:?}).", error.kind()));
        core.cue(Cue::Error);
        return None;
    }
    let outcome = if to_workspace {
        let draft = core.shared.lock().unwrap().captured_draft;
        core.emit(UiEvent::ToWorkspace(text.clone(), draft));
        core.set_outcome("Review in Listen", 3.0);
        "added to Prompt Lab".to_string()
    } else {
        core.shared.lock().unwrap().paste_waiting = true;
        core.repaint();
        let result = deliver(&text, target);
        core.shared.lock().unwrap().paste_waiting = false;
        if let Err(
            crate::winput::DeliveryError::NotSent(ref reason)
            | crate::winput::DeliveryError::NotSentKeepClipboard(ref reason)
            | crate::winput::DeliveryError::Uncertain(ref reason),
        ) = result
        {
            if !core.preview {
                crate::diagnostics::error(&format!("Delivery did not complete automatically: {reason}"));
            }
        }
        let result = match result {
            Err(crate::winput::DeliveryError::NotSent(_)) if copy_fallback(&text) => Ok(Delivery::Copied),
            result => result,
        };
        match result {
            Ok(Delivery::Confirmed) => {
                core.set_outcome("Paste confirmed", 1.5);
                "paste confirmed".to_string()
            }
            Ok(Delivery::Sent) => {
                core.set_outcome("Text sent", 1.5);
                "text sent".to_string()
            }
            Err(crate::winput::DeliveryError::Uncertain(reason)) => {
                core.emit(UiEvent::Retained { text: text.clone(), reason });
                core.set_outcome("Check the original app first", 3.0);
                core.cue(Cue::DeliveryMissed);
                "sent to Windows · paste not confirmed".to_string()
            }
            Ok(Delivery::Copied) => {
                core.set_outcome("Copied · Press Ctrl+V", 3.0);
                core.cue(Cue::DeliveryMissed);
                "copied to clipboard".to_string()
            }
            Err(crate::winput::DeliveryError::NotSent(reason) | crate::winput::DeliveryError::NotSentKeepClipboard(reason)) => {
                core.emit(UiEvent::Retained { text: text.clone(), reason });
                core.set_outcome("Open Listen to recover text", 3.0);
                core.cue(Cue::DeliveryMissed);
                "kept in Listen".to_string()
            }
        }
    };

    if matches!(outcome.as_str(), "paste confirmed" | "text sent" | "copied to clipboard" | "kept in Listen") {
        // If the final status cannot be saved, the original row still says to check before copying.
        // Run the post-paste rewrite off the controller thread in live use so Listen returns to Idle
        // immediately after pasting; `storage` flushes pending updates before any later read/append.
        if core.preview {
            let _ = storage::update_latest_history_outcome(&entry, &outcome);
        } else {
            storage::update_latest_history_outcome_async(entry.clone(), outcome.clone());
        }
    }
    entry.outcome = outcome.clone();
    core.emit(UiEvent::History(entry));
    Some(outcome)
}

impl Shared {
    pub fn engine_label(&self) -> String {
        match &self.engine {
            EngineState::Ready(l) => l.clone(),
            _ => String::new(),
        }
    }
}
#[cfg(test)]
mod offline_flow {
    use super::*;

    #[test]
    fn empty_recording_reports_microphone_error_only_after_half_a_second() {
        for (held_ms, samples, expected_error) in [
            (100, vec![], ""),
            (500, vec![], ""),
            (501, vec![], "No audio arrived from the microphone."),
            (2000, vec![], "No audio arrived from the microphone."),
            (2000, vec![0.0; 2 * speech::SAMPLE_RATE], ""),
        ] {
            let (core, _) = Core::new(Settings::default(), true);
            process(&core, samples, Duration::from_millis(held_ms), None, None);
            let state = core.shared.lock().unwrap();
            let expected = if expected_error.is_empty() { "No speech heard" } else { "Processing failed" };
            assert_eq!(state.outcome.as_ref().map(|(message, _)| message.as_str()), Some(expected), "{held_ms} ms");
            assert_eq!(state.recording_error, expected_error, "{held_ms} ms");
        }
    }

    #[test]
    fn dictation_keeps_clear_lists_when_model_is_unavailable_or_regroups_words() {
        let settings = Settings {
            cleanup: true,
            ..Settings::default()
        };
        let raw = "Make a shopping list with four items: apples, mac and cheese, pears and milk.";
        let list = "Make a shopping list with four items:\n- apples\n- mac and cheese\n- pears\n- and milk.";
        let (text, _) = clean_transcript(&settings, raw, |input| {
            assert_eq!(input, list);
            Err("local model unavailable".into())
        });
        assert_eq!(text, list);
        let (text, _) = clean_transcript(&settings, raw, |_| {
            Ok(
            "Make a shopping list with four items:\n- apples mac\n- and cheese\n- pears\n- and milk.".into()
        )
        });
        assert_eq!(text, list);
        let (text, _) =
            clean_transcript(&settings, raw, |_| Ok(list.replace("- apples", "- Apples")));
        assert_eq!(text, list.replace("- apples", "- Apples"));

        let settings = Settings {
            cleanup: false,
            ..settings
        };
        let (text, _) = clean_transcript(&settings, raw, |_| panic!("Cleanup is off"));
        assert!(text.is_empty());
    }

    #[test]
    #[ignore = "uses an already-loaded local Ollama model with fixed sample text"]
    fn local_cleanup_preserves_meaning_in_sample() {
        let settings = Settings {
            local_model: std::env::var("LISTEN_TEST_LOCAL_MODEL").unwrap(),
            cloud_enabled: false,
            ..Settings::default()
        };
        let raw = "Um, please send fourteen copies on Monday. Do not change the delivery address.";
        let clean = cleanup::clean_dictation(&settings, raw).unwrap();
        let output = clean.to_ascii_lowercase();
        assert!(crate::formatting::is_safe(raw, &clean, &settings.vocab));
        assert!(output.contains("monday") && output.contains("address"));
        assert!(output.contains("fourteen") || output.contains("14"));
        assert!(output.contains("not") || output.contains("don't"));
    }

    #[test]
    #[ignore = "requires a disposable APPDATA profile; does not use the physical clipboard"]
    fn unsupported_delivery_copies_once_and_reuses_the_original_target() {
        use std::cell::Cell;
        let (core, _) = Core::new(Settings {
            destination: Destination::LastApp, sounds: false, ..Settings::default()
        }, true);
        let original = Target::fixture();
        let fixture = "Sample dictation for recovery.";
        let copies = Cell::new(0);
        let outcome = save_and_deliver_using(&core, fixture.into(), String::new(), 1.0, 0.25,
            Some(original), |text, destination| {
                assert_eq!(text, fixture);
                assert_eq!(destination, Some(original));
                Err("The destination cannot accept a verified edit.".into())
            }, |text| {
                assert_eq!(text, fixture);
                copies.set(copies.get() + 1);
                true
            });
        assert_eq!(outcome.as_deref(), Some("copied to clipboard"));
        assert_eq!(copies.get(), 1);
        assert_eq!(core.shared.lock().unwrap().outcome.as_ref().map(|(message, _)| message.as_str()),
            Some("Copied · Press Ctrl+V"));
        let history = storage::load_history();
        assert_eq!(history.last().unwrap().outcome, "copied to clipboard");
        let unverified = save_and_deliver_using(&core, fixture.into(), String::new(), 1.0, 0.25,
            Some(original), |_, _| Err(crate::winput::DeliveryError::Uncertain("Delayed edit.".into())),
            |_| panic!("A possibly inserted transcript must not be copied for automatic recovery."));
        assert_eq!(unverified.as_deref(), Some("sent to Windows · paste not confirmed"));
        let sent = save_and_deliver_using(&core, fixture.into(), String::new(), 1.0, 0.25,
            Some(original), |_, _| Ok(crate::winput::Delivery::Sent),
            |_| panic!("A sent transcript must not fall back to clipboard copy."));
        assert_eq!(sent.as_deref(), Some("text sent"));
        assert_eq!(storage::load_history().last().unwrap().outcome, sent.unwrap());
        let preserved_clipboard = save_and_deliver_using(&core, fixture.into(), String::new(), 1.0, 0.25,
            Some(original), |_, _| Err(crate::winput::DeliveryError::NotSentKeepClipboard("Clipboard recovery pending.".into())),
            |_| panic!("NotSentKeepClipboard must never overwrite the clipboard via copy_fallback."));
        assert_eq!(preserved_clipboard.as_deref(), Some("kept in Listen"));
        let failed_copy = save_and_deliver_using(&core, fixture.into(), String::new(), 1.0, 0.25,
            None, |_, _| Err("No destination.".into()), |_| false);
        assert_eq!(failed_copy.as_deref(), Some("kept in Listen"));
    }

    #[test]
    #[ignore = "requires an existing local runtime/model and a disposable APPDATA profile"]
    fn native_speech_saves_fixture_when_no_destination_is_open() {
        let fixture = std::env::var("LISTEN_TEST_PCM").unwrap();
        let installed_runtime = std::env::var("LISTEN_TEST_RUNTIME").unwrap();
        let installed_model = std::env::var("LISTEN_TEST_MODEL").unwrap();
        let fixture = std::fs::read(fixture).unwrap();
        assert_eq!(fixture.len() % 2, 0);
        let samples: Vec<f32> = fixture.chunks_exact(2)
            .map(|bytes| f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32768.0)
            .collect();
        assert!(storage::load_history().is_empty());
        let (core, events) = Core::new(Settings {
            cleanup: false, sounds: false, destination: Destination::LastApp, ..Settings::default()
        }, true);
        let model = speech::DllEngine::load(
            std::path::Path::new(&installed_runtime), std::path::Path::new(&installed_model), 4,
        ).unwrap();
        *core.engine.lock().unwrap() = Some(Speech::Dll(model));
        process(&core, samples, Duration::from_secs(3), None, None);
        let entries = storage::load_history();
        assert_eq!(entries.len(), 1, "one completed dictation must be saved exactly once");
        let raw = entries[0].raw.to_ascii_lowercase();
        for expected in ["quick", "brown", "fox", "lazy", "dog"] {
            assert!(raw.contains(expected), "fixture speech did not contain {expected}");
        }
        assert!(events.try_iter().any(|event| matches!(event, UiEvent::Retained { .. })));
        process(&core, vec![0.0; speech::SAMPLE_RATE], Duration::from_secs(1), None, None);
        assert_eq!(storage::load_history().len(), 1, "silence must not create a History row");
    }

    #[test]
    fn commit_cut_finds_quiet_pauses_trailing_silence_and_max_chunk_fallback() {
        let sr = speech::SAMPLE_RATE;
        // 1. Mid-speech 200 ms quiet pause (even at an unaligned offset) -> cuts strictly inside pause.
        let mut speech_with_pause = vec![0.2f32; 5 * sr];
        let pause_start = 3 * sr + 333;
        let pause_end = pause_start + 2 * PREVIEW_WIN;
        for s in &mut speech_with_pause[pause_start..pause_end] {
            *s = 0.0;
        }
        let cut = find_commit_cut(&speech_with_pause, SILENCE_RMS).expect("should find 200 ms pause");
        assert!(cut >= pause_start && cut <= pause_end, "cut {cut} outside [{pause_start}, {pause_end}]");

        // 2. A brief 100 ms dip is NOT enough to commit mid-speech (avoids splitting stop consonants).
        let mut short_dip = vec![0.2f32; 5 * sr];
        for s in &mut short_dip[3 * sr..3 * sr + PREVIEW_WIN] {
            *s = 0.0;
        }
        assert_eq!(find_commit_cut(&short_dip, SILENCE_RMS), None);

        // 3. Trailing 500 ms silence after 1.5 s of speech -> commits tail early.
        let mut trailing_quiet = vec![0.2f32; 2 * sr];
        trailing_quiet.extend(vec![0.0f32; TRAILING_SILENCE_WIN * PREVIEW_WIN]);
        let cut = find_commit_cut(&trailing_quiet, SILENCE_RMS).expect("should commit on trailing silence");
        assert_eq!(cut, trailing_quiet.len() - 2 * PREVIEW_WIN);

        // 4. Nonstop speech >= 18 s with no full pause -> falls back to quietest 100 ms window in 15..18 s.
        let mut nonstop = vec![0.2f32; MAX_LIVE_CHUNK];
        for s in &mut nonstop[16 * sr..16 * sr + PREVIEW_WIN] {
            *s = 0.05;
        }
        let cut = find_commit_cut(&nonstop, SILENCE_RMS).expect("should fall back at max chunk");
        assert_eq!(cut, 16 * sr + PREVIEW_WIN / 2);
    }

    #[test]
    fn incremental_transcription_reuses_committed_chunks_and_guards_edge_cases() {
        let sr = speech::SAMPLE_RATE;
        // 5 s recording: first 3 s committed, 3..4 s speech in tail, 4..5 s silence.
        let mut samples = vec![0.2f32; 4 * sr];
        samples.extend(vec![0.0f32; sr]);
        let cache = LiveCache {
            gen: 7,
            vocab: "Listen".into(),
            committed_upto: 3 * sr,
            committed: vec![CommittedPiece { raw: "First sentence.".into(), cleaned: PieceClean::Pending }],
            tail_end: 4 * sr + PREVIEW_WIN,
            tail_text: "Second sentence.".into(),
            failed: false,
            ..LiveCache::default()
        };

        // Fast Path A: trailing audio after tail_end is silent and tail_end is on silence -> 0 transcribe calls!
        let mut calls = 0usize;
        let fast = resolve_incremental_transcript(&samples, &cache, 7, "Listen", SILENCE_RMS, true, |_| {
            calls += 1;
            panic!("Fast Path A must not re-transcribe");
        }).unwrap();
        assert_eq!(calls, 0);
        assert_eq!(fast.raw, "First sentence. Second sentence.");
        assert_eq!(fast.committed_count, 1);

        // Fast Path B: new speech arrives after tail_end -> transcribes ONLY samples[committed_upto..].
        let mut extended = samples.clone();
        extended.extend(vec![0.2f32; sr]);
        let mut seen_len = 0usize;
        let partial = resolve_incremental_transcript(&extended, &cache, 7, "Listen", SILENCE_RMS, true, |slice| {
            seen_len = slice.len();
            Ok("Second sentence plus more.".into())
        }).unwrap();
        assert_eq!(seen_len, extended.len() - 3 * sr);
        assert_eq!(partial.raw, "First sentence. Second sentence plus more.");
        assert_eq!(partial.committed_count, 1);

        // Mid-word straddle guard: tail_end ended on loud audio and more audio followed -> re-transcribes tail!
        let straddle_cache = LiveCache { tail_end: 4 * sr, ..LiveCache {
            gen: 7,
            vocab: "Listen".into(),
            committed_upto: 3 * sr,
            committed: vec![CommittedPiece { raw: "First sentence.".into(), cleaned: PieceClean::Pending }],
            tail_text: "Second sent".into(),
            ..LiveCache::default()
        }};
        let mut straddle_calls = 0usize;
        let straddle = resolve_incremental_transcript(&samples, &straddle_cache, 7, "Listen", SILENCE_RMS, true, |slice| {
            straddle_calls += 1;
            assert_eq!(slice.len(), samples.len() - 3 * sr);
            Ok("Second sentence.".into())
        }).unwrap();
        assert_eq!(straddle_calls, 1);
        assert_eq!(straddle.raw, "First sentence. Second sentence.");

        // Fallback Path C: failed live preview or changed vocab -> transcribes full recording.
        let mut full_len = 0usize;
        let fallback = resolve_incremental_transcript(&samples, &cache, 7, "ChangedVocab", SILENCE_RMS, true, |slice| {
            full_len = slice.len();
            Ok("Full fallback.".into())
        }).unwrap();
        assert_eq!(full_len, samples.len());
        assert_eq!(fallback.committed_count, 0);
        assert!(!fallback.preclean_eligible);
    }

    #[test]
    fn precleaned_chunks_reuse_work_and_bypass_cross_chunk_commands_and_lists() {
        let settings = Settings { cleanup: true, ..Settings::default() };
        let committed = vec![
            CommittedPiece {
                raw: "Um first sentence.".into(),
                cleaned: PieceClean::Done("First sentence.".into(), String::new()),
            },
        ];

        // Reuses pre-cleaned committed chunk and only sends the short tail to the cleaner.
        let mut tail_input = String::new();
        let (cleaned, note) = assemble_precleaned(
            &settings,
            "Um first sentence. uh second sentence.",
            &committed,
            "uh second sentence.",
            |t| {
                tail_input = t.to_owned();
                Ok("Second sentence.".into())
            },
        ).expect("should reuse pre-cleaned chunk");
        assert_eq!(tail_input, "uh second sentence.");
        assert_eq!(cleaned, "First sentence. Second sentence.");
        assert!(note.is_empty());

        // Preserves partial skip note when the tail fails cleanup.
        let (cleaned, note) = assemble_precleaned(
            &settings,
            "Um first sentence. uh second sentence.",
            &committed,
            "uh second sentence.",
            |_| Err("model busy".into()),
        ).expect("should still reuse chunk 0 and report tail skip note");
        assert_eq!(cleaned, "First sentence. uh second sentence.");
        assert!(note.contains("model busy"));

        // Empty tail (user paused before Finish): zero Ollama calls!
        let (cleaned, _) = assemble_precleaned(
            &settings,
            "Um first sentence.",
            &committed,
            "",
            |_| panic!("empty tail must not call cleaner"),
        ).expect("should finish with zero tail calls");
        assert_eq!(cleaned, "First sentence.");

        // Multiline pre-cleaned output (lists/tables) falls back to full-text cleanup instead of space-joining.
        let multiline = vec![CommittedPiece {
            raw: "Apples and pears.".into(),
            cleaned: PieceClean::Done("- Apples\n- pears.".into(), String::new()),
        }];
        assert!(assemble_precleaned(
            &settings,
            "Apples and pears. Call Mom.",
            &multiline,
            "Call Mom.",
            |_| panic!("multiline chunk must bypass assembly"),
        ).is_none());

        // Mid-sentence skipped chunk falls back to full-text cleanup (never inserts bogus periods).
        assert!(!ends_sentence("Please send the updated report to"));
        assert!(ends_sentence("Please send the updated report."));
        assert!(ends_sentence("Did it work?\""));
        let skipped = vec![CommittedPiece {
            raw: "Please send the updated report to".into(),
            cleaned: PieceClean::Skipped,
        }];
        assert!(assemble_precleaned(
            &settings,
            "Please send the updated report to Sarah.",
            &skipped,
            "Sarah.",
            |_| panic!("skipped mid-sentence chunk must bypass assembly"),
        ).is_none());

        // Cross-chunk spoken command ("scratch that") must bypass chunk cache for full-context cleanup.
        assert!(assemble_precleaned(
            &settings,
            "Um first sentence. scratch that second sentence.",
            &committed,
            "scratch that second sentence.",
            |_| panic!("should have bypassed preclean"),
        ).is_none());

        // Explicit spoken list must bypass chunk cache so list formatting sees full context.
        assert!(assemble_precleaned(
            &settings,
            "Make a shopping list: apples, pears, milk.",
            &committed,
            "pears, milk.",
            |_| panic!("should have bypassed preclean for spoken list"),
        ).is_none());
    }

    #[test]
    fn clean_short_fast_skip_never_bypasses_triggers_fillers_lists_or_vocab() {
        let clean_settings = Settings {
            cleanup: true,
            style: crate::storage::Style::Clean,
            vocab: String::new(),
            ..Settings::default()
        };

        // Crisp, clean short sentences skip Ollama with 0 model calls.
        for clean_raw in [
            "Run the unit tests now.",
            "Can you check why the build failed?",
            "Ship it!",
            "Open the dashboard and check recent activity.",
        ] {
            assert!(is_already_clean_short(&clean_settings, clean_raw), "should fast-skip: {clean_raw}");
            let (out, note) = clean_transcript(&clean_settings, clean_raw, |_| {
                panic!("clean short sentence should not call Ollama: {clean_raw}")
            });
            assert!(out.is_empty());
            assert!(note.is_empty());
        }

        // Triggers, fillers, lists, repeats, capitalization/punctuation gaps, and long text MUST still call Ollama.
        for must_clean in [
            "Send the email scratch that message them.",
            "Wait delete that please.",
            "Hello new line world.",
            "Done new paragraph next thought.",
            "Make a shopping list: apples, pears, milk.",
            "Add a bullet for testing.",
            "First wash hands, second dry hands.",
            "Put this in a table.",
            "Um, run the unit tests now.",
            "It is, like, really fast.",
            "You know this works well.",
            "Run the the unit tests now.",
            "run the unit tests now.",
            "Run the unit tests now",
            "When i run this it fails.",
            "First sentence. second sentence.",
            "This is a much longer dictation that goes well past the short single-sentence fast-skip length limit so that Ollama always gets a chance to clean up structure and punctuation properly.",
        ] {
            assert!(!is_already_clean_short(&clean_settings, must_clean), "must NOT fast-skip: {must_clean}");
        }

        // Tighten style and custom vocabulary never fast-skip.
        let tighten = Settings { style: crate::storage::Style::Tighten, ..clean_settings.clone() };
        assert!(!is_already_clean_short(&tighten, "Run the unit tests now."));
        let with_vocab = Settings { vocab: "Parakeet".into(), ..clean_settings };
        assert!(!is_already_clean_short(&with_vocab, "Run the unit tests now."));
    }

    #[test]
    fn self_corrections_snippets_and_tone_integrate_with_cleanup_pipeline() {
        use crate::app_tone::TonePreset;
        use crate::snippets::validate_snippet;

        // 1. Self-correction applies before AI formatting, validates against corrected text, and falls back safely.
        let clean_on = Settings {
            cleanup: true,
            style: crate::storage::Style::Tighten,
            ..Settings::default()
        };
        let (out, note) = clean_transcript(&clean_on, "Let's meet at 4 PM, actually make that 3 PM", |input| {
            assert_eq!(input, "Let's meet at 3 PM");
            Ok("Let's meet at 3 PM.".into())
        });
        assert_eq!(out, "Let's meet at 3 PM.");
        assert!(note.is_empty());

        // When Ollama hallucinates a different hour, falls back to the self-corrected transcript.
        let (fallback, _) = clean_transcript(&clean_on, "Let's meet at 4 PM, actually make that 3 PM", |_| {
            Ok("Let's meet at 2 PM.".into())
        });
        assert_eq!(fallback, "Let's meet at 3 PM");

        // Self-correction also resolves deterministically when AI cleanup is OFF.
        let clean_off = Settings { cleanup: false, ..clean_on.clone() };
        let (off_out, _) = clean_transcript(&clean_off, "Order 5 boxes, make that 8 boxes", |_| {
            panic!("cleanup is off")
        });
        assert_eq!(off_out, "Order 8 boxes");

        // assemble_precleaned bypasses chunk cache when full raw transcript has a self-correction.
        let committed = vec![CommittedPiece {
            raw: "Let's meet at 4 PM.".into(),
            cleaned: PieceClean::Done("Let's meet at 4 PM.".into(), String::new()),
        }];
        assert!(assemble_precleaned(
            &clean_on,
            "Let's meet at 4 PM. Actually make that 3 PM.",
            &committed,
            "Actually make that 3 PM.",
            |_| panic!("self-corrected transcript must bypass preclean chunk assembly"),
        ).is_none());

        // 2. Non-default app tone does not fast-skip Ollama even under Style::Clean.
        let clean_style = Settings {
            cleanup: true,
            style: crate::storage::Style::Clean,
            ..Settings::default()
        };
        let mut called = false;
        let (casual_out, _) = clean_transcript_with_tone(
            &clean_style,
            "Run the unit tests now.",
            TonePreset::Casual,
            |_| {
                called = true;
                Ok("Run the unit tests now".into())
            },
        );
        assert!(called, "non-default tone must reach AI cleanup");
        assert_eq!(casual_out, "Run the unit tests now");

        // 3. Standalone snippet detection skips AI while inline triggers wait until post-cleanup.
        let snippets = vec![validate_snippet("my email", "cam@example.com").unwrap()];
        assert_eq!(
            standalone_snippet_expansion("my email.", &snippets).as_deref(),
            Some("cam@example.com")
        );
        assert_eq!(
            standalone_snippet_expansion("Send it to my email please.", &snippets),
            None
        );
    }

    #[test]
    fn cleanup_off_applies_locally_edited_vocab_and_spoken_commands() {
        let settings = Settings {
            cleanup: false,
            vocab: "Ollama, CLAUDE.md, listen-rs".into(),
            ..Settings::default()
        };

        let (vocab_out, note) = clean_transcript(&settings, "check ollama and claude md in listen rs", |_| {
            panic!("cleanup is off")
        });
        assert_eq!(vocab_out, "check Ollama and CLAUDE.md in listen-rs");
        assert!(note.is_empty());

        let (para_out, _) = clean_transcript(&settings, "the build passed new paragraph now test the UI", |_| {
            panic!("cleanup is off")
        });
        assert_eq!(para_out, "the build passed\n\nNow test the UI");

        let (scratch_out, _) = clean_transcript(
            &settings,
            "set the port to 8080 scratch that set the port to 3000",
            |_| panic!("cleanup is off"),
        );
        assert_eq!(scratch_out, "set the port to 3000");
    }

    #[test]
    fn standalone_snippet_does_not_misclassify_inline_collision() {
        use crate::snippets::validate_snippet;

        let snippets = vec![
            validate_snippet("a", "hello").unwrap(),
            validate_snippet("b", "hello world").unwrap(),
        ];
        // "a world" expands inline snippet "a" to "hello world", which happens to equal snippet "b"'s
        // expansion, but it is NOT a standalone trigger and must still flow through normal cleanup.
        assert_eq!(standalone_snippet_expansion("a world", &snippets), None);
        assert_eq!(
            standalone_snippet_expansion("b.", &snippets).as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn style_tighten_accepts_trigger_preserving_condensation_and_rejects_trigger_drop() {
        use crate::app_tone::TonePreset;
        use crate::snippets::validate_snippet;

        let snippets = vec![
            validate_snippet("my email", "cam@example.com").unwrap(),
            validate_snippet("follow up", "follow up next week").unwrap(),
        ];
        let settings = Settings {
            cleanup: true,
            style: crate::storage::Style::Tighten,
            snippets,
            ..Settings::default()
        };

        // Filler-heavy condensation preserving the snippet trigger passes `is_safe_output_for_style`,
        // and inline snippet expansion happens strictly post-Ollama.
        let outcome = resolve_dictation_outcome(
            &settings,
            "Um so I basically just want to send it to my email",
            TonePreset::Default,
            None,
            |input| {
                assert_eq!(input, "Um so I basically just want to send it to my email");
                Ok("I want to send it to my email.".into())
            },
        );
        assert_eq!(
            outcome,
            TranscriptResult::Deliver {
                cleaned: "I want to send it to cam@example.com.".into(),
                note: String::new(),
            }
        );

        // Hyphenated raw match ("follow-up") for multi-word trigger ("follow up") is preserved and expanded post-Ollama.
        let hyphen_outcome = resolve_dictation_outcome(
            &settings,
            "Um so let's follow-up tomorrow",
            TonePreset::Default,
            None,
            |input| {
                assert_eq!(input, "Um so let's follow-up tomorrow");
                Ok("Let's follow-up tomorrow.".into())
            },
        );
        assert_eq!(
            hyphen_outcome,
            TranscriptResult::Deliver {
                cleaned: "Let's follow up next week tomorrow.".into(),
                note: String::new(),
            }
        );

        // Dropping the raw snippet trigger during Tighten condensation must be rejected so
        // post-Ollama snippet expansion still sees the trigger in the raw fallback.
        let (rejected, rej_note) = clean_transcript(
            &settings,
            "Um so please send it to my email tomorrow",
            |_| Ok("Send it tomorrow.".into()),
        );
        assert!(rejected.is_empty(), "dropping snippet trigger must fall back to raw");
        assert!(rej_note.contains("changed too much"), "unexpected note: {rej_note}");
    }

    #[test]
    fn discarded_dictation_aborts_cleanly_without_pasting_raw() {
        use crate::app_tone::TonePreset;
        use crate::snippets::validate_snippet;

        let snippets = vec![validate_snippet("my email", "cam@example.com").unwrap()];
        for (cleanup_on, raw) in [
            (true, "Send at 4 PM scratch that"),
            (false, "Send at 4 PM scratch that"),
            (true, "Send at 4 PM, delete that"),
            (true, "my email scratch that"),
        ] {
            let settings = Settings {
                cleanup: cleanup_on,
                sounds: false,
                snippets: snippets.clone(),
                ..Settings::default()
            };
            let (core, events) = Core::new(settings.clone(), true);
            let outcome = resolve_dictation_outcome(&settings, raw, TonePreset::Default, None, |_| {
                panic!("discarded dictation must not invoke AI cleanup: {raw}")
            });
            assert_eq!(outcome, TranscriptResult::Discarded, "raw={raw} cleanup={cleanup_on}");

            let delivered = apply_dictation_outcome(
                &core,
                raw.to_owned(),
                outcome,
                1.0,
                0.2,
                0.3,
                None,
                |_, _| panic!("discarded dictation must never deliver/paste raw"),
                |_| panic!("discarded dictation must never copy raw to clipboard"),
            );
            assert_eq!(delivered, None);
            let sh = core.shared.lock().unwrap();
            assert_eq!(sh.outcome.as_ref().map(|(m, _)| m.as_str()), Some("Discarded"));
            assert_eq!(sh.status, "Dictation discarded.");
            assert!(
                events.try_iter().next().is_none(),
                "discarded dictation must not emit History, ToWorkspace, or Retained events"
            );
        }
    }
}
