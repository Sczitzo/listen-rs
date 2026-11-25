use crate::engine::{Core, EngineState, Phase};
use crate::storage::HistoryEntry;
use serde_json::{json, Value};

pub fn state(core: &Core, history_len: usize, workspace_version: u64, workspace_busy: bool, models: &[String]) -> Value {
    let state = core.shared.lock().unwrap();
    let (speech, error) = match &state.engine {
        EngineState::Loading => ("loading", ""),
        EngineState::Ready(_) => ("ready", ""),
        EngineState::Unavailable(error) => ("failed", error.as_str()),
    };
    let preview_phase = match state.phase {
        Phase::Recording => "recording", Phase::Working => "finishing", Phase::Idle => "idle",
    };
    #[cfg(windows)]
    let controller_capture = core.controller_capture.lock().unwrap().as_ref().and_then(|capture| {
        capture.picked.map(|binding| json!({"token": capture.token, "binding": binding}))
    });
    #[cfg(not(windows))]
    let controller_capture = Value::Null;
    json!({
        "state": if state.phase == Phase::Recording { "recording" } else if speech == "loading" { "loading" } else { "idle" },
        "busy": if state.phase == Phase::Working { "processing" } else { "" },
        "paste_waiting": state.paste_waiting,
        "recording_ready": state.phase == Phase::Recording,
        "speech_runtime": { "state": speech },
        "speech_summary": match &state.engine {
            EngineState::Ready(label) => label.as_str(), EngineState::Unavailable(error) => error.as_str(),
            EngineState::Loading => "Starting local speech",
        },
        "speech_retry_ready": speech == "failed" && state.phase == Phase::Idle,
        "last_error": if speech == "loading" { "" } else if !error.is_empty() { error } else if !state.shortcut_error.is_empty() {
            &state.shortcut_error
        } else { &state.recording_error },
        "llm_ok": !models.is_empty(),
        "lab_busy": if state.lab_busy || workspace_busy { "working" } else { "" }, "lab_ver": workspace_version,
        "history_len": history_len, "retained_ver": workspace_version,
        "levels": if state.phase == Phase::Recording { vec![core.level(); 20] } else { Vec::new() },
        "locked": state.hands_free,
        "controller_hotkey_capture": controller_capture,
        "preview": { "version": state.rec_gen, "active": state.phase == Phase::Recording,
            "phase": preview_phase, "text": state.live_text, "state": "ready" },
    })
}

pub fn history(entries: &[HistoryEntry], all: bool, clipboard_recovery: bool) -> Value {
    let entries = entries.iter().rev().take(if all { usize::MAX } else { 8 });
    Value::Array(entries.map(|entry| {
        let delivery = match entry.outcome.as_str() {
            "paste confirmed" => "verified",
            "text sent" => "sent",
            "copied to clipboard" => "copied",
            "kept in Listen" | "added to Prompt Lab" | "saved for Prompt Lab" => "retained",
            "saved · delivery not confirmed" | "sent to Windows · paste not confirmed"
            | "paste shortcut sent · check Craft Anything" => "uncertain",
            _ => "unknown",
        };
        let app = (!entry.app_name.is_empty())
            .then(|| json!({ "name": entry.app_name, "icon": entry.app_icon }));
        json!({ "ts": entry.when.replace(' ', "T"), "kind": "dictate", "text": entry.text(),
            "raw": entry.raw, "secs": entry.seconds, "latency": entry.processing_secs, "cost": 0,
            "delivery_status": delivery, "clipboard_recovery": clipboard_recovery, "app": app })
    }).collect())
}

pub fn stats(entries: &[HistoryEntry], unpriced: usize) -> Value {
    let (when, unix) = crate::storage::now_stamp();
    stats_at(entries, unpriced, &when, unix)
}

const DAY: i64 = 24 * 60 * 60;
const WEEK: i64 = 7 * DAY;

