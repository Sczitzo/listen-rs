//! The window: four layouts, four pages, live themes, a command palette and the floating card.

use crate::cleanup::{self, LabAction};
use crate::engine::{Cmd, Core, EngineState, Phase, UiEvent};
use crate::storage::{
    self, Destination, Favorite, HistoryEntry, Hotkey, PromptTarget, Settings, SpeechChoice, Style,
};
use crate::theme::{self, blend, LayoutId, Palette, ThemeId};
use crate::{audio, lantern, winput};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Frame, Key, Layout, Margin, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
    ViewportCommand,
};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::sync::atomic::Ordering;
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
    pub const ALL: [Page; 5] = [Page::Dictate, Page::Lab, Page::Meetings, Page::History, Page::Settings];
    pub fn label(self) -> &'static str {
        match self {
            Page::Dictate => "Dictate",
            Page::Lab => "Prompt Lab",
            Page::Meetings => "Meetings",
            Page::History => "History",
            Page::Settings => "Settings",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Page::Dictate => "dictate",
            Page::Lab => "lab",
            Page::Meetings => "meetings",
            Page::History => "history",
            Page::Settings => "settings",
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
    pub dashboard_check: Option<PathBuf>,
    pub hardware_trial: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub theme: Option<ThemeId>,
    pub layout: Option<LayoutId>,
    pub page: Option<Page>,
    /// Screenshot mode only: draw the Dictate page as if mid-recording with live text.
    pub demo_recording: bool,
    pub demo_lantern: bool,
    pub launch_target: Option<winput::Target>,
}

impl LaunchOptions {
    pub fn original_picture(&self) -> bool {
        self.screenshot.is_some() && self.dashboard_check.is_none() && self.page.is_none()
            && self.layout.is_none() && self.theme.is_none() && !self.demo_lantern && !self.demo_recording
    }
}

#[cfg(test)]
#[test]
fn original_picture_uses_only_the_default_dictate_capture() {
    let mut options = LaunchOptions::default();
    assert!(!options.original_picture());
    options.screenshot = Some("build.png".into());
    assert!(options.original_picture());
    options.page = Some(Page::Dictate);
    assert!(!options.original_picture());
    options.page = None;
    options.layout = Some(LayoutId::Compact);
    assert!(!options.original_picture());
    options.layout = None;
    options.demo_recording = true;
    assert!(!options.original_picture());
    options.demo_recording = false;
    options.demo_lantern = true;
    assert!(!options.original_picture());
}

#[derive(Clone)]
struct Snap {
    phase: Phase,
    started: Option<Instant>,
    generation: u64,
    anchor: isize,
    engine: EngineState,
    status: String,
    lab_busy: bool,
    live_text: String,
    live_note: String,
    outcome: Option<String>,
}

const DEMO_LIVE: &str = "Okay so for the release notes I want three short sections, what changed, what got faster, and what people should try first. Keep it friendly and";

#[derive(Clone)]
enum PCmd {
    Go(Page),
    Theme(ThemeId),
    Layout(LayoutId),
    ToggleCleanup,
    ToggleDestination,
    Start,
    Finish,
    Cancel,
    PasteDraft,
    ReloadEngine,
}

#[cfg(windows)]
struct PendingIpc {
    id: u64,
    value: serde_json::Value,
    models: Option<Vec<String>>,
    ollama_available: Option<bool>,
}

#[cfg(windows)]
enum AsyncIpcJob {
    LocalModels { id: u64 },
    SetSetting { id: u64, args: Vec<serde_json::Value> },
}

pub struct ListenApp {
    #[cfg(windows)]
    dashboard: Option<crate::dashboard::Dashboard>,
    #[cfg(windows)]
    dashboard_capture: Option<Receiver<Result<(), String>>>,
    #[cfg(windows)]
    async_ipc_tx: Option<std::sync::mpsc::SyncSender<AsyncIpcJob>>,
    #[cfg(windows)]
    pending_ipc_tx: std::sync::mpsc::Sender<PendingIpc>,
    #[cfg(windows)]
    pending_ipc_rx: Receiver<PendingIpc>,
    #[cfg(windows)]
    installer: Arc<crate::models::InstallManager>,
    #[cfg(windows)]
    benchmarker: Arc<crate::models::BenchmarkManager>,
    dashboard_ready: bool,
    dashboard_error: Option<String>,
    dashboard_check: Option<PathBuf>,
    hardware_trial: Option<PathBuf>,
    workspace: crate::workspace::Workspace,
    hotkey_lease: Option<(String, Instant)>,
    lease_count: u64,
    draft_focused: bool,
    draft_change_stamp: u64,
    launch_target: Option<winput::Target>,
    core: Arc<Core>,
    rx: Receiver<UiEvent>,
    settings: Settings,
    saved: Settings,
    applied: Option<(ThemeId, u32)>,
    last_layout: LayoutId,
    page: Page,
    history: Vec<HistoryEntry>,
    favorites: Vec<Favorite>,
    retained: Vec<(String, String)>,
    draft: String,
    draft_saved: String,
    draft_changed: Option<Instant>,
    revisions: Vec<String>,
    rev_idx: usize,
    instruction: String,
    lab_error: Option<String>,
    hist_filter: String,
    hist_open: HashSet<u64>,
    confirm_clear: bool,
    palette_open: bool,
    palette_query: String,
    levels: VecDeque<f32>,
    models: Vec<String>,
    ollama_available: bool,
    mics: Vec<String>,
    screenshot: Option<PathBuf>,
    #[cfg(windows)]
    original_picture: bool,
    frames: u32,
    shot_requested: bool,
    launched: Instant,
    toast: Option<(String, Instant)>,
    snap: Snap,
    demo_recording: bool,
    demo_lantern: bool,
    lantern: lantern::State,
    lantern_animation: bool,
    /// Native window handle of the main window (0 when unknown), for hide/show from the tray.
    hwnd: isize,
    tray: Option<crate::tray::Tray>,
    /// The draft has been saved and the next close request can exit.
    quitting: bool,
    hide_test: Option<(Instant, u32)>,
}

const METER_LEN: usize = 220;
/// Transcript box: text size, and the height it keeps while recording.
const LIVE_TEXT_PX: f32 = 20.0;
/// Dictate hero (Check 6, measured from the person's reference screenshot): headline size,
/// key picture size, gap below the hero, and the single Style dropdown chip.
const HERO_TITLE_PX: f32 = 25.0;
const HERO_SUB_MAX_W: f32 = 380.0;
const HERO_KEY_PX: f32 = 100.0;
const HERO_GAP_PX: f32 = 36.0;
const STYLE_CHIP_W: f32 = 150.0;
const STYLE_CHIP_H: f32 = 34.0;
/// Top rail height; the reference shot measures 66.5 pt including its 1 px bottom line.
const RAIL_H: f32 = 66.0;
const LIVE_TEXT_MIN_H: f32 = 110.0;
const LIVE_TEXT_MAX_H: f32 = 240.0;

