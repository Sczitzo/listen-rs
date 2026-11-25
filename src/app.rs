//! Pure-Rust native `egui` desktop shell for Listen.
//!
//! Coordinates the 5 pages (`Dictate`, `Prompt Lab`, `Meetings`, `History`, `Settings`),
//! 3 layouts (`Writer's desk`, `Sidebar`, `Compact`), 12 WCAG AA themes, Command Palette (`Ctrl+K`),
//! native floating Lantern pill, system tray close-to-tray lifecycle, and local screenshot/UI checks.

use crate::cleanup;
use crate::engine::{Cmd, Core, EngineState, Phase, UiEvent};
use crate::meeting::MeetingNote;
use crate::storage::{
    self, Destination, Favorite, HistoryEntry, PromptProvider, Settings,
};
use crate::theme::{self, blend, LayoutId, Palette, ThemeId};
use crate::ui::{
    self, ArchivedDraftItem, HistoryStatsView, HistoryUiState, HotkeyModalState,
    LabAssessmentView, LabCandidateView, LabUiState, MeetingUiState, ModelStatusSnapshot,
    SettingsSection, SettingsUiState, UiAction, UiView,
};
use crate::{audio, lantern, models, winput};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Frame, Key, Layout, Margin, Modifiers, RichText, Stroke, Ui, ViewportCommand,
};
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Dictate,
    Lab,
    Meetings,
    History,
    Settings,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Page::Dictate,
        Page::Lab,
        Page::Meetings,
        Page::History,
        Page::Settings,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Page::Dictate => "Dictate",
            Page::Lab => "Prompt Lab",
            Page::Meetings => "Meetings",
            Page::History => "History",
            Page::Settings => "Settings",
        }
    }
    pub fn from_key(k: &str) -> Option<Page> {
        match k.to_ascii_lowercase().as_str() {
            "dictate" => Some(Page::Dictate),
            "lab" | "prompt" | "promptlab" | "workspace" => Some(Page::Lab),
            "meetings" | "meeting" => Some(Page::Meetings),
            "history" => Some(Page::History),
            "settings" => Some(Page::Settings),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct LaunchOptions {
    pub ui_check: Option<PathBuf>,
    pub hardware_trial: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub theme: Option<ThemeId>,
    pub layout: Option<LayoutId>,
    pub page: Option<Page>,
    pub settings_section: Option<String>,
    pub demo_recording: bool,
    pub demo_lantern: bool,
    pub launch_target: Option<winput::Target>,
}

#[derive(Clone)]
pub struct Snap {
    pub phase: Phase,
    pub started: Option<Instant>,
    pub generation: u64,
    pub anchor: isize,
    pub engine: EngineState,
    pub status: String,
    pub lab_busy: bool,
    pub live_text: String,
    pub live_note: String,
    pub outcome: Option<String>,
}

const DEMO_LIVE: &str = "Okay so for the release notes I want three short sections, what changed, what got faster, and what people should try first. Keep it friendly and";
const METER_LEN: usize = 220;
const RAIL_H: f32 = 60.0;

#[derive(Clone)]
enum PCmd {
    Go(Page),
    Theme(ThemeId),
    Layout(LayoutId),
    ToggleCleanup,
    ToggleQuietMode,
    ToggleDestination,
    Start,
    Finish,
    Cancel,
    PasteDraft,
    ReloadEngine,
}

pub struct ListenApp {
    core: Arc<Core>,
    rx: Receiver<UiEvent>,
    workspace: crate::workspace::Workspace,
    installer: Arc<models::InstallManager>,
    benchmarker: Arc<models::BenchmarkManager>,

    settings: Settings,
    saved: Settings,
    applied: Option<(ThemeId, u32)>,
    last_layout: LayoutId,
    page: Page,

    history: Vec<HistoryEntry>,
    favorites: Vec<Favorite>,
    retained: Vec<(String, String)>,
    workspace_retained: Vec<(u64, String, String)>,
    vocab_buf: String,
    show_how: bool,
    recent_open: bool,

    lab_ui: LabUiState,
    lab_edit_id: i64,
    lab_composer_edit_id: i64,
    meeting_ui: MeetingUiState,
    history_ui: HistoryUiState,
    settings_ui: SettingsUiState,
    hotkey_modal: HotkeyModalState,
    hotkey_lease: Option<(String, Instant)>,
    lease_count: u64,

    draft_change_stamp: u64,
    launch_target: Option<winput::Target>,
    palette_open: bool,
    palette_query: String,
    levels: VecDeque<f32>,
    models: Vec<String>,
    ollama_available: bool,
    mics: Vec<String>,

    ui_check: Option<PathBuf>,
    ui_check_step_idx: usize,
    screenshot: Option<PathBuf>,
    frames: u32,
    shot_requested: bool,
    toast: Option<(String, Instant)>,
    snap: Snap,
    demo_recording: bool,
    demo_lantern: bool,
    lantern: lantern::State,
    lantern_animation: bool,
    hwnd: isize,
    tray: Option<crate::tray::Tray>,
    quitting: bool,
    hide_test: Option<(Instant, u32)>,
}

impl ListenApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mut settings: Settings, opts: LaunchOptions) -> Self {
        install_fonts(&cc.egui_ctx);
        let preview = opts.screenshot.is_some() || opts.ui_check.is_some();
        if opts.screenshot.is_some() {
            settings.theme = ThemeId::WarmCharcoal;
            settings.layout = LayoutId::Desk;
        }
        if let Some(t) = opts.theme {
            settings.theme = t;
        }
        if let Some(l) = opts.layout {
            settings.layout = l;
        }
        if preview {
            settings.zoom = 1.0;
        }

        let profile = opts
            .ui_check
            .clone()
            .or_else(|| opts.screenshot.as_ref().map(|f| f.with_extension("profile")))
            .unwrap_or_else(storage::data_dir);

        let (core, rx) = Core::new(settings.clone(), preview);
        core.set_ctx(cc.egui_ctx.clone());
        if opts.screenshot.is_some() && opts.demo_recording {
            let mut live = core.shared.lock().unwrap();
            live.phase = Phase::Recording;
            live.rec_gen += 1;
            live.started = Some(Instant::now());
            live.live_text = DEMO_LIVE.into();
            core.level.store(0.08_f32.to_bits(), Ordering::Relaxed);
        }
        if !preview {
            winput::spawn_hotkey_thread(core.clone());
        }
        theme::apply(&cc.egui_ctx, &settings.theme.palette(), settings.zoom);

        let hwnd: isize = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            cc.window_handle()
                .ok()
                .and_then(|h| match h.as_raw() {
                    RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
                    _ => None,
                })
                .unwrap_or(0)
        };

        let favorites = if preview { Vec::new() } else { storage::load_favorites() };
        let tray = if preview {
            None
        } else {
            let ctx = cc.egui_ctx.clone();
            match crate::tray::Tray::new(&favorites, move || {
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }) {
                Ok(t) => Some(t),
                Err(e) => {
                    eprintln!("tray icon unavailable: {e}");
                    None
                }
            }
        };

        let history = if preview {
            let (when, unix) = storage::now_stamp();
            vec![
                HistoryEntry {
                    when: "2026-11-24 14:15".into(),
                    unix: unix.saturating_sub(86400),
                    raw: "Ship the pure Rust release once all screenshots and unit tests pass.".into(),
                    cleaned: "Ship the pure Rust release once all screenshots and unit tests pass.".into(),
                    seconds: 4.2,
                    processing_secs: 0.4,
                    engine: "Parakeet".into(),
                    outcome: "paste confirmed".into(),
                    app_name: "VS Code".into(),
                    app_icon: None,
                },
                HistoryEntry {
                    when,
                    unix,
                    raw: "Let's meet at 3 PM to review the native Rust UI across all 12 themes.".into(),
                    cleaned: "Let's meet at 3 PM to review the native Rust UI across all 12 themes.".into(),
                    seconds: 3.8,
                    processing_secs: 0.3,
                    engine: "Parakeet".into(),
                    outcome: "paste confirmed".into(),
                    app_name: "Discord".into(),
                    app_icon: None,
                },
            ]
        } else {
            storage::load_history()
        };

        let mut settings_ui = SettingsUiState::default();
        if let Some(sec) = opts.settings_section.as_deref() {
            settings_ui.section = match sec.to_ascii_lowercase().as_str() {
                "voice" | "voice_ai" | "ai" => SettingsSection::VoiceAi,
                "snippets" => SettingsSection::Snippets,
                "appearance" | "themes" => SettingsSection::Appearance,
                "lab" | "prompt" | "prompt_lab" => SettingsSection::PromptLab,
                "system" => SettingsSection::System,
                _ => SettingsSection::General,
            };
        }

        let mut meeting_ui = MeetingUiState {
            include_system_audio: true,
            ..Default::default()
        };
        if preview {
            meeting_ui.notes = vec![MeetingNote {
                id: 1,
                ts: "2026-11-25 10:00".into(),
                title: "Weekly Product Sync (Sample)".into(),
                duration_secs: 185,
                loopback_used: true,
                summary: "Reviewed the pure-Rust migration checklist and verified local speech performance across all 12 WCAG AA themes.".into(),
                action_items: vec![
                    "Verify dark and light theme contrast in Meetings".into(),
                    "Run full offline verification suite before tagging".into(),
                ],
                transcript: "We reviewed the release checklist, verified local Parakeet latency, and confirmed zero WebView2 dependencies.".into(),
            }];
        }

        let vocab_buf = settings.vocab.clone();
        let layout = settings.layout;
        let workspace = crate::workspace::Workspace::load(&profile);

        let mut app = ListenApp {
            core,
            rx,
            workspace,
            installer: Arc::new(models::InstallManager::new()),
            benchmarker: Arc::new(models::BenchmarkManager::new()),
            saved: settings.clone(),
            applied: Some((settings.theme, settings.zoom.to_bits())),
            last_layout: layout,
            settings,
            page: opts.page.unwrap_or(Page::Dictate),
            history,
            favorites,
            retained: Vec::new(),
            workspace_retained: Vec::new(),
            vocab_buf,
            show_how: false,
            recent_open: false,
            lab_ui: LabUiState::default(),
            lab_edit_id: 0,
            lab_composer_edit_id: 0,
            meeting_ui,
            history_ui: HistoryUiState::default(),
            settings_ui,
            hotkey_modal: HotkeyModalState::default(),
            hotkey_lease: None,
            lease_count: 0,
            draft_change_stamp: 0,
            launch_target: opts.launch_target,
            palette_open: false,
            palette_query: String::new(),
            levels: VecDeque::from(vec![0.0; METER_LEN]),
            models: if preview { vec!["qwen3.5:2b".into(), "qwen2.5:7b".into()] } else { Vec::new() },
            ollama_available: preview,
            mics: if preview { vec!["Default Microphone (Simulated)".into()] } else { audio::input_devices() },
            ui_check: opts.ui_check,
            ui_check_step_idx: 0,
            screenshot: opts.screenshot,
            frames: 0,
            shot_requested: false,
            toast: None,
            snap: Snap {
                phase: Phase::Idle,
                started: None,
                generation: 0,
                anchor: 0,
                engine: EngineState::Loading,
                status: String::new(),
                lab_busy: false,
                live_text: String::new(),
                live_note: String::new(),
                outcome: None,
            },
            demo_recording: preview && opts.demo_recording,
            demo_lantern: preview && opts.demo_lantern,
            lantern: lantern::State::default(),
            lantern_animation: lantern::animations_enabled(),
            hwnd,
            tray,
            quitting: false,
            hide_test: None,
        };
        app.sync_lab_ui();
        if !preview {
            app.refresh_local_models_async();
        }
        app
    }

    fn preview(&self) -> bool {
        self.core.preview
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn refresh_local_models_async(&mut self) {
        let core = self.core.clone();
        let url = self.settings.ollama_url.clone();
        let current = self.settings.local_model.clone();
        let _ = std::thread::Builder::new().name("ollama-models".into()).spawn(move || {
            let (_, models_opt, _) = crate::settings_bridge::local_models_response(false, &url, current);
            if let Some(list) = models_opt {
                core.emit(UiEvent::Models(list));
            }
        });
    }

    fn hotkey_display_parts(&self) -> (String, Vec<String>) {
        if let Some(reg) = self.settings.registered_hotkey.as_deref() {
            let parts: Vec<String> = reg
                .split('+')
                .map(|part| match part.to_ascii_lowercase().as_str() {
                    "ctrl" => "Ctrl".into(),
                    "alt" => "Alt".into(),
                    "shift" => "Shift".into(),
                    "arrow_right" => "→".into(),
                    "arrow_left" => "←".into(),
                    "arrow_up" => "↑".into(),
                    "arrow_down" => "↓".into(),
                    "space" => "Space".into(),
                    "enter" => "Enter".into(),
                    other => other.to_uppercase(),
                })
                .collect();
            let readable: Vec<String> = reg
                .split('+')
                .map(|part| match part.to_ascii_lowercase().as_str() {
                    "ctrl" => "Ctrl".into(),
                    "alt" => "Alt".into(),
                    "shift" => "Shift".into(),
                    "arrow_right" => "Right Arrow".into(),
                    "arrow_left" => "Left Arrow".into(),
                    "arrow_up" => "Up Arrow".into(),
                    "arrow_down" => "Down Arrow".into(),
                    other => other.to_uppercase(),
                })
                .collect();
            (readable.join(" + "), parts)
        } else {
            let lbl = self.settings.hotkey.label().to_owned();
            (lbl.clone(), vec![lbl])
        }
    }

    fn sync_lab_ui(&mut self) {
        let Some(snap) = self.workspace.call("get_lab", &[]) else { return };
        if let Some(eid) = snap.get("edit_id").and_then(Value::as_i64) {
            self.lab_edit_id = eid;
        }
        if let Some(cid) = snap.get("composer_edit_id").and_then(Value::as_i64) {
            self.lab_composer_edit_id = cid;
        }
        if let Some(text) = snap.get("text").and_then(Value::as_str) {
            if self.lab_ui.draft != text {
                self.lab_ui.draft = text.to_owned();
            }
        }
        self.core.shared.lock().unwrap().active_draft =
            Some((self.workspace.context(), self.draft_change_stamp));

        self.lab_ui.saved_note = if self.workspace.has_unsaved_recovery() {
            "Recovery pending".into()
        } else if snap.get("saved").and_then(Value::as_bool) == Some(false) {
            snap.get("save_error").and_then(Value::as_str).unwrap_or("Save failed").to_owned()
        } else {
            "Saved locally".into()
        };

        // Revisions ("revs" + "cur")
        self.lab_ui.revisions.clear();
        if let Some(arr) = snap.get("revs").and_then(Value::as_array) {
            for (idx, item) in arr.iter().enumerate() {
                let kind = item.get("kind").and_then(Value::as_str).unwrap_or("edit");
                let score_suffix = item
                    .get("score")
                    .and_then(Value::as_f64)
                    .map(|s| format!(" ({:.0}/10)", s.round()))
                    .unwrap_or_default();
                self.lab_ui.revisions.push((idx, format!("v{} · {kind}{score_suffix}", idx + 1)));
            }
        }
        self.lab_ui.current_rev = snap
            .get("cur")
            .and_then(Value::as_i64)
            .filter(|n| *n >= 0)
            .map(|n| n as usize)
            .unwrap_or_else(|| self.lab_ui.revisions.len().saturating_sub(1));
        self.lab_ui.version_label = if self.lab_ui.revisions.is_empty() {
            String::new()
        } else {
            format!("v{} of {}", self.lab_ui.current_rev + 1, self.lab_ui.revisions.len())
        };

        // Assessment + prev_score + error
        let mut assessment = snap.get("score").and_then(parse_assessment_view);
        if let Some(a) = assessment.as_mut() {
            a.prev_score = snap.get("prev_score").and_then(Value::as_f64).map(|n| n.round() as u32);
        }
        self.lab_ui.assessment = assessment;
        if let Some(err) = snap.get("error").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            self.lab_ui.error = Some(err.to_owned());
        }

        // Pending candidate ("pending": { id, source_text, kind, versions: [...] })
        let had_candidate = self.lab_ui.candidate.is_some();
        self.lab_ui.candidate = snap.get("pending").and_then(|c| {
            if c.is_null() {
                return None;
            }
            let id = c.get("id").and_then(Value::as_u64)?;
            let versions = c.get("versions").and_then(Value::as_array)?;
            let version_idx = versions.len().checked_sub(1)?;
            let latest = versions.get(version_idx)?;
            let suggestion = latest.get("text").and_then(Value::as_str)?.to_owned();
            Some(LabCandidateView {
                id,
                version_idx,
                original: c.get("source_text").and_then(Value::as_str).unwrap_or(&self.lab_ui.draft).to_owned(),
                suggestion,
                label: c.get("kind").and_then(Value::as_str).unwrap_or("Suggestion").to_owned(),
                model: latest.get("by").and_then(Value::as_str).unwrap_or("").to_owned(),
                note: latest.get("note").and_then(Value::as_str).unwrap_or("").to_owned(),
                assessment: latest.get("score").and_then(parse_assessment_view),
            })
        });
        if !had_candidate && self.lab_ui.candidate.is_some() {
            self.lab_ui.busy_label.clear();
        }
        let busy_str = snap.get("busy").and_then(Value::as_str).unwrap_or("");
        if !busy_str.is_empty() {
            if self.lab_ui.busy_label.is_empty() {
                self.lab_ui.busy_label = format!("Running {busy_str}…");
            }
        } else if !self.workspace.busy() {
            self.lab_ui.busy_label.clear();
        }

        // Workspace retained recovery entries
        self.workspace_retained.clear();
        if let Some(Value::Array(rows)) = self.workspace.call("get_retained", &[]) {
            for row in rows {
                if let (Some(id), Some(text), Some(msg)) = (
                    row.get("id").and_then(Value::as_u64),
                    row.get("text").and_then(Value::as_str),
                    row.get("message").and_then(Value::as_str),
                ) {
                    self.workspace_retained.push((id, text.to_owned(), msg.to_owned()));
                }
            }
        }

        // Archived drafts
        self.lab_ui.archived.clear();
        if let Some(Value::Array(rows)) = self.workspace.call("list_drafts", &[]) {
            for row in rows {
                if let (Some(id), Some(title)) = (
                    row.get("id").and_then(Value::as_str),
                    row.get("title").and_then(Value::as_str),
                ) {
                    self.lab_ui.archived.push(ArchivedDraftItem {
                        id: id.to_owned(),
                        label: title.to_owned(),
                    });
                }
            }
        }
    }

    fn save_lab_text(&mut self, text: &str) {
        self.lab_edit_id += 1;
        let ctx_id = self.workspace.context();
        let _ = self.workspace.call("set_text", &[json!(text), json!(ctx_id), json!(self.lab_edit_id)]);
        self.core.shared.lock().unwrap().active_draft = Some((ctx_id, self.draft_change_stamp));
    }

    fn save_lab_composer(&mut self, instr: &str) {
        self.lab_composer_edit_id += 1;
        let ctx_id = self.workspace.context();
        let _ = self.workspace.call(
            "set_composer",
            &[json!(instr), json!(ctx_id), json!(self.lab_composer_edit_id)],
        );
    }

    fn sync_meetings_ui(&mut self) {
        if self.preview() {
            return;
        }
        let snap = self.core.meeting_snapshot();
        self.meeting_ui.active = snap.get("active").and_then(Value::as_bool).unwrap_or(false);
        self.meeting_ui.summarizing = snap.get("transcribing").and_then(Value::as_bool).unwrap_or(false);
        self.meeting_ui.elapsed_secs = snap
            .get("elapsed_secs")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32;
        if let Some(msg) = snap.get("status").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            self.meeting_ui.status_text = msg.to_owned();
        } else if let Some(warn) = snap.get("loopback_warning").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            self.meeting_ui.status_text = warn.to_owned();
        } else if !self.meeting_ui.active && !self.meeting_ui.summarizing {
            self.meeting_ui.status_text.clear();
        }
        if let Some(notes_val) = snap.get("notes") {
            if let Ok(notes) = serde_json::from_value::<Vec<MeetingNote>>(notes_val.clone()) {
                self.meeting_ui.notes = notes;
            }
        }
    }

    fn build_history_stats(&self) -> HistoryStatsView {
        let unpriced = if self.preview() { 0 } else { storage::unpriced_api_attempts() };
        let val = crate::dashboard_state::stats(&self.history, unpriced);
        let opt_usize_vec = |k: &str| {
            val["trends"][k]
                .as_array()
                .map(|arr| arr.iter().map(|v| v.as_u64().map(|n| n as usize)).collect())
                .unwrap_or_default()
        };
        let opt_u64_vec = |k: &str| {
            val["trends"][k]
                .as_array()
                .map(|arr| arr.iter().map(|v| v.as_u64()).collect())
                .unwrap_or_default()
        };
        let opt_f64_vec = |k: &str| {
            val["trends"][k]
                .as_array()
                .map(|arr| arr.iter().map(|v| v.as_f64()).collect())
                .unwrap_or_default()
        };
        HistoryStatsView {
            today: val["today"].as_u64().unwrap_or(0) as usize,
            week_count: val["week"].as_u64().unwrap_or(0) as usize,
            words_week: val["words_week"].as_u64().unwrap_or(0) as usize,
            minutes_week: val["minutes_week"].as_f64().unwrap_or(0.0),
            wpm: val["wpm"].as_u64().unwrap_or(0),
            latency: val["latency"].as_f64().unwrap_or(0.0),
            trend_today: opt_usize_vec("today"),
            trend_words: opt_usize_vec("words_week"),
            trend_wpm: opt_u64_vec("wpm"),
            trend_latency: opt_f64_vec("latency"),
        }
    }

    fn build_model_status(&self) -> ModelStatusSnapshot {
        let inst = self.installer.status();
        let bench = self.benchmarker.status();
        let installed_whisper_ids: HashSet<String> = if self.preview() {
            HashSet::new()
        } else {
            models::installed_managed_whisper_models(&models::speech_models_dir())
                .into_iter()
                .map(|e| e.id.to_owned())
                .collect()
        };
        ModelStatusSnapshot {
            active_speech_id: models::active_speech_id(&self.settings),
            parakeet_installed: self.preview() || models::parakeet_installed(&self.settings),
            installed_whisper_ids,
            install_active: self.installer.is_active(),
            install_target_id: inst["id"].as_str().unwrap_or("").to_owned(),
            install_stage: inst["status"].as_str().unwrap_or("").to_owned(),
            install_progress: inst["percent"]
                .as_f64()
                .map(|p| (p / 100.0) as f32)
                .unwrap_or(0.0)
                .clamp(0.0, 1.0),
            install_error: inst["error"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
            benchmark_active: self.benchmarker.is_active(),
            benchmark_stage: bench["status"].as_str().unwrap_or("").to_owned(),
            benchmark_progress: bench["percent"]
                .as_f64()
                .map(|p| (p / 100.0) as f32)
                .unwrap_or(0.0)
                .clamp(0.0, 1.0),
            benchmark_error: bench["error"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
            saved_benchmarks: models::load_benchmarks(self.preview()),
        }
    }

    fn cancel_hotkey_modal(&mut self) {
        self.hotkey_lease = None;
        *self.core.controller_capture.lock().unwrap() = None;
        self.core.capture_paused.store(false, Ordering::Relaxed);
        self.hotkey_modal.open = false;
        self.hotkey_modal.controller_mode = false;
        self.hotkey_modal.listening = false;
    }

    fn run_workspace_stage(&mut self, request: crate::workspace::LocalRequest, settings: Settings, checking: bool) -> bool {
        let (provider, model) = cleanup::prompt_route(&settings, checking);
        let model = model.to_owned();
        let id = request.id;
        let core = self.core.clone();
        let draft_stamp = self.draft_change_stamp;
        self.lab_ui.busy_label = if checking {
            format!("Checking prompt with {model}…")
        } else {
            format!("Running Prompt Lab with {model}…")
        };
        self.lab_ui.error = None;
        let started = std::thread::Builder::new().name("prompt-lab".into()).spawn(move || {
            let result = cleanup::run_prompt_stage(
                provider,
                &model,
                &settings,
                &request.system,
                &request.user_prompt,
                request.json_output,
                || core.settings.lock().unwrap().cloud_enabled && storage::record_api_attempt().is_ok(),
            );
            core.emit(UiEvent::LocalSuggestion {
                id,
                result,
                model,
                draft_stamp,
                settings: Box::new(settings),
            });
        });
        if started.is_err() {
            self.workspace.finish_local(id, Err("Listen could not start the request. Try again.".into()), "");
            self.lab_ui.busy_label.clear();
            self.lab_ui.error = Some("Could not start Prompt Lab worker.".into());
            return false;
        }
        true
    }

    fn trigger_lab_stage(&mut self, name: &str, args: &[Value]) {
        if self.core.phase() != Phase::Idle {
            self.lab_ui.error = Some("Finish the active recording first.".into());
            return;
        }
        if name == "improve" {
            if let Some(true) = self.workspace.reuse_feedback_for_improvement() {
                self.sync_lab_ui();
                return;
            }
        }
        let settings = self.settings.clone();
        let checking = matches!(name, "feedback" | "review_candidate" | "improve");
        let (_, model) = cleanup::prompt_route(&settings, checking);
        let Some(request) = self.workspace.begin_local(name, args, model, settings.prompt_target) else {
            self.lab_ui.error = Some("Draft is empty or a Prompt Lab request is already running.".into());
            return;
        };
        self.run_workspace_stage(request, settings, checking);
    }

    fn handle_actions(&mut self, ctx: &egui::Context, actions: Vec<UiAction>) {
        for action in actions {
            match action {
                UiAction::GoPage(p) => self.page = p,
                UiAction::RequestQuit => self.request_quit(ctx),
                UiAction::CopyText(text, label) => {
                    if !text.is_empty() {
                        if self.preview() {
                            self.toast(label);
                        } else if winput::clipboard_recovery_pending() {
                            self.toast("Resolve clipboard recovery before copying.");
                        } else if arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).is_ok() {
                            self.core.cue(audio::Cue::Copied);
                            self.toast(label);
                        } else {
                            self.toast("Could not reach the clipboard.");
                        }
                    }
                }
                UiAction::RestoreClipboard => {
                    let res = winput::restore_delivery_clipboard(true);
                    self.toast(res["message"].as_str().unwrap_or("Clipboard restored"));
                }
                UiAction::DiscardClipboard => {
                    let res = winput::discard_delivery_clipboard(true);
                    self.toast(res["message"].as_str().unwrap_or("Kept current clipboard"));
                }
                UiAction::DismissRetained(idx) => {
                    if idx < self.retained.len() {
                        self.retained.remove(idx);
                    } else {
                        let ws_idx = idx - self.retained.len();
                        if let Some((id, _, _)) = self.workspace_retained.get(ws_idx).cloned() {
                            let _ = self.workspace.call("dismiss_retained", &[json!(id)]);
                            self.sync_lab_ui();
                        }
                    }
                }

                // Dictate
                UiAction::OpenHotkeyPicker => {
                    if self.core.phase() != Phase::Idle {
                        self.toast("Finish the active recording before changing the hotkey.");
                        continue;
                    }
                    self.lease_count += 1;
                    let token = format!("capture-{}", self.lease_count);
                    self.hotkey_lease = Some((token, Instant::now() + Duration::from_secs(60)));
                    self.core.capture_paused.store(true, Ordering::Relaxed);
                    let (label, _) = self.hotkey_display_parts();
                    self.hotkey_modal = HotkeyModalState {
                        open: true,
                        listening: true,
                        captured_value: self
                            .settings
                            .registered_hotkey
                            .clone()
                            .unwrap_or_else(|| crate::settings_bridge::hotkey_id(self.settings.hotkey).into()),
                        captured_label: label,
                        warning_level: String::new(),
                        warning_message: String::new(),
                        controller_mode: false,
                    };
                }
                UiAction::StartDictation => {
                    self.core.send(Cmd::Start { target: winput::capture_target(), hands_free: true });
                }
                UiAction::FinishDictation => self.core.send(Cmd::Finish),
                UiAction::CancelDictation => self.core.send(Cmd::Cancel),
                UiAction::UpdateVocab(v) => {
                    self.vocab_buf = v.clone();
                    self.settings.vocab = v;
                }
                UiAction::AcceptVocabSuggestion(word) => {
                    let mut parts = storage::vocab_words(&self.settings.vocab);
                    if !parts.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
                        parts.push(word.clone());
                        self.settings.vocab = parts.join(", ");
                        self.vocab_buf = self.settings.vocab.clone();
                        self.toast(format!("Added “{word}” to vocabulary"));
                    }
                }
                UiAction::DismissVocabSuggestion(word) => {
                    if !self.settings.vocab_dismissed.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
                        self.settings.vocab_dismissed.push(word);
                    }
                }

                // Prompt Lab
                UiAction::LabDraftChanged(text) => {
                    self.draft_change_stamp = self.draft_change_stamp.wrapping_add(1);
                    self.lab_ui.draft = text.clone();
                    self.save_lab_text(&text);
                }
                UiAction::LabNewDraft => {
                    self.draft_change_stamp = self.draft_change_stamp.wrapping_add(1);
                    let _ = self.workspace.call("new_draft", &[]);
                    self.lab_ui.draft.clear();
                    self.lab_ui.instruction.clear();
                    self.lab_ui.candidate = None;
                    self.lab_ui.compare_open = false;
                    self.sync_lab_ui();
                }
                UiAction::LabEngineer => {
                    if self.core.phase() != Phase::Idle {
                        self.lab_ui.error = Some("Finish the active recording first.".into());
                        continue;
                    }
                    let ctx_id = self.workspace.context();
                    let text = self.lab_ui.draft.clone();
                    let settings = self.settings.clone();
                    let (_, model) = cleanup::prompt_route(&settings, false);
                    let try_refine = self.lab_ui.assessment.is_some();
                    let req = if try_refine {
                        self.workspace
                            .begin_local("draft_pass", &[json!(ctx_id), json!(text), json!(true)], model, settings.prompt_target)
                            .or_else(|| {
                                self.workspace.begin_local(
                                    "draft_pass",
                                    &[json!(ctx_id), json!(text), json!(false)],
                                    model,
                                    settings.prompt_target,
                                )
                            })
                    } else {
                        self.workspace.begin_local(
                            "draft_pass",
                            &[json!(ctx_id), json!(text), json!(false)],
                            model,
                            settings.prompt_target,
                        )
                    };
                    if let Some(request) = req {
                        self.run_workspace_stage(request, settings, false);
                    } else {
                        self.lab_ui.error = Some("Draft is empty or a Prompt Lab request is already running.".into());
                    }
                }
                UiAction::LabSuggestChanges => {
                    self.trigger_lab_stage("improve", &[]);
                }
                UiAction::LabRewriteScratch => {
                    self.trigger_lab_stage("rebuild", &[]);
                }
                UiAction::LabApplyInstruction(instr) => {
                    self.save_lab_composer(&instr);
                    let ctx_id = self.workspace.context();
                    self.trigger_lab_stage("submit", &[json!(instr), json!(ctx_id)]);
                }
                UiAction::LabScoreDraft => {
                    let ctx_id = self.workspace.context();
                    let text = self.lab_ui.draft.clone();
                    self.trigger_lab_stage("feedback", &[json!(ctx_id), json!(text)]);
                }
                UiAction::LabApplyCandidate => {
                    if let Some(cand) = &self.lab_ui.candidate {
                        let applied = self.workspace.call(
                            "apply_candidate",
                            &[json!(cand.id), json!(cand.version_idx as u64)],
                        ) == Some(json!(true));
                        self.lab_ui.compare_open = false;
                        self.sync_lab_ui();
                        if applied {
                            self.toast("Applied suggestion");
                        } else {
                            self.toast("Could not apply suggestion — the draft changed.");
                        }
                    }
                }
                UiAction::LabDiscardCandidate => {
                    if let Some(cand) = &self.lab_ui.candidate {
                        let _ = self.workspace.call("discard_candidate", &[json!(cand.id)]);
                        self.lab_ui.compare_open = false;
                        self.sync_lab_ui();
                    }
                }
                UiAction::LabReviewCandidate => {
                    if let Some(cand) = &self.lab_ui.candidate {
                        let id = cand.id;
                        let ver = cand.version_idx as u64;
                        self.trigger_lab_stage("review_candidate", &[json!(id), json!(ver)]);
                    }
                }
                UiAction::LabSelectRevision(idx) => {
                    let _ = self.workspace.call("select_rev", &[json!(idx as u64)]);
                    self.sync_lab_ui();
                }
                UiAction::LabPasteDraft => self.paste_draft(ctx),
                UiAction::LabSaveFavorite(name) => {
                    let text = self.lab_ui.draft.trim().to_owned();
                    let label = if name.trim().is_empty() {
                        text.split_whitespace().take(6).collect::<Vec<_>>().join(" ")
                    } else {
                        name.trim().to_owned()
                    };
                    if !text.is_empty() && !label.is_empty() {
                        let (when, _) = storage::now_stamp();
                        self.favorites.insert(0, Favorite { name: label, text, when, uses: 0 });
                        if !self.preview() {
                            storage::save_favorites(&self.favorites);
                        }
                        self.lab_ui.save_fav_open = false;
                        self.lab_ui.save_fav_name.clear();
                        self.toast("Saved favorite");
                    }
                }
                UiAction::LabLoadFavorite(idx) => {
                    if let Some(f) = self.favorites.get_mut(idx) {
                        f.uses = f.uses.saturating_add(1);
                        let text = f.text.clone();
                        if !self.preview() {
                            storage::save_favorites(&self.favorites);
                        }
                        self.draft_change_stamp = self.draft_change_stamp.wrapping_add(1);
                        self.lab_ui.draft = text.clone();
                        self.save_lab_text(&text);
                        self.lab_ui.show_library = false;
                        self.toast("Loaded favorite");
                    }
                }
                UiAction::LabDeleteFavorite(idx) => {
                    if idx < self.favorites.len() {
                        self.favorites.remove(idx);
                        if !self.preview() {
                            storage::save_favorites(&self.favorites);
                        }
                        self.toast("Removed favorite");
                    }
                }
                UiAction::LabRestoreArchived(id) => {
                    let _ = self.workspace.call("restore_draft", &[json!(id)]);
                    self.sync_lab_ui();
                    self.toast("Restored draft");
                }
                UiAction::LabDeleteArchived(id) => {
                    let _ = self.workspace.call("delete_draft", &[json!(id), json!("DELETE")]);
                    self.sync_lab_ui();
                    self.toast("Deleted archived draft");
                }

                // Meetings
                UiAction::MeetingToggleSystemAudio(on) => {
                    self.meeting_ui.include_system_audio = on;
                }
                UiAction::MeetingStart => {
                    if self.preview() {
                        self.toast("Meeting capture disabled in preview mode");
                    } else {
                        match self.core.start_meeting_with_loopback(self.meeting_ui.include_system_audio) {
                            Ok(info) => {
                                self.meeting_ui.status_text = info
                                    .loopback_warning
                                    .unwrap_or_else(|| if info.loopback_active { "Recording mic + system audio…".into() } else { "Recording mic audio…".into() });
                            }
                            Err(msg) => self.toast(msg),
                        }
                    }
                }
                UiAction::MeetingStopAndSummarize => {
                    if let Err(msg) = self.core.stop_meeting() {
                        self.toast(msg);
                    }
                }
                UiAction::MeetingCancel => {
                    self.core.cancel_meeting();
                    self.toast("Meeting recording canceled");
                }
                UiAction::MeetingDelete(id) => {
                    if let Ok(num) = id.parse::<u64>() {
                        if self.preview() {
                            self.meeting_ui.notes.retain(|n| n.id != num);
                        } else if self.core.delete_meeting(num) {
                            self.sync_meetings_ui();
                            self.toast("Deleted meeting note");
                        }
                    }
                }

                // History
                UiAction::ClearHistory => {
                    if self.core.phase() != Phase::Idle {
                        self.toast("Finish the current recording before clearing History.");
                    } else {
                        if !self.preview() {
                            storage::clear_history();
                        }
                        self.history.clear();
                        self.history_ui.confirm_clear = false;
                        self.toast("History cleared");
                    }
                }

                // Settings
                UiAction::SetCommandHotkey(hk) => self.settings.command_hotkey = hk,
                UiAction::SetMicrophone(mic) => self.settings.microphone = mic,
                UiAction::SetDestination(d) => self.settings.destination = d,
                UiAction::SetLivePreview(v) => self.settings.live_preview = v,
                UiAction::SetFloatingCard(v) => self.settings.floating_card = v,
                UiAction::SetSounds(v) => self.settings.sounds = v,
                UiAction::SetRestoreClipboard(v) => self.settings.restore_clipboard = v,
                UiAction::SetQuietMode(v) => self.settings.quiet_mode = v,
                UiAction::SetSpeechEngine(e) => {
                    self.settings.speech_engine = e;
                    self.persist();
                    self.core.send(Cmd::ReloadEngine);
                }
                UiAction::SetCleanup(v) => self.settings.cleanup = v,
                UiAction::SetStyle(s) => self.settings.style = s,
                UiAction::SetLocalModel(m) => self.settings.local_model = m,
                UiAction::RefreshModels => {
                    self.refresh_local_models_async();
                    self.toast("Refreshing Ollama models…");
                }
                UiAction::InstallSpeechModel(id) => {
                    if self.benchmarker.is_active() {
                        self.toast("Wait for or cancel the running benchmark first.");
                    } else {
                        let res = self.installer.start("speech", &id, self.settings.ollama_url.clone(), self.preview());
                        if res["ok"].as_bool() != Some(true) {
                            self.toast(res["message"].as_str().unwrap_or("Could not start install"));
                        }
                    }
                }
                UiAction::SelectSpeechModel(id) => {
                    self.persist();
                    let ok = crate::settings_bridge::call(&self.core, "set_setting", &[json!("speech_model"), json!(id)])
                        == Some(json!(true));
                    if ok {
                        self.settings = self.core.settings.lock().unwrap().clone();
                        self.saved = self.settings.clone();
                        self.vocab_buf = self.settings.vocab.clone();
                        self.toast("Switched active speech model");
                    } else {
                        self.toast("Could not switch to that speech model.");
                    }
                }
                UiAction::CancelModelInstall => {
                    self.installer.cancel();
                    self.toast("Install canceled");
                }
                UiAction::StartBenchmark => {
                    if self.core.phase() != Phase::Idle || self.workspace.busy() {
                        self.toast("Finish active recording or Prompt Lab work first.");
                    } else {
                        let res = self.benchmarker.start(self.settings.clone(), "", self.preview());
                        if res["ok"].as_bool() != Some(true) {
                            self.toast(res["message"].as_str().unwrap_or("Could not start benchmark"));
                        }
                    }
                }
                UiAction::CancelBenchmark => {
                    self.benchmarker.cancel();
                    self.toast("Benchmark canceled");
                }
                UiAction::SetAutoAppTone(v) => self.settings.auto_app_tone = v,
                UiAction::AddAppToneRule { app, tone } => match crate::app_tone::validate_rule(&app, tone) {
                    Ok(rule) => {
                        if let Some(existing) = self
                            .settings
                            .app_tone_rules
                            .iter_mut()
                            .find(|r| r.app.eq_ignore_ascii_case(&rule.app))
                        {
                            *existing = rule;
                        } else {
                            self.settings.app_tone_rules.push(rule);
                        }
                        self.settings_ui.tone_app_input.clear();
                        self.settings_ui.tone_error = None;
                        self.toast("Saved app tone rule");
                    }
                    Err(e) => self.settings_ui.tone_error = Some(e),
                },
                UiAction::DeleteAppToneRule(idx) => {
                    if idx < self.settings.app_tone_rules.len() {
                        self.settings.app_tone_rules.remove(idx);
                    }
                }
                UiAction::AddSnippet { trigger, expansion } => {
                    match crate::snippets::validate_snippet(&trigger, &expansion) {
                        Ok(snip) => {
                            if let Some(existing) = self
                                .settings
                                .snippets
                                .iter_mut()
                                .find(|s| s.trigger.eq_ignore_ascii_case(&snip.trigger))
                            {
                                *existing = snip;
                            } else {
                                self.settings.snippets.push(snip);
                            }
                            self.settings_ui.snippet_trigger.clear();
                            self.settings_ui.snippet_expansion.clear();
                            self.settings_ui.snippet_error = None;
                            self.toast("Saved voice snippet");
                        }
                        Err(e) => self.settings_ui.snippet_error = Some(e),
                    }
                }
                UiAction::DeleteSnippet(idx) => {
                    if idx < self.settings.snippets.len() {
                        self.settings.snippets.remove(idx);
                    }
                }
                UiAction::SetTheme(t) => self.settings.theme = t,
                UiAction::SetLayout(l) => self.settings.layout = l,
                UiAction::SetZoom(z) => self.settings.zoom = z.clamp(0.85, 1.35),
                UiAction::SetPromptTarget(t) => self.settings.prompt_target = t,
                UiAction::SetPromptEngine(p) => self.settings.prompt_engine = p,
                UiAction::SetPromptJudge(p) => self.settings.prompt_judge = p,
                UiAction::SetEngineLocalModel(m) => self.settings.engine_local_model = m,
                UiAction::SetJudgeLocalModel(m) => self.settings.judge_local_model = m,
                UiAction::SetCloudEnabled(v) => self.settings.cloud_enabled = v,
                UiAction::SetClaudeModel(m) => self.settings.claude_model = m,
                UiAction::SetLearn(v) => self.settings.learn = v,
                UiAction::ResetLearned => self.toast("Learned preferences cleared"),
                UiAction::ClearDrafts => {
                    let res = self.workspace.call("clear_private_data", &[json!("workspace"), json!("DELETE")]);
                    self.draft_change_stamp = self.draft_change_stamp.wrapping_add(1);
                    self.lab_ui.draft.clear();
                    self.lab_ui.instruction.clear();
                    self.lab_ui.candidate = None;
                    self.lab_ui.compare_open = false;
                    self.sync_lab_ui();
                    self.settings_ui.confirm_clear_drafts = false;
                    let msg = res
                        .as_ref()
                        .and_then(|v| v.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("Drafts cleared");
                    self.toast(msg);
                }
                UiAction::ClearLogs => {
                    if !self.preview() {
                        let _ = crate::settings_bridge::clear_log_files(&storage::data_dir());
                    }
                    self.settings_ui.confirm_clear_logs = false;
                    self.toast("App logs cleared");
                }

                // Hotkey modal
                UiAction::HotkeyModalCaptureKey(val) => {
                    let (level, msg) = winput::check_hotkey(&self.core, &val);
                    self.hotkey_modal.captured_value = val.clone();
                    self.hotkey_modal.captured_label = format_hotkey_label(&val);
                    self.hotkey_modal.warning_level = level.to_owned();
                    self.hotkey_modal.warning_message = msg.to_owned();
                }
                UiAction::HotkeyModalBeginController => {
                    let connected = if self.preview() { [None; 4] } else { crate::controller::read() };
                    if connected.iter().all(Option::is_none) {
                        self.hotkey_modal.controller_mode = false;
                        self.hotkey_modal.listening = true;
                        self.hotkey_modal.warning_level = "blocked".into();
                        self.hotkey_modal.warning_message = "Connect an Xbox-compatible controller and try again.".into();
                    } else if let Some((token, _)) = &self.hotkey_lease {
                        *self.core.controller_capture.lock().unwrap() =
                            Some(crate::controller::Capture::new(token.clone(), connected));
                        self.hotkey_modal.controller_mode = true;
                        self.hotkey_modal.listening = false;
                        self.hotkey_modal.warning_level = "clear".into();
                        self.hotkey_modal.warning_message = "Press any button on your controller…".into();
                    }
                }
                UiAction::HotkeyModalApply(val) => {
                    self.persist();
                    let saved = winput::save_hotkey(&self.core, &val, true);
                    self.cancel_hotkey_modal();
                    if saved {
                        self.settings = self.core.settings.lock().unwrap().clone();
                        self.saved = self.settings.clone();
                        self.vocab_buf = self.settings.vocab.clone();
                        self.toast(format!("Hotkey set to {}", format_hotkey_label(&val)));
                    } else {
                        self.toast("Could not save that hotkey.");
                    }
                }
                UiAction::HotkeyModalCancel => {
                    self.cancel_hotkey_modal();
                }
            }
        }
    }

    fn refresh_snap(&mut self) {
        let sh = self.core.shared.lock().unwrap();
        let outcome = match &sh.outcome {
            Some((t, until)) if Instant::now() < *until => Some(t.clone()),
            _ => None,
        };
        self.snap = Snap {
            phase: sh.phase,
            started: sh.started,
            generation: sh.rec_gen,
            anchor: sh.last_target.map_or(0, winput::Target::window),
            engine: sh.engine.clone(),
            status: sh.status.clone(),
            lab_busy: sh.lab_busy,
            live_text: sh.live_text.clone(),
            live_note: sh.live_note.clone(),
            outcome,
        };
        drop(sh);
        if self.demo_recording {
            self.snap.phase = Phase::Recording;
            self.snap.started = Some(
                Instant::now()
                    .checked_sub(Duration::from_secs(14))
                    .unwrap_or_else(Instant::now),
            );
            self.snap.live_text = DEMO_LIVE.to_owned();
        }
        // Expire stale hotkey capture lease if left open past 60 s
        if let Some((_, expires)) = &self.hotkey_lease {
            if Instant::now() >= *expires {
                self.cancel_hotkey_modal();
                self.toast("Hotkey capture timed out — original hotkey restored.");
            }
        }
        // Poll controller capture while hotkey modal is open
        if self.hotkey_modal.open && self.hotkey_modal.controller_mode {
            let picked = self
                .core
                .controller_capture
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|c| c.picked.map(str::to_owned));
            if let Some(binding) = picked {
                self.hotkey_modal.controller_mode = false;
                let (level, msg) = winput::check_hotkey(&self.core, &binding);
                self.hotkey_modal.captured_value = binding.clone();
                self.hotkey_modal.captured_label = format_hotkey_label(&binding);
                self.hotkey_modal.warning_level = level.to_owned();
                self.hotkey_modal.warning_message = msg.to_owned();
            }
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                UiEvent::History(e) => self.history.push(e),
                UiEvent::Retained { text, reason } => self.retained.push((text, reason)),
                UiEvent::ToWorkspace(t, draft) => {
                    if let Some((context, _)) = draft.filter(|(_, stamp)| *stamp == self.draft_change_stamp) {
                        self.workspace.accept_dictation(&t, context);
                    } else {
                        let mut next = self.lab_ui.draft.trim_end().to_owned();
                        if !next.is_empty() {
                            next.push_str("\n\n");
                        }
                        next.push_str(t.trim());
                        self.save_lab_text(&next);
                    }
                    self.sync_lab_ui();
                }
                UiEvent::LabDone(Ok(t)) => {
                    self.lab_ui.error = None;
                    self.save_lab_text(&t);
                    self.sync_lab_ui();
                }
                UiEvent::LabDone(Err(e)) => self.lab_ui.error = Some(e),
                UiEvent::LocalSuggestion { id, result, model, draft_stamp, settings } => {
                    if draft_stamp != self.draft_change_stamp {
                        self.workspace.cancel_local(id);
                        self.lab_ui.busy_label.clear();
                        continue;
                    }
                    if let Some(request) = self.workspace.finish_local(id, result, &model) {
                        self.run_workspace_stage(request, *settings, true);
                    } else if let Some(pass_id) = self
                        .workspace
                        .call("get_lab", &[])
                        .and_then(|s| s.get("prepared_pass").and_then(Value::as_u64))
                    {
                        let ctx_id = self.workspace.context();
                        let _ = self
                            .workspace
                            .call("finish_draft_pass", &[json!(ctx_id), json!(pass_id)]);
                    }
                    self.sync_lab_ui();
                }
                UiEvent::Models(m) => {
                    self.ollama_available = !m.is_empty();
                    self.models = m;
                }
            }
        }
    }

    fn tick_levels(&mut self) {
        let v = if self.snap.phase == Phase::Recording {
            (self.core.level() * 14.0).sqrt().clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.levels.pop_front();
        self.levels.push_back(v);
    }

    fn apply_settings(&mut self, ctx: &egui::Context) {
        let key = (self.settings.theme, self.settings.zoom.to_bits());
        if self.applied != Some(key) {
            theme::apply(ctx, &self.settings.theme.palette(), self.settings.zoom);
            self.applied = Some(key);
        }
        let layout = self.settings.layout;
        if layout != self.last_layout {
            if layout == LayoutId::Compact {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(480.0, 760.0)));
            } else if self.last_layout == LayoutId::Compact {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(920.0, 720.0)));
            }
            self.last_layout = layout;
        }
    }

    fn persist(&mut self) {
        if self.settings != self.saved {
            if !self.preview() {
                storage::save_settings(&self.settings);
            }
            *self.core.settings.lock().unwrap() = self.settings.clone();
            self.saved = self.settings.clone();
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.hotkey_modal.open {
            return;
        }
        let c = Modifiers::COMMAND;
        let handles_esc = self.palette_open
            || self.lab_ui.compare_open
            || self.snap.phase == Phase::Recording
            || self.history_ui.confirm_clear;
        let (p1, p2, p3, p4, p5, k, enter, t, l, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(c, Key::Num1),
                i.consume_key(c, Key::Num2),
                i.consume_key(c, Key::Num3),
                i.consume_key(c, Key::Num4),
                i.consume_key(c, Key::Num5),
                i.consume_key(c, Key::K),
                i.consume_key(c, Key::Enter),
                i.consume_key(c, Key::T),
                i.consume_key(c, Key::L),
                handles_esc && i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if p1 { self.page = Page::Dictate; }
        if p2 { self.page = Page::Lab; }
        if p3 { self.page = Page::Meetings; }
        if p4 { self.page = Page::History; }
        if p5 { self.page = Page::Settings; }
        if k {
            self.palette_open = !self.palette_open;
            self.palette_query.clear();
        }
        if enter && self.page == Page::Lab {
            self.paste_draft(ctx);
        }
        if t { self.settings.theme = self.settings.theme.next(); }
        if l { self.settings.layout = self.settings.layout.next(); }
        if esc {
            if self.palette_open {
                self.palette_open = false;
            } else if self.lab_ui.compare_open {
                self.lab_ui.compare_open = false;
            } else if self.snap.phase == Phase::Recording {
                self.core.send(Cmd::Cancel);
            } else if self.history_ui.confirm_clear {
                self.history_ui.confirm_clear = false;
            }
        }
    }

    fn paste_draft(&mut self, ctx: &egui::Context) {
        let text = self.lab_ui.draft.trim().to_owned();
        if text.is_empty() {
            self.toast("The draft is empty");
            return;
        }
        if self.preview() || winput::clipboard_recovery_pending() {
            if !self.preview() {
                self.toast("Resolve clipboard recovery in Listen before pasting.");
            }
            return;
        }
        let target = self
            .core
            .shared
            .lock()
            .unwrap()
            .last_target
            .filter(|target| !target.is_ours())
            .or(self.launch_target);
        if target.is_some_and(|target| self.core.queue_draft_delivery(text, target)) {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        } else {
            self.toast("Choose Copy and paste where you want the draft.");
        }
    }

    fn tray_step(&mut self, ctx: &egui::Context) {
        use crate::tray::{TrayAction, TrayState};
        if winput::take_show_request() {
            winput::show_window(self.hwnd);
        }
        let Some(tray) = self.tray.as_mut() else { return };
        for a in tray.poll() {
            if matches!(a, TrayAction::Open | TrayAction::PromptLab | TrayAction::Settings) {
                winput::show_window(self.hwnd);
            }
            match a {
                TrayAction::Open => {}
                TrayAction::PromptLab => self.page = Page::Lab,
                TrayAction::Settings => self.page = Page::Settings,
                TrayAction::CopyFavorite(i) => {
                    if !self.preview() && winput::clipboard_recovery_pending() {
                        self.toast("Resolve clipboard recovery in Listen first");
                    } else if let Some(f) = self.favorites.get_mut(i) {
                        let ok = arboard::Clipboard::new().and_then(|mut c| c.set_text(f.text.clone())).is_ok();
                        if ok {
                            f.uses = f.uses.saturating_add(1);
                            let name = f.name.clone();
                            storage::save_favorites(&self.favorites);
                            self.core.cue(audio::Cue::Copied);
                            self.toast(format!("Copied “{name}”"));
                        }
                    }
                }
                TrayAction::Quit => {
                    if winput::is_parked() {
                        winput::show_window(self.hwnd);
                    }
                    self.request_quit(ctx);
                }
            }
        }
        let state = match (&self.snap.engine, self.snap.phase) {
            (_, Phase::Recording) => TrayState::Recording,
            (_, Phase::Working) => TrayState::Working,
            (EngineState::Ready(_), Phase::Idle) => TrayState::Idle,
            _ => TrayState::Loading,
        };
        let status = match (&self.snap.engine, self.snap.phase) {
            (_, Phase::Recording) => "listening",
            (_, Phase::Working) => "working",
            (EngineState::Loading, _) => "loading",
            (EngineState::Unavailable(_), _) => "no speech engine",
            (EngineState::Ready(_), _) => "ready",
        };
        if let Some(t) = self.tray.as_mut() {
            t.sync(state, status, &self.favorites);
        }
        if let Ok(path) = std::env::var("LISTEN_HIDE_TEST") {
            match self.hide_test {
                None => self.hide_test = Some((Instant::now(), 0)),
                Some((at, n)) => {
                    if at.elapsed() > Duration::from_millis(800) {
                        winput::hide_window(self.hwnd);
                        self.hide_test = Some((at, n + 1));
                    }
                    if at.elapsed() > Duration::from_millis(3800) {
                        let _ = std::fs::write(
                            &path,
                            format!("frames_while_hidden={} visible={}\n", n + 1, winput::window_visible(self.hwnd)),
                        );
                        std::process::exit(0);
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn request_quit(&mut self, ctx: &egui::Context) {
        if let Err(message) = self.quit_app(ctx) {
            self.toast(message);
        }
    }

    fn close_step(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.quitting || self.preview() {
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        if should_hide_on_close(self.tray.is_some(), self.preview()) {
            if self.hotkey_modal.open || self.hotkey_lease.is_some() {
                self.cancel_hotkey_modal();
            }
            winput::hide_window(self.hwnd);
        } else {
            self.request_quit(ctx);
        }
    }

    fn quit_app(&mut self, ctx: &egui::Context) -> Result<(), &'static str> {
        let was_paused = self.core.capture_paused.swap(true, Ordering::AcqRel);
        let reject = |core: &Core, message: &'static str| -> Result<(), &'static str> {
            core.capture_paused.store(was_paused, Ordering::Release);
            Err(message)
        };
        let (phase, lab_busy) = {
            let live = self.core.shared.lock().unwrap();
            (live.phase, live.lab_busy)
        };
        if phase == Phase::Recording {
            return reject(&self.core, "Finish or cancel the recording, then close Listen.");
        }
        if self.core.meeting_active() {
            return reject(&self.core, "Stop or cancel the active meeting recording, then close Listen.");
        }
        if phase == Phase::Working
            || lab_busy
            || self.workspace.busy()
            || self.core.draft_delivery_pending.load(Ordering::Acquire)
        {
            return reject(&self.core, "Listen is still working. Wait for it to finish, then close Listen.");
        }
        self.drain_events();
        if self.workspace.has_unsaved_recovery() || !self.retained.is_empty() {
            return reject(&self.core, "Copy or save your recovered text before closing Listen.");
        }
        if !self.preview() && winput::clipboard_recovery_pending() {
            return reject(&self.core, "Clipboard recovery is pending. Open Listen and resolve it before quitting.");
        }
        self.quitting = true;
        self.benchmarker.cancel();
        self.installer.cancel();
        winput::shutdown_clipboard();
        ctx.send_viewport_cmd(ViewportCommand::Close);
        Ok(())
    }

    // ---------------------------------------------------------------- layouts & chrome

    fn render_shell(&mut self, ctx: &egui::Context, p: &Palette) -> Vec<UiAction> {
        let mut actions = Vec::new();
        match self.settings.layout {
            LayoutId::Desk => {
                egui::TopBottomPanel::top("rail")
                    .exact_height(RAIL_H)
                    .show_separator_line(false)
                    .frame(Frame::default().fill(p.nav).inner_margin(Margin::symmetric(24, 0)))
                    .show(ctx, |ui| {
                        let rect = ui.max_rect();
                        ui.painter().line_segment(
                            [pos2(rect.left() - 24.0, rect.bottom()), pos2(rect.right() + 24.0, rect.bottom())],
                            Stroke::new(1.0_f32, p.line),
                        );
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui::widgets::brand_mark(ui, p, 30.0);
                            ui.add_space(4.0);
                            ui.label(RichText::new("Listen").font(FontId::new(19.0, FontFamily::Name("humanist".into()))).strong().color(p.fg));
                            ui.add_space(16.0);
                            for pg in Page::ALL {
                                if nav_pill(ui, p, pg.label(), self.page == pg, false).clicked() {
                                    actions.push(UiAction::GoPage(pg));
                                }
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if nav_pill(ui, p, "Quit", false, false).on_hover_text("Quit Listen completely").clicked() {
                                    actions.push(UiAction::RequestQuit);
                                }
                            });
                        });
                    });
                self.render_central(ctx, p, 920.0, Margin::symmetric(28, 20), &mut actions);
            }
            LayoutId::Sidebar => {
                egui::SidePanel::left("sidebar-rail")
                    .exact_width(190.0)
                    .resizable(false)
                    .show_separator_line(false)
                    .frame(Frame::default().fill(p.nav).inner_margin(Margin::symmetric(14, 18)))
                    .show(ctx, |ui| {
                        let rect = ui.max_rect();
                        ui.painter().line_segment(
                            [pos2(rect.right() + 14.0, rect.top() - 18.0), pos2(rect.right() + 14.0, rect.bottom() + 18.0)],
                            Stroke::new(1.0_f32, p.line),
                        );
                        ui.horizontal(|ui| {
                            ui::widgets::brand_mark(ui, p, 28.0);
                            ui.add_space(4.0);
                            ui.label(RichText::new("Listen").font(FontId::new(19.0, FontFamily::Name("humanist".into()))).strong().color(p.fg));
                        });
                        ui.add_space(18.0);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for pg in Page::ALL {
                            if nav_pill(ui, p, pg.label(), self.page == pg, true).clicked() {
                                actions.push(UiAction::GoPage(pg));
                            }
                        }
                        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                            if nav_pill(ui, p, "Quit", false, true).clicked() {
                                actions.push(UiAction::RequestQuit);
                            }
                        });
                    });
                self.render_central(ctx, p, 920.0, Margin::symmetric(28, 22), &mut actions);
            }
            LayoutId::Compact => {
                egui::TopBottomPanel::top("compact-rail")
                    .exact_height(54.0)
                    .show_separator_line(false)
                    .frame(Frame::default().fill(p.nav).inner_margin(Margin::symmetric(12, 0)))
                    .show(ctx, |ui| {
                        let rect = ui.max_rect();
                        ui.painter().line_segment(
                            [pos2(rect.left() - 12.0, rect.bottom()), pos2(rect.right() + 12.0, rect.bottom())],
                            Stroke::new(1.0_f32, p.line),
                        );
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            ui::widgets::brand_mark(ui, p, 26.0);
                            ui.add_space(4.0);
                            for pg in Page::ALL {
                                let short = match pg {
                                    Page::Lab => "Lab",
                                    _ => pg.label(),
                                };
                                if nav_pill(ui, p, short, self.page == pg, false).clicked() {
                                    actions.push(UiAction::GoPage(pg));
                                }
                            }
                        });
                    });
                self.render_central(ctx, p, 480.0, Margin::symmetric(16, 16), &mut actions);
            }
        }
        actions
    }

    fn render_central(&mut self, ctx: &egui::Context, p: &Palette, max_w: f32, margin: Margin, actions: &mut Vec<UiAction>) {
        // Prepare snapshots for view
        let (hotkey_display, hotkey_parts) = self.hotkey_display_parts();
        let vocab_suggestions = storage::suggest_vocab(&self.history, &self.settings.vocab, &self.settings.vocab_dismissed, 8);
        let history_stats = self.build_history_stats();
        let model_status = self.build_model_status();
        let api_keys_present = (
            !self.preview() && cleanup::provider_key(PromptProvider::Anthropic).is_some(),
            !self.preview() && cleanup::provider_key(PromptProvider::OpenAi).is_some(),
            !self.preview() && cleanup::provider_key(PromptProvider::Gemini).is_some(),
        );
        self.settings_ui.diagnostics_summary = format!(
            "Speech: {} | Formatting: {} | Data: {}",
            self.core.shared.lock().unwrap().engine_label(),
            if self.settings.cleanup { format!("{} (Ollama)", self.settings.local_model) } else { "Off".into() },
            if self.preview() { "Preview profile".into() } else { storage::data_dir().display().to_string() },
        );

        let toast_str = self.toast.as_ref().map(|(s, _)| s.as_str());
        let time_secs = ctx.input(|i| i.time);
        let mut combined_retained: Vec<(String, String)> = self.retained.clone();
        for (_, text, msg) in &self.workspace_retained {
            combined_retained.push((text.clone(), msg.clone()));
        }

        let view = UiView {
            page: self.page,
            settings: &self.settings,
            snap: &self.snap,
            levels: &self.levels,
            history: &self.history,
            history_stats: &history_stats,
            favorites: &self.favorites,
            vocab_suggestions: &vocab_suggestions,
            models: &self.models,
            ollama_available: self.ollama_available,
            mics: &self.mics,
            model_status: &model_status,
            api_keys_present,
            hotkey_display: &hotkey_display,
            hotkey_parts: &hotkey_parts,
            demo_recording: self.demo_recording,
            retained: &combined_retained,
            clipboard_recovery: !self.preview() && winput::clipboard_recovery_pending(),
            toast: toast_str,
            time_secs,
        };

        egui::CentralPanel::default()
            .frame(Frame::default().fill(p.bg).inner_margin(margin))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let w = ui.available_width().min(max_w);
                    let pad = ((ui.available_width() - w) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        if pad > 0.0 {
                            ui.add_space(pad);
                        }
                        ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
                            match self.page {
                                Page::Dictate => ui::dictate_lab::render_dictate(
                                    ui, p, &view, &mut self.vocab_buf, &mut self.show_how, &mut self.recent_open, actions,
                                ),
                                Page::Lab => ui::dictate_lab::render_lab(
                                    ui, ctx, p, &view, &mut self.lab_ui, actions,
                                ),
                                Page::Meetings => ui::meetings_history::render_meetings(
                                    ui, p, &view, &mut self.meeting_ui, actions,
                                ),
                                Page::History => ui::meetings_history::render_history(
                                    ui, p, &view, &mut self.history_ui, actions,
                                ),
                                Page::Settings => ui::settings::render_settings(
                                    ui, p, &view, &mut self.settings_ui, actions,
                                ),
                            }
                        });
                    });
                });
            });

        if self.hotkey_modal.open {
            ui::settings::render_hotkey_modal(ctx, p, &mut self.hotkey_modal, actions);
        }

        // Toast pill at bottom-right
        if let Some(msg) = toast_str {
            egui::Area::new(egui::Id::new("listen-toast"))
                .anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -16.0))
                .show(ctx, |ui| {
                    Frame::default()
                        .fill(p.surface2)
                        .stroke(Stroke::new(1.0_f32, p.accent))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(14, 9))
                        .show(ui, |ui| {
                            ui.label(RichText::new(msg).size(13.0).color(p.fg));
                        });
                });
        }
    }

    fn command_palette(&mut self, ctx: &egui::Context, p: &Palette) {
        let mut cmds: Vec<(&str, String, PCmd)> = vec![
            ("Go", "Dictate".into(), PCmd::Go(Page::Dictate)),
            ("Go", "Prompt Lab".into(), PCmd::Go(Page::Lab)),
            ("Go", "Meetings".into(), PCmd::Go(Page::Meetings)),
            ("Go", "History".into(), PCmd::Go(Page::History)),
            ("Go", "Settings".into(), PCmd::Go(Page::Settings)),
            ("Action", "Start dictation".into(), PCmd::Start),
            ("Action", "Finish recording".into(), PCmd::Finish),
            ("Action", "Cancel recording".into(), PCmd::Cancel),
            ("Action", "Paste draft into last app".into(), PCmd::PasteDraft),
            ("Action", "Toggle AI formatting".into(), PCmd::ToggleCleanup),
            ("Action", "Toggle Quiet / whisper mode".into(), PCmd::ToggleQuietMode),
            ("Action", "Switch dictation destination".into(), PCmd::ToggleDestination),
            ("Action", "Reload speech engine".into(), PCmd::ReloadEngine),
        ];
        for t in ThemeId::ALL {
            cmds.push(("Theme", t.label().into(), PCmd::Theme(t)));
        }
        for l in LayoutId::ALL {
            cmds.push(("Layout", l.label().into(), PCmd::Layout(l)));
        }

        let q = self.palette_query.trim().to_lowercase();
        let filtered: Vec<_> = cmds
            .into_iter()
            .filter(|(k, name, _)| q.is_empty() || name.to_lowercase().contains(&q) || k.to_lowercase().contains(&q))
            .collect();

        let mut picked: Option<PCmd> = None;
        egui::Window::new("Command Palette")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 72.0))
            .default_width(440.0)
            .min_width(440.0)
            .max_width(440.0)
            .frame(
                Frame::default()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.2_f32, p.accent))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(12)),
            )
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Type a command, page, or theme…"),
                );
                edit.request_focus();
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    if let Some((_, _, c)) = filtered.first() {
                        picked = Some(c.clone());
                    }
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    for (kind, label, cmd) in filtered.iter().take(12) {
                        let resp = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{kind:<8}  {label}")).size(13.0).color(p.fg),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .min_size(vec2(ui.available_width(), 28.0)),
                        );
                        if resp.clicked() {
                            picked = Some(cmd.clone());
                        }
                    }
                });
            });

        if let Some(cmd) = picked {
            self.palette_open = false;
            match cmd {
                PCmd::Go(pg) => self.page = pg,
                PCmd::Theme(t) => self.settings.theme = t,
                PCmd::Layout(l) => self.settings.layout = l,
                PCmd::ToggleCleanup => self.settings.cleanup = !self.settings.cleanup,
                PCmd::ToggleQuietMode => self.settings.quiet_mode = !self.settings.quiet_mode,
                PCmd::ToggleDestination => {
                    self.settings.destination = match self.settings.destination {
                        Destination::LastApp => Destination::Workspace,
                        Destination::Workspace => Destination::LastApp,
                    };
                }
                PCmd::Start => self.core.send(Cmd::Start { target: winput::capture_target(), hands_free: true }),
                PCmd::Finish => self.core.send(Cmd::Finish),
                PCmd::Cancel => self.core.send(Cmd::Cancel),
                PCmd::PasteDraft => self.paste_draft(ctx),
                PCmd::ReloadEngine => self.core.send(Cmd::ReloadEngine),
            }
        }
    }

    fn floating_card(&mut self, ctx: &egui::Context) {
        let snap = &self.snap;
        let recording = snap.phase == Phase::Recording;
        let retained_outcome = snap
            .outcome
            .as_deref()
            .filter(|o| !matches!(*o, "Text typed" | "Text pasted"));
        if self.preview()
            || !self.settings.floating_card
            || (snap.phase == Phase::Idle && retained_outcome.is_none())
        {
            self.lantern.clear();
            return;
        }
        let caption = if self.settings.live_preview {
            if snap.live_note == "Preview paused" { "Preview paused" } else { "Live preview" }
        } else {
            "Recording"
        };
        let status = match (&snap.engine, snap.phase) {
            (_, Phase::Recording) => "Listening",
            (_, Phase::Working) => "Working…",
            _ => snap.outcome.as_deref().unwrap_or("Ready"),
        };
        let card = lantern::Card {
            generation: snap.generation,
            recording,
            caption,
            text: if recording && self.settings.live_preview { &snap.live_text } else { "" },
            status,
        };
        let t = ctx.input(|i| i.time);
        self.lantern.update(&card, t, self.core.level(), self.lantern_animation);
        let size = self.lantern.size(ctx, lantern::work_area_size(ctx, snap.anchor), &card);
        let title = format!("Listen - Lantern ({})", std::process::id());
        let cancel = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("listen-lantern"),
            egui::ViewportBuilder::default()
                .with_title(&title)
                .with_inner_size(size)
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_resizable(false)
                .with_taskbar(false)
                .with_active(false)
                .with_visible(false),
            |ctx, class| {
                if class == egui::ViewportClass::Embedded
                    || !self.lantern.show_without_focus(&title, snap.anchor, ctx.pixels_per_point())
                {
                    return false;
                }
                if recording {
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
                self.lantern.draw(ctx, &card, t, false)
            },
        );
        if cancel {
            self.core.send(Cmd::CancelLantern { generation: card.generation });
        }
    }

    fn ui_check_step(&mut self, ctx: &egui::Context) {
        let Some(dir) = self.ui_check.clone() else { return };
        let steps: &[(Page, SettingsSection, ThemeId, LayoutId)] = &[
            (Page::Dictate, SettingsSection::General, ThemeId::WarmCharcoal, LayoutId::Desk),
            (Page::Lab, SettingsSection::General, ThemeId::Midnight, LayoutId::Desk),
            (Page::Meetings, SettingsSection::General, ThemeId::Ocean, LayoutId::Desk),
            (Page::History, SettingsSection::General, ThemeId::Paper, LayoutId::Desk),
            (Page::Settings, SettingsSection::General, ThemeId::WarmCharcoal, LayoutId::Desk),
            (Page::Settings, SettingsSection::VoiceAi, ThemeId::Nordic, LayoutId::Desk),
            (Page::Settings, SettingsSection::Snippets, ThemeId::Plum, LayoutId::Desk),
            (Page::Settings, SettingsSection::Appearance, ThemeId::WarmCharcoal, LayoutId::Desk),
            (Page::Settings, SettingsSection::PromptLab, ThemeId::Sage, LayoutId::Sidebar),
            (Page::Settings, SettingsSection::System, ThemeId::HighContrast, LayoutId::Compact),
        ];
        ctx.request_repaint();
        if self.ui_check_step_idx < steps.len() {
            let (pg, sec, th, lay) = steps[self.ui_check_step_idx];
            self.page = pg;
            self.settings_ui.section = sec;
            self.settings.theme = th;
            self.settings.layout = lay;
            self.ui_check_step_idx += 1;
            return;
        }
        let report = json!({
            "passed": true,
            "mode": "pure-rust-egui",
            "checks": steps.iter().map(|(pg, sec, th, lay)| json!({
                "page": pg.label(),
                "section": sec.label(),
                "theme": th.label(),
                "layout": lay.label(),
                "ok": true
            })).collect::<Vec<_>>(),
        });
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("result.json"), serde_json::to_string_pretty(&report).unwrap_or_default());
        self.quitting = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frames += 1;
        ctx.request_repaint();
        if self.frames >= 30 && !self.shot_requested {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
            self.shot_requested = true;
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match image::save_buffer(&path, &bytes, w as u32, h as u32, image::ExtendedColorType::Rgba8) {
                Ok(()) => eprintln!("saved screenshot {} ({w}x{h})", path.display()),
                Err(e) => eprintln!("could not save screenshot: {e}"),
            }
            self.quitting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if self.frames > 400 {
            eprintln!("screenshot timed out");
            self.quitting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

impl eframe::App for ListenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.demo_lantern {
            let card = lantern::Card {
                generation: 1,
                recording: true,
                caption: "Live preview",
                text: lantern::SAMPLE_TEXT,
                status: "",
            };
            let t = f64::from(self.frames.min(120)) / 25.0;
            self.lantern.update(&card, t, 0.092, true);
            self.lantern.draw(ctx, &card, t, true);
            self.screenshot_step(ctx);
            return;
        }

        self.refresh_snap();
        self.drain_events();
        self.sync_meetings_ui();
        self.close_step(ctx);
        self.shortcuts(ctx);
        self.apply_settings(ctx);
        self.tick_levels();
        self.tray_step(ctx);

        let p = self.settings.theme.palette();
        let actions = self.render_shell(ctx, &p);
        self.handle_actions(ctx, actions);

        if self.palette_open {
            self.command_palette(ctx, &p);
        }
        self.floating_card(ctx);
        self.persist();

        let busy = self.snap.phase != Phase::Idle
            || self.snap.lab_busy
            || self.meeting_ui.active
            || self.meeting_ui.summarizing
            || self.installer.is_active()
            || self.benchmarker.is_active()
            || matches!(self.snap.engine, EngineState::Loading);
        if busy {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else if self.toast.is_some() || self.snap.outcome.is_some() {
            ctx.request_repaint_after(Duration::from_millis(350));
        }
        if let Some((_, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(3) {
                self.toast = None;
            }
        }
        self.ui_check_step(ctx);
        self.screenshot_step(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.demo_lantern {
            [0.0; 4]
        } else {
            self.settings.theme.palette().bg.to_normalized_gamma_f32()
        }
    }
}

fn nav_pill(ui: &mut Ui, p: &Palette, label: &str, active: bool, full_width: bool) -> egui::Response {
    let fill = if active { blend(p.surface, p.accent, 0.14) } else { Color32::TRANSPARENT };
    let stroke = if active { Stroke::new(1.0_f32, p.accent.gamma_multiply(0.75)) } else { Stroke::NONE };
    let fg = if active { p.fg } else { p.muted };
    let min_w = if full_width { ui.available_width() } else { 0.0 };
    let b = egui::Button::new(RichText::new(label).size(13.5).color(fg))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(min_w, 32.0));
    ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn parse_assessment_view(val: &Value) -> Option<LabAssessmentView> {
    let score = val
        .get("score")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| n.round() as u32);
    let gaps: Vec<String> = val
        .get("gaps")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let mut criteria = Vec::new();
    if let Some(obj) = val.get("criteria").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(n) = v.as_f64().filter(|n| n.is_finite() && *n >= 0.0) {
                criteria.push((k.replace('_', " "), n.round() as u32));
            }
        }
    }
    let summary = val.get("note").and_then(Value::as_str).unwrap_or("").to_owned();
    let by = val.get("by").and_then(Value::as_str).unwrap_or("").to_owned();
    if score.is_none() && gaps.is_empty() && criteria.is_empty() {
        return None;
    }
    Some(LabAssessmentView {
        score,
        prev_score: None,
        summary,
        gaps,
        criteria,
        by,
    })
}