// Compare local wall-clock periods, including across a daylight-saving change.
fn local_seconds(when: &str, unix: u64) -> Option<i64> {
    let (date, time) = when.split_once(' ')?;
    let (year, month_day) = date.split_once('-')?;
    let (month, day) = month_day.split_once('-')?;
    let (hour, minute) = time.split_once(':')?;
    let year: i64 = year.parse().ok().filter(|y| (1..=9999).contains(y))?;
    let month: usize = month.parse().ok().filter(|m| (1..=12).contains(m))?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let day: i64 = day.parse().ok().filter(|d| (1..=months[month - 1]).contains(d))?;
    let hour: i64 = hour.parse().ok().filter(|h| (0..24).contains(h))?;
    let minute: i64 = minute.parse().ok().filter(|m| (0..60).contains(m))?;
    let y = year - 1;
    let days = y * 365 + y / 4 - y / 100 + y / 400 + months[..month - 1].iter().sum::<i64>() + day - 1;
    Some(days * DAY + hour * 3600 + minute * 60 + (unix % 60) as i64)
}

#[derive(Default)]
struct WeekStats {
    count: usize,
    words: usize,
    seconds: f64,
    latency_sum: f64,
    latency_count: u32,
}

impl WeekStats {
    fn wpm(&self) -> Option<u64> {
        (self.seconds > 30.0).then(|| (self.words as f64 * 60.0 / self.seconds).round_ties_even() as u64)
    }

    fn latency(&self) -> Option<f64> {
        (self.latency_count > 0).then(|| self.latency_sum / f64::from(self.latency_count))
    }
}