impl ListenApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, opts: LaunchOptions) -> Self {
        install_fonts(&cc.egui_ctx);
        let mut settings = settings;
        if opts.screenshot.is_some() {
            // Screenshots start from the original's look; flags below can still pick another.
            settings.theme = ThemeId::WarmCharcoal;
            settings.layout = LayoutId::Desk;
        }
        if let Some(t) = opts.theme {
            settings.theme = t;
        }
        if let Some(l) = opts.layout {
            settings.layout = l;
        }
        let profile = opts.dashboard_check.clone()
            .or_else(|| opts.screenshot.as_ref().map(|file| file.with_extension("profile")))
            .unwrap_or_else(storage::data_dir);
        let original_picture = opts.original_picture();
        let mut dashboard_error = None;
        #[cfg(windows)]
        let dashboard = if !opts.demo_lantern {
            let initial = serde_json::json!({
                "appearance": crate::dashboard::appearance(&settings),
                "page": opts.page.unwrap_or(Page::Dictate).key(),
                "originalPicture": original_picture,
            });
            let bg = settings.theme.palette().bg;
            match crate::dashboard::Dashboard::new(cc, &cc.egui_ctx, profile.join("WebView2"),
                opts.dashboard_check.is_some(), opts.screenshot.is_some(), &initial,
                (bg.r(), bg.g(), bg.b(), 255)) {
                Ok(view) => Some(view),
                Err(error) => {
                    crate::diagnostics::error_at(&profile, "Listen could not open its dashboard.");
                    dashboard_error = Some(error.to_string());
                    None
                }
            }
        } else { None };
        let preview = opts.screenshot.is_some() || opts.dashboard_check.is_some() || dashboard_error.is_some();
        if preview {
            settings.zoom = 1.0;
        }
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
        let draft = if preview { String::new() } else { storage::load_draft() };
        let layout = settings.layout;
        #[cfg(windows)]
        let installer = Arc::new(crate::models::InstallManager::new());
        #[cfg(windows)]
        let benchmarker = Arc::new(crate::models::BenchmarkManager::new());
        #[cfg(windows)]
        let (async_ipc_tx, pending_ipc_tx, pending_ipc_rx) = {
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let job_tx = if !preview {
                let (tx, rx) = std::sync::mpsc::sync_channel::<AsyncIpcJob>(32);
                let worker_core = core.clone();
                let worker_ctx = cc.egui_ctx.clone();
                let worker_done_tx = done_tx.clone();
                let _ = std::thread::Builder::new().name("settings-ipc".into()).spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let done = match job {
                            AsyncIpcJob::LocalModels { id } => {
                                let settings = worker_core.settings.lock().unwrap().clone();
                                let (value, models, ollama_ok) = crate::settings_bridge::local_models_response(
                                    false,
                                    &settings.ollama_url,
                                    settings.local_model,
                                );
                                PendingIpc { id, value, models, ollama_available: Some(ollama_ok) }
                            }
                            AsyncIpcJob::SetSetting { id, args } => {
                                let value = crate::settings_bridge::call(&worker_core, "set_setting", &args)
                                    .unwrap_or(serde_json::json!(false));
                                PendingIpc { id, value, models: None, ollama_available: None }
                            }
                        };
                        if worker_done_tx.send(done).is_err() {
                            break;
                        }
                        worker_ctx.request_repaint();
                    }
                });
                Some(tx)
            } else {
                None
            };
            (job_tx, done_tx, done_rx)
        };
        ListenApp {
            #[cfg(windows)]
            dashboard,
            #[cfg(windows)]
            dashboard_capture: None,
            #[cfg(windows)]
            async_ipc_tx,
            #[cfg(windows)]
            pending_ipc_tx,
            #[cfg(windows)]
            pending_ipc_rx,
            #[cfg(windows)]
            installer,
            #[cfg(windows)]
            benchmarker,
            dashboard_ready: false,
            dashboard_error,
            workspace: crate::workspace::Workspace::load(&profile),
            dashboard_check: opts.dashboard_check.clone(),
            hardware_trial: opts.hardware_trial,
            hotkey_lease: None,
            lease_count: 0,
            draft_focused: false,
            draft_change_stamp: 0,
            launch_target: opts.launch_target,
            mics: if preview { Vec::new() } else { audio::input_devices() },
            history: if original_picture {
                let (when, unix) = storage::now_stamp();
                [
                    "Send the shorter draft to the team. Leave a note about what changed.",
                    "Read the examples out loud before the meeting. Flag any sentence that sounds awkward, especially where the reader needs to pause or take a breath.",
                    "Move the longer explanation to the end of the outline.",
                    "Put the Voice and Vibe notes beside the Opus sample. Include the Craft outline and the Maybe folder under Code.",
                    "Keep the Voice sample in Vibe, and I'd like to try Paseo and send the short Opus example next. Replay the Voice clip, and I'd keep the sample.",
                    "Open the Voice and Vibe demos, then the Opus example in Paseo, and I'd keep the notes about Craft and Code. Save the Voice and Vibe clips, and I'd keep Paseo open.",
                    "Compare the Voice recording with Vibe before opening Code. Share the Opus notes in Paseo. Keep Voice and Vibe with the Opus sample, and I'd send them tomorrow.",
                    concat!(
                        "Try the Voice example in Vibe and put Maybe on the list, and I'd leave the rest in the shared folder. ",
                        "If the first take is too fast, do it once more. Keep both cuts so we can hear the change. ",
                        "Use the one that has a bit more space at the end. Let the team hear it and see if it is clear. ",
                        "If they want a new take, ask which bit they want us to say again. We can do that in the next pass. ",
                        "Send the best cut with a short note on what we did and why."
                    ),
                ].into_iter().map(|raw| HistoryEntry {
                    when: when.clone(), unix, raw: raw.into(), seconds: 2.5,
                    outcome: "saved · delivery not confirmed".into(), app_name: "Sample editor".into(),
                    ..Default::default()
                }).collect()
            } else if opts.dashboard_check.is_some() {
                let (when, unix) = storage::now_stamp();
                vec![
                    HistoryEntry {
                        when: "2000-01-01 12:00".into(), unix: 946_728_000,
                        raw: "Older synthetic History sample".into(), ..Default::default()
                    },
                    HistoryEntry {
                        when, unix, raw: "Synthetic sample for History".into(), seconds: 2.5,
                        outcome: "saved · delivery not confirmed".into(), app_name: "Sample editor".into(),
                        ..Default::default()
                    },
                ]
            } else if preview { Vec::new() } else { storage::load_history() },
            favorites,
            saved: settings.clone(),
            applied: Some((settings.theme, settings.zoom.to_bits())),
            last_layout: layout,
            settings,
            core,
            rx,
            page: opts.page.unwrap_or(Page::Dictate),
            retained: Vec::new(),
            draft_saved: draft.clone(),
            draft,
            draft_changed: None,
            revisions: Vec::new(),
            rev_idx: 0,
            instruction: String::new(),
            lab_error: None,
            hist_filter: String::new(),
            hist_open: HashSet::new(),
            confirm_clear: false,
            palette_open: false,
            palette_query: String::new(),
            levels: VecDeque::from(vec![0.0; METER_LEN]),
            models: if original_picture { vec!["Sample local model".into()] } else { Vec::new() },
            ollama_available: original_picture,
            screenshot: opts.screenshot,
            #[cfg(windows)]
            original_picture,
            frames: 0,
            shot_requested: false,
            launched: Instant::now(),
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
        }
    }

    fn preview(&self) -> bool {
        self.core.preview
    }

    fn dashboard_active(&self) -> bool {
        #[cfg(windows)]
        { self.dashboard.is_some() }
        #[cfg(not(windows))]
        { false }
    }

    fn open_page(&mut self, page: Page) {
        self.page = page;
        #[cfg(windows)]
        if let Some(view) = &self.dashboard {
            view.evaluate(&format!(
                "document.querySelector('button.rail-btn[data-page=\"{}\"]')?.click()",
                page.key()
            ));
        }
    }

    #[cfg(windows)]
    fn dashboard_step(&mut self, ctx: &egui::Context) {
        if self.hotkey_lease.as_ref().is_some_and(|(_, expiration)| Instant::now() >= *expiration) {
            self.hotkey_lease = None;
            *self.core.controller_capture.lock().unwrap() = None;
            self.core.capture_paused.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        while let Ok(done) = self.pending_ipc_rx.try_recv() {
            if let Some(models) = done.models {
                self.models = models;
            }
            if let Some(ok) = done.ollama_available {
                self.ollama_available = ok;
            }
            if let Some(dashboard) = self.dashboard.as_ref() {
                dashboard.respond(done.id, done.value);
            }
        }
        if self.core.phase() != Phase::Idle {
            self.benchmarker.cancel();
        }
        for _ in 0..128 {
            let Some(call) = self.dashboard.as_ref().and_then(|dashboard| dashboard.requests.try_recv().ok()) else { break };
            if self.enqueue_async_dashboard_call(ctx, &call) {
                continue;
            }
            let value = self.dashboard_call(ctx, &call.method, &call.args);
            self.core.shared.lock().unwrap().active_draft = self.draft_focused
                .then(|| (self.workspace.context(), self.draft_change_stamp));
            self.dashboard.as_ref().unwrap().respond(call.id, value);
        }
        self.settings = self.core.settings.lock().unwrap().clone();
        self.apply_settings(ctx);
        if let Some(trial) = &self.hardware_trial {
            if matches!(self.snap.engine, EngineState::Ready(_)) && !trial.join("speech-ready").is_file() {
                let _ = std::fs::write(trial.join("speech-ready"), b"ready");
            }
            let close_request = trial.join("quit-request");
            if self.snap.phase == Phase::Idle && close_request.is_file() {
                if std::fs::remove_file(close_request).is_ok() {
                    self.request_quit(ctx);
                }
            }
        }
    }

    #[cfg(windows)]
    fn enqueue_async_dashboard_call(&self, ctx: &egui::Context, call: &crate::dashboard::Call) -> bool {
        if self.core.preview {
            return false;
        }
        if call.method == "check_model_updates" {
            let id = call.id;
            let core = self.core.clone();
            let installer = Arc::clone(&self.installer);
            let benchmarker = Arc::clone(&self.benchmarker);
            let tx = self.pending_ipc_tx.clone();
            let ctx = ctx.clone();
            return std::thread::Builder::new()
                .name("model-check".into())
                .spawn(move || {
                    let settings = core.settings.lock().unwrap().clone();
                    let (value, models, ollama_ok) =
                        crate::models::check_updates(&settings, false, &installer, &benchmarker);
                    let _ = tx.send(PendingIpc { id, value, models, ollama_available: Some(ollama_ok) });
                    ctx.request_repaint();
                })
                .is_ok();
        }
        let Some(tx) = self.async_ipc_tx.as_ref() else {
            return false;
        };
        let job = if call.method == "get_local_models" {
            AsyncIpcJob::LocalModels { id: call.id }
        } else if call.method == "set_setting"
            && call
                .args
                .first()
                .and_then(serde_json::Value::as_str)
                .is_some_and(|key| {
                    matches!(key, "local_model" | "engine_local_model" | "judge_local_model")
                })
        {
            AsyncIpcJob::SetSetting { id: call.id, args: call.args.clone() }
        } else {
            return false;
        };
        tx.try_send(job).is_ok()
    }

    #[cfg(windows)]
    fn run_workspace_stage(&mut self, request: crate::workspace::LocalRequest, settings: Settings, checking: bool) -> bool {
        let (provider, model) = cleanup::prompt_route(&settings, checking);
        let model = model.to_owned();
        let id = request.id;
        if self.dashboard_check.is_some() {
            let result = if checking {
                let score = if request.text.starts_with("Refined") { 9 } else { 8 };
                serde_json::json!({"score": score, "criteria": {
                    "clarity": 5, "context": 4, "task": 4, "constraints": 4,
                    "success_criteria": 4, "format": 4
                }, "gaps": ["Add a verification step"], "improved": "Reviewed suggestion."}).to_string()
            } else if request.stage == crate::workspace::StageKind::Engineer {
                "Engineered dashboard draft.".into()
            } else if request.stage == crate::workspace::StageKind::Refine {
                if request.text.starts_with("Refined") { request.text } else { "Refined dashboard draft.".into() }
            } else {
                "Synthetic suggestion for review.".into()
            };
            if let Some(follow_up) = self.workspace.finish_local(id, Ok(result), &model) {
                return self.run_workspace_stage(follow_up, settings, true);
            }
            return true;
        }
        let core = self.core.clone();
        let draft_stamp = self.draft_change_stamp;
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
            core.emit(UiEvent::LocalSuggestion { id, result, model, draft_stamp, settings: Box::new(settings) });
        });
        if started.is_err() {
            self.workspace.finish_local(id, Err("Listen could not start the request. Try again.".into()), "");
            return false;
        }
        true
    }

    #[cfg(windows)]
    fn dashboard_call(&mut self, ctx: &egui::Context, name: &str, args: &[serde_json::Value]) -> serde_json::Value {
        use serde_json::{json, Value};
        let text = |idx| args.get(idx).and_then(Value::as_str).unwrap_or("");
        let copy = |text: &str| {
            !text.is_empty()
                && (self.core.preview
                    || (!winput::clipboard_recovery_pending()
                        && arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)).is_ok()))
        };
        match name {
            "__dashboard_ready" => {
                self.dashboard_ready = true;
                json!(true)
            }
            "set_appearance" => {
                let mut settings = self.core.settings.lock().unwrap();
                let mut next = settings.clone();
                match text(0) {
                    "theme" => match ThemeId::from_key(text(1)) {
                        Some(theme) => next.theme = theme,
                        None => return Value::Null,
                    },
                    "layout" => match LayoutId::from_key(text(1)) {
                        Some(layout) => next.layout = layout,
                        None => return Value::Null,
                    },
                    _ => return Value::Null,
                }
                if !self.core.preview && storage::save_settings_checked(&next).is_err() {
                    return Value::Null;
                }
                *settings = next;
                crate::dashboard::appearance(&settings)
            }
            "__draft_focus" => {
                self.draft_focused = args.first().and_then(Value::as_bool) == Some(true);
                json!(true)
            }
            "__draft_changed" => {
                self.draft_change_stamp = self.draft_change_stamp.wrapping_add(1);
                json!(true)
            }
            "get_state" => crate::dashboard_state::state(&self.core, self.history.len(), self.workspace.version(), self.workspace.busy(), &self.models),
            "get_history" => crate::dashboard_state::history(
                &self.history,
                args.first().and_then(Value::as_bool) == Some(true),
                !self.core.preview && winput::clipboard_recovery_pending(),
            ),
            "get_stats" => crate::dashboard_state::stats(&self.history, if self.core.preview { 0 } else { storage::unpriced_api_attempts() }),
            "get_delivery_status" => winput::delivery_status(),
            "restore_delivery_clipboard" => winput::restore_delivery_clipboard(
                args.first().and_then(Value::as_bool) == Some(true),
            ),
            "discard_delivery_clipboard" => winput::discard_delivery_clipboard(
                args.first().and_then(Value::as_bool) == Some(true),
            ),
            "copy_text" => json!(copy(text(0))),
            "copy_retained" => json!(args.first().and_then(Value::as_u64)
                .and_then(|id| self.workspace.retained_text(id)).is_some_and(copy)),
            "finish_recording" | "cancel_recording" => {
                let active = self.core.phase() == Phase::Recording;
                if active {
                    if self.dashboard_check.is_some() {
                        let mut live = self.core.shared.lock().unwrap();
                        live.phase = Phase::Idle;
                        live.live_text.clear();
                        live.started = None;
                    } else {
                        self.core.send(if name == "finish_recording" { Cmd::Finish } else { Cmd::Cancel });
                    }
                }
                json!(active)
            }
            "retry_startup" => {
                let retry = !self.core.preview && self.core.phase() == Phase::Idle
                    && matches!(self.snap.engine, EngineState::Unavailable(_));
                if retry { self.core.send(Cmd::ReloadEngine); }
                json!(retry)
            }
            "quit_app" => {
                match self.quit_app(ctx) {
                    Ok(()) => json!(true),
                    Err(message) => json!(message),
                }
            }
            "paste_draft" => {
                let draft = text(0);
                if self.core.preview
                    || self.core.phase() != Phase::Idle
                    || winput::clipboard_recovery_pending()
                    || !self.workspace.saved_draft_matches(draft)
                {
                    return json!(false);
                }
                let target = self.core.shared.lock().unwrap().last_target.filter(|target| !target.is_ours())
                    .or(self.launch_target);
                let Some(target) = target else { return json!(false) };
                if !self.core.queue_draft_delivery(draft.to_owned(), target) { return json!(false); }
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                json!(true)
            }
            "submit" | "feedback" | "draft_pass" | "review_candidate" | "improve" | "rebuild" => {
                if (self.core.preview && self.dashboard_check.is_none()) || self.core.phase() != Phase::Idle {
                    return json!(false);
                }
                if name == "improve" {
                    if let Some(saved) = self.workspace.reuse_feedback_for_improvement() {
                        return json!(saved);
                    }
                }
                let settings = self.core.settings.lock().unwrap().clone();
                let checking = matches!(name, "feedback" | "review_candidate" | "improve");
                let (_, model) = cleanup::prompt_route(&settings, checking);
                let Some(request) = self.workspace.begin_local(name, args, model, settings.prompt_target) else {
                    return json!(false);
                };
                let id = request.id;
                if !self.run_workspace_stage(request, settings, checking) {
                    return json!(false);
                }
                if matches!(name, "feedback" | "draft_pass") { json!(id) } else { json!(true) }
            }
            "__check_recording" if self.dashboard_check.is_some() => {
                let mut live = self.core.shared.lock().unwrap();
                live.phase = Phase::Recording;
                live.rec_gen += 1;
                live.hands_free = true;
                live.started = Some(Instant::now());
                live.live_text = lantern::SAMPLE_TEXT.into();
                self.core.level.store(0.08_f32.to_bits(), Ordering::Relaxed);
                json!(true)
            }
            "__check_lantern" if self.dashboard_check.is_some() => json!(self.lantern.native_check()),
            "__check_done" if self.dashboard_check.is_some() => {
                let directory = self.dashboard_check.as_ref().unwrap();
                let data = args.first().unwrap_or(&Value::Null);
                let _ = std::fs::write(directory.join("result.json"), data.to_string());
                self.quitting = data["passed"].as_bool() != Some(true);
                ctx.send_viewport_cmd(ViewportCommand::Close);
                json!(true)
            }
            "begin_hotkey_capture" | "begin_controller_hotkey_capture" | "check_hotkey_capture" | "finish_hotkey_capture" => {
                self.hotkey_call(name, args)
            }
            "get_vocab_suggestions" if self.screenshot.is_some() => {
                let settings = self.core.settings.lock().unwrap();
                json!(storage::suggest_vocab(&self.history, &settings.vocab, &settings.vocab_dismissed, 8))
            }
            "get_model_catalog" => {
                let settings = self.core.settings.lock().unwrap().clone();
                let tag = self.installer.whisper_tag();
                let saved = crate::models::load_benchmarks(self.core.preview);
                crate::models::catalog_snapshot(
                    &settings,
                    &self.models,
                    self.ollama_available || !self.models.is_empty(),
                    self.installer.status(),
                    self.benchmarker.status(),
                    &saved,
                    Some(&tag),
                    false,
                    "",
                )
            }
            "check_model_updates" => {
                let settings = self.core.settings.lock().unwrap().clone();
                crate::models::check_updates(&settings, self.core.preview, &self.installer, &self.benchmarker).0
            }
            "install_model" => {
                if self.benchmarker.is_active() {
                    return json!({"ok": false, "message": "Wait for or cancel the running benchmark before downloading a model."});
                }
                let url = self.core.settings.lock().unwrap().ollama_url.clone();
                self.installer.start(text(0), text(1), url, self.core.preview)
            }
            "get_install_status" => self.installer.status(),
            "cancel_install" => json!(self.installer.cancel()),
            "start_benchmark" => {
                if self.core.phase() != Phase::Idle || self.workspace.busy() {
                    return json!({"ok": false, "message": "Finish the current recording or Prompt Lab task before running a benchmark."});
                }
                if self.installer.is_active() {
                    return json!({"ok": false, "message": "Wait for the model download to finish before running a benchmark."});
                }
                let settings = self.core.settings.lock().unwrap().clone();
                self.benchmarker.start(settings, text(0), self.core.preview)
            }
            "get_benchmark_status" => self.benchmarker.status(),
            "cancel_benchmark" => json!(self.benchmarker.cancel()),
            "clear_private_data" if text(0) == "history" => {
                if text(1) != "DELETE" || self.core.phase() != Phase::Idle {
                    return json!({ "ok": false, "message": "Finish the current recording before clearing History." });
                }
                if !self.core.preview && std::fs::remove_file(storage::data_dir().join("history.jsonl"))
                    .is_err_and(|error| error.kind() != std::io::ErrorKind::NotFound) {
                    return json!({ "ok": false, "message": "History could not be cleared." });
                }
                self.history.clear();
                json!({ "ok": true, "message": "History cleared" })
            }
            "clear_private_data" if text(0) == "logs" => {
                if text(1) != "DELETE" {
                    return json!({ "ok": false, "message": "Canceled. Nothing was changed." });
                }
                if self.core.phase() != Phase::Idle {
                    return json!({ "ok": false, "message": "Finish the current recording before clearing app logs." });
                }
                if !self.core.preview && crate::settings_bridge::clear_log_files(&storage::data_dir()).is_err() {
                    return json!({ "ok": false, "message": "App logs could not be cleared." });
                }
                json!({ "ok": true, "message": "App logs cleared" })
            }
            "clear_private_data" if text(0) == "learned" => {
                if text(1) != "DELETE" {
                    return json!({ "ok": false, "message": "Canceled. Nothing was changed." });
                }
                if self.core.phase() != Phase::Idle {
                    return json!({ "ok": false, "message": "Finish the current recording before clearing saved items." });
                }
                json!({ "ok": true, "message": "Learned preferences cleared" })
            }
            _ => self.workspace.call(name, args)
                .or_else(|| crate::settings_bridge::call(&self.core, name, args)).unwrap_or(json!(false)),
        }
    }

    #[cfg(windows)]
    fn hotkey_call(&mut self, name: &str, args: &[serde_json::Value]) -> serde_json::Value {
        use serde_json::{json, Value};
        let text = |idx| args.get(idx).and_then(Value::as_str).unwrap_or("");
        if name == "begin_hotkey_capture" {
            if self.core.phase() != Phase::Idle || self.hotkey_lease.is_some() {
                return json!({ "ok": false });
            }
            self.lease_count += 1;
            let token = format!("capture-{}", self.lease_count);
            self.hotkey_lease = Some((token.clone(), Instant::now() + Duration::from_secs(60)));
            self.core.capture_paused.store(true, std::sync::atomic::Ordering::Relaxed);
            return json!({ "ok": true, "token": token, "expires_in": 60 });
        }
        if !self.hotkey_lease.as_ref().is_some_and(|(token, _)| token == text(0)) {
            return json!({ "ok": false, "message": "Hotkey setup expired. Open it again." });
        }
        if name == "begin_controller_hotkey_capture" {
            let connected = if self.core.preview { [None; 4] } else { crate::controller::read() };
            if connected.iter().all(Option::is_none) {
                return json!({ "ok": false, "message": "Connect an Xbox-compatible controller and try again." });
            }
            if connected.iter().flatten().count() > 1 {
                return json!({ "ok": false, "message": "Keep one controller connected while choosing the shortcut." });
            }
            *self.core.controller_capture.lock().unwrap() = Some(crate::controller::Capture::new(text(0).to_owned(), connected));
            return json!({ "ok": true });
        }
        let choice = text(1);
        if choice.is_empty() && name == "finish_hotkey_capture" {
            self.hotkey_lease = None;
            *self.core.controller_capture.lock().unwrap() = None;
            self.core.capture_paused.store(false, std::sync::atomic::Ordering::Relaxed);
            return json!({ "ok": true });
        }
        if name == "check_hotkey_capture" {
            let (level, message) = winput::check_hotkey(&self.core, choice);
            return json!({ "ok": true, "level": level, "message": message });
        }
        let acknowledged = args.get(2).and_then(Value::as_bool).unwrap_or(false);
        let saved = winput::save_hotkey(&self.core, choice, acknowledged);
        if saved {
            self.hotkey_lease = None;
            *self.core.controller_capture.lock().unwrap() = None;
            self.core.capture_paused.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        json!({ "ok": saved, "hotkey": choice, "message": if saved { "" } else { "This shortcut could not be saved. Pick again or keep your current shortcut." } })
    }

    /// Handle tray choices and keep the icon, tooltip and favorites current.
    fn tray_step(&mut self, ctx: &egui::Context) {
        use crate::tray::{TrayAction, TrayState};
        let Some(tray) = self.tray.as_mut() else { return };
        for a in tray.poll() {
            if matches!(a, TrayAction::Open | TrayAction::PromptLab | TrayAction::Settings) {
                winput::show_window(self.hwnd);
            }
            match a {
                TrayAction::Open => {}
                TrayAction::PromptLab => self.open_page(Page::Lab),
                TrayAction::Settings => self.open_page(Page::Settings),
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
                            self.toast = Some((format!("Copied “{name}”"), Instant::now()));
                        } else {
                            self.toast = Some(("Couldn't reach the clipboard".into(), Instant::now()));
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
            (_, Phase::Recording) => "listening".to_string(),
            (_, Phase::Working) => "working".to_string(),
            (EngineState::Loading, _) => "loading".to_string(),
            (EngineState::Unavailable(_), _) => "no speech engine".to_string(),
            (EngineState::Ready(_), _) => "ready".to_string(),
        };
        if let Some(t) = self.tray.as_mut() {
            t.sync(state, &status, &self.favorites);
        }
        // Diagnostic (LISTEN_HIDE_TEST=<file>): hide the window, count frames for 3 s, write the
        // count and exit. Shows whether the app keeps running its frame loop while hidden.
        if let Ok(path) = std::env::var("LISTEN_HIDE_TEST") {
            match self.hide_test {
                None => self.hide_test = Some((Instant::now(), 0)),
                Some((at, n)) => {
                    // eframe shows the window after its first frames, so hide a little later.
                    if at.elapsed() > Duration::from_millis(800) {
                        winput::hide_window(self.hwnd);
                        self.hide_test = Some((at, n + 1));
                    }
                    if at.elapsed() > Duration::from_millis(3800) {
                        let _ = std::fs::write(&path, format!("frames_while_hidden={} visible={}\n", n + 1, winput::window_visible(self.hwnd)));
                        std::process::exit(0);
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        // Keep the tray icon current while hidden and idle.
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn toast(&mut self, s: &str) {
        self.toast = Some((s.to_string(), Instant::now()));
    }

    // ---------------------------------------------------------------- state upkeep

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
            self.snap.started = Some(Instant::now() - Duration::from_secs(14));
            self.snap.live_text = DEMO_LIVE.to_string();
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                UiEvent::History(e) => self.history.push(e),
                UiEvent::Retained { text, reason } => {
                    if !self.dashboard_active() {
                        self.retained.push((text, reason));
                    } else {
                        self.workspace.retain(&text, &reason);
                    }
                }
                UiEvent::ToWorkspace(t, draft) => {
                    if self.dashboard_active() {
                        if let Some((context, _)) = draft.filter(|(_, stamp)| *stamp == self.draft_change_stamp) {
                            self.workspace.accept_dictation(&t, context);
                        } else {
                            self.workspace.retain(&t, "Review this dictation before adding it to a draft.");
                        }
                        continue;
                    }
                    let mut next = self.draft.trim_end().to_string();
                    if !next.is_empty() {
                        next.push_str("\n\n");
                    }
                    next.push_str(t.trim());
                    self.set_draft_revision(next);
                }
                UiEvent::LabDone(Ok(t)) => {
                    self.lab_error = None;
                    self.set_draft_revision(t);
                }
                UiEvent::LabDone(Err(e)) => self.lab_error = Some(e),
                UiEvent::LocalSuggestion { id, result, model, draft_stamp, settings } => {
                    if draft_stamp != self.draft_change_stamp {
                        self.workspace.cancel_local(id);
                        continue;
                    }
                    if let Some(request) = self.workspace.finish_local(id, result, &model) {
                        #[cfg(windows)]
                        self.run_workspace_stage(request, *settings, true);
                        #[cfg(not(windows))]
                        self.workspace.cancel_local(request.id);
                    }
                }
                UiEvent::Models(m) => self.models = m,
            }
        }
    }

    fn set_draft_revision(&mut self, next: String) {
        if self.revisions.is_empty() {
            self.revisions.push(self.draft.clone());
            self.rev_idx = 0;
        }
        self.revisions.truncate(self.rev_idx + 1);
        if self.revisions.last() != Some(&self.draft) {
            self.revisions.push(self.draft.clone());
        }
        self.revisions.push(next.clone());
        self.rev_idx = self.revisions.len() - 1;
        self.draft = next;
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
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(900.0, 700.0)));
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
        if self.draft != self.draft_saved {
            let first = *self.draft_changed.get_or_insert_with(Instant::now);
            if first.elapsed() > Duration::from_millis(600) {
                if self.preview() || storage::save_draft(&self.draft).is_ok() {
                    self.draft_saved = self.draft.clone();
                    self.draft_changed = None;
                } else {
                    self.draft_changed = Some(Instant::now());
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let c = Modifiers::COMMAND;
        let (p1, p2, p3, p4, k, enter, s, t, l, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(c, Key::Num1),
                i.consume_key(c, Key::Num2),
                i.consume_key(c, Key::Num3),
                i.consume_key(c, Key::Num4),
                i.consume_key(c, Key::K),
                i.consume_key(c, Key::Enter),
                i.consume_key(c, Key::S),
                i.consume_key(c, Key::T),
                i.consume_key(c, Key::L),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if p1 {
            self.page = Page::Dictate;
        }
        if p2 {
            self.page = Page::Lab;
        }
        if p3 {
            self.page = Page::History;
        }
        if p4 {
            self.page = Page::Settings;
        }
        if k {
            self.palette_open = !self.palette_open;
            self.palette_query.clear();
        }
        if enter {
            self.paste_draft(ctx);
        }
        if s {
            self.save_favorite();
        }
        if t {
            self.settings.theme = self.settings.theme.next();
        }
        if l {
            self.settings.layout = self.settings.layout.next();
        }
        if esc {
            if self.palette_open {
                self.palette_open = false;
            } else if self.snap.phase == Phase::Recording {
                self.core.send(Cmd::Cancel);
            } else if self.confirm_clear {
                self.confirm_clear = false;
            }
        }
    }

    fn paste_draft(&mut self, _ctx: &egui::Context) {
        let text = self.draft.trim().to_string();
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
        let target = self.core.shared.lock().unwrap().last_target.filter(|target| !target.is_ours())
            .or(self.launch_target);
        if target.is_some_and(|target| self.core.queue_draft_delivery(text, target)) {
            _ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        } else {
            self.toast("Choose Copy and paste where you want the draft.");
        }
    }

    fn save_favorite(&mut self) {
        let text = self.draft.trim().to_string();
        if text.is_empty() {
            self.toast("Nothing to save yet");
            return;
        }
        let name: String = text.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
        let (when, _) = storage::now_stamp();
        self.favorites.insert(0, Favorite { name, text, when, uses: 0 });
        if !self.preview() {
            storage::save_favorites(&self.favorites);
        }
        self.toast("Saved to favorites");
    }

    fn start_dictation(&self) {
        self.core.send(Cmd::Start { target: winput::capture_target(), hands_free: true });
    }

    fn run_pcmd(&mut self, ctx: &egui::Context, cmd: PCmd) {
        match cmd {
            PCmd::Go(p) => self.page = p,
            PCmd::Theme(t) => self.settings.theme = t,
            PCmd::Layout(l) => self.settings.layout = l,
            PCmd::ToggleCleanup => self.settings.cleanup = !self.settings.cleanup,
            PCmd::ToggleDestination => {
                self.settings.destination = match self.settings.destination {
                    Destination::LastApp => Destination::Workspace,
                    Destination::Workspace => Destination::LastApp,
                }
            }
            PCmd::Start => self.start_dictation(),
            PCmd::Finish => self.core.send(Cmd::Finish),
            PCmd::Cancel => self.core.send(Cmd::Cancel),
            PCmd::PasteDraft => self.paste_draft(ctx),
            PCmd::ReloadEngine => self.core.send(Cmd::ReloadEngine),
        }
    }

    // ---------------------------------------------------------------- layouts

    fn desk(&mut self, ctx: &egui::Context, p: &Palette) {
        // The original's top rail (app.css L35-40, L339, L341; index.html L30-36).
        egui::TopBottomPanel::top("rail")
            .exact_height(RAIL_H)
            .show_separator_line(false)
            .frame(Frame::default().fill(p.nav).inner_margin(Margin::symmetric(27, 0)))
            .show(ctx, |ui| {
                // Only a bottom border, `border-bottom: 1px solid var(--line)` (app.css 35),
                // snapped to whole pixels so it stays crisp at any scale.
                let full = ui.max_rect().expand2(vec2(27.0, 0.0));
                let ppp = ui.ctx().pixels_per_point();
                let y1 = (full.bottom() * ppp).round() / ppp;
                let y0 = y1 - (ppp.round().max(1.0) / ppp);
                ui.painter().rect_filled(Rect::from_x_y_ranges(full.x_range(), y0..=y1), CornerRadius::ZERO, p.line);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    brand_mark(ui, 28.0);
                    ui.add_space(2.0);
                    ui.label(RichText::new("Listen").font(FontId::new(21.0, FontFamily::Name("humanist".into()))).color(p.fg));
                    ui.add_space(19.0);
                    for pg in Page::ALL {
                        let active = self.page == pg;
                        let r = rail_button(ui, p, Some(page_icon(pg)), pg.label(), active);
                        if r.clicked() {
                            self.page = pg;
                        }
                        if active {
                            let line = Rect::from_min_max(pos2(r.rect.left(), ui.max_rect().bottom() - 2.0), pos2(r.rect.right(), ui.max_rect().bottom()));
                            ui.painter().rect_filled(line, CornerRadius::ZERO, p.accent);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let t = ui.ctx().input(|i| i.time);
                        // rail-status (index.html): the dot plus a short processing note.
                        let note = match &self.toast {
                            Some((m, at)) if at.elapsed() < Duration::from_millis(2500) => Some(m.clone()),
                            _ if self.snap.phase == Phase::Working => Some(short_status(&self.snap.status)),
                            _ => None,
                        };
                        if let Some(m) = note {
                            ui.label(RichText::new(m).size(12.0).color(p.muted));
                        }
                        status_dot(ui, p, &self.snap, t).on_hover_text(format!(
                            "{}\nSpeech: {}\n{} · {} (Ctrl+T for the next theme)",
                            live_status(&self.snap),
                            self.core.shared.lock().unwrap().engine_label(),
                            self.settings.theme.label(),
                            self.settings.layout.label()
                        ));
                        if rail_button(ui, p, Some(Icon::Power), "Quit", false).on_hover_text("Quit Listen").clicked() {
                            self.request_quit(ui.ctx());
                        }
                    });
                });
            });
        // The original's desk window has no bottom status bar (Check 6); its notes live in the rail.
        self.central(ctx, p, 920.0, Margin { left: 36, right: 36, top: 31, bottom: 28 });
    }

    fn request_quit(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        if let Some(dashboard) = self.dashboard.as_ref().filter(|_| self.dashboard_ready) {
            if dashboard.request_quit().is_err() {
                crate::diagnostics::error("Listen could not ask its dashboard to save and quit.");
                winput::show_quit_error(self.hwnd);
            }
            return;
        }
        if let Err(message) = self.quit_app(ctx) {
            self.toast(message);
        }
    }

    fn close_step(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.quitting
            || (self.preview() && self.dashboard_check.is_none()) {
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        if self.tray.is_some() && std::env::var_os("LISTEN_PARK_ON_CLOSE").is_some() {
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
        if phase == Phase::Working || lab_busy || self.workspace.busy()
            || self.core.draft_delivery_pending.load(Ordering::Acquire) {
            return reject(&self.core, "Listen is still working. Wait for it to finish, then close Listen.");
        }
        self.drain_events();
        if self.workspace.has_unsaved_recovery() || !self.retained.is_empty() {
            crate::diagnostics::error("Listen could not close because text still needs to be saved or copied.");
            return reject(&self.core, "Copy or save your recovered text before closing Listen.");
        }
        if !self.preview() && winput::clipboard_recovery_pending() {
            crate::diagnostics::error("Listen could not close while clipboard recovery is pending.");
            return reject(&self.core, "Clipboard recovery is pending. Open Listen and resolve it before quitting.");
        }
        if self.draft != self.draft_saved {
            if !self.preview() && storage::save_draft(&self.draft).is_err() {
                crate::diagnostics::error("Listen could not save its draft before closing.");
                return reject(&self.core, "Your draft could not be saved. Copy it before closing Listen.");
            }
            self.draft_saved = self.draft.clone();
        }
        self.quitting = true;
        #[cfg(windows)]
        {
            self.benchmarker.cancel();
            self.installer.cancel();
        }
        winput::shutdown_clipboard();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        Ok(())
    }

    fn sidebar(&mut self, ctx: &egui::Context, p: &Palette) {
        egui::SidePanel::left("side")
            .exact_width(212.0)
            .resizable(false)
            .frame(Frame::default().fill(p.nav).inner_margin(Margin::symmetric(14, 18)).stroke(Stroke::new(1.0_f32, p.line_faint())))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    brand_mark(ui, 28.0);
                    ui.label(RichText::new("Listen").font(FontId::new(20.0, FontFamily::Name("semibold".into()))).color(p.fg));
                });
                ui.add_space(22.0);
                let w = ui.available_width();
                let icons = ["◉", "✎", "◎", "☰", "⚙"];
                for (i, pg) in Page::ALL.into_iter().enumerate() {
                    let label = format!("{}   {}", icons[i], pg.label());
                    if nav_tab(ui, p, &label, self.page == pg, Some(w)).clicked() {
                        self.page = pg;
                    }
                    ui.add_space(2.0);
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        for t in ThemeId::ALL {
                            if theme_dot(ui, p, t, self.settings.theme == t).clicked() {
                                self.settings.theme = t;
                            }
                        }
                    });
                    ui.label(RichText::new("THEME").size(11.0).color(p.faint));
                    ui.add_space(10.0);
                    ui.label(RichText::new(short_status(&self.snap.status)).size(12.0).color(p.muted));
                    status_badge(ui, p, &self.snap, ui.ctx().input(|i| i.time));
                });
            });
        self.central(ctx, p, 860.0, Margin::symmetric(30, 26));
    }

    fn compact(&mut self, ctx: &egui::Context, p: &Palette) {
        egui::TopBottomPanel::top("bar")
            .exact_height(48.0)
            .frame(Frame::default().fill(p.nav).inner_margin(Margin::symmetric(12, 0)).stroke(Stroke::new(1.0_f32, p.line_faint())))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    brand_mark(ui, 22.0);
                    ui.add_space(4.0);
                    let short = ["Dictate", "Lab", "Meet", "History", "Settings"];
                    for (i, pg) in Page::ALL.into_iter().enumerate() {
                        if nav_tab(ui, p, short[i], self.page == pg, None).clicked() {
                            self.page = pg;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 5.0, status_color(p, &self.snap));
                    });
                });
            });
        self.central(ctx, p, 640.0, Margin::symmetric(16, 16));
    }

    #[allow(dead_code)]
    fn status_bar(&mut self, ctx: &egui::Context, p: &Palette) {
        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(30.0)
            .frame(Frame::default().fill(p.nav).inner_margin(Margin::symmetric(22, 0)).stroke(Stroke::new(1.0_f32, p.line_faint())))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let msg = match &self.toast {
                        Some((t, at)) if at.elapsed() < Duration::from_millis(2500) => t.clone(),
                        _ => short_status(&self.snap.status),
                    };
                    ui.label(RichText::new(msg).size(12.0).color(p.muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let small = |s: String| RichText::new(s).size(12.0).color(p.faint);
                        if ui.add(egui::Label::new(small(format!("{} · {}", self.settings.theme.label(), self.settings.layout.label()))).sense(Sense::click()))
                            .on_hover_text("Click for the next theme (Ctrl+T)")
                            .clicked()
                        {
                            self.settings.theme = self.settings.theme.next();
                        }
                        ui.label(small("  │  ".into()));
                        ui.label(small(format!("Cleanup: {}", if self.settings.cleanup { cleanup::engine_label(&self.settings) } else { "off".into() })));
                        ui.label(small("  │  ".into()));
                        ui.label(small(format!("Speech: {}", self.core.shared.lock().unwrap().engine_label())));
                    });
                });
            });
    }

    fn central(&mut self, ctx: &egui::Context, p: &Palette, max_w: f32, margin: Margin) {
        egui::CentralPanel::default()
            .frame(Frame::default().fill(p.bg).inner_margin(margin))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let avail = ui.available_width();
                    let w = avail.min(max_w);
                    let pad = ((avail - w) / 2.0).max(0.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(pad);
                        ui.vertical(|ui| {
                            ui.set_width(w - 2.0);
                            match self.page {
                                Page::Dictate => self.page_dictate(ui, p),
                                Page::Lab => self.page_lab(ui, p),
                                Page::Meetings => {
                                    ui.label(RichText::new("Meetings are available in the Listen dashboard.").color(p.muted));
                                }
                                Page::History => self.page_history(ui, p),
                                Page::Settings => self.page_settings(ui, p),
                            }
                            ui.add_space(24.0);
                        });
                    });
                });
            });
    }

    // ---------------------------------------------------------------- pages

    fn page_dictate(&mut self, ui: &mut Ui, p: &Palette) {
        // Laid out like the original's Dictate page (index.html; app.css .dictate-center, .text-row, .frame).
        let t = ui.ctx().input(|i| i.time);
        let phase = self.snap.phase;
        let unavailable = matches!(self.snap.engine, EngineState::Unavailable(_));
        let hotkey = self.settings.hotkey.label();
        let humanist = FontFamily::Name("humanist".into());

        // Page head: title, intro, and the status word on the right.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Dictate").font(FontId::new(29.0, humanist.clone())).color(p.fg));
                ui.label(RichText::new("Your voice, where you're working.").size(14.0).color(p.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new(status_word(&self.snap)).size(12.0).color(p.muted));
                status_dot(ui, p, &self.snap, t);
            });
        });

        // The centre block: key picture on the left, words and controls on the right.
        ui.add_space(20.0);
        let (title, sub) = match phase {
            Phase::Recording => ("Listening…".to_string(), format!("Release {hotkey} to send, or tap it again if you went hands-free. Esc throws it away.")),
            Phase::Working => ("Writing it up…".to_string(), "Turning your speech into tidy text.".to_string()),
            Phase::Idle if unavailable => ("No speech engine yet".to_string(), "Listen needs its speech files before it can hear you. Open speech settings to point it at them.".to_string()),
            Phase::Idle => ("Ready when you are".to_string(), format!("Hold {hotkey} to speak. Release to send your words, or tap for hands-free.")),
        };
        // Centre block (index.html .dictate-center): the key picture is centred on the words beside it,
        // so the headline starts above the key's top edge. The column height comes from the last frame.
        let col_id = egui::Id::new("dictate-hero-col-h");
        let how_id = egui::Id::new("dictate-how");
        let col_h: f32 = ui.ctx().data(|d| d.get_temp(col_id)).unwrap_or(HERO_KEY_PX);
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.vertical(|ui| {
                ui.add_space(((col_h - HERO_KEY_PX) / 2.0).max(0.0));
                key_visual(ui, p, self.settings.hotkey, phase);
            });
            ui.add_space(28.0);
            let col = ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                ui.set_width(ui.available_width());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    quiet_instrument(ui, p, phase, t);
                    ui.add_space(4.0);
                    ui.label(RichText::new(&title).font(FontId::new(HERO_TITLE_PX, humanist.clone())).color(p.fg));
                    if let (Phase::Recording, Some(st)) = (phase, self.snap.started) {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let s = st.elapsed().as_secs();
                            ui.label(RichText::new(format!("{}:{:02}", s / 60, s % 60)).font(FontId::monospace(15.0)).color(p.rec));
                        });
                    }
                });
                ui.scope(|ui| {
                    // app.css 356: 14px, line-height 1.6, margin-top 9px, max-width 51ch.
                    ui.add_space(-5.0);
                    ui.set_max_width(HERO_SUB_MAX_W);
                    let mut job = egui::text::LayoutJob::single_section(
                        sub.to_string(),
                        egui::TextFormat { font_id: FontId::proportional(14.0), color: p.muted, line_height: Some(22.4), ..Default::default() },
                    );
                    job.wrap.max_width = HERO_SUB_MAX_W;
                    ui.label(job);
                });
                ui.add_space(20.0); // .recording-actions margin-top 18px
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    match phase {
                        Phase::Idle if unavailable => {
                            if primary(ui, p, "Open speech settings").clicked() {
                                self.page = Page::Settings;
                            }
                            if secondary(ui, p, "Try again").clicked() {
                                self.core.send(Cmd::ReloadEngine);
                            }
                        }
                        Phase::Idle => {
                            // index.html line 67: Finish is a real button, disabled until recording.
                            hero_button(ui, p, "Finish recording", false, false);
                            quiet_button(ui, p, "Cancel recording", false);
                        }
                        Phase::Recording => {
                            if hero_button(ui, p, "Finish recording", true, true).clicked() {
                                self.core.send(Cmd::Finish);
                            }
                            if quiet_button(ui, p, "Cancel recording", true).clicked() {
                                self.core.send(Cmd::Cancel);
                            }
                        }
                        Phase::Working => {
                            ui.add(egui::Spinner::new().size(14.0).color(p.amber));
                            ui.label(RichText::new(short_status(&self.snap.status)).size(13.0).color(p.muted));
                        }
                    }
                    ui.add_space(18.0);
                    let how = ui
                        .add(egui::Label::new(RichText::new("How it works").size(13.0).color(p.accent)).sense(Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if how.clicked() {
                        let open = ui.ctx().data(|d| d.get_temp::<bool>(how_id)).unwrap_or(false);
                        ui.ctx().data_mut(|d| d.insert_temp(how_id, !open));
                    }
                });
                if ui.ctx().data(|d| d.get_temp::<bool>(how_id)).unwrap_or(false) {
                    ui.scope(|ui| {
                        ui.set_max_width(520.0);
                        ui.label(
                            RichText::new(format!(
                                "Tap {hotkey} to go hands-free, tap again to finish or Esc to discard. {hotkey} plus another key is a normal shortcut. Begin with \"Hey, prompt engineer this\" followed by your request to use your selected Engineer instead of cleanup."
                            ))
                            .size(12.0)
                            .color(p.muted),
                        );
                    });
                }
                // The level meter only shows while the microphone is open.
                if phase == Phase::Recording || self.demo_recording {
                    hero_bars(ui, p, &self.levels, p.rec, t, phase);
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(col_id, col.response.rect.height()));
        });
        ui.add_space(HERO_GAP_PX);

        // Text style row.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Text style").font(FontId::new(16.0, humanist.clone())).color(p.fg));
                ui.label(RichText::new("Keep the meaning. Tidy the words.").size(13.0).color(p.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 30.0;
                // Muted note on the right, like the original's chip hint.
                let lbl = if self.settings.cleanup { "AI clean-up on" } else { "AI clean-up off" };
                if ui
                    .add(egui::Label::new(RichText::new(lbl).size(13.0).color(p.muted)).sense(Sense::click()))
                    .on_hover_text("Tidy the words with your local model (click to switch)")
                    .clicked()
                {
                    self.settings.cleanup = !self.settings.cleanup;
                }
                // One bordered dropdown chip (app.js chip("style", …)); where text goes lives in Settings.
                let style = self.settings.style;
                ui.scope(|ui| {
                    let v = ui.visuals_mut();
                    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                        w.weak_bg_fill = p.surface;
                        w.bg_fill = p.surface;
                        w.bg_stroke = Stroke::new(1.0_f32, p.line);
                        w.corner_radius = CornerRadius::same((STYLE_CHIP_H / 2.0) as u8);
                        w.fg_stroke = Stroke::new(1.0_f32, p.fg);
                    }
                    ui.spacing_mut().interact_size.y = STYLE_CHIP_H;
                    egui::ComboBox::from_id_salt("dictate-style")
                        .width(STYLE_CHIP_W)
                        .selected_text(RichText::new(style.label()).size(13.0).color(p.fg))
                        .show_ui(ui, |ui| {
                            for s in [Style::Tighten, Style::Clean] {
                                ui.selectable_value(&mut self.settings.style, s, s.label());
                            }
                        });
                });
            });
        });
        ui.add_space(1.0);
        hline(ui, p);

        // Recording transcript frame: only while recording (or in the demo screenshot).
        if phase == Phase::Recording || self.demo_recording {
        ui.add_space(22.0);
        frame_titled(ui, p, "Recording transcript · provisional", None, false, |ui| {
            ui.label(RichText::new("Provisional. Final text is generated when you finish.").size(12.0).color(p.faint));
            ui.add_space(4.0);
            self.live_box(ui, p, t);
        });
        }

        // Vocabulary frame (saved by persist() like any other setting).
        ui.add_space(22.0);
        let vid = egui::Id::new("dictate-vocab");
        let focused = ui.ctx().memory(|m| m.has_focus(vid));
        frame_titled(ui, p, "Vocabulary", None, focused, |ui| {
            ui.label(RichText::new("Names and terms to spell exactly, comma or line separated.").size(12.0).color(p.muted));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::multiline(&mut self.settings.vocab)
                    .id(vid)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .frame(false)
                    .hint_text("e.g. Sczitzo, Parakeet, Ollama"),
            );
            let n = self.settings.vocab.split(|c| c == ',' || c == '\n').filter(|s| !s.trim().is_empty()).count();
            let note = match n {
                0 => "No terms yet.".to_string(),
                1 => "1 term · saved automatically".to_string(),
                _ => format!("{n} terms · saved automatically"),
            };
            ui.label(RichText::new(note).size(12.0).color(p.faint));

            // Original #vocab-suggest: words seen often in history but not yet listed.
            let key = egui::Id::new(("vocab-suggest", self.history.len(), &self.settings.vocab, self.settings.vocab_dismissed.len()));
            let words: Vec<String> = match ui.ctx().data(|d| d.get_temp::<Vec<String>>(key)) {
                Some(w) => w,
                None => {
                    let w = storage::suggest_vocab(&self.history, &self.settings.vocab, &self.settings.vocab_dismissed, 8);
                    ui.ctx().data_mut(|d| d.insert_temp(key, w.clone()));
                    w
                }
            };
            if !words.is_empty() {
                let mut add: Option<String> = None;
                let mut dismiss: Option<String> = None;
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    ui.label(RichText::new("Seen often, not in your vocabulary:").size(12.0).color(p.muted));
                    for w in &words {
                        egui::Frame::new()
                            .fill(p.surface2)
                            .stroke(Stroke::new(1.0_f32, p.line))
                            .corner_radius(CornerRadius::same(14))
                            .inner_margin(egui::Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(RichText::new(w).size(12.5).color(p.fg));
                                if ui.add(egui::Label::new(RichText::new("+").size(13.0).color(p.accent)).sense(Sense::click()))
                                    .on_hover_text("Add to vocabulary").clicked() {
                                    add = Some(w.clone());
                                }
                                if ui.add(egui::Label::new(RichText::new("×").size(13.0).color(p.muted)).sense(Sense::click()))
                                    .on_hover_text("Not a term").clicked() {
                                    dismiss = Some(w.clone());
                                }
                            });
                    }
                });
                if let Some(w) = add {
                    let known = storage::vocab_words(&self.settings.vocab).iter().any(|v| v.eq_ignore_ascii_case(&w));
                    if !known {
                        let base = self.settings.vocab.trim_end().trim_end_matches(',').to_string();
                        self.settings.vocab = if base.is_empty() { w } else { format!("{base}, {w}") };
                    }
                }
                if let Some(w) = dismiss {
                    if !self.settings.vocab_dismissed.contains(&w) {
                        self.settings.vocab_dismissed.push(w);
                    }
                }
            }
        });

        // Things kept here because they could not be typed.
        ui.add_space(8.0);
        let mut drop: Option<usize> = None;
        let mut to_draft: Option<String> = None;
        for (i, (text, reason)) in self.retained.iter().enumerate() {
            ui.add_space(12.0);
            Frame::default()
                .fill(blend(p.surface, p.amber, 0.10))
                .stroke(Stroke::new(1.0_f32, blend(p.surface, p.amber, 0.55)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(format!("Kept here — {reason}")).strong().color(p.amber));
                    ui.add_space(4.0);
                    ui.label(RichText::new(text).color(p.fg));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if secondary(ui, p, "Copy").clicked() {
                            ui.ctx().copy_text(text.clone());
                        }
                        if secondary(ui, p, "Send to Prompt Lab").clicked() {
                            to_draft = Some(text.clone());
                            drop = Some(i);
                        }
                        if ghost_button(ui, p, "Dismiss").clicked() {
                            drop = Some(i);
                        }
                    });
                });
        }
        if let Some(i) = drop {
            self.retained.remove(i);
        }
        if let Some(t) = to_draft {
            let mut next = self.draft.trim_end().to_string();
            if !next.is_empty() {
                next.push_str("\n\n");
            }
            next.push_str(&t);
            self.set_draft_revision(next);
            self.page = Page::Lab;
        }
        // Recent frame.
        ui.add_space(22.0);
        let mut copy: Option<String> = None;
        let all = frame_titled(ui, p, "Recent", Some("All history"), false, |ui| {
            ui.set_min_height(90.0);
            if self.history.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Nothing yet. Hold {hotkey} and say something.")).size(13.0).color(p.muted));
                return;
            }
            for e in self.history.iter().rev().take(5) {
                let idx = ui.painter().add(egui::Shape::Noop);
                let row = Frame::default().inner_margin(Margin::symmetric(10, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        ui.allocate_ui_with_layout(vec2(96.0, 22.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_width(96.0);
                            ui.label(RichText::new(time_of(&e.when)).size(12.0).color(p.muted));
                        });
                        let w = (ui.available_width() - 70.0).max(80.0);
                        ui.allocate_ui(vec2(w, 0.0), |ui| {
                            ui.label(RichText::new(clip(&e.text(), 220)).size(15.0).color(p.fg));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if ghost_button(ui, p, "Copy").clicked() {
                                copy = Some(e.text().to_string());
                            }
                        });
                    });
                });
                if ui.rect_contains_pointer(row.response.rect) {
                    ui.painter().set(idx, egui::Shape::rect_filled(row.response.rect, CornerRadius::same(8), p.surface2));
                }
            }
        });
        if all {
            self.page = Page::History;
        }
        if let Some(text) = copy {
            ui.ctx().copy_text(text);
            self.toast("Copied");
        }
    }

    fn page_lab(&mut self, ui: &mut Ui, p: &Palette) {
        page_heading(ui, p, "Prompt Lab", "Shape a draft by voice or by hand, then paste it anywhere.");
        let narrow = ui.available_width() < 760.0;
        if narrow {
            self.lab_editor(ui, p);
            ui.add_space(14.0);
            self.lab_favorites(ui, p);
        } else {
            let total = ui.available_width();
            let side = 260.0;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(total - side - 16.0);
                    self.lab_editor(ui, p);
                });
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.set_width(side);
                    self.lab_favorites(ui, p);
                });
            });
        }
    }

    fn lab_editor(&mut self, ui: &mut Ui, p: &Palette) {
        let busy = self.snap.lab_busy;
        card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let has_revs = self.revisions.len() > 1;
                ui.add_enabled_ui(has_revs && self.rev_idx > 0, |ui| {
                    if icon_button(ui, p, "‹", "Earlier version").clicked() {
                        self.rev_idx -= 1;
                        self.draft = self.revisions[self.rev_idx].clone();
                    }
                });
                ui.add_enabled_ui(has_revs && self.rev_idx + 1 < self.revisions.len(), |ui| {
                    if icon_button(ui, p, "›", "Later version").clicked() {
                        self.rev_idx += 1;
                        self.draft = self.revisions[self.rev_idx].clone();
                    }
                });
                let label = if has_revs { format!("Version {} of {}", self.rev_idx + 1, self.revisions.len()) } else { "Draft".into() };
                ui.label(RichText::new(label).size(13.0).color(p.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let words = self.draft.split_whitespace().count();
                    ui.label(RichText::new(format!("{words} words")).size(12.0).color(p.faint));
                    if busy {
                        ui.spinner();
                    }
                });
            });
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::multiline(&mut self.draft)
                    .hint_text("Dictate here (set Destination to Prompt Lab), paste, or type…")
                    .desired_rows(12)
                    .desired_width(f32::INFINITY)
                    .margin(Margin::same(12)),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let w = ui.available_width() - 130.0;
                ui.add(
                    egui::TextEdit::singleline(&mut self.instruction)
                        .hint_text("Tell it what to change — e.g. “make it friendlier”")
                        .desired_width(w.max(120.0))
                        .margin(Margin::symmetric(10, 7)),
                );
                ui.add_enabled_ui(!busy && !self.instruction.trim().is_empty(), |ui| {
                    if primary(ui, p, "Apply change").clicked() {
                        self.lab(LabAction::Instruct);
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if secondary(ui, p, "Clean up").on_hover_text("Fix punctuation and filler words").clicked() {
                            self.lab(LabAction::Clean);
                        }
                        if secondary(ui, p, "Tighten").on_hover_text("Shorter, same meaning").clicked() {
                            self.lab(LabAction::Tighten);
                        }
                        let hover = match self.settings.prompt_target {
                            PromptTarget::Auto => "Rewrite as a clear prompt (auto-detects coding vs. general)".to_owned(),
                            other => format!("Rewrite as a clear prompt for {}", other.label()),
                        };
                        if secondary(ui, p, "Make it a prompt").on_hover_text(hover).clicked() {
                            self.lab(LabAction::Prompt);
                        }
                    });
                });
            });
            if let Some(e) = &self.lab_error {
                ui.add_space(6.0);
                ui.label(RichText::new(e).size(13.0).color(p.rec));
            }
            ui.add_space(8.0);
            hline(ui, p);
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if primary(ui, p, "Paste to app").on_hover_text("Ctrl+Enter — Listen hides and types it into the window behind").clicked() {
                    self.paste_draft(ui.ctx());
                }
                if secondary(ui, p, "Copy").clicked() {
                    ui.ctx().copy_text(self.draft.clone());
                    self.toast("Copied");
                }
                if secondary(ui, p, "☆ Save favorite").on_hover_text("Ctrl+S").clicked() {
                    self.save_favorite();
                }
                if ghost_button(ui, p, "Clear").clicked() && !self.draft.is_empty() {
                    self.set_draft_revision(String::new());
                }
            });
        });
    }

    fn lab(&mut self, action: LabAction) {
        let text = self.draft.trim().to_string();
        if text.is_empty() {
            self.lab_error = Some("Write or dictate something first.".into());
            return;
        }
        self.lab_error = None;
        let instruction = std::mem::take(&mut self.instruction);
        self.core.send(Cmd::Lab { action, text, instruction });
    }

    fn lab_favorites(&mut self, ui: &mut Ui, p: &Palette) {
        card(p).inner_margin(Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, p, "FAVORITES");
            ui.add_space(6.0);
            if self.favorites.is_empty() {
                ui.label(RichText::new("Save drafts you reuse. They stay here between sessions.").size(13.0).color(p.faint));
                return;
            }
            let mut remove = None;
            let mut use_it = None;
            let mut used = None;
            for (i, f) in self.favorites.iter().enumerate() {
                if i > 0 {
                    hline(ui, p);
                }
                ui.add_space(4.0);
                ui.label(RichText::new(&f.name).color(p.fg).strong());
                ui.label(RichText::new(clip(&f.text, 90)).size(12.5).color(p.muted));
                ui.horizontal(|ui| {
                    if ghost_button(ui, p, "Use").clicked() {
                        use_it = Some(f.text.clone());
                        used = Some(i);
                    }
                    if ghost_button(ui, p, "Copy").clicked() {
                        ui.ctx().copy_text(f.text.clone());
                        used = Some(i);
                    }
                    if ghost_button(ui, p, "Remove").clicked() {
                        remove = Some(i);
                    }
                });
                ui.add_space(4.0);
            }
            if let Some(t) = use_it {
                self.set_draft_revision(t);
            }
            // The use count shown in the tray menu ("name  (uses)").
            if let Some(i) = used {
                if let Some(f) = self.favorites.get_mut(i) {
                    f.uses = f.uses.saturating_add(1);
                }
                if !self.preview() {
                    storage::save_favorites(&self.favorites);
                }
            }
            if let Some(i) = remove {
                self.favorites.remove(i);
                if !self.preview() {
                    storage::save_favorites(&self.favorites);
                }
            }
        });
    }

    fn page_history(&mut self, ui: &mut Ui, p: &Palette) {
        page_heading(ui, p, "History", "Everything you've dictated, newest first. Stored only on this PC.");
        let words: usize = self.history.iter().map(|e| e.text().split_whitespace().count()).sum();
        let secs: f64 = self.history.iter().map(|e| e.seconds as f64).sum();
        ui.horizontal_wrapped(|ui| {
            stat(ui, p, &self.history.len().to_string(), "dictations");
            stat(ui, p, &words.to_string(), "words");
            stat(ui, p, &format!("{:.0}", secs / 60.0), "minutes spoken");
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let w = ui.available_width() - 120.0;
            ui.add(egui::TextEdit::singleline(&mut self.hist_filter).hint_text("🔍  Search history").desired_width(w.max(120.0)).margin(Margin::symmetric(10, 7)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.confirm_clear {
                    if danger(ui, p, "Delete all").clicked() {
                        self.history.clear();
                        if !self.preview() {
                            storage::clear_history();
                        }
                        self.confirm_clear = false;
                    }
                    if ghost_button(ui, p, "Keep").clicked() {
                        self.confirm_clear = false;
                    }
                } else if ui.add_enabled(!self.history.is_empty(), egui::Button::new(RichText::new("Clear all").color(p.muted)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0_f32, p.line_soft())).corner_radius(8).min_size(vec2(0.0, 32.0))).clicked() {
                    self.confirm_clear = true;
                }
            });
        });
        ui.add_space(10.0);
        let q = self.hist_filter.to_lowercase();
        let items: Vec<&HistoryEntry> = self.history.iter().rev().filter(|e| q.is_empty() || e.text().to_lowercase().contains(&q) || e.raw.to_lowercase().contains(&q)).collect();
        if items.is_empty() {
            empty_state(ui, p, if self.history.is_empty() { "No history yet" } else { "No matches" }, if self.history.is_empty() { "Finished dictations are listed here." } else { "Try a different word." });
            return;
        }
        let mut toggle = None;
        let mut send = None;
        for e in items.iter().take(300) {
            card(p).inner_margin(Margin::same(14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&e.when).size(12.0).color(p.faint));
                    ui.label(RichText::new(format!("· {:.0}s · {}", e.seconds, e.outcome)).size(12.0).color(p.faint));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost_button(ui, p, "Copy").clicked() {
                            ui.ctx().copy_text(e.text().to_string());
                        }
                        if ghost_button(ui, p, "To Lab").clicked() {
                            send = Some(e.text().to_string());
                        }
                        if !e.cleaned.is_empty() && e.cleaned != e.raw && ghost_button(ui, p, if self.hist_open.contains(&e.unix) { "Hide original" } else { "Original" }).clicked() {
                            toggle = Some(e.unix);
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(RichText::new(e.text()).color(p.fg));
                if self.hist_open.contains(&e.unix) {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("You said: {}", e.raw)).size(13.0).italics().color(p.muted));
                }
            });
            ui.add_space(8.0);
        }
        if let Some(u) = toggle {
            if !self.hist_open.remove(&u) {
                self.hist_open.insert(u);
            }
        }
        if let Some(t) = send {
            self.set_draft_revision(t);
            self.page = Page::Lab;
        }
    }

    fn page_settings(&mut self, ui: &mut Ui, p: &Palette) {
        page_heading(ui, p, "Settings", "Changes apply at once and are saved automatically.");

        // Appearance
        settings_card(ui, p, "Appearance", |ui| {
            ui.label(RichText::new("Color scheme").color(p.muted));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                for t in ThemeId::ALL {
                    if theme_swatch(ui, p, t, self.settings.theme == t).clicked() {
                        self.settings.theme = t;
                    }
                }
            });
            ui.add_space(12.0);
            ui.label(RichText::new("Layout").color(p.muted));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                for l in LayoutId::ALL {
                    if layout_tile(ui, p, l, self.settings.layout == l).on_hover_text(l.blurb()).clicked() {
                        self.settings.layout = l;
                    }
                }
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Text size").color(p.muted));
                ui.add(egui::Slider::new(&mut self.settings.zoom, 0.8..=1.4).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            });
        });

        settings_card(ui, p, "Dictation", |ui| {
            egui::Grid::new("dict").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
                ui.label("Push-to-talk key");
                egui::ComboBox::from_id_salt("hotkey").selected_text(self.settings.hotkey.label()).show_ui(ui, |ui| {
                    for h in Hotkey::ALL {
                        ui.selectable_value(&mut self.settings.hotkey, h, h.label());
                    }
                });
                ui.end_row();
                ui.label("Microphone");
                let cur = if self.settings.microphone.is_empty() { "System default".to_string() } else { self.settings.microphone.clone() };
                egui::ComboBox::from_id_salt("mic").width(300.0).selected_text(cur).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.settings.microphone, String::new(), "System default");
                    for m in &self.mics {
                        ui.selectable_value(&mut self.settings.microphone, m.clone(), m);
                    }
                });
                ui.end_row();
                ui.label("Finished text goes");
                ui.vertical(|ui| {
                    ui.radio_value(&mut self.settings.destination, Destination::LastApp, Destination::LastApp.label());
                    ui.radio_value(&mut self.settings.destination, Destination::Workspace, Destination::Workspace.label());
                });
                ui.end_row();
                ui.label("");
                ui.vertical(|ui| {
                    ui.checkbox(&mut self.settings.sounds, "Play start and stop sounds");
                    ui.checkbox(&mut self.settings.floating_card, "Show a recording card on screen");
                    ui.checkbox(&mut self.settings.live_preview, "Live preview — show words while I speak")
                        .on_hover_text("Re-reads the recording every couple of seconds. Turn off on slow computers.");
                    ui.checkbox(&mut self.settings.restore_clipboard, "Put my clipboard back after typing");
                });
                ui.end_row();
            });
        });

        settings_card(ui, p, "Speech engine", |ui| {
            let (txt, col) = match &self.snap.engine {
                EngineState::Loading => ("Loading…".to_string(), p.amber),
                EngineState::Ready(s) => (format!("Ready — {s}"), p.green),
                EngineState::Unavailable(s) => (format!("Not available — {s}"), p.rec),
            };
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.5, col);
                ui.label(RichText::new(txt).color(p.fg));
            });
            ui.add_space(8.0);
            egui::Grid::new("speech").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
                ui.label("Engine");
                egui::ComboBox::from_id_salt("engine").selected_text(self.settings.speech_engine.label()).show_ui(ui, |ui| {
                    for c in [SpeechChoice::Auto, SpeechChoice::Transcribe, SpeechChoice::WhisperCli] {
                        ui.selectable_value(&mut self.settings.speech_engine, c, c.label());
                    }
                });
                ui.end_row();
                ui.label("Parakeet runtime folder");
                path_edit(ui, &mut self.settings.transcribe_dir);
                ui.end_row();
                ui.label("Parakeet model file");
                path_edit(ui, &mut self.settings.speech_model);
                ui.end_row();
                ui.label("whisper-cli program");
                path_edit(ui, &mut self.settings.whisper_cli);
                ui.end_row();
                ui.label("Whisper model file");
                path_edit(ui, &mut self.settings.whisper_model);
                ui.end_row();
                ui.label("CPU threads");
                ui.add(egui::DragValue::new(&mut self.settings.speech_threads).range(1..=32));
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if secondary(ui, p, "Reload engine").clicked() {
                    self.core.send(Cmd::ReloadEngine);
                }
                if ghost_button(ui, p, "Use defaults").clicked() {
                    self.settings.transcribe_dir = storage::default_transcribe_dir();
                    self.settings.speech_model = storage::default_speech_model();
                }
            });
        });

        settings_card(ui, p, "Cleanup", |ui| {
            ui.checkbox(&mut self.settings.cleanup, "Tidy dictation with a local AI model before typing");
            ui.add_space(6.0);
            egui::Grid::new("clean").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
                ui.label("Style");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.settings.style, Style::Tighten, Style::Tighten.label());
                    ui.radio_value(&mut self.settings.style, Style::Clean, Style::Clean.label());
                });
                ui.end_row();
                ui.label("Ollama address");
                ui.add(egui::TextEdit::singleline(&mut self.settings.ollama_url).desired_width(300.0));
                ui.end_row();
                ui.label("Local model");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("model").width(220.0).selected_text(self.settings.local_model.clone()).show_ui(ui, |ui| {
                        if self.models.is_empty() {
                            ui.label("Press Refresh to list models");
                        }
                        for m in &self.models {
                            ui.selectable_value(&mut self.settings.local_model, m.clone(), m);
                        }
                    });
                    if ghost_button(ui, p, "Refresh").clicked() {
                        self.core.send(Cmd::RefreshModels);
                    }
                });
                ui.end_row();
                ui.label("Words to get right");
                ui.add(egui::TextEdit::multiline(&mut self.settings.vocab).hint_text("Names, product terms… one per line or comma-separated").desired_rows(3).desired_width(300.0));
                ui.end_row();
            });
        });

        settings_card(ui, p, "Cloud (optional)", |ui| {
            ui.checkbox(&mut self.settings.cloud_enabled, "Use Claude for Prompt Lab when a key is set");
            let has_key = cleanup::claude_key().is_some();
            ui.label(RichText::new(if has_key { "✓ An API key was found in the ANTHROPIC_API_KEY environment variable." } else { "No key found. Set the ANTHROPIC_API_KEY environment variable to use this." }).size(13.0).color(if has_key { p.green } else { p.faint }));
            ui.add_space(6.0);
            egui::Grid::new("cloud").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
                ui.label("Model");
                ui.add(egui::TextEdit::singleline(&mut self.settings.claude_model).desired_width(240.0));
                ui.end_row();
                ui.label("Prompts are for");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.settings.prompt_target, PromptTarget::Auto, PromptTarget::Auto.label());
                    ui.radio_value(&mut self.settings.prompt_target, PromptTarget::ClaudeCode, PromptTarget::ClaudeCode.label());
                    ui.radio_value(&mut self.settings.prompt_target, PromptTarget::General, PromptTarget::General.label());
                });
                ui.end_row();
            });
        });

        settings_card(ui, p, "Keyboard shortcuts", |ui| {
            let hk = format!("{} (hold / tap)", self.settings.hotkey.label());
            let rows: [(&str, &str); 9] = [
                (&hk, "Dictate from any app"),
                ("Esc", "Cancel a recording"),
                ("Ctrl+1 … 4", "Switch page"),
                ("Ctrl+K", "Command palette"),
                ("Ctrl+Enter", "Paste the draft into the last app"),
                ("Ctrl+S", "Save the draft as a favorite"),
                ("Ctrl+T", "Next color scheme"),
                ("Ctrl+L", "Next layout"),
                ("Say “new line”, “new paragraph”", "Voice commands while dictating"),
            ];
            egui::Grid::new("keys").num_columns(2).spacing(vec2(18.0, 6.0)).show(ui, |ui| {
                for (k, d) in rows {
                    ui.label(RichText::new(k).font(FontId::monospace(13.0)).color(p.accent));
                    ui.label(RichText::new(d).color(p.muted));
                    ui.end_row();
                }
            });
        });

        settings_card(ui, p, "Storage & about", |ui| {
            let dir = storage::data_dir();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Settings, history and favorites live in {}", dir.display())).size(13.0).color(p.muted));
            });
            if ghost_button(ui, p, "Open folder").clicked() && !self.preview() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Listen {} — native build of Listen (original Python app by Sczitzo, MIT).", env!("CARGO_PKG_VERSION"))).color(p.fg));
            ui.label(RichText::new("Original work © Sczitzo, released under the MIT License; this rebuild keeps that notice and credit. Speech runs on your PC with the Parakeet model.").size(13.0).color(p.faint));
        });
    }

    // ---------------------------------------------------------------- overlays

    fn command_palette(&mut self, ctx: &egui::Context, p: &Palette) {
        let mut cmds: Vec<(String, PCmd)> = Vec::new();
        for pg in Page::ALL {
            cmds.push((format!("Go to {}", pg.label()), PCmd::Go(pg)));
        }
        match self.snap.phase {
            Phase::Idle => cmds.push(("Start dictation".into(), PCmd::Start)),
            Phase::Recording => {
                cmds.push(("Finish dictation".into(), PCmd::Finish));
                cmds.push(("Cancel dictation".into(), PCmd::Cancel));
            }
            Phase::Working => {}
        }
        cmds.push(("Paste draft into last app".into(), PCmd::PasteDraft));
        cmds.push((format!("Turn cleanup {}", if self.settings.cleanup { "off" } else { "on" }), PCmd::ToggleCleanup));
        cmds.push(("Switch where text goes".into(), PCmd::ToggleDestination));
        cmds.push(("Reload speech engine".into(), PCmd::ReloadEngine));
        for t in ThemeId::ALL {
            cmds.push((format!("Theme: {}", t.label()), PCmd::Theme(t)));
        }
        for l in LayoutId::ALL {
            cmds.push((format!("Layout: {}", l.label()), PCmd::Layout(l)));
        }
        let q = self.palette_query.to_lowercase();
        let hits: Vec<(String, PCmd)> = cmds.into_iter().filter(|(n, _)| q.split_whitespace().all(|w| n.to_lowercase().contains(w))).collect();
        let mut run = None;
        let mut close = false;
        egui::Area::new(egui::Id::new("palette"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 90.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Frame::default()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.line_soft()))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(12))
                    .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(90) })
                    .show(ui, |ui| {
                        ui.set_width(460.0);
                        let r = ui.add(egui::TextEdit::singleline(&mut self.palette_query).hint_text("Type a command…").desired_width(f32::INFINITY).margin(Margin::symmetric(10, 8)));
                        r.request_focus();
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            run = hits.first().map(|h| h.1.clone());
                        }
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (i, (name, cmd)) in hits.iter().enumerate() {
                                let b = egui::Button::new(RichText::new(name).color(if i == 0 { p.fg } else { p.muted }))
                                    .fill(if i == 0 { p.accent_soft() } else { Color32::TRANSPARENT })
                                    .stroke(Stroke::NONE)
                                    .corner_radius(6)
                                    .min_size(vec2(ui.available_width(), 30.0));
                                if ui.add(b).clicked() {
                                    run = Some(cmd.clone());
                                }
                            }
                            if hits.is_empty() {
                                ui.label(RichText::new("No matching commands. Try another search.").color(p.faint));
                            }
                        });
                        ui.add_space(4.0);
                        ui.label(RichText::new("Enter runs the top match · Esc closes").size(11.5).color(p.faint));
                        if ui.input(|i| i.pointer.any_click()) && !ui.ui_contains_pointer() {
                            close = true;
                        }
                    });
            });
        if let Some(c) = run {
            self.palette_open = false;
            self.run_pcmd(ctx, c);
        } else if close {
            self.palette_open = false;
        }
    }

    /// The words heard so far, shown under the meter while recording or writing up.
    /// Live transcript box text size (mechanics.json ui.transcript_font_px, estimated).
    fn live_box(&self, ui: &mut Ui, p: &Palette, t: f64) {
        let s = &self.snap;
        Frame::default()
            .fill(p.surface2)
            .stroke(Stroke::new(1.0_f32, p.line_faint()))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    let pulse = if s.phase == Phase::Recording { 0.55 + 0.45 * (t * 3.0).sin().abs() as f32 } else { 1.0 };
                    ui.painter().circle_filled(r.center(), 3.5, status_color(p, s).gamma_multiply(pulse));
                    ui.label(RichText::new("RECORDING TRANSCRIPT · PROVISIONAL").size(12.0).strong().color(p.faint));
                    ui.label(RichText::new(live_status(s)).size(13.0).color(p.muted));
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("live-text")
                    .min_scrolled_height(LIVE_TEXT_MIN_H)
                    .max_height(LIVE_TEXT_MAX_H)
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_height(LIVE_TEXT_MIN_H);
                        if s.live_text.is_empty() {
                            let msg = if !self.settings.live_preview {
                                "Live preview is off. Turn it on in Settings → Dictation."
                            } else {
                                "Words appear here a couple of seconds after you start speaking…"
                            };
                            ui.label(RichText::new(msg).italics().size(16.0).color(p.faint));
                        } else {
                            ui.label(RichText::new(&s.live_text).size(LIVE_TEXT_PX).line_height(Some(LIVE_TEXT_PX * 1.4)).color(p.fg));
                        }
                    });
            });
    }

    fn floating_card(&mut self, ctx: &egui::Context) {
        let snap = &self.snap;
        let recording = snap.phase == Phase::Recording;
        let retained_outcome = snap.outcome.as_deref().filter(|outcome| {
            !matches!(*outcome, "Text typed" | "Text pasted")
        });
        if (self.preview() && self.dashboard_check.is_none()) || !self.settings.floating_card
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
        let status = live_status(snap);
        let card = lantern::Card {
            generation: snap.generation,
            recording,
            caption,
            text: if recording && self.settings.live_preview { &snap.live_text } else { "" },
            status: &status,
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
                // Keep it hidden unless the native window is protected from activation.
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

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        #[cfg(windows)]
        if let Some(dashboard) = &self.dashboard {
            ctx.request_repaint_after(Duration::from_millis(16));
            if self.dashboard_ready {
                self.frames += 1;
            }
            if self.frames >= 120 && !self.shot_requested {
                self.shot_requested = true;
                match dashboard.screenshot(&path, ctx, self.hwnd, self.original_picture) {
                    Ok(result) => self.dashboard_capture = Some(result),
                    Err(error) => {
                        eprintln!("Could not capture Listen: {error}");
                        std::process::exit(1);
                    }
                }
            }
            if let Some(result) = self.dashboard_capture.as_ref().and_then(|capture| capture.try_recv().ok()) {
                if let Err(error) = result {
                    eprintln!("Could not save the Listen screenshot: {error}");
                    std::process::exit(1);
                }
                eprintln!("Saved screenshot {}", path.display());
                self.quitting = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            } else if self.launched.elapsed() > Duration::from_secs(25) {
                eprintln!("Listen screenshot timed out");
                std::process::exit(1);
            }
            return;
        }
        self.frames += 1;
        ctx.request_repaint();
        if self.frames >= 120 && !self.shot_requested {
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
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if self.frames > 900 {
            eprintln!("screenshot timed out");
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
        #[cfg(windows)]
        if self.dashboard_active() {
            self.dashboard_step(ctx);
            self.drain_events();
            self.close_step(ctx);
            self.tray_step(ctx);
            self.floating_card(ctx);
            self.screenshot_step(ctx);
            ctx.request_repaint_after(Duration::from_millis(if self.snap.phase == Phase::Idle { 250 } else { 33 }));
            return;
        }
        self.drain_events();
        self.close_step(ctx);
        if let Some(error) = &self.dashboard_error {
            if self.screenshot.is_some() {
                eprintln!("Listen could not open its dashboard: {error}");
                std::process::exit(1);
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Listen could not open its window");
                ui.label(error);
            });
            return;
        }
        self.shortcuts(ctx);
        self.apply_settings(ctx);
        self.tick_levels();
        self.tray_step(ctx);
        let p = self.settings.theme.palette();
        match self.settings.layout {
            LayoutId::Desk => self.desk(ctx, &p),
            LayoutId::Sidebar => self.sidebar(ctx, &p),
            LayoutId::Compact => self.compact(ctx, &p),
        }
        if self.palette_open {
            self.command_palette(ctx, &p);
        }
        self.floating_card(ctx);
        self.persist();
        let busy = self.snap.phase != Phase::Idle || self.snap.lab_busy || matches!(self.snap.engine, EngineState::Loading);
        if busy {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else if self.draft_changed.is_some() || self.toast.is_some() || self.snap.outcome.is_some() {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
        if let Some((_, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(3) {
                self.toast = None;
            }
        }
        self.screenshot_step(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.demo_lantern { [0.0; 4] } else { self.settings.theme.palette().bg.to_normalized_gamma_f32() }
    }
}

// -------------------------------------------------------------------- fonts

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
    if let Some(b) = read(&["Candarab.ttf", "Candara.ttf"]) {
        fonts.font_data.insert("humanist".into(), Arc::new(FontData::from_owned(b)));
        humanist.insert(0, "humanist".into());
    }
    let mut transcript = prop.clone();
    if let Some(b) = read(&["Candara.ttf"]) {
        fonts.font_data.insert("lantern-transcript".into(), Arc::new(FontData::from_owned(b)));
        transcript.insert(0, "lantern-transcript".into());
    }
    fonts.families.insert(FontFamily::Name("lantern-transcript".into()), transcript);
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name("semibold".into()), semi);
    fonts.families.insert(FontFamily::Name("humanist".into()), humanist);
    ctx.set_fonts(fonts);
}

// -------------------------------------------------------------------- widgets

fn card(p: &Palette) -> Frame {
    Frame::default()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line_faint()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(20))
}

fn settings_card(ui: &mut Ui, p: &Palette, title: &str, add: impl FnOnce(&mut Ui)) {
    card(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(FontId::new(17.0, FontFamily::Name("semibold".into()))).color(p.fg));
        ui.add_space(10.0);
        add(ui);
    });
    ui.add_space(14.0);
}