fn format_hotkey_label(val: &str) -> String {
    if let Some(hk) = crate::settings_bridge::parse_hotkey(val) {
        return hk.label().to_owned();
    }
    val.split('+')
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "ctrl" => "Ctrl".into(),
            "alt" => "Alt".into(),
            "shift" => "Shift".into(),
            "arrow_right" => "Right Arrow".into(),
            "arrow_left" => "Left Arrow".into(),
            "arrow_up" => "Up Arrow".into(),
            "arrow_down" => "Down Arrow".into(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts");
    let read = |names: &[&str]| names.iter().find_map(|n| std::fs::read(dir.join(n)).ok());
    let base: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = base.clone();
    if let Some(b) = read(&["segoeui.ttf", "SegUIVar.ttf"]) {
        fonts.font_data.insert("ui".into(), Arc::new(FontData::from_owned(b)));
        prop.insert(0, "ui".into());
    }
    let mut semi = prop.clone();
    if let Some(b) = read(&["seguisb.ttf", "segoeuib.ttf"]) {
        fonts.font_data.insert("ui-semibold".into(), Arc::new(FontData::from_owned(b)));
        semi.insert(0, "ui-semibold".into());
    }
    let mut humanist = semi.clone();
    if let Some(b) = read(&["seguisb.ttf", "Candarab.ttf", "Candara.ttf"]) {
        fonts.font_data.insert("humanist".into(), Arc::new(FontData::from_owned(b)));
        humanist.insert(0, "humanist".into());
    }
    let mut transcript = prop.clone();
    if let Some(b) = read(&["segoeui.ttf", "Candara.ttf"]) {
        fonts.font_data.insert("lantern-transcript".into(), Arc::new(FontData::from_owned(b)));
        transcript.insert(0, "lantern-transcript".into());
    }
    fonts.families.insert(FontFamily::Name("lantern-transcript".into()), transcript);
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name("semibold".into()), semi);
    fonts.families.insert(FontFamily::Name("humanist".into()), humanist);
    ctx.set_fonts(fonts);
}

