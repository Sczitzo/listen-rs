//! Pure-Rust native egui dashboard UI for Listen (zero JS/HTML/CSS/WebView2).
//!
//! Modular split:
//! - `widgets`: shared visual primitives (border-inline frames, pill toggles, 3-band theme tiles, sparklines, icons)
//! - `dictate_lab`: Dictate page + Prompt Lab page + Compare suggestion modal
//! - `meetings_history`: Local Meeting Notetaker page + History & 4-week stats page
//! - `settings`: Direction A filterable split-pane Settings + interactive Hotkey Picker modal

pub mod dictate_lab;
pub mod meetings_history;
pub mod settings;
pub mod widgets;

use crate::app::{Page, Snap};
use crate::app_tone::TonePreset;
use crate::meeting::MeetingNote;
use crate::models::SavedBenchmarks;
use crate::storage::{
    Destination, Favorite, HistoryEntry, Hotkey, PromptProvider, PromptTarget, Settings,
    SpeechChoice, Style,
};
use crate::theme::{LayoutId, ThemeId};
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsSection {
    #[default]
    General,
    VoiceAi,
    Snippets,
    Appearance,
    PromptLab,
    System,
}

impl SettingsSection {
    pub const ALL: [Self; 6] = [
        Self::General,
        Self::VoiceAi,
        Self::Snippets,
        Self::Appearance,
        Self::PromptLab,
        Self::System,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::VoiceAi => "Voice & AI",
            Self::Snippets => "Snippets",
            Self::Appearance => "Appearance",
            Self::PromptLab => "Prompt Lab",
            Self::System => "System",
        }
    }

    pub fn keywords(self) -> &'static str {
        match self {
            Self::General => "general hotkey dictation command mode microphone destination preview floating card sound cues clipboard quiet whisper mode",
            Self::VoiceAi => "voice ai speech engine parakeet whisper download model install ollama formatting style benchmark app tone casual polished technical rules",
            Self::Snippets => "snippets voice triggers expansion templates email signature",
            Self::Appearance => "appearance theme color scheme palette warm charcoal paper midnight sage contrast espresso nordic plum ocean obsidian mist blossom layout desk sidebar compact zoom",
            Self::PromptLab => "prompt lab target auto coding general writing checking provider claude openai gemini local",
            Self::System => "system shortcuts keyboard saved files clear drafts logs diagnostics version",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HistoryGrouping {
    #[default]
    Day,
    App,
}

#[derive(Clone, Debug, Default)]
pub struct LabAssessmentView {
    pub score: Option<u32>,
    pub prev_score: Option<u32>,
    pub summary: String,
    pub gaps: Vec<String>,
    pub criteria: Vec<(String, u32)>,
    pub by: String,
}

#[derive(Clone, Debug, Default)]
pub struct LabCandidateView {
    pub id: u64,
    pub version_idx: usize,
    pub original: String,
    pub suggestion: String,
    pub label: String,
    pub model: String,
    pub note: String,
    pub assessment: Option<LabAssessmentView>,
}

#[derive(Clone, Debug, Default)]
pub struct ArchivedDraftItem {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct LabUiState {
    pub draft: String,
    pub instruction: String,
    pub busy_label: String,
    pub status_note: String,
    pub error: Option<String>,
    pub saved_note: String,
    pub version_label: String,
    pub revisions: Vec<(usize, String)>,
    pub current_rev: usize,
    pub assessment: Option<LabAssessmentView>,
    pub assessment_stale: bool,
    pub candidate: Option<LabCandidateView>,
    pub compare_open: bool,
    pub compare_show_original: bool,
    pub show_models_popover: bool,
    pub show_more_menu: bool,
    pub show_library: bool,
    pub save_fav_open: bool,
    pub save_fav_name: String,
    pub archived: Vec<ArchivedDraftItem>,
    pub selected_archived: String,
    pub show_more_tools: bool,
}

#[derive(Clone, Debug, Default)]
pub struct MeetingUiState {
    pub active: bool,
    pub summarizing: bool,
    pub elapsed_secs: f32,
    pub include_system_audio: bool,
    pub status_text: String,
    pub notes: Vec<MeetingNote>,
    pub expanded_transcripts: HashSet<String>,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryStatsView {
    pub today: usize,
    pub week_count: usize,
    pub words_week: usize,
    pub minutes_week: f64,
    pub wpm: u64,
    pub latency: f64,
    pub trend_today: Vec<Option<usize>>,
    pub trend_words: Vec<Option<usize>>,
    pub trend_wpm: Vec<Option<u64>>,
    pub trend_latency: Vec<Option<f64>>,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryUiState {
    pub filter: String,
    pub grouping: HistoryGrouping,
    pub open_ids: HashSet<u64>,
    pub collapsed_groups: HashSet<String>,
    pub selected_group: Option<String>,
    pub jump_target: Option<String>,
    pub jump_frames: u8,
    pub display_limit: usize,
    pub last_filter: String,
    pub confirm_clear: bool,
}

#[derive(Clone, Debug, Default)]
pub struct HotkeyModalState {
    pub open: bool,
    pub listening: bool,
    pub captured_value: String,
    pub captured_label: String,
    pub warning_level: String, // "", "clear", "warning", "blocked"
    pub warning_message: String,
    pub controller_mode: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SettingsUiState {
    pub section: SettingsSection,
    pub filter: String,
    pub snippet_trigger: String,
    pub snippet_expansion: String,
    pub snippet_error: Option<String>,
    pub tone_app_input: String,
    pub tone_preset_input: TonePreset,
    pub tone_error: Option<String>,
    pub confirm_clear_drafts: bool,
    pub confirm_clear_logs: bool,
    pub diagnostics_summary: String,
}

#[derive(Clone, Debug, Default)]
pub struct ModelStatusSnapshot {
    pub active_speech_id: String,
    pub parakeet_installed: bool,
    pub installed_whisper_ids: HashSet<String>,
    pub install_active: bool,
    pub install_target_id: String,
    pub install_stage: String,
    pub install_progress: f32,
    pub install_error: Option<String>,
    pub benchmark_active: bool,
    pub benchmark_stage: String,
    pub benchmark_progress: f32,
    pub benchmark_error: Option<String>,
    pub saved_benchmarks: SavedBenchmarks,
}

/// Read-only view of app data passed to every UI screen.
pub struct UiView<'a> {
    pub page: Page,
    pub settings: &'a Settings,
    pub snap: &'a Snap,
    pub levels: &'a VecDeque<f32>,
    pub history: &'a [HistoryEntry],
    pub history_stats: &'a HistoryStatsView,
    pub favorites: &'a [Favorite],
    pub vocab_suggestions: &'a [String],
    pub models: &'a [String],
    pub ollama_available: bool,
    pub mics: &'a [String],
    pub model_status: &'a ModelStatusSnapshot,
    pub api_keys_present: (bool, bool, bool), // (anthropic, openai, gemini)
    pub hotkey_display: &'a str,
    pub hotkey_parts: &'a [String],
    pub demo_recording: bool,
    pub retained: &'a [(String, String)],
    pub clipboard_recovery: bool,
    pub toast: Option<&'a str>,
    pub time_secs: f64,
}

/// Actions emitted by UI screens and handled by `ListenApp`.
#[derive(Clone, Debug)]
pub enum UiAction {
    // Navigation & lifecycle
    GoPage(Page),
    RequestQuit,
    CopyText(String, &'static str), // (text, toast label)
    RestoreClipboard,
    DiscardClipboard,
    DismissRetained(usize),

    // Dictate
    OpenHotkeyPicker,
    StartDictation,
    FinishDictation,
    CancelDictation,
    UpdateVocab(String),
    AcceptVocabSuggestion(String),
    DismissVocabSuggestion(String),
    ClearDismissedVocab,

    // Prompt Lab
    LabDraftChanged(String),
    LabNewDraft,
    LabEngineer,
    LabSuggestChanges,
    LabRewriteScratch,
    LabApplyInstruction(String),
    LabScoreDraft,
    LabApplyCandidate,
    LabDiscardCandidate,
    LabReviewCandidate,
    LabSelectRevision(usize),
    LabPasteDraft,
    LabSaveFavorite(String),
    LabLoadFavorite(usize),
    LabDeleteFavorite(usize),
    LabRestoreArchived(String),
    LabDeleteArchived(String),

    // Meetings
    MeetingToggleSystemAudio(bool),
    MeetingStart,
    MeetingStopAndSummarize,
    MeetingCancel,
    MeetingDelete(String),
    MeetingResummarize(u64),

    // History
    ClearHistory,

    // Settings
    SetCommandHotkey(Option<Hotkey>),
    SetMicrophone(String),
    SetDestination(Destination),
    SetLivePreview(bool),
    SetFloatingCard(bool),
    SetSounds(bool),
    SetRestoreClipboard(bool),
    SetQuietMode(bool),
    SetSpeechEngine(SpeechChoice),
    SetCleanup(bool),
    SetStyle(Style),
    SetLocalModel(String),
    RefreshModels,
    InstallSpeechModel(String),
    SelectSpeechModel(String),
    CancelModelInstall,
    StartBenchmark,
    CancelBenchmark,
    SetAutoAppTone(bool),
    AddAppToneRule { app: String, tone: TonePreset },
    DeleteAppToneRule(usize),
    AddSnippet { trigger: String, expansion: String },
    DeleteSnippet(usize),
    SetTheme(ThemeId),
    SetLayout(LayoutId),
    SetZoom(f32),
    SetPromptTarget(PromptTarget),
    SetPromptEngine(PromptProvider),
    SetPromptJudge(PromptProvider),
    SetEngineLocalModel(String),
    SetJudgeLocalModel(String),
    SetCloudEnabled(bool),
    SetClaudeModel(String),
    ClearDrafts,
    ClearLogs,

    // Hotkey modal
    HotkeyModalCaptureKey(String),
    HotkeyModalBeginController,
    HotkeyModalApply(String),
    HotkeyModalCancel,
}