fn page_heading(ui: &mut Ui, p: &Palette, title: &str, sub: &str) {
    ui.label(RichText::new(title).text_style(egui::TextStyle::Heading).color(p.fg));
    ui.add_space(2.0);
    ui.label(RichText::new(sub).color(p.muted));
    ui.add_space(16.0);
}

fn section_title(ui: &mut Ui, p: &Palette, t: &str) {
    ui.label(RichText::new(t).size(11.5).color(p.faint).strong());
}

fn hline(ui: &mut Ui, p: &Palette) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    // The original's separators are `1px solid var(--line)` (app.css 346), the full line colour.
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.line);
}

fn empty_state(ui: &mut Ui, p: &Palette, title: &str, sub: &str) {
    Frame::default()
        .stroke(Stroke::new(1.0_f32, p.line_faint()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(20, 22))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).color(p.muted).strong());
                ui.label(RichText::new(sub).size(13.0).color(p.faint));
            });
        });
}

fn stat(ui: &mut Ui, p: &Palette, value: &str, label: &str) {
    Frame::default()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line_faint()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).font(FontId::new(22.0, FontFamily::Name("semibold".into()))).color(p.fg));
                ui.label(RichText::new(label).size(12.0).color(p.faint));
            });
        });
}

fn primary(ui: &mut Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.accent_fg).font(FontId::new(14.0, FontFamily::Name("semibold".into()))))
            .fill(p.accent)
            .stroke(Stroke::NONE)
            .corner_radius(8)
            .min_size(vec2(0.0, 34.0)),
    )
}

