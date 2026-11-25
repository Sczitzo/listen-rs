//! Observe the built app with controlled inputs. The reference sheet is read by the separate checker.
use crate::{audio, engine, speech, winput};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::path::Path;
use std::time::Duration;

fn first_matching(limit: u64, mut matches: impl FnMut(u64) -> bool) -> Result<u64, String> {
    if matches(0) || !matches(limit) {
        return Err(format!("No transition between 0 and {limit}"));
    }
    let (mut low, mut high) = (0, limit);
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if matches(mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    Ok(high)
}

fn seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

fn note_hz(samples: &[f32], rate: f32) -> Result<u32, String> {
    let crossings: Vec<_> = samples.windows(2).enumerate()
        .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
        .skip(1)
        .map(|(index, pair)| index as f32 - pair[0] / (pair[1] - pair[0]))
        .collect();
    match crossings.as_slice() {
        [first, .., last] => Ok(((crossings.len() - 1) as f32 * rate / (last - first)).round() as u32),
        _ => Err("The chime had too few audible periods to measure".into()),
    }
}

pub(crate) fn write(path: &Path) -> Result<(), String> {
    let mut values = serde_json::Map::new();
    let mut observe = |name: &str, value: Value, method: &str| {
        values.insert(name.to_owned(), json!({ "value": value, "method": method }));
    };

    let tap = first_matching(60_000, |ms| !winput::locks_recording(seconds(ms) as f32))?;
    let start = first_matching(60_000, |ms| winput::starts_recording(seconds(ms) as f32))?;
    let finish = first_matching(3_600_000, |ms| winput::exceeds_recording_limit(seconds(ms) as f32))?;
    observe("hotkey.tap_lock", json!(seconds(tap)), "First simulated release that does not lock recording");
    observe("recording.start_chime", json!(seconds(start)), "First simulated hold that requests recording and its start cue");
    observe("recording.max_length", json!(seconds(finish - 1)), "Last simulated elapsed millisecond before automatic finish");

    let empty = first_matching(60_000, |ms| {
        engine::sample_recording_outcome(Vec::new(), Duration::from_millis(ms)) == "Processing failed"
    })?;
    observe("recording.no_audio_error_after", json!(seconds(empty - 1)), "Last simulated millisecond before the empty recording reports a microphone error");

    const INPUT_RATE: u32 = 48_000;
    let input_second = vec![0.0; INPUT_RATE as usize];
    let sample_rate = audio::resample_recording(&input_second, INPUT_RATE).len();
    if sample_rate == 0 || sample_rate > 192_000 {
        return Err(format!("Unexpected recording resample rate: {sample_rate}"));
    }
    observe("speech.sample_rate", json!(sample_rate), "Output samples from one second of 48 kHz input");
    let steps = 1_000_000;
    let silence = first_matching(steps, |amplitude| {
        let samples = vec![amplitude as f32 / steps as f32; sample_rate];
        engine::sample_recording_outcome(samples, Duration::from_secs(1)) != "No speech heard"
    })?;
    observe("recording.silence_rms", json!(silence as f64 / steps as f64), "First constant audio level that reaches recognition, measured in steps of 0.000001");

    let long_recording = vec![0.1; 60 * sample_rate];
    let split = first_matching(long_recording.len() as u64, |samples| {
        speech::split_chunks(&long_recording[..samples as usize]).len() > 1
    })?;
    observe("speech.chunk_max", json!((split - 1) as f64 / sample_rate as f64), "Longest sample input returned as one recognition chunk");

    let rate = INPUT_RATE as f32;
    let (note_length, _) = audio::cue_samples(audio::Cue::Copied, rate);
    if note_length == 0 || note_length > INPUT_RATE as usize {
        return Err(format!("Unexpected copied chime length: {note_length} samples"));
    }
    let waveform = "Frequency measured from the app's generated speaker samples";
    for (cue, name) in [
        (audio::Cue::Start, "chime.start_notes"),
        (audio::Cue::Stop, "chime.stop_notes"),
        (audio::Cue::Cancel, "chime.cancel_note"),
        (audio::Cue::Error, "chime.error_notes"),
        (audio::Cue::Copied, "chime.copied_hz"),
    ] {
        let (count, mut next) = audio::cue_samples(cue, rate);
        if count == 0 || count % note_length != 0 || count > 4 * INPUT_RATE as usize {
            return Err(format!("Unexpected chime length for {name}: {count} samples"));
        }
        let samples: Vec<_> = (0..count).map(|_| next()).collect();
        let notes = samples.chunks_exact(note_length)
            .map(|note| note_hz(note, rate))
            .collect::<Result<Vec<_>, _>>()?;
        let actual = match notes.as_slice() {
            [hz] if matches!(cue, audio::Cue::Copied | audio::Cue::Cancel) => json!(hz),
            _ => json!(notes.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")),
        };
        observe(name, actual, waveform);
    }
    observe("chime.copied_length", json!(note_length as f64 / INPUT_RATE as f64), "Copied chime samples divided by the speaker rate");

    let file = OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|error| format!("Cannot create measurements: {error}"))?;
    serde_json::to_writer(file, &values).map_err(|error| format!("Cannot write measurements: {error}"))
}
