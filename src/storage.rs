//! Settings, history, favorites and the draft, saved as plain JSON files.

use crate::theme::{LayoutId, ThemeId};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("ListenRs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hotkey {
    RightCtrl,
    RightShift,
    RightAlt,
    Pause,
    ScrollLock,
}

impl Hotkey {
    pub const ALL: [Hotkey; 5] = [Hotkey::RightCtrl, Hotkey::RightShift, Hotkey::RightAlt, Hotkey::Pause, Hotkey::ScrollLock];
    pub fn label(self) -> &'static str {
        match self {
            Hotkey::RightCtrl => "Right Ctrl",
            Hotkey::RightShift => "Right Shift",
            Hotkey::RightAlt => "Right Alt",
            Hotkey::Pause => "Pause",
            Hotkey::ScrollLock => "Scroll Lock",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Tighten,
    Clean,
}

impl Style {
    pub fn label(self) -> &'static str {
        match self {
            Style::Tighten => "Tighten & format",
            Style::Clean => "Keep wording",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpeechChoice {
    Auto,
    Transcribe,
    WhisperCli,
}

impl SpeechChoice {
    pub fn label(self) -> &'static str {
        match self {
            SpeechChoice::Auto => "Automatic",
            SpeechChoice::Transcribe => "Parakeet (transcribe runtime)",
            SpeechChoice::WhisperCli => "whisper.cpp program",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PromptTarget {
    #[default]
    Auto,
    ClaudeCode,
    General,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum PromptProvider {
    #[default]
    Local,
    Anthropic,
    OpenAi,
    Gemini,
}

impl PromptProvider {
    pub const ALL: [Self; 4] = [Self::Local, Self::Anthropic, Self::OpenAi, Self::Gemini];

    pub fn id(self) -> &'static str {
        match self { Self::Local => "local", Self::Anthropic => "anthropic", Self::OpenAi => "openai", Self::Gemini => "gemini" }
    }

    pub fn label(self) -> &'static str {
        match self { Self::Local => "Local model", Self::Anthropic => "Claude", Self::OpenAi => "OpenAI", Self::Gemini => "Gemini" }
    }

    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Local => &[], Self::Anthropic => &["ANTHROPIC_API_KEY"], Self::OpenAi => &["OPENAI_API_KEY"],
            Self::Gemini => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        }
    }

    pub fn model<'a>(self, local: &'a str, claude: &'a str) -> &'a str {
        match self { Self::Local => local, Self::Anthropic => claude, Self::OpenAi => "gpt-5-mini", Self::Gemini => "gemini-3.5-flash-lite" }
    }
}

impl PromptTarget {
    pub fn label(self) -> &'static str {
        match self {
            PromptTarget::Auto => "Auto (detect from draft)",
            PromptTarget::ClaudeCode => "Coding agent",
            PromptTarget::General => "General AI",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Destination {
    LastApp,
    Workspace,
}

impl Destination {
    pub fn label(self) -> &'static str {
        match self {
            Destination::LastApp => "Type into the app I was using",
            Destination::Workspace => "Add to my Prompt Lab draft",
        }
    }
    #[allow(dead_code)]
    pub fn short(self) -> &'static str {
        match self {
            Destination::LastApp => "Into last app",
            Destination::Workspace => "Into Prompt Lab",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub hotkey: Hotkey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered_hotkey: Option<String>,
    pub command_hotkey: Option<Hotkey>,
    pub microphone: String,
    pub destination: Destination,
    pub sounds: bool,
    pub floating_card: bool,
    /// Show words on the card while you speak (the original's `live_preview`, default on).
    pub live_preview: bool,
    pub restore_clipboard: bool,
    pub quiet_mode: bool,

    pub speech_engine: SpeechChoice,
    pub transcribe_dir: String,
    pub speech_model: String,
    pub whisper_cli: String,
    pub whisper_model: String,
    pub speech_threads: i32,

    pub cleanup: bool,
    pub style: Style,
    pub ollama_url: String,
    pub local_model: String,
    pub vocab: String,
    /// Suggested words the person dismissed (original: learned.json "dismissed").
    pub vocab_dismissed: Vec<String>,
    pub snippets: Vec<crate::snippets::Snippet>,
    pub auto_app_tone: bool,
    pub app_tone_rules: Vec<crate::app_tone::AppToneRule>,

    pub cloud_enabled: bool,
    pub claude_model: String,
    pub prompt_engine: PromptProvider,
    pub prompt_judge: PromptProvider,
    pub engine_local_model: String,
    pub judge_local_model: String,
    pub prompt_target: PromptTarget,
    pub learn: bool,

    pub theme: ThemeId,
    pub layout: LayoutId,
    pub zoom: f32,
}

pub const PARAKEET_REVISION: &str = "7e948f21b7bdbac698d3318db9d350f1096f3b6c";

/// Folder of the running program (where a packaged copy keeps its runtime and model).
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// The runtime packaged beside the program: `<exe>\runtime\transcribe-0.4.2-cpu-vulkan`.
pub fn bundled_transcribe_dir() -> Option<PathBuf> {
    let d = exe_dir()?.join("runtime").join("transcribe-0.4.2-cpu-vulkan");
    d.join("transcribe.dll").is_file().then_some(d)
}

/// The model packaged beside the program: `<exe>\models\parakeet-unified-en-0.6b-Q8_0.gguf`.
pub fn bundled_speech_model() -> Option<PathBuf> {
    let f = exe_dir()?.join("models").join("parakeet-unified-en-0.6b-Q8_0.gguf");
    f.is_file().then_some(f)
}

/// The runtime folder to use: a folder the person chose (with transcribe.dll), else the
/// packaged copy, else the original app's install. A saved setting that just names the
/// original's install (the old default) doesn't override the packaged copy.
pub fn effective_transcribe_dir(configured: &str) -> PathBuf {
    let c = PathBuf::from(configured);
    let chosen = !configured.is_empty() && !same_path(&c, &PathBuf::from(installed_transcribe_dir()));
    if chosen && c.join("transcribe.dll").is_file() {
        return c;
    }
    bundled_transcribe_dir().unwrap_or_else(|| PathBuf::from(installed_transcribe_dir()))
}

/// The model file to use: one the person chose, else the packaged copy, else the cache.
pub fn effective_speech_model(configured: &str) -> PathBuf {
    let c = PathBuf::from(configured);
    let chosen = !configured.is_empty() && !same_path(&c, &PathBuf::from(cached_speech_model()));
    if chosen && c.is_file() {
        return c;
    }
    bundled_speech_model().unwrap_or_else(|| PathBuf::from(cached_speech_model()))
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    let norm = |p: &std::path::Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase().replace('/', "\\");
    norm(a) == norm(b)
}

pub fn default_transcribe_dir() -> String {
    bundled_transcribe_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(installed_transcribe_dir)
}

fn installed_transcribe_dir() -> String {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    base.join("Listen").join("runtimes").join("transcribe-0.4.2-cpu-vulkan").to_string_lossy().into_owned()
}

pub fn default_speech_model() -> String {
    bundled_speech_model().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(cached_speech_model)
}

fn cached_speech_model() -> String {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    home.join(".cache")
        .join("huggingface")
        .join("hub")
        .join("models--handy-computer--parakeet-unified-en-0.6b-gguf")
        .join("snapshots")
        .join(PARAKEET_REVISION)
        .join("parakeet-unified-en-0.6b-Q8_0.gguf")
        .to_string_lossy()
        .into_owned()
}

fn default_speech_threads() -> i32 {
    let logical = std::thread::available_parallelism().map_or(4, |n| n.get() as i32).max(1);
    (logical / 2).clamp(1, 8).max(logical.min(4))
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: Hotkey::RightCtrl,
            registered_hotkey: None,
            command_hotkey: None,
            microphone: String::new(),
            destination: Destination::LastApp,
            sounds: true,
            floating_card: true,
            live_preview: true,
            restore_clipboard: true,
            quiet_mode: false,
            speech_engine: SpeechChoice::Auto,
            transcribe_dir: default_transcribe_dir(),
            speech_model: default_speech_model(),
            whisper_cli: String::new(),
            whisper_model: String::new(),
            speech_threads: default_speech_threads(),
            cleanup: true,
            style: Style::Tighten,
            ollama_url: "http://127.0.0.1:11434".into(),
            local_model: "qwen2.5:7b".into(),
            vocab: String::new(),
            vocab_dismissed: Vec::new(),
            snippets: Vec::new(),
            auto_app_tone: true,
            app_tone_rules: Vec::new(),
            cloud_enabled: false,
            claude_model: "claude-haiku-5-5".into(),
            prompt_engine: PromptProvider::Local,
            prompt_judge: PromptProvider::Local,
            engine_local_model: String::new(),
            judge_local_model: String::new(),
            prompt_target: PromptTarget::Auto,
            learn: true,
            theme: ThemeId::WarmCharcoal,
            layout: LayoutId::Desk,
            zoom: 1.0,
        }
    }
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let mut f = std::fs::File::create_new(&tmp)?;
    let written = (|| {
        f.write_all(bytes)?;
        f.sync_all()
    })();
    drop(f);
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn load_settings() -> Settings {
    std::fs::read_to_string(data_dir().join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    let _ = save_settings_checked(s);
}

pub(crate) fn save_settings_checked(s: &Settings) -> std::io::Result<()> {
    write_settings(&data_dir().join("settings.json"), s)
}

fn write_settings(path: &Path, settings: &Settings) -> std::io::Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(settings)?)
}

#[cfg(test)]
#[test]
fn checked_settings_replace_the_old_file_and_report_failure() {
    let root = std::env::temp_dir().join(format!("listen-rs-settings-test-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("settings.json");
    let mut settings = Settings::default();
    write_settings(&path, &settings).unwrap();
    settings.live_preview = false;
    write_settings(&path, &settings).unwrap();
    let saved: Settings = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!saved.live_preview);
    assert!(write_settings(&root.join("missing").join("settings.json"), &settings).is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();

    // Backward compatibility: legacy JSON missing all new fields deserializes with safe defaults.
    let legacy: Settings = serde_json::from_str(r#"{"hotkey":"RightCtrl","cleanup":true}"#).unwrap();
    assert!(!legacy.quiet_mode);
    assert_eq!(legacy.command_hotkey, None);
    assert!(legacy.snippets.is_empty());
    assert!(legacy.auto_app_tone);
    assert!(legacy.app_tone_rules.is_empty());
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct HistoryEntry {
    pub when: String,
    pub unix: u64,
    pub raw: String,
    pub cleaned: String,
    pub seconds: f32,
    pub processing_secs: f32,
    pub engine: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub app_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_icon: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
}

impl HistoryEntry {
    pub fn text(&self) -> &str {
        if self.cleaned.is_empty() {
            &self.raw
        } else {
            &self.cleaned
        }
    }
}

pub fn load_history() -> Vec<HistoryEntry> {
    flush_history_updates();
    read_history_rows(&data_dir().join("history.jsonl"))
}

pub fn record_api_attempt() -> std::io::Result<()> {
    let (date, _) = now_stamp();
    let month = date.get(..7).ok_or_else(|| std::io::Error::other("Date unavailable"))?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(data_dir().join("api-attempts.txt"))?;
    writeln!(file, "{month}")
}

pub fn unpriced_api_attempts() -> usize {
    let (date, _) = now_stamp();
    let Some(month) = date.get(..7) else { return 0 };
    std::fs::read_to_string(data_dir().join("api-attempts.txt")).map_or(0, |text| text.lines().filter(|line| *line == month).count())
}

fn read_history_rows(path: &Path) -> Vec<HistoryEntry> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    bytes.split(|byte| *byte == b'\n').filter_map(|row| serde_json::from_slice(row).ok()).collect()
}

/// Vocabulary words already listed (split on commas, new lines, and semicolons).
pub fn vocab_words(vocab: &str) -> Vec<String> {
    vocab
        .split([',', '\n', ';'])
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Lowercase stop-words excluded from automatic vocabulary suggestions (matched case-insensitively).
const SUGGEST_STOP: &[&str] = &[
    "i", "i'm", "i'll", "i've", "i'd", "ok", "okay", "claude", "ai", "api",
    "am", "pm", "us", "uk", "eu", "ui", "ux", "os", "id", "tv", "pc", "hr", "pr", "qa",
    "the", "a", "an", "and", "but", "or", "nor", "for", "yet", "so", "then", "if",
    "when", "what", "why", "how", "who", "where", "which", "also", "yes", "no", "not",
    "it", "is", "in", "on", "at", "to", "of", "by", "as", "be", "do", "go", "we", "he", "me", "my", "up",
    "all", "are", "was", "were", "has", "had", "have", "can", "will", "would", "could", "should", "must",
    "might", "shall", "this", "that", "these", "those", "they", "them", "their", "there", "here",
    "you", "your", "our", "out", "now", "new", "one", "two", "too", "its", "his", "her", "him", "she",
    "from", "with", "into", "over", "just", "like", "more", "some", "such", "very", "only",
    "please", "thanks", "thank", "hello", "hey", "hi", "dear", "best", "regards", "cheers",
    "fyi", "tbd", "tba", "eta", "asap", "usa", "utc", "gmt", "est", "pst", "cst", "mst",
    "ceo", "cto", "cfo",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    "january", "february", "march", "april", "may", "june", "july", "august", "september",
    "october", "november", "december",
];

fn is_shouting_entry(text: &str) -> bool {
    let mut alpha = 0usize;
    let mut upper = 0usize;
    let mut words = 0usize;
    let mut all_upper_words = 0usize;
    for raw_word in text.split_whitespace() {
        let mut w_alpha = 0usize;
        let mut w_upper = 0usize;
        for c in raw_word.chars() {
            if c.is_alphabetic() {
                w_alpha += 1;
                if c.is_uppercase() {
                    w_upper += 1;
                }
            }
        }
        if w_alpha > 0 {
            alpha += w_alpha;
            upper += w_upper;
            words += 1;
            if w_alpha >= 2 && w_upper == w_alpha {
                all_upper_words += 1;
            }
        }
    }
    words >= 2 && all_upper_words >= 2 && upper * 5 >= alpha * 4
}

/// Capitalised words and acronyms seen mid-sentence at least twice in dictation history,
/// minus known and dismissed words, case-insensitively merged (choosing the most frequent
/// casing), most frequent first, at most `limit`.
pub fn suggest_vocab(history: &[HistoryEntry], vocab: &str, dismissed: &[String], limit: usize) -> Vec<String> {
    let mut known: std::collections::HashSet<String> = vocab_words(vocab).iter().map(|w| w.to_lowercase()).collect();
    known.extend(dismissed.iter().map(|w| w.to_lowercase()));
    // Map lowercase key -> (exact casing variant -> frequency)
    let mut counts: std::collections::HashMap<String, std::collections::HashMap<String, usize>> =
        std::collections::HashMap::new();
    for e in history {
        if e.kind == "command" {
            continue;
        }
        let text = e.text();
        if is_shouting_entry(text) {
            continue;
        }
        let mut start_of_sentence = true;
        let mut tok = String::new();
        let mut flush = |tok: &mut String, sos: &mut bool| {
            let trimmed = tok.trim_end_matches(['\'', '’', '-']);
            if !trimmed.is_empty() {
                let char_len = trimmed.chars().count();
                if !*sos && char_len >= 2 {
                    let first_upper = trimmed.chars().next().is_some_and(|c| c.is_uppercase());
                    let all_upper = trimmed.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());
                    let acronym_ok = !all_upper || char_len >= 3;
                    if first_upper && acronym_ok {
                        let lower = trimmed.to_lowercase();
                        if !SUGGEST_STOP.contains(&lower.as_str()) {
                            *counts.entry(lower).or_default().entry(trimmed.to_owned()).or_default() += 1;
                        }
                    }
                }
                *sos = false;
            }
            tok.clear();
        };
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let ok = if tok.is_empty() {
                c.is_alphabetic()
            } else {
                c.is_alphanumeric() || c == '\'' || c == '’' || c == '-'
            };
            if ok {
                tok.push(c);
            } else {
                flush(&mut tok, &mut start_of_sentence);
                if c.is_numeric() {
                    start_of_sentence = false;
                } else if matches!(c, '!' | '?' | '\n') {
                    start_of_sentence = true;
                } else if c == '.' && !chars.peek().is_some_and(|next| next.is_alphanumeric()) {
                    start_of_sentence = true;
                }
            }
        }
        flush(&mut tok, &mut start_of_sentence);
    }
    let mut out: Vec<(String, usize)> = counts
        .into_iter()
        .filter_map(|(lower, forms)| {
            if known.contains(&lower) {
                return None;
            }
            let total: usize = forms.values().sum();
            if total < 2 {
                return None;
            }
            let best_form = forms
                .into_iter()
                .max_by(|(a_form, a_cnt), (b_form, b_cnt)| {
                    a_cnt.cmp(b_cnt).then_with(|| b_form.cmp(a_form))
                })
                .map(|(form, _)| form)?;
            Some((best_form, total))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.into_iter().take(limit).map(|(t, _)| t).collect()
}

enum HistoryJob {
    UpdateOutcome {
        path: PathBuf,
        entry: HistoryEntry,
        outcome: String,
    },
    Flush(mpsc::SyncSender<()>),
}

static HISTORY_WRITER: OnceLock<mpsc::Sender<HistoryJob>> = OnceLock::new();

fn history_writer() -> &'static mpsc::Sender<HistoryJob> {
    HISTORY_WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<HistoryJob>();
        let _ = std::thread::Builder::new()
            .name("history-writer".into())
            .spawn(move || {
                for job in rx {
                    match job {
                        HistoryJob::UpdateOutcome { path, entry, outcome } => {
                            let _ = replace_history_outcome(&path, &entry, &outcome);
                        }
                        HistoryJob::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        tx
    })
}

pub fn flush_history_updates() {
    if let Some(tx) = HISTORY_WRITER.get() {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        if tx.send(HistoryJob::Flush(done_tx)).is_ok() {
            let _ = done_rx.recv();
        }
    }
}

pub fn append_history(e: &HistoryEntry) -> std::io::Result<()> {
    flush_history_updates();
    append_history_row(&data_dir().join("history.jsonl"), e)
}

pub fn update_latest_history_outcome(entry: &HistoryEntry, outcome: &str) -> std::io::Result<()> {
    flush_history_updates();
    replace_history_outcome(&data_dir().join("history.jsonl"), entry, outcome)
}

pub fn update_latest_history_outcome_async(entry: HistoryEntry, outcome: String) {
    let path = data_dir().join("history.jsonl");
    let _ = history_writer().send(HistoryJob::UpdateOutcome { path, entry, outcome });
}

fn replace_history_outcome(path: &Path, entry: &HistoryEntry, outcome: &str) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind, Read};
    const REWRITE_LIMIT: u64 = 8 * 1024 * 1024;
    let mut history = Vec::new();
    std::fs::File::open(path)?.take(REWRITE_LIMIT + 1).read_to_end(&mut history)?;
    let mut original = serde_json::to_vec(entry)?;
    original.push(b'\n');
    if history.len() as u64 > REWRITE_LIMIT || !history.ends_with(&original)
        || (history.len() > original.len() && history[history.len() - original.len() - 1] != b'\n')
    {
        return Err(Error::new(ErrorKind::InvalidData, "Could not confirm the latest History row"));
    }
    history.truncate(history.len() - original.len());
    let mut updated = entry.clone();
    updated.outcome = outcome.into();
    serde_json::to_writer(&mut history, &updated)?;
    history.push(b'\n');
    atomic_write(path, &history)
}

fn append_history_row(path: &Path, entry: &HistoryEntry) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut bytes = serde_json::to_vec(entry)?;
    bytes.push(b'\n');
    let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
    if file.metadata()?.len() > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            // Keep a possibly interrupted old row separate from the new transcript.
            bytes.insert(0, b'\n');
        }
    }
    file.write_all(&bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod history_tests {
    use super::*;

    #[test]
    fn appending_preserves_old_rows_and_isolates_truncated_tail() {
        let dir = std::env::temp_dir().join(format!("listen-rs-history-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("history.jsonl");
        let sample = HistoryEntry { raw: "Test dictation".into(), ..Default::default() };
        let interrupted = b"{\"raw\":\"older text\"}\n{\"raw\":\"\xf0\x9f";
        std::fs::write(&file, interrupted).unwrap();
        append_history_row(&file, &sample).unwrap();
        let data = std::fs::read(&file).unwrap();
        assert!(data.starts_with(interrupted));
        let readable: Vec<_> = read_history_rows(&file).into_iter().map(|row| row.raw).collect();
        assert_eq!(readable, vec!["older text".to_string(), sample.raw.clone()]);
        let unrelated = HistoryEntry { raw: "Another entry".into(), ..sample.clone() };
        assert!(replace_history_outcome(&file, &unrelated, "paste confirmed").is_err());
        assert_eq!(std::fs::read(&file).unwrap(), data);
        replace_history_outcome(&file, &sample, "paste confirmed").unwrap();
        assert_eq!(read_history_rows(&file).last().unwrap().outcome, "paste confirmed");
        assert!(std::fs::read(&file).unwrap().starts_with(interrupted));
        assert!(replace_history_outcome(&file, &sample, "paste confirmed").is_err());
        assert!(append_history_row(&dir.join("missing").join("history.jsonl"), &sample).is_err());
        // Verify background outcome updater flushes cleanly in order before readback.
        let sample2 = HistoryEntry { raw: "Async outcome test".into(), outcome: "saved · delivery not confirmed".into(), ..Default::default() };
        append_history_row(&file, &sample2).unwrap();
        let _ = history_writer().send(HistoryJob::UpdateOutcome {
            path: file.clone(),
            entry: sample2,
            outcome: "paste confirmed".into(),
        });
        flush_history_updates();
        assert_eq!(read_history_rows(&file).last().unwrap().outcome, "paste confirmed");
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}

fn remove_history_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn clear_history() -> std::io::Result<()> {
    flush_history_updates();
    remove_history_file(&data_dir().join("history.jsonl"))
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Favorite {
    pub name: String,
    pub text: String,
    pub when: String,
    /// How many times it was copied from the tray menu (the original shows "name  (uses)").
    pub uses: u32,
}

pub fn load_favorites() -> Vec<Favorite> {
    std::fs::read_to_string(data_dir().join("favorites.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_favorites(f: &[Favorite]) {
    if let Ok(json) = serde_json::to_vec_pretty(f) {
        let _ = atomic_write(&data_dir().join("favorites.json"), &json);
    }
}

pub fn load_draft() -> String {
    std::fs::read_to_string(data_dir().join("draft.txt")).unwrap_or_default()
}

pub fn save_draft(s: &str) -> std::io::Result<()> {
    atomic_write(&data_dir().join("draft.txt"), s.as_bytes())
}

/// Local time as "YYYY-MM-DD HH:MM" plus Unix seconds.
pub fn now_stamp() -> (String, u64) {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        let mut t = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut t) };
        let s = format!("{:04}-{:02}-{:02} {:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute);
        (s, unix)
    }
    #[cfg(not(windows))]
    {
        (format!("t+{unix}"), unix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> HistoryEntry {
        HistoryEntry {
            raw: text.into(),
            cleaned: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn suggest_vocab_unicode_semicolons_acronyms_and_sos() {
        // Semicolon-delimited vocab parsed consistently alongside commas and newlines.
        assert_eq!(
            vocab_words("Alpha; Beta, Gamma\nDelta ;  Epsilon "),
            vec!["Alpha", "Beta", "Gamma", "Delta", "Epsilon"]
        );

        let history = vec![
            // Unicode proper nouns mid-sentence
            entry("We met at Café yesterday."),
            entry("The Café served great coffee."),
            entry("Take an Über home."),
            entry("Book an Über now."),
            // Case-insensitive count merging choosing most-frequent casing
            entry("Push to GitHub today."),
            entry("Open GitHub PR."),
            entry("Check Github issues."),
            // Acronyms >= 3 chars suggested; 2-char & stop-list excluded
            entry("Run CUDA kernels with WASM support."),
            entry("Optimize CUDA and WASM builds."),
            entry("Meet at AM and PM with OK and US and UI and OS and ID and AI and API and THE and AND."),
            entry("Again AM and PM with OK and US and UI and OS and ID and AI and API and THE and AND."),
            // Per-entry shouting guard skips all-caps shouting utterances
            entry("DO NOT DELETE THIS FILE"),
            entry("DO NOT DELETE THIS FILE"),
            // 1-char starter ("I Parakeet") and internal-dot non-reset ("Check CLAUDE.md and Parakeet")
            entry("I Parakeet daily."),
            entry("Check CLAUDE.md and Parakeet."),
            // Semicolon-separated known vocab entry should be excluded
            entry("Deploy Kubernetes now."),
            entry("Scale Kubernetes cluster."),
            // Command Mode entries must be skipped completely
            HistoryEntry {
                raw: "Rewrite using CommandOnlyWord.".into(),
                cleaned: "Rewrite using CommandOnlyWord.".into(),
                kind: "command".into(),
                ..Default::default()
            },
            HistoryEntry {
                raw: "Make CommandOnlyWord concise.".into(),
                cleaned: "Make CommandOnlyWord concise.".into(),
                kind: "command".into(),
                ..Default::default()
            },
        ];

        let suggestions = suggest_vocab(&history, "Existing; Kubernetes", &[], 20);

        assert!(suggestions.contains(&"Café".to_string()), "expected Café in {suggestions:?}");
        assert!(suggestions.contains(&"Über".to_string()), "expected Über in {suggestions:?}");
        assert!(suggestions.contains(&"GitHub".to_string()), "expected GitHub in {suggestions:?}");
        assert!(!suggestions.contains(&"Github".to_string()), "Github should merge into GitHub: {suggestions:?}");
        assert!(suggestions.contains(&"CUDA".to_string()), "expected CUDA in {suggestions:?}");
        assert!(suggestions.contains(&"WASM".to_string()), "expected WASM in {suggestions:?}");
        assert!(suggestions.contains(&"Parakeet".to_string()), "expected Parakeet in {suggestions:?}");

        for excluded in [
            "Kubernetes", "CommandOnlyWord", "AM", "PM", "OK", "US", "UI", "OS", "ID", "AI", "API",
            "THE", "AND", "DO", "NOT", "DELETE", "THIS", "FILE",
        ] {
            assert!(
                !suggestions.iter().any(|s| s.eq_ignore_ascii_case(excluded)),
                "unexpected stop/shouting/known/command word {excluded} in {suggestions:?}"
            );
        }

        // Internal-dot non-reset even when the capitalized word immediately follows `CLAUDE.md`.
        let dot_history = vec![
            entry("Check CLAUDE.md Parakeet."),
            entry("Check v2.0 Parakeet."),
        ];
        assert_eq!(suggest_vocab(&dot_history, "", &[], 10), vec!["Parakeet".to_string()]);

        // HistoryEntry.kind backward compatibility: omitted on legacy JSON & empty serialization.
        let legacy: HistoryEntry = serde_json::from_str(r#"{"raw":"hello"}"#).unwrap();
        assert_eq!(legacy.kind, "");
        let legacy_json = serde_json::to_string(&legacy).unwrap();
        assert!(!legacy_json.contains("\"kind\""));
        let cmd = HistoryEntry { raw: "fix".into(), kind: "command".into(), ..Default::default() };
        let cmd_json = serde_json::to_string(&cmd).unwrap();
        assert!(cmd_json.contains("\"kind\":\"command\""));
        let cmd_roundtrip: HistoryEntry = serde_json::from_str(&cmd_json).unwrap();
        assert_eq!(cmd_roundtrip.kind, "command");

        // remove_history_file: Ok(()) on existing file and NotFound, Err on real filesystem failure.
        let dir = std::env::temp_dir().join(format!("listen-rs-clear-history-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("history.jsonl");
        std::fs::write(&file, b"{\"raw\":\"hi\"}\n").unwrap();
        assert!(remove_history_file(&file).is_ok());
        assert!(!file.exists());
        assert!(remove_history_file(&file).is_ok(), "NotFound should return Ok(())");
        let blocker_dir = dir.join("not-a-file");
        std::fs::create_dir(&blocker_dir).unwrap();
        assert!(remove_history_file(&blocker_dir).is_err(), "real remove_file error should propagate");
        std::fs::remove_dir(&blocker_dir).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