/// The original's `.btn` in the hero: a filled, bordered button that turns primary while recording.
fn hero_button(ui: &mut Ui, p: &Palette, text: &str, enabled: bool, primary_look: bool) -> egui::Response {
    if primary_look {
        return primary(ui, p, text);
    }
    let fill = blend(p.bg, p.surface, 0.5);
    let color = if enabled { p.fg } else { p.muted };
    let b = egui::Button::new(RichText::new(text).size(13.0).color(color))
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, blend(p.bg, p.line, 0.35)))
        .corner_radius(8)
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

fn secondary(ui: &mut Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.fg))
            .fill(p.surface2)
            .stroke(Stroke::new(1.0_f32, p.line_soft()))
            .corner_radius(8)
            .min_size(vec2(0.0, 32.0)),
    )
}

fn danger(ui: &mut Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::from_rgb(30, 14, 14)).strong())
            .fill(p.rec)
            .stroke(Stroke::NONE)
            .corner_radius(8)
            .min_size(vec2(0.0, 32.0)),
    )
}

fn ghost_button(ui: &mut Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0).color(p.muted))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(6)
            .min_size(vec2(0.0, 26.0)),
    )
}

fn icon_button(ui: &mut Ui, p: &Palette, text: &str, tip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0).color(p.muted))
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.line_faint()))
            .corner_radius(8)
            .min_size(vec2(32.0, 30.0)),
    )
    .on_hover_text(tip)
}