fn should_hide_on_close(has_tray: bool, preview: bool) -> bool {
    has_tray && !preview
}

#[cfg(test)]
mod close_policy_tests {
    use super::{parse_assessment_view, should_hide_on_close};
    use serde_json::json;

    #[test]
    fn window_close_hides_to_tray_by_default_unless_preview_or_no_tray() {
        assert!(should_hide_on_close(true, false));
        assert!(!should_hide_on_close(false, false));
        assert!(!should_hide_on_close(true, true));
        assert!(!should_hide_on_close(false, true));
    }

    #[test]
    fn parse_assessment_view_accepts_float_scores_and_criteria() {
        let raw = json!({
            "score": 8.0,
            "criteria": {
                "clarity": 9.0,
                "constraints": 7.5
            },
            "gaps": ["Add output format"],
            "note": "Strong start",
            "by": "qwen3.5:2b"
        });
        let parsed = parse_assessment_view(&raw).expect("float assessment should parse");
        assert_eq!(parsed.score, Some(8));
        assert_eq!(parsed.gaps, vec!["Add output format"]);
        assert_eq!(parsed.summary, "Strong start");
        assert_eq!(parsed.by, "qwen3.5:2b");
        assert!(parsed.criteria.iter().any(|(k, v)| k == "clarity" && *v == 9));
        assert!(parsed.criteria.iter().any(|(k, v)| k == "constraints" && *v == 8));
    }
}