fn stats_at(entries: &[HistoryEntry], unpriced: usize, when: &str, unix: u64) -> Value {
    let local_now = local_seconds(when, unix);
    let now = local_now.unwrap_or(unix as i64);
    let midnight = now / DAY * DAY;
    let mut earliest = i64::MAX;
    let mut daily = [0; 7];
    let mut weekly: [WeekStats; 4] = Default::default();
    for entry in entries {
        let stamp = if local_now.is_some() {
            let Some(stamp) = local_seconds(&entry.when, entry.unix) else { continue };
            stamp
        } else {
            entry.unix as i64
        };
        if stamp > now { continue; }
        earliest = earliest.min(stamp);
        let day_offset = (midnight - stamp / DAY * DAY) / DAY;
        if let Some(count) = daily.get_mut(day_offset as usize) {
            if stamp % DAY <= now % DAY { *count += 1; }
        }
        let week_offset = (now - stamp).saturating_sub(1).max(0) / WEEK;
        if let Some(week) = weekly.get_mut(week_offset as usize) {
            week.count += 1;
            week.words += entry.text().split_whitespace().count();
            week.seconds += f64::from(entry.seconds);
            if entry.processing_secs > 0.0 {
                week.latency_sum += f64::from(entry.processing_secs);
                week.latency_count += 1;
            }
        }
    }

    let today = (0..7).rev().map(|offset| {
        (offset == 0 || earliest <= midnight - offset as i64 * DAY).then_some(daily[offset])
    }).collect::<Vec<_>>();
    let weeks = (0..4).rev().map(|offset| {
        (offset == 0 || earliest <= now - (offset as i64 + 1) * WEEK).then_some(&weekly[offset])
    }).collect::<Vec<_>>();
    let current = &weekly[0];
    json!({ "today": daily[0], "week": current.count, "words_week": current.words,
        "minutes_week": current.seconds / 60.0,
        "wpm": current.wpm().unwrap_or(0), "latency": current.latency().unwrap_or(0.0),
        "spend_month": 0, "spend_unknown": unpriced,
        "trends": {
            "today": today,
            "words_week": weeks.iter().map(|week| week.map(|week| week.words)).collect::<Vec<_>>(),
            "wpm": weeks.iter().map(|week| week.and_then(WeekStats::wpm)).collect::<Vec<_>>(),
            "latency": weeks.iter().map(|week| week.and_then(WeekStats::latency)).collect::<Vec<_>>(),
        } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaking_speed_requires_more_than_thirty_seconds_this_week() {
        let mut entry = HistoryEntry {
            when: "2026-11-21 11:58".into(), raw: "One two three four five six seven eight nine ten".into(),
            seconds: 30.0, ..Default::default()
        };
        let older = HistoryEntry { when: "2026-11-13 12:00".into(), seconds: 60.0, ..entry.clone() };
        let short = stats_at(&[older.clone(), entry.clone()], 0, "2026-11-21 12:00", 0);
        assert_eq!(short["words_week"], 10);
        assert_eq!(short["wpm"], 0);
        entry.seconds = 31.0;
        assert_eq!(stats_at(&[older, entry], 0, "2026-11-21 12:00", 0)["wpm"], 19);
    }

    #[test]
    fn history_reports_zero_activity_only_for_observed_periods() {
        let when = "2026-11-21 12:00";
        let current = HistoryEntry {
            when: when.into(), raw: "Four synthetic sample words".into(), seconds: 10.0,
            processing_secs: 0.5, ..Default::default()
        };
        let earliest = HistoryEntry {
            when: "2000-01-01 12:00".into(), unix: 946_728_000, ..current.clone()
        };
        let recent_only = stats_at(std::slice::from_ref(&current), 0, when, 0);
        assert_eq!(recent_only["trends"]["today"], json!([null, null, null, null, null, null, 1]));
        let established = stats_at(&[earliest, current], 0, when, 0);
        assert_eq!(established["trends"]["today"], json!([0, 0, 0, 0, 0, 0, 1]));
        assert_eq!(established["trends"]["words_week"], json!([0, 0, 0, 4]));
        assert_eq!(established["trends"]["wpm"], json!([null, null, null, null]));
        assert_eq!(established["trends"]["latency"], json!([null, null, null, 0.5]));
    }

    #[test]
    fn history_compares_the_same_time_yesterday_and_adjacent_weeks() {
        let sample = |when: &str, processing_secs| HistoryEntry {
            when: when.into(), raw: "Four synthetic sample words".into(), seconds: 60.0,
            processing_secs, ..Default::default()
        };
        let entries = [
            sample("2024-02-20 12:00", 0.0),
            sample("2024-03-05 11:59", 1.0),
            sample("2024-03-05 12:00", 0.0),
            sample("2024-03-11 12:00", 2.0),
            sample("2024-03-11 12:01", 4.0),
        ];
        let result = stats_at(&entries, 0, "2024-03-12 12:00", 0);
        assert_eq!(result["week"], 3);
        assert_eq!(result["trends"]["today"], json!([0, 0, 0, 0, 0, 1, 0]));
        assert_eq!(result["trends"]["words_week"], json!([null, 4, 4, 12]));
        assert_eq!(result["trends"]["wpm"], json!([null, 4, 4, 4]));
        assert_eq!(result["trends"]["latency"], json!([null, null, 1.0, 3.0]));
        assert!(local_seconds("2026-02-29 12:00", 0).is_none());
    }

    #[test]
    fn recent_history_returns_the_newest_eight_entries() {
        let entries: Vec<_> = (0..10).map(|i| HistoryEntry {
            raw: format!("Sample {i}"), ..Default::default()
        }).collect();
        let recent = history(&entries, false, false);
        let labels: Vec<_> = recent.as_array().unwrap().iter()
            .map(|row| row["text"].as_str().unwrap()).collect();
        assert_eq!(labels, ["Sample 9", "Sample 8", "Sample 7", "Sample 6",
            "Sample 5", "Sample 4", "Sample 3", "Sample 2"]);
        assert_eq!(history(&entries, true, false).as_array().unwrap().len(), entries.len());
    }

    #[test]
    fn saved_and_paste_status_are_distinct() {
        let rows = history(&[HistoryEntry {
            raw: "Sample words".into(), outcome: "saved · delivery not confirmed".into(),
            ..Default::default()
        }], true, true);
        assert_eq!(rows[0]["text"], "Sample words");
        assert_eq!(rows[0]["delivery_status"], "uncertain");
        assert_eq!(rows[0]["clipboard_recovery"], true);
        assert!(rows[0]["app"].is_null());
        let rows = history(&[HistoryEntry {
            raw: "One\nTwo".into(), outcome: "paste confirmed".into(), app_name: "Notepad".into(),
            app_icon: Some("sample icon".into()), ..Default::default()
        }], true, false);
        assert_eq!(rows[0]["text"], "One\nTwo");
        assert_eq!(rows[0]["delivery_status"], "verified");
        assert_eq!(rows[0]["clipboard_recovery"], false);
        assert_eq!(rows[0]["app"]["name"], "Notepad");
        assert_eq!(rows[0]["app"]["icon"], "sample icon");
        let copied = history(&[HistoryEntry {
            raw: "Sample clipboard recovery".into(), outcome: "copied to clipboard".into(), ..Default::default()
        }], true, false);
        assert_eq!(copied[0]["delivery_status"], "copied");
        let sent = history(&[HistoryEntry {
            raw: "Sample direct typing".into(), outcome: "text sent".into(), ..Default::default()
        }], true, false);
        assert_eq!(sent[0]["delivery_status"], "sent");
    }
}