#[allow(dead_code)]
fn chip(ui: &mut Ui, p: &Palette, text: &str, on: bool) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0).color(if on { p.fg } else { p.muted }))
            .fill(if on { p.accent_soft() } else { p.surface })
            .stroke(Stroke::new(1.0_f32, if on { blend(p.surface, p.accent, 0.6) } else { p.line_faint() }))
            .corner_radius(14)
            .min_size(vec2(0.0, 28.0)),
    )
}

fn nav_tab(ui: &mut Ui, p: &Palette, text: &str, selected: bool, width: Option<f32>) -> egui::Response {
    let mut b = egui::Button::new(RichText::new(text).size(14.5).color(if selected { p.fg } else { p.muted }))
        .fill(if selected { p.accent_soft() } else { Color32::TRANSPARENT })
        .stroke(Stroke::NONE)
        .corner_radius(8)
        .min_size(vec2(width.unwrap_or(0.0), 34.0));
    if width.is_some() {
        b = b.wrap_mode(egui::TextWrapMode::Truncate);
    }
    if width.is_some() {
        ui.with_layout(Layout::top_down_justified(Align::Min), |ui| ui.add(b)).inner
    } else {
        ui.add(b)
    }
}

/// The card's status line, using the original's wording (fnd_6).
fn live_status(s: &Snap) -> String {
    match s.phase {
        Phase::Recording if s.live_note.is_empty() => "Listening…".into(),
        Phase::Recording => format!("Listening · {}", s.live_note),
        Phase::Working if s.live_note == "Cleaning" => "Writing · Cleaning up…".into(),
        Phase::Working => "Writing it up…".into(),
        Phase::Idle => s.outcome.clone().unwrap_or_else(|| "Ready".into()),
    }
}

fn status_color(p: &Palette, s: &Snap) -> Color32 {
    match s.phase {
        Phase::Recording => p.rec,
        Phase::Working => p.amber,
        Phase::Idle => match s.engine {
            EngineState::Ready(_) => p.green,
            EngineState::Loading => p.amber,
            EngineState::Unavailable(_) => p.rec,
        },
    }
}

fn status_badge(ui: &mut Ui, p: &Palette, s: &Snap, t: f64) {
    let text = match s.phase {
        Phase::Recording => match s.started {
            Some(st) => {
                let n = st.elapsed().as_secs();
                format!("Recording {}:{:02}", n / 60, n % 60)
            }
            None => "Recording".into(),
        },
        Phase::Working => "Working".into(),
        Phase::Idle => match s.engine {
            EngineState::Ready(_) => "Ready".into(),
            EngineState::Loading => "Loading model".into(),
            EngineState::Unavailable(_) => "Speech unavailable".into(),
        },
    };
    let col = status_color(p, s);
    Frame::default()
        .fill(blend(p.surface, col, 0.10))
        .stroke(Stroke::new(1.0_f32, blend(p.surface, col, 0.45)))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(12, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                let (rect, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                let pulse = if s.phase == Phase::Recording { 0.55 + 0.45 * (t * 4.0).sin().abs() as f32 } else { 1.0 };
                ui.painter().circle_filled(rect.center(), 4.0, col.gamma_multiply(pulse));
                ui.label(RichText::new(text).size(13.0).color(p.fg));
            });
        });
}

#[allow(dead_code)] // kept for the other layouts' level meter
fn meter(ui: &mut Ui, p: &Palette, levels: &VecDeque<f32>, color: Color32, height: f32, t: f64, phase: Phase) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let step = 6.0;
    let n = ((w / step) as usize).min(levels.len());
    let start_x = rect.right() - n as f32 * step;
    let skip = levels.len() - n;
    for i in 0..n {
        let v = levels[skip + i];
        let fi = (skip + i) as f64;
        let idle = match phase {
            Phase::Idle => 0.06 + 0.05 * ((fi * 0.33 + t * 1.3).sin() * 0.5 + 0.5) as f32 * (0.6 + 0.4 * (fi * 0.07).sin() as f32),
            Phase::Working => 0.10 + 0.25 * ((fi * 0.25 - t * 6.0).sin() * 0.5 + 0.5).powi(3) as f32,
            Phase::Recording => 0.04,
        };
        let v = v.max(idle);
        let h = (3.0 + v * (height - 3.0)).min(height);
        let x = start_x + i as f32 * step + 1.5;
        let r = Rect::from_center_size(pos2(x, rect.center().y), vec2(3.0, h));
        let c = if v <= idle + 0.001 { blend(p.surface, color, 0.55) } else { color };
        painter.rect_filled(r, CornerRadius::same(1), c);
    }
}

fn theme_dot(ui: &mut Ui, cur: &Palette, t: ThemeId, selected: bool) -> egui::Response {
    let tp = t.palette();
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let c = rect.center();
    ui.painter().circle_filled(c, 9.0, tp.bg);
    ui.painter().circle_filled(c, 5.0, tp.accent);
    ui.painter().circle_stroke(c, 10.5, Stroke::new(if selected { 2.0_f32 } else { 1.0 }, if selected { cur.accent } else { cur.line_soft() }));
    resp.on_hover_text(t.label())
}

fn theme_swatch(ui: &mut Ui, cur: &Palette, t: ThemeId, selected: bool) -> egui::Response {
    let tp = t.palette();
    let (rect, resp) = ui.allocate_exact_size(vec2(136.0, 92.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), tp.bg);
    let inner = rect.shrink(9.0);
    painter.rect_filled(Rect::from_min_size(inner.min, vec2(inner.width(), 9.0)), CornerRadius::same(3), tp.nav);
    painter.rect_filled(Rect::from_min_size(inner.min + vec2(4.0, 2.5), vec2(4.0, 4.0)), CornerRadius::same(2), tp.accent);
    let body = Rect::from_min_max(inner.min + vec2(0.0, 14.0), pos2(inner.right(), inner.bottom() - 20.0));
    painter.rect_filled(body, CornerRadius::same(4), tp.surface);
    for i in 0..12 {
        let h = 4.0 + 10.0 * ((i as f32 * 1.7).sin() * 0.5 + 0.5);
        let x = body.left() + 8.0 + i as f32 * 5.0;
        painter.rect_filled(Rect::from_center_size(pos2(x, body.center().y), vec2(2.5, h)), CornerRadius::same(1), tp.accent);
    }
    painter.circle_filled(pos2(body.right() - 10.0, body.center().y), 3.5, tp.rec);
    painter.text(pos2(inner.left(), rect.bottom() - 9.0), Align2::LEFT_BOTTOM, t.label(), FontId::proportional(12.5), tp.fg);
    let stroke = if selected { Stroke::new(2.0_f32, cur.accent) } else if resp.hovered() { Stroke::new(1.0_f32, cur.line) } else { Stroke::new(1.0_f32, cur.line_faint()) };
    painter.rect_stroke(rect, CornerRadius::same(10), stroke, StrokeKind::Outside);
    resp.on_hover_text(format!("{} (Ctrl+T cycles)", t.label()))
}

fn layout_tile(ui: &mut Ui, p: &Palette, l: LayoutId, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(136.0, 92.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), p.surface);
    let area = Rect::from_min_max(rect.min + vec2(10.0, 10.0), pos2(rect.right() - 10.0, rect.bottom() - 26.0));
    painter.rect_filled(area, CornerRadius::same(4), p.bg);
    let nav = p.surface2;
    let block = blend(p.bg, p.accent, 0.35);
    match l {
        LayoutId::Desk => {
            painter.rect_filled(Rect::from_min_size(area.min, vec2(area.width(), 8.0)), CornerRadius::same(2), nav);
            painter.rect_filled(Rect::from_min_size(area.min + vec2(area.width() * 0.2, 13.0), vec2(area.width() * 0.6, area.height() - 23.0)), CornerRadius::same(2), block);
            painter.rect_filled(Rect::from_min_size(pos2(area.left(), area.bottom() - 5.0), vec2(area.width(), 5.0)), CornerRadius::same(1), nav);
        }
        LayoutId::Sidebar => {
            painter.rect_filled(Rect::from_min_size(area.min, vec2(26.0, area.height())), CornerRadius::same(2), nav);
            painter.rect_filled(Rect::from_min_max(area.min + vec2(34.0, 6.0), area.max - vec2(8.0, 6.0)), CornerRadius::same(2), block);
        }
        LayoutId::Compact => {
            let col = Rect::from_center_size(area.center(), vec2(36.0, area.height()));
            painter.rect_filled(Rect::from_min_size(col.min, vec2(col.width(), 7.0)), CornerRadius::same(2), nav);
            painter.rect_filled(Rect::from_min_max(col.min + vec2(3.0, 11.0), col.max - vec2(3.0, 3.0)), CornerRadius::same(2), block);
        }
    }
    painter.text(pos2(rect.left() + 10.0, rect.bottom() - 8.0), Align2::LEFT_BOTTOM, l.label(), FontId::proportional(12.5), p.fg);
    let stroke = if selected { Stroke::new(2.0_f32, p.accent) } else if resp.hovered() { Stroke::new(1.0_f32, p.line) } else { Stroke::new(1.0_f32, p.line_faint()) };
    painter.rect_stroke(rect, CornerRadius::same(10), stroke, StrokeKind::Outside);
    resp
}

fn path_edit(ui: &mut Ui, s: &mut String) {
    ui.add(egui::TextEdit::singleline(s).desired_width(380.0).font(egui::TextStyle::Small));
}

fn short_status(s: &str) -> String {
    let line = s.lines().next().unwrap_or("");
    clip(line, 110)
}

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}

fn time_of(when: &str) -> String {
    when.split_whitespace().last().unwrap_or(when).chars().take(5).collect()
}

// ---- Pieces that copy the original's look (ui/index.html, ui/app.css) ----

#[derive(Clone, Copy)]
enum Icon {
    Mic,
    Flask,
    Clock,
    Gear,
    Power,
}

fn page_icon(pg: Page) -> Icon {
    match pg {
        Page::Dictate => Icon::Mic,
        Page::Lab => Icon::Flask,
        Page::Meetings => Icon::Mic,
        Page::History => Icon::Clock,
        Page::Settings => Icon::Gear,
    }
}

/// Line icons drawn on a 24-unit grid, like the original's stroke SVG symbols.
fn draw_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width() / 24.0;
    let o = rect.min;
    let pt = |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<egui::Pos2> {
        (0..=28)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / 28.0;
                pos2(o.x + (cx + r * a.cos()) * s, o.y + (cy + r * a.sin()) * s)
            })
            .collect()
    };
    let st = Stroke::new(1.7_f32, color);
    match icon {
        Icon::Mic => {
            painter.rect_stroke(Rect::from_min_max(pt(9.0, 3.0), pt(15.0, 14.5)), CornerRadius::same((3.0 * s) as u8), st, StrokeKind::Middle);
            painter.add(egui::Shape::line(arc(12.0, 11.0, 6.5, 0.0, std::f32::consts::PI), st));
            painter.line_segment([pt(12.0, 17.5), pt(12.0, 21.0)], st);
            painter.line_segment([pt(8.5, 21.0), pt(15.5, 21.0)], st);
        }
        Icon::Flask => {
            painter.add(egui::Shape::line(
                vec![pt(10.0, 3.0), pt(10.0, 9.5), pt(4.5, 19.0), pt(5.5, 21.0), pt(18.5, 21.0), pt(19.5, 19.0), pt(14.0, 9.5), pt(14.0, 3.0)],
                st,
            ));
            painter.line_segment([pt(8.5, 3.0), pt(15.5, 3.0)], st);
            painter.line_segment([pt(7.2, 15.0), pt(16.8, 15.0)], st);
        }
        Icon::Clock => {
            painter.circle_stroke(pt(12.0, 12.0), 9.0 * s, st);
            painter.add(egui::Shape::line(vec![pt(12.0, 7.0), pt(12.0, 12.0), pt(15.5, 14.0)], st));
        }
        Icon::Gear => {
            painter.circle_stroke(pt(12.0, 12.0), 3.0 * s, st);
            painter.circle_stroke(pt(12.0, 12.0), 6.8 * s, st);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                painter.line_segment([pt(12.0 + 6.8 * a.cos(), 12.0 + 6.8 * a.sin()), pt(12.0 + 9.4 * a.cos(), 12.0 + 9.4 * a.sin())], st);
            }
        }
        Icon::Power => {
            painter.add(egui::Shape::line(arc(12.0, 13.0, 8.0, -1.0472, 4.18879), st));
            painter.line_segment([pt(12.0, 3.0), pt(12.0, 12.0)], st);
        }
    }
}

fn brand_mark(ui: &mut Ui, size: f32) {
    let id = egui::Id::new("listen-rust-logo");
    let texture = ui.ctx().data(|data| data.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let image = egui::ColorImage::from_rgba_unmultiplied([128, 128], &crate::mark::rgba(128, crate::mark::IDLE));
        let texture = ui.ctx().load_texture("listen-rust-logo", image, egui::TextureOptions::LINEAR);
        ui.ctx().data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    ui.image((texture.id(), vec2(size, size)));
}

/// A rail button: icon and label, muted 14 px, 38 px tall, hover fill (app.css .rail-btn).
fn rail_button(ui: &mut Ui, p: &Palette, icon: Option<Icon>, label: &str, active: bool) -> egui::Response {
    let font = FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, p.fg);
    let icon_w = if icon.is_some() { 17.0 + 7.0 } else { 0.0 };
    let size = vec2(11.0 + icon_w + galley.size().x + 11.0, 38.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    if hovered && !active {
        ui.painter().rect_filled(rect, CornerRadius::same(8), p.surface2);
    }
    let color = if active || hovered { p.fg } else { p.muted };
    let mut x = rect.left() + 11.0;
    if let Some(ic) = icon {
        let ir = Rect::from_center_size(pos2(x + 8.5, rect.center().y), vec2(17.0, 17.0));
        draw_icon(ui.painter(), ir, ic, color);
        x += icon_w;
    }
    ui.painter().galley_with_override_text_color(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn status_word(s: &Snap) -> &'static str {
    match s.phase {
        Phase::Recording => "recording",
        Phase::Working => "working",
        Phase::Idle => match s.engine {
            EngineState::Unavailable(_) => "no engine",
            EngineState::Loading => "loading",
            _ => "idle",
        },
    }
}

/// The 8 px status dot (app.css .dot), with a soft pulse while recording.
fn status_dot(ui: &mut Ui, p: &Palette, s: &Snap, t: f64) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    let c = status_color(p, s);
    if s.phase == Phase::Recording {
        let k = (0.5 + 0.5 * (t * 2.5).sin()) as f32;
        ui.painter().circle_filled(r.center(), 4.0 + 3.0 * k, c.gamma_multiply(0.35 * (1.0 - k)));
    }
    ui.painter().circle_filled(r.center(), 4.0, c);
    resp
}

/// The 100x100 key picture: "In Listen" / key cap / "right key" (app.css .key-visual).
fn key_visual(ui: &mut Ui, p: &Palette, hotkey: Hotkey, phase: Phase) {
    let (rect, _) = ui.allocate_exact_size(vec2(100.0, 100.0), Sense::hover());
    let border = if phase == Phase::Recording { p.rec } else { p.line };
    ui.painter().rect(rect, CornerRadius::same(12), p.surface, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    let label = hotkey.label();
    let (cap, low) = match label.strip_prefix("Right ") {
        Some(rest) => (rest.to_string(), "right key"),
        None => (label.to_string(), "key"),
    };
    let cap_px = if cap.chars().count() > 5 { 19.0 } else { 27.0 };
    let painter = ui.painter();
    painter.text(pos2(rect.center().x, rect.top() + 22.0), egui::Align2::CENTER_CENTER, "In Listen", FontId::proportional(12.0), p.muted);
    painter.text(rect.center() + vec2(0.0, 2.0), egui::Align2::CENTER_CENTER, cap, FontId::new(cap_px, FontFamily::Name("humanist".into())), p.fg);
    painter.text(pos2(rect.center().x, rect.bottom() - 20.0), egui::Align2::CENTER_CENTER, low, FontId::proportional(12.0), p.muted);
}

/// Five small accent bars beside the hero title (app.css .quiet-instrument, 34x28).
fn quiet_instrument(ui: &mut Ui, p: &Palette, phase: Phase, t: f64) {
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 28.0), Sense::hover());
    let rest = [8.0, 12.0, 16.0, 12.0, 8.0];
    let live = [10.0, 18.0, 25.0, 18.0, 10.0];
    for i in 0..5 {
        let (h, a) = match phase {
            Phase::Recording => {
                let wob = 0.85 + 0.15 * ((t * 7.0 + i as f64 * 1.3).sin() as f32);
                (live[i] * wob, if i == 2 { 1.0 } else { 0.85 })
            }
            Phase::Working => (rest[i] * (0.8 + 0.2 * ((t * 5.0 - i as f64).sin() as f32)), 0.7),
            Phase::Idle => (rest[i], 0.55),
        };
        let x = rect.left() + 1.0 + i as f32 * 7.0;
        let r = Rect::from_center_size(pos2(x + 2.0, rect.center().y), vec2(4.0, h));
        ui.painter().rect_filled(r, CornerRadius::same(2), p.accent.gamma_multiply(a));
    }
}

/// The 96 px level strip under the hero text: 3 px bars, gap 3, 28 tall (app.css .hero-bars).
fn hero_bars(ui: &mut Ui, _p: &Palette, levels: &VecDeque<f32>, color: Color32, t: f64, phase: Phase) {
    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 28.0), Sense::hover());
    let n = 16usize;
    let recent: Vec<f32> = levels.iter().rev().take(n).rev().copied().collect();
    for i in 0..n {
        let fi = i as f64;
        let v = match phase {
            Phase::Recording => recent.get(i + recent.len().saturating_sub(n)).copied().unwrap_or(0.0).clamp(0.0, 1.0).max(0.06),
            Phase::Working => 0.12 + 0.3 * ((fi * 0.5 - t * 6.0).sin() * 0.5 + 0.5).powi(3) as f32,
            Phase::Idle => 0.10 + 0.06 * ((fi * 0.6 + t * 1.2).sin() * 0.5 + 0.5) as f32,
        };
        let h = (v * 28.0).max(3.0);
        let x = rect.left() + i as f32 * 6.0;
        let r = Rect::from_center_size(pos2(x + 1.5, rect.center().y), vec2(3.0, h));
        ui.painter().rect_filled(r, CornerRadius::same(1), color);
    }
}

/// A transparent 32 px button with muted text (app.css .btn.quiet).
fn quiet_button(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> egui::Response {
    let color = if enabled { p.muted } else { p.faint.gamma_multiply(0.55) };
    let b = egui::Button::new(RichText::new(text).size(13.0).color(color))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

/// A bordered box whose title sits on the top border like a legend (app.css .frame, .frame-title).
/// Returns true when the optional link on the right was clicked.
fn frame_titled(ui: &mut Ui, p: &Palette, title: &str, link: Option<&str>, focused: bool, add: impl FnOnce(&mut Ui)) -> bool {
    let border = if focused { p.accent } else { p.line };
    let bg_idx = ui.painter().add(egui::Shape::Noop);
    let inner = Frame::default()
        .inner_margin(Margin { left: 16, right: 16, top: 18, bottom: 14 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    let rect = inner.response.rect;
    let painter = ui.painter();
    painter.set(bg_idx, egui::Shape::Vec(vec![
        egui::Shape::rect_filled(rect, CornerRadius::same(12), p.surface),
        egui::Shape::rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0_f32, border), StrokeKind::Inside),
    ]));
    // Legend: cover the border behind the title with the page colour, then draw the title.
    let font = FontId::new(16.0, FontFamily::Name("humanist".into()));
    let galley = painter.layout_no_wrap(title.to_string(), font, p.fg);
    let tl = pos2(rect.left() + 14.0, rect.top() - galley.size().y / 2.0);
    let cover = Rect::from_min_size(pos2(tl.x - 6.0, rect.top() - 2.0), vec2(galley.size().x + 12.0, 4.0));
    painter.rect_filled(cover, CornerRadius::ZERO, p.surface);
    let title_right = tl.x + galley.size().x;
    painter.galley(tl, galley, p.fg);
    let mut clicked = false;
    if let Some(link) = link {
        let lg = painter.layout_no_wrap(link.to_string(), FontId::proportional(13.0), p.accent);
        // Original: the link sits right after the legend title (margin-inline-start 8px).
        let lr = Rect::from_min_size(pos2(title_right + 8.0 - 6.0, rect.top() - lg.size().y / 2.0), vec2(lg.size().x + 12.0, lg.size().y));
        let resp = ui.interact(lr, ui.id().with(("frame-link", title)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter();
        painter.rect_filled(lr, CornerRadius::ZERO, p.surface);
        let c = if resp.hovered() { p.fg } else { p.accent };
        painter.galley_with_override_text_color(pos2(lr.left() + 6.0, lr.top()), lg, c);
        clicked = resp.clicked();
    }
    clicked
}

