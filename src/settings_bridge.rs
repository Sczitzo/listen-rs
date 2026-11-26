use crate::engine::{Cmd, Core, Phase};
use crate::storage::{
    self, Destination, Hotkey, PromptProvider, PromptTarget, Settings, SpeechChoice, Style,
};
use crate::{audio, cleanup, models, speech};
use serde_json::{json, Map, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

const PARAKEET: &str = models::PARAKEET_ID;
const PARAKEET_LABEL: &str = "Parakeet Unified English 0.6B (default)";
const WINDOWS_MIC: &str = "Use Windows default microphone";
const MAX_VOCAB: usize = 16_000;
const MAX_SNIPPETS: usize = 200;
const MAX_APP_TONE_RULES: usize = 100;

pub(crate) fn hotkey_id(hotkey: Hotkey) -> &'static str {
    match hotkey {
        Hotkey::RightCtrl => "ctrl_r",
        Hotkey::RightShift => "shift_r",
        Hotkey::RightAlt => "alt_r",
        Hotkey::Pause => "pause",
        Hotkey::ScrollLock => "scroll_lock",
    }
}

pub(crate) fn parse_hotkey(id: &str) -> Option<Hotkey> {
    Hotkey::ALL.into_iter().find(|key| hotkey_id(*key) == id)
}

fn speech_id(settings: &Settings) -> String {
    models::active_speech_id(settings)
}

fn speech_label_for(id: &str) -> String {
    if id == PARAKEET {
        PARAKEET_LABEL.into()
    } else if let Some(entry) = models::find_speech_entry(id) {
        entry.name.into()
    } else {
        "Whisper.cpp (configured model)".into()
    }
}

fn destination_id(destination: Destination) -> &'static str {
    match destination {
        Destination::LastApp => "last_app",
        Destination::Workspace => "workspace",
    }
}

fn settings_view(settings: &Settings) -> Value {
    json!({
        "hotkey": settings.registered_hotkey.as_deref().unwrap_or_else(|| hotkey_id(settings.hotkey)),
        "command_hotkey": settings.command_hotkey.map(hotkey_id).unwrap_or(""),
        "microphone": (!settings.microphone.is_empty()).then_some(&settings.microphone),
        "destination": destination_id(settings.destination),
        "live_preview": settings.live_preview,
        "floating_card": settings.floating_card,
        "sounds": settings.sounds,
        "restore_clipboard": settings.restore_clipboard,
        "quiet_mode": settings.quiet_mode,
        "speech_model": speech_id(settings),
        "cleanup": settings.cleanup,
        "style": if settings.style == Style::Clean { "clean" } else { "tighten" },
        "local_model": settings.local_model,
        "vocab": settings.vocab,
        "snippets": settings.snippets,
        "auto_app_tone": settings.auto_app_tone,
        "app_tone_rules": settings.app_tone_rules,
        "learn": settings.learn,
        "target": match settings.prompt_target {
            PromptTarget::Auto => "auto",
            PromptTarget::ClaudeCode => "claude_code",
            PromptTarget::General => "general",
        },
        "engine": settings.prompt_engine.id(), "judge": settings.prompt_judge.id(),
        "engine_local_model": settings.engine_local_model, "judge_local_model": settings.judge_local_model,
        "judge_model": settings.claude_model,
        "cloud_enabled": settings.cloud_enabled,
    })
}

fn microphones(devices: Vec<String>) -> Value {
    let mut options = Map::new();
    options.insert(WINDOWS_MIC.into(), Value::Null);
    for name in devices.into_iter().filter(|name| !name.is_empty()) {
        let label = if name == WINDOWS_MIC {
            format!("{name} (device)")
        } else {
            name.clone()
        };
        options.insert(label, Value::String(name));
    }
    Value::Object(options)
}

fn microphone_options(preview: bool) -> Value {
    let devices = if preview {
        vec!["Demo microphone (simulated)".into()]
    } else {
        audio::input_devices()
    };
    microphones(devices)
}

fn speech_options(settings: &Settings, preview: bool) -> Map<String, Value> {
    let mut options = Map::new();
    if preview {
        options.insert(PARAKEET_LABEL.into(), json!(PARAKEET));
        return options;
    }
    let probe_parakeet = Settings {
        speech_engine: SpeechChoice::Transcribe,
        ..settings.clone()
    };
    if speech::probe(&probe_parakeet).is_ok() {
        options.insert(PARAKEET_LABEL.into(), json!(PARAKEET));
    }
    if models::find_whisper_cli(settings).is_some() {
        for entry in models::installed_managed_whisper_models(&models::speech_models_dir()) {
            options.insert(entry.name.into(), json!(entry.id));
        }
        let custom = Settings {
            speech_engine: SpeechChoice::WhisperCli,
            ..settings.clone()
        };
        if speech::probe(&custom).is_ok() && speech_id(settings) == "whisper-cpp" {
            options.insert("Whisper.cpp (configured model)".into(), json!("whisper-cpp"));
        }
    }
    options
}

fn options_view(settings: &Settings, preview: bool) -> Value {
    let hotkeys = Hotkey::ALL
        .into_iter()
        .map(|key| (key.label(), hotkey_id(key)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let routes = PromptProvider::ALL.map(|provider| {
        let label = if provider == PromptProvider::Local { provider.label().to_owned() } else { format!("{} (API)", provider.label()) };
        (label, provider.id())
    }).into_iter().collect::<std::collections::BTreeMap<_, _>>();
    let providers = PromptProvider::ALL.into_iter().filter(|provider| *provider != PromptProvider::Local)
        .map(|provider| (provider.id(), json!({
            "name": provider.label(), "key_env": provider.keys().join(" or "),
            "model": provider.model("", &settings.claude_model),
            "configured": !preview && cleanup::provider_key(provider).is_some(),
        }))).collect::<std::collections::BTreeMap<_, _>>();
    json!({
        "hotkey": hotkeys,
        "destination": {
            Destination::LastApp.label(): "last_app",
            Destination::Workspace.label(): "workspace",
        },
        "style": {
            Style::Tighten.label(): "tighten",
            Style::Clean.label(): "clean",
        },
        "target": {
            PromptTarget::Auto.label(): "auto",
            PromptTarget::ClaudeCode.label(): "claude_code",
            PromptTarget::General.label(): "general",
        },
        "engine": routes, "judge": routes,
        "prompt_providers": providers,
        "judge_model": {"Haiku 5.5 · budget": "claude-haiku-5-5", "Sonnet 5.5 · deeper critique": "claude-sonnet-5-5", "Opus 5.5 · detailed review": "claude-opus-5-5"},
        "microphone": { WINDOWS_MIC: Value::Null },
        "speech_model": speech_id(settings),
        "speech_models": speech_options(settings, preview),
        "speech_model_label": speech_label_for(&speech_id(settings)),
        "speech_model_note": "Shows the local speech models installed on this computer. Download more below.",
    })
}

fn update(core: &Core, change: impl FnOnce(&mut Settings) -> bool) -> bool {
    let mut settings = core.settings.lock().unwrap();
    let mut updated = settings.clone();
    if !change(&mut updated) || (!core.preview && storage::save_settings_checked(&updated).is_err())
    {
        return false;
    }
    *settings = updated;
    true
}

fn local_models(preview: bool, url: &str) -> Result<Vec<String>, String> {
    if preview {
        return Err(
            "Sample mode doesn't check installed models. Your selection is preserved.".into(),
        );
    }
    let mut models = cleanup::ollama_models(url).map_err(|_| "Could not list installed models. Start Ollama and refresh; your selection is preserved.".to_string())?;
    models.sort();
    models.dedup();
    Ok(models)
}

fn set_setting(core: &Core, key: &str, value: &Value) -> bool {
    let string = value.as_str();
    if matches!(key, "hotkey" | "command_hotkey" | "microphone" | "speech_model")
        && core.phase() != Phase::Idle
    {
        return false;
    }
    if key == "speech_model" && !core.can_reload_engine() {
        return false;
    }
    let checked = match key {
        "microphone" => {
            if !(value.is_null() || string.is_some()) {
                return false;
            }
            microphone_options(core.preview)
                .as_object()
                .is_some_and(|items| items.values().any(|device| device == value))
        }
        "speech_model" => {
            let settings = core.settings.lock().unwrap().clone();
            speech_options(&settings, core.preview)
                .values()
                .any(|model| model == value)
        }
        "local_model" | "engine_local_model" | "judge_local_model" => {
            let url = core.settings.lock().unwrap().ollama_url.clone();
            string.is_some_and(|model| {
                (model.is_empty() && key != "local_model") ||
                    local_models(core.preview, &url).is_ok_and(|models| models.iter().any(|name| name == model))
            })
        }
        _ => true,
    };
    if !checked {
        return false;
    }
    let saved = update(core, |s| {
        match (key, string, value.as_bool()) {
            ("microphone", name, _) => s.microphone = name.unwrap_or_default().into(),
            ("speech_model", Some(PARAKEET), _) => s.speech_engine = SpeechChoice::Transcribe,
            ("speech_model", Some("whisper-cpp"), _) => s.speech_engine = SpeechChoice::WhisperCli,
            ("speech_model", Some(id), _) if id.starts_with("whisper:") => {
                let Some(entry) = models::find_speech_entry(id) else { return false };
                let Some(cli) = models::find_whisper_cli(s) else { return false };
                let Some(model_path) = models::managed_whisper_model_path(entry) else { return false };
                if !cli.is_file() || !model_path.is_file() {
                    return false;
                }
                s.speech_engine = SpeechChoice::WhisperCli;
                s.whisper_cli = cli.to_string_lossy().into_owned();
                s.whisper_model = model_path.to_string_lossy().into_owned();
            }
            ("local_model", Some(model), _) => s.local_model = model.into(),
            ("engine_local_model", Some(model), _) => s.engine_local_model = model.into(),
            ("judge_local_model", Some(model), _) => s.judge_local_model = model.into(),
            ("live_preview", _, Some(on)) => s.live_preview = on,
            ("floating_card", _, Some(on)) => s.floating_card = on,
            ("sounds", _, Some(on)) => s.sounds = on,
            ("restore_clipboard", _, Some(on)) => s.restore_clipboard = on,
            ("quiet_mode", _, Some(on)) => s.quiet_mode = on,
            ("auto_app_tone", _, Some(on)) => s.auto_app_tone = on,
            ("command_hotkey", Some(""), _) => s.command_hotkey = None,
            ("command_hotkey", Some(id), _) => {
                let Some(key) = parse_hotkey(id) else { return false };
                let conflicts = s.registered_hotkey.as_deref() == Some(id)
                    || (s.registered_hotkey.is_none() && key == s.hotkey);
                if conflicts {
                    return false;
                }
                s.command_hotkey = Some(key);
            }
            ("destination", Some("last_app"), _) => s.destination = Destination::LastApp,
            ("destination", Some("workspace"), _) => s.destination = Destination::Workspace,
            ("cleanup", _, Some(on)) => s.cleanup = on,
            ("learn", _, Some(on)) => s.learn = on,
            ("style", Some("clean"), _) => s.style = Style::Clean,
            ("style", Some("tighten"), _) => s.style = Style::Tighten,
            ("target", Some("auto"), _) => s.prompt_target = PromptTarget::Auto,
            ("target", Some("general"), _) => s.prompt_target = PromptTarget::General,
            ("target", Some("claude_code"), _) => s.prompt_target = PromptTarget::ClaudeCode,
            ("vocab", Some(vocab), _) => match sanitize_vocab(vocab) {
                Ok(clean) => s.vocab = clean,
                Err(_) => return false,
            },
            ("cloud_enabled", _, Some(allowed)) => s.cloud_enabled = allowed,
            ("engine" | "judge", Some(id), _) => {
                let Some(provider) = PromptProvider::ALL.into_iter().find(|route| route.id() == id) else { return false };
                if key == "engine" { s.prompt_engine = provider; } else { s.prompt_judge = provider; }
            }
            ("judge_model", Some(model @ ("claude-haiku-5-5" | "claude-sonnet-5-5" | "claude-opus-5-5")), _) => s.claude_model = model.into(),
            _ => return false,
        }
        true
    });
    if saved && key == "speech_model" && !core.preview {
        core.send(Cmd::ReloadEngine);
    }
    saved
}

pub(crate) fn sanitize_vocab(vocab: &str) -> Result<String, String> {
    if vocab.contains('\0') {
        return Err("Vocabulary cannot contain NUL bytes.".into());
    }
    if vocab.len() > MAX_VOCAB {
        return Err(format!(
            "Vocabulary is too long (maximum {MAX_VOCAB} bytes)."
        ));
    }
    Ok(vocab.to_owned())
}

fn validate_vocab_word(word: &str) -> Result<&str, String> {
    let trimmed = word.trim();
    if trimmed.is_empty() {
        return Err("Vocabulary term cannot be empty.".into());
    }
    if trimmed.len() > 100 {
        return Err("Vocabulary term is too long (maximum 100 bytes).".into());
    }
    if trimmed.contains([',', ';', '\n', '\r', '\0']) {
        return Err(
            "Vocabulary term cannot contain commas, semicolons, line breaks, or NUL bytes.".into(),
        );
    }
    Ok(trimmed)
}

/// Validates and adds a single word to `settings.vocab`, preserving the user's existing
/// newline vs. comma delimiter style, deduplicating case-insensitively via
/// `storage::vocab_words`, enforcing `MAX_VOCAB` and NUL checks, and pruning the word
/// from `settings.vocab_dismissed`.
///
/// Returns `Ok(true)` if the word was newly appended, `Ok(false)` if already present.
pub(crate) fn add_vocab_word(settings: &mut Settings, word: &str) -> Result<bool, String> {
    let word = validate_vocab_word(word)?;
    let lower = word.to_lowercase();
    let already_present = storage::vocab_words(&settings.vocab)
        .iter()
        .any(|entry| entry.to_lowercase() == lower);
    if already_present {
        settings
            .vocab_dismissed
            .retain(|entry| entry.to_lowercase() != lower);
        return Ok(false);
    }
    let use_newlines = settings.vocab.contains('\n');
    let old = settings
        .vocab
        .trim_end_matches([',', ';', ' ', '\t', '\n', '\r']);
    let candidate = if old.is_empty() {
        word.to_owned()
    } else if use_newlines {
        format!("{old}\n{word}")
    } else {
        format!("{old}, {word}")
    };
    let clean = sanitize_vocab(&candidate)?;
    settings.vocab = clean;
    settings
        .vocab_dismissed
        .retain(|entry| entry.to_lowercase() != lower);
    Ok(true)
}

pub(crate) fn dismiss_vocab_word(settings: &mut Settings, word: &str) -> Result<bool, String> {
    let word = validate_vocab_word(word)?;
    let lower = word.to_lowercase();
    if settings
        .vocab_dismissed
        .iter()
        .any(|entry| entry.to_lowercase() == lower)
    {
        return Ok(false);
    }
    settings.vocab_dismissed.push(word.to_owned());
    Ok(true)
}

fn change_word(core: &Core, word: &Value, dismiss: bool) -> bool {
    let Some(word) = word.as_str() else {
        return false;
    };
    update(core, |s| {
        if dismiss {
            dismiss_vocab_word(s, word).is_ok()
        } else {
            add_vocab_word(s, word).is_ok()
        }
    })
}

pub(crate) fn clear_log_files(dir: &std::path::Path) -> Result<(), String> {
    let mut first_error = None;
    for name in ["last-error.txt", "last-crash.txt"] {
        if let Err(error) = std::fs::remove_file(dir.join(name)) {
            if error.kind() != std::io::ErrorKind::NotFound && first_error.is_none() {
                first_error = Some(format!("Could not remove {name}: {error}"));
            }
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

pub(crate) fn local_models_response(preview: bool, url: &str, selected: String) -> (Value, Option<Vec<String>>, bool) {
    match local_models(preview, url) {
        Ok(models) => {
            let error = if models.contains(&selected) {
                ""
            } else {
                "The selected model is not installed. Choose an installed model or restore it in Ollama."
            };
            let value = json!({"models":models,"selected":selected,"available":true,"error":error});
            (value, Some(models), true)
        }
        Err(error) => (
            json!({"models":[],"selected":selected,"available":false,"error":error}),
            Some(Vec::new()),
            false,
        ),
    }
}

pub(crate) fn upsert_snippet(
    snippets: &mut Vec<crate::snippets::Snippet>,
    trigger: &str,
    expansion: &str,
) -> Result<(), String> {
    let snippet = crate::snippets::validate_snippet(trigger, expansion)?;
    if let Some(existing) = snippets
        .iter_mut()
        .find(|item| crate::snippets::triggers_equal(&item.trigger, &snippet.trigger))
    {
        *existing = snippet;
        return Ok(());
    }
    if snippets.len() >= MAX_SNIPPETS {
        return Err(format!("You can save up to {MAX_SNIPPETS} snippets."));
    }
    snippets.push(snippet);
    Ok(())
}

fn add_snippet(core: &Core, args: &[Value]) -> Value {
    let trigger = args.first().and_then(Value::as_str).unwrap_or("");
    let expansion = args.get(1).and_then(Value::as_str).unwrap_or("");
    let mut err_msg: Option<String> = None;
    let saved = update(core, |s| {
        match upsert_snippet(&mut s.snippets, trigger, expansion) {
            Ok(()) => true,
            Err(msg) => {
                err_msg = Some(msg);
                false
            }
        }
    });
    if let Some(msg) = err_msg {
        return json!({ "ok": false, "message": msg });
    }
    if !saved {
        return json!({ "ok": false, "message": "Could not save snippet." });
    }
    let snippets = core.settings.lock().unwrap().snippets.clone();
    json!({ "ok": true, "snippets": snippets })
}

fn delete_snippet(core: &Core, args: &[Value]) -> Value {
    let trigger = args.first().and_then(Value::as_str).unwrap_or("").trim();
    if trigger.is_empty() {
        return json!({ "ok": false });
    }
    let saved = update(core, |s| {
        let before = s.snippets.len();
        s.snippets
            .retain(|item| !crate::snippets::triggers_equal(&item.trigger, trigger));
        s.snippets.len() != before
    });
    if !saved {
        return json!({ "ok": false });
    }
    let snippets = core.settings.lock().unwrap().snippets.clone();
    json!({ "ok": true, "snippets": snippets })
}

fn app_tones_view(settings: &Settings) -> Value {
    use crate::app_tone::TonePreset;
    let presets: Vec<[&str; 2]> = TonePreset::ALL
        .into_iter()
        .filter(|p| *p != TonePreset::Default)
        .chain(std::iter::once(TonePreset::Default))
        .map(|p| [p.id(), p.label()])
        .collect();
    json!({
        "auto_enabled": settings.auto_app_tone,
        "presets": presets,
        "rules": settings.app_tone_rules,
    })
}

pub(crate) fn upsert_app_tone_rule(
    rules: &mut Vec<crate::app_tone::AppToneRule>,
    app: &str,
    tone: crate::app_tone::TonePreset,
) -> Result<(), String> {
    let rule = crate::app_tone::validate_rule(app, tone)?;
    if let Some(existing) = rules
        .iter_mut()
        .find(|item| crate::app_tone::same_app_rule(&item.app, &rule.app))
    {
        *existing = rule;
        return Ok(());
    }
    if rules.len() >= MAX_APP_TONE_RULES {
        return Err(format!(
            "You can save up to {MAX_APP_TONE_RULES} app tone rules."
        ));
    }
    rules.push(rule);
    Ok(())
}

fn add_app_tone(core: &Core, args: &[Value]) -> Value {
    let app = args.first().and_then(Value::as_str).unwrap_or("");
    let tone_id = args.get(1).and_then(Value::as_str).unwrap_or("");
    let Some(tone) = crate::app_tone::TonePreset::from_id(tone_id) else {
        return json!({ "ok": false, "message": "Choose a valid tone preset." });
    };
    let mut err_msg: Option<String> = None;
    let saved = update(core, |s| {
        match upsert_app_tone_rule(&mut s.app_tone_rules, app, tone) {
            Ok(()) => true,
            Err(msg) => {
                err_msg = Some(msg);
                false
            }
        }
    });
    if let Some(msg) = err_msg {
        return json!({ "ok": false, "message": msg });
    }
    if !saved {
        return json!({ "ok": false, "message": "Could not save app tone rule." });
    }
    let rules = core.settings.lock().unwrap().app_tone_rules.clone();
    json!({ "ok": true, "rules": rules })
}

fn delete_app_tone(core: &Core, args: &[Value]) -> Value {
    let app = args.first().and_then(Value::as_str).unwrap_or("").trim();
    if app.is_empty() {
        return json!({ "ok": false });
    }
    let saved = update(core, |s| {
        let before = s.app_tone_rules.len();
        s.app_tone_rules
            .retain(|item| !crate::app_tone::same_app_rule(&item.app, app));
        s.app_tone_rules.len() != before
    });
    if !saved {
        return json!({ "ok": false });
    }
    let rules = core.settings.lock().unwrap().app_tone_rules.clone();
    json!({ "ok": true, "rules": rules })
}

pub fn call(core: &Arc<Core>, name: &str, args: &[Value]) -> Option<Value> {
    let result = match name {
        "get_settings" => settings_view(&core.settings.lock().unwrap()),
        "get_options" => {
            let settings = core.settings.lock().unwrap().clone();
            options_view(&settings, core.preview)
        }
        "get_microphones" => microphone_options(core.preview),
        "set_setting" => {
            json!(matches!(args, [Value::String(key), value] if set_setting(core, key, value)))
        }
        "get_snippets" => json!(core.settings.lock().unwrap().snippets),
        "add_snippet" => add_snippet(core, args),
        "delete_snippet" => delete_snippet(core, args),
        "get_app_tones" => app_tones_view(&core.settings.lock().unwrap()),
        "add_app_tone" => add_app_tone(core, args),
        "delete_app_tone" => delete_app_tone(core, args),
        "get_meetings" => core.meeting_snapshot(),
        "start_meeting" => match core.start_meeting() {
            Ok(info) => json!({
                "ok": true,
                "loopback_active": info.loopback_active,
                "loopback_warning": info.loopback_warning,
            }),
            Err(message) => json!({ "ok": false, "message": message }),
        },
        "stop_meeting" => match core.stop_meeting() {
            Ok(()) => json!({ "ok": true }),
            Err(message) => json!({ "ok": false, "message": message }),
        },
        "cancel_meeting" => {
            core.cancel_meeting();
            json!({ "ok": true })
        }
        "delete_meeting" => {
            let id = args
                .first()
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
            json!({ "ok": id.is_some_and(|id| core.delete_meeting(id)) })
        }
        "resummarize_meeting" => {
            let id = args
                .first()
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
            match id {
                Some(id) => match core.resummarize_meeting(id) {
                    Ok(()) => json!({ "ok": true }),
                    Err(message) => json!({ "ok": false, "message": message }),
                },
                None => json!({ "ok": false, "message": "Missing or invalid meeting ID." }),
            }
        }
        "get_local_models" => {
            let settings = core.settings.lock().unwrap().clone();
            local_models_response(core.preview, &settings.ollama_url, settings.local_model).0
        }
        "get_vocab_suggestions" => {
            let settings = core.settings.lock().unwrap().clone();
            let history = if core.preview {
                Vec::new()
            } else {
                storage::load_history()
            };
            json!(storage::suggest_vocab(
                &history,
                &settings.vocab,
                &settings.vocab_dismissed,
                8
            ))
        }
        "add_vocab" | "dismiss_vocab" => {
            json!(matches!(args, [word] if change_word(core, word, name == "dismiss_vocab")))
        }
        "get_learned" => {
            json!({"lines":[],"snippet":"","kept":0,"enabled":core.settings.lock().unwrap().learn})
        }
        "diagnostics" => {
            let settings = core.settings.lock().unwrap().clone();
            let cleanup_info = if settings.cleanup {
                format!("{} via local Ollama", settings.local_model)
            } else {
                "Off (raw speech transcript)".into()
            };
            let files_info = if core.preview {
                "Sample profile (read-only)".into()
            } else {
                storage::data_dir().display().to_string()
            };
            json!({
                "speech": core.shared.lock().unwrap().engine_label(),
                "clean-up": cleanup_info,
                "prompt lab": if settings.cloud_enabled { "Paid cloud allowed when a cloud provider is selected" } else { "Paid cloud off" },
                "files": files_info,
            })
        }
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_use_ui_keys_and_keep_missing_mic_until_the_user_changes_it() {
        let (core, _) = Core::new(
            Settings {
                microphone: "Disconnected fixture".into(),
                ..Default::default()
            },
            true,
        );
        let get = || call(&core, "get_settings", &[]).unwrap();
        assert_eq!(get()["hotkey"], "ctrl_r");
        assert!(!set_setting(&core, "hotkey", &json!("pause")));
        assert!(!set_setting(&core, "hotkey", &json!("ctrl+alt+d")));
        assert_eq!(get()["hotkey"], "ctrl_r");
        assert_eq!(get()["microphone"], "Disconnected fixture");
        assert_eq!(
            call(&core, "get_microphones", &[]).unwrap()[WINDOWS_MIC],
            Value::Null
        );
        assert_eq!(
            call(
                &core,
                "set_setting",
                &[json!("microphone"), json!("unknown")]
            ),
            Some(json!(false))
        );
        assert_eq!(get()["microphone"], "Disconnected fixture");
        assert_eq!(
            call(&core, "set_setting", &[json!("microphone"), Value::Null]),
            Some(json!(true))
        );
        assert_eq!(get()["microphone"], Value::Null);
        assert_eq!(
            call(&core, "set_setting", &[json!("live_preview"), json!(false)]),
            Some(json!(true))
        );
        assert_eq!(get()["live_preview"], false);
        assert_eq!(get()["cloud_enabled"], false);
        assert_eq!(
            call(&core, "set_setting", &[json!("cloud_enabled"), json!(true)]),
            Some(json!(true))
        );
        assert_eq!(get()["cloud_enabled"], true);
        assert_eq!(get()["target"], "auto");
        assert_eq!(
            call(&core, "set_setting", &[json!("target"), json!("claude_code")]),
            Some(json!(true))
        );
        assert_eq!(get()["target"], "claude_code");
        assert_eq!(
            call(&core, "set_setting", &[json!("target"), json!("general")]),
            Some(json!(true))
        );
        assert_eq!(get()["target"], "general");
        assert_eq!(
            call(&core, "set_setting", &[json!("target"), json!("auto")]),
            Some(json!(true))
        );
        assert_eq!(get()["target"], "auto");
    }

    #[test]
    fn vocabulary_updates_are_validated_and_idempotent() {
        let (core, _) = Core::new(Settings::default(), true);
        assert_eq!(
            call(&core, "add_vocab", &[json!("Example")]),
            Some(json!(true))
        );
        assert_eq!(
            call(&core, "add_vocab", &[json!("example")]),
            Some(json!(true))
        );
        assert_eq!(
            call(&core, "add_vocab", &[json!("A, B")]),
            Some(json!(false))
        );
        assert_eq!(
            call(&core, "get_settings", &[]).unwrap()["vocab"],
            "Example"
        );
        assert_eq!(parse_hotkey("pause"), Some(Hotkey::Pause));
        assert_eq!(parse_hotkey("ctrl+alt+d"), None);
    }

    #[test]
    fn recording_and_delivery_settings_round_trip() {
        let (core, _) = Core::new(Settings::default(), true);
        let get = || call(&core, "get_settings", &[]).unwrap();
        assert_eq!(get()["destination"], "last_app");
        assert_eq!(get()["sounds"], true);
        assert_eq!(get()["floating_card"], true);
        assert_eq!(get()["restore_clipboard"], true);

        assert_eq!(
            call(&core, "set_setting", &[json!("destination"), json!("workspace")]),
            Some(json!(true))
        );
        assert_eq!(
            call(&core, "set_setting", &[json!("destination"), json!("invalid")]),
            Some(json!(false))
        );
        assert_eq!(
            call(&core, "set_setting", &[json!("sounds"), json!(false)]),
            Some(json!(true))
        );
        assert_eq!(
            call(&core, "set_setting", &[json!("floating_card"), json!(false)]),
            Some(json!(true))
        );
        assert_eq!(
            call(&core, "set_setting", &[json!("restore_clipboard"), json!(false)]),
            Some(json!(true))
        );

        let updated = get();
        assert_eq!(updated["destination"], "workspace");
        assert_eq!(updated["sounds"], false);
        assert_eq!(updated["floating_card"], false);
        assert_eq!(updated["restore_clipboard"], false);

        assert_eq!(
            call(&core, "set_setting", &[json!("cleanup"), json!(false)]),
            Some(json!(true))
        );
        let diag = call(&core, "diagnostics", &[]).unwrap();
        assert_eq!(diag["clean-up"], "Off (raw speech transcript)");
    }

    #[test]
    fn clear_log_files_removes_known_logs_and_ignores_missing() {
        let dir = std::env::temp_dir().join(format!("listen-logs-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let err_log = dir.join("last-error.txt");
        let keep = dir.join("settings.json");
        std::fs::write(&err_log, b"err").unwrap();
        std::fs::write(&keep, b"{}").unwrap();
        assert!(clear_log_files(&dir).is_ok());
        assert!(!err_log.exists());
        assert!(keep.exists());
        // Calling a second time when both files are missing still succeeds.
        assert!(clear_log_files(&dir).is_ok());
        let _ = std::fs::remove_file(&keep);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn new_feature_ipc_contract_and_preview_safety() {
        let (core, _) = Core::new(Settings::default(), true);
        let get = || call(&core, "get_settings", &[]).unwrap();
        assert_eq!(get()["quiet_mode"], false);
        assert_eq!(get()["auto_app_tone"], true);
        assert_eq!(get()["command_hotkey"], "");
        assert_eq!(get()["snippets"], json!([]));
        assert_eq!(get()["app_tone_rules"], json!([]));

        // Toggle quiet_mode and auto_app_tone.
        assert_eq!(call(&core, "set_setting", &[json!("quiet_mode"), json!(true)]), Some(json!(true)));
        assert_eq!(call(&core, "set_setting", &[json!("auto_app_tone"), json!(false)]), Some(json!(true)));
        assert_eq!(get()["quiet_mode"], true);
        assert_eq!(get()["auto_app_tone"], false);

        // command_hotkey rejects primary hotkey conflict and invalid ids, accepts distinct hotkeys and "".
        assert_eq!(call(&core, "set_setting", &[json!("command_hotkey"), json!("ctrl_r")]), Some(json!(false)));
        assert_eq!(call(&core, "set_setting", &[json!("command_hotkey"), json!("invalid")]), Some(json!(false)));
        assert_eq!(call(&core, "set_setting", &[json!("command_hotkey"), json!("shift_r")]), Some(json!(true)));
        assert_eq!(get()["command_hotkey"], "shift_r");
        assert_eq!(call(&core, "set_setting", &[json!("command_hotkey"), json!("")]), Some(json!(true)));
        assert_eq!(get()["command_hotkey"], "");

        // Snippets CRUD.
        assert_eq!(call(&core, "add_snippet", &[json!(""), json!("x")]).unwrap()["ok"], false);
        let added = call(&core, "add_snippet", &[json!("my email"), json!("cam@example.com")]).unwrap();
        assert_eq!(added["ok"], true);
        assert_eq!(added["snippets"].as_array().unwrap().len(), 1);
        // Case-insensitive upsert updates existing entry rather than duplicating.
        let updated = call(&core, "add_snippet", &[json!("MY EMAIL"), json!("new@example.com")]).unwrap();
        assert_eq!(updated["ok"], true);
        assert_eq!(updated["snippets"].as_array().unwrap().len(), 1);
        assert_eq!(updated["snippets"][0]["expansion"], "new@example.com");
        assert_eq!(call(&core, "get_snippets", &[]).unwrap().as_array().unwrap().len(), 1);
        let deleted = call(&core, "delete_snippet", &[json!("my email")]).unwrap();
        assert_eq!(deleted["ok"], true);
        assert_eq!(deleted["snippets"], json!([]));

        // App tones CRUD & preset order.
        let tones = call(&core, "get_app_tones", &[]).unwrap();
        assert_eq!(tones["auto_enabled"], false);
        assert_eq!(
            tones["presets"],
            json!([
                ["casual", "Casual"],
                ["polished", "Polished"],
                ["technical", "Technical"],
                ["default", "Default"],
            ])
        );
        assert_eq!(call(&core, "add_app_tone", &[json!("Discord"), json!("bogus")]).unwrap()["ok"], false);
        let rule_added = call(&core, "add_app_tone", &[json!("Discord"), json!("polished")]).unwrap();
        assert_eq!(rule_added["ok"], true);
        assert_eq!(rule_added["rules"][0]["tone"], "polished");
        let rule_deleted = call(&core, "delete_app_tone", &[json!("discord")]).unwrap();
        assert_eq!(rule_deleted["ok"], true);
        assert_eq!(rule_deleted["rules"], json!([]));

        // Meetings IPC in preview mode: returns sample note, rejects hardware capture cleanly, deletes sample note.
        let meetings = call(&core, "get_meetings", &[]).unwrap();
        assert_eq!(meetings["active"], false);
        assert_eq!(meetings["transcribing"], false);
        assert_eq!(meetings["notes"].as_array().unwrap().len(), 1);
        assert_eq!(call(&core, "start_meeting", &[]).unwrap()["ok"], false);
        assert_eq!(call(&core, "stop_meeting", &[]).unwrap()["ok"], false);
        assert_eq!(call(&core, "cancel_meeting", &[]).unwrap()["ok"], true);
        let sample_id = meetings["notes"][0]["id"].as_u64().unwrap();
        assert_eq!(call(&core, "delete_meeting", &[json!(sample_id)]).unwrap()["ok"], true);
        assert_eq!(call(&core, "get_meetings", &[]).unwrap()["notes"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn vocab_cap_nul_newline_preservation_and_dismissed_pruning() {
        // 1. Existing newline-formatted vocab preserves '\n' delimiter, rejects ';',
        //    deduplicates Unicode case-insensitively, and prunes accepted words from vocab_dismissed.
        let (core, _) = Core::new(
            Settings {
                vocab: "Parakeet\nCafé".into(),
                vocab_dismissed: vec!["Kubernetes".into(), "Other".into()],
                ..Default::default()
            },
            true,
        );
        // Rejects semicolon and NUL inside an accepted word.
        assert_eq!(call(&core, "add_vocab", &[json!("Foo; Bar")]), Some(json!(false)));
        assert_eq!(call(&core, "add_vocab", &[json!("Foo\0Bar")]), Some(json!(false)));
        // Unicode case-insensitive dedup does not duplicate "Café".
        assert_eq!(call(&core, "add_vocab", &[json!("café")]), Some(json!(true)));
        assert_eq!(
            call(&core, "get_settings", &[]).unwrap()["vocab"],
            "Parakeet\nCafé"
        );
        // Accepting a previously dismissed word appends with '\n' and prunes vocab_dismissed.
        assert_eq!(call(&core, "add_vocab", &[json!("kubernetes")]), Some(json!(true)));
        assert_eq!(
            call(&core, "get_settings", &[]).unwrap()["vocab"],
            "Parakeet\nCafé\nkubernetes"
        );
        assert_eq!(
            core.settings.lock().unwrap().vocab_dismissed,
            vec!["Other".to_string()]
        );

        // 2. Comma-formatted vocab appends with ", " and enforces MAX_VOCAB & NUL rejection.
        let (core2, _) = Core::new(
            Settings {
                vocab: "Alpha, Beta".into(),
                ..Default::default()
            },
            true,
        );
        assert_eq!(call(&core2, "add_vocab", &[json!("Gamma")]), Some(json!(true)));
        assert_eq!(
            call(&core2, "get_settings", &[]).unwrap()["vocab"],
            "Alpha, Beta, Gamma"
        );
        assert_eq!(
            call(&core2, "set_setting", &[json!("vocab"), json!("bad\0vocab")]),
            Some(json!(false))
        );
        let too_big = "a".repeat(MAX_VOCAB + 1);
        assert_eq!(
            call(&core2, "set_setting", &[json!("vocab"), json!(too_big)]),
            Some(json!(false))
        );
        let near_cap = "a".repeat(MAX_VOCAB - 2);
        assert_eq!(
            call(&core2, "set_setting", &[json!("vocab"), json!(near_cap)]),
            Some(json!(true))
        );
        assert_eq!(call(&core2, "add_vocab", &[json!("overflow")]), Some(json!(false)));

        // 3. Direct helper parity (the exact functions called by app.rs):
        assert!(sanitize_vocab("ok, vocab").is_ok());
        assert!(sanitize_vocab("bad\0vocab").is_err());
        assert!(sanitize_vocab(&"x".repeat(MAX_VOCAB + 1)).is_err());

        let mut s = Settings {
            vocab: "Rust\nParakeet\n".into(),
            vocab_dismissed: vec!["Ollama".into(), "KeepMe".into()],
            ..Default::default()
        };
        assert!(add_vocab_word(&mut s, "bad;word").is_err());
        assert_eq!(add_vocab_word(&mut s, "ollama").unwrap(), true);
        assert_eq!(s.vocab, "Rust\nParakeet\nollama");
        assert_eq!(s.vocab_dismissed, vec!["KeepMe".to_string()]);
        // Re-adding an existing word is idempotent and returns Ok(false).
        assert_eq!(add_vocab_word(&mut s, "PARAKEET").unwrap(), false);
        // Semicolon-separated existing vocab deduplicates via storage::vocab_words.
        let mut semi = Settings {
            vocab: "Alpha; Beta".into(),
            vocab_dismissed: vec!["Beta".into()],
            ..Default::default()
        };
        assert_eq!(add_vocab_word(&mut semi, "beta").unwrap(), false);
        assert!(semi.vocab_dismissed.is_empty());
        assert_eq!(dismiss_vocab_word(&mut semi, "Café").unwrap(), true);
        assert_eq!(dismiss_vocab_word(&mut semi, "café").unwrap(), false);
    }

    #[test]
    fn snippet_unicode_whitespace_dedup_and_max_cap() {
        // Seed a legacy unnormalized whitespace trigger plus a Unicode trigger.
        let mut initial = vec![
            crate::snippets::Snippet {
                trigger: "my  email".into(),
                expansion: "old@example.com".into(),
            },
            crate::snippets::Snippet {
                trigger: "café".into(),
                expansion: "Old Café".into(),
            },
        ];
        for i in 2..MAX_SNIPPETS {
            initial.push(crate::snippets::Snippet {
                trigger: format!("trigger {i}"),
                expansion: format!("expansion {i}"),
            });
        }
        assert_eq!(initial.len(), MAX_SNIPPETS);

        let (core, _) = Core::new(
            Settings {
                snippets: initial,
                ..Default::default()
            },
            true,
        );

        // At MAX_SNIPPETS (200), updating an existing Unicode trigger (different case) succeeds without growing.
        let upd_unicode = call(&core, "add_snippet", &[json!("CAFÉ"), json!("New Café")]).unwrap();
        assert_eq!(upd_unicode["ok"], true);
        assert_eq!(upd_unicode["snippets"].as_array().unwrap().len(), MAX_SNIPPETS);
        assert_eq!(upd_unicode["snippets"][1]["expansion"], "New Café");

        // At MAX_SNIPPETS (200), updating an existing legacy multi-space trigger succeeds without growing.
        let upd_ws = call(&core, "add_snippet", &[json!("MY   \t EMAIL"), json!("new@example.com")]).unwrap();
        assert_eq!(upd_ws["ok"], true);
        assert_eq!(upd_ws["snippets"].as_array().unwrap().len(), MAX_SNIPPETS);
        assert_eq!(upd_ws["snippets"][0]["trigger"], "MY EMAIL");
        assert_eq!(upd_ws["snippets"][0]["expansion"], "new@example.com");

        // Adding a 201st distinct trigger is blocked with a clear error.
        let overflow = call(&core, "add_snippet", &[json!("brand new trigger"), json!("val")]).unwrap();
        assert_eq!(overflow["ok"], false);
        assert!(
            overflow["message"].as_str().unwrap_or("").contains("200"),
            "expected cap error mentioning 200, got: {overflow}"
        );
        assert_eq!(core.settings.lock().unwrap().snippets.len(), MAX_SNIPPETS);

        // Deleting by Unicode/whitespace variant removes the matching snippet.
        let del = call(&core, "delete_snippet", &[json!("  CAFÉ ")]).unwrap();
        assert_eq!(del["ok"], true);
        assert_eq!(del["snippets"].as_array().unwrap().len(), MAX_SNIPPETS - 1);

        // Direct upsert_snippet helper parity (the exact function called by app.rs).
        let mut direct: Vec<crate::snippets::Snippet> = (0..MAX_SNIPPETS)
            .map(|i| crate::snippets::Snippet {
                trigger: if i == 0 { "café  note".into() } else { format!("snip {i}") },
                expansion: format!("exp {i}"),
            })
            .collect();
        assert!(upsert_snippet(&mut direct, "CAFÉ\tnote", "updated").is_ok());
        assert_eq!(direct.len(), MAX_SNIPPETS);
        assert_eq!(direct[0].trigger, "CAFÉ note");
        assert_eq!(direct[0].expansion, "updated");
        assert!(upsert_snippet(&mut direct, "distinct 201st", "nope").is_err());
        assert_eq!(direct.len(), MAX_SNIPPETS);
    }

    #[test]
    fn speech_model_change_blocked_while_benchmark_or_meeting_active_and_allowed_once_cleared() {
        let (core, _) = Core::new(Settings::default(), true);

        // Baseline: switching speech_model to PARAKEET succeeds in preview mode when idle.
        assert_eq!(
            call(&core, "set_setting", &[json!("speech_model"), json!(PARAKEET)]),
            Some(json!(true))
        );

        // Blocked while benchmark_active is true.
        core.benchmark_active.store(true, Ordering::Relaxed);
        assert_eq!(
            call(&core, "set_setting", &[json!("speech_model"), json!(PARAKEET)]),
            Some(json!(false))
        );
        // resummarize_meeting RPC is also blocked by benchmark_active.
        let res = call(&core, "resummarize_meeting", &[json!(1)]).unwrap();
        assert_eq!(res["ok"], false);
        assert!(res["message"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains("benchmark"));

        // Allowed again once benchmark_active is cleared.
        core.benchmark_active.store(false, Ordering::Relaxed);
        assert_eq!(
            call(&core, "set_setting", &[json!("speech_model"), json!(PARAKEET)]),
            Some(json!(true))
        );

        // Blocked while a meeting is active (recording) or transcribing/summarizing.
        core.meeting.lock().unwrap().active = true;
        assert_eq!(
            call(&core, "set_setting", &[json!("speech_model"), json!(PARAKEET)]),
            Some(json!(false))
        );
        {
            let mut m = core.meeting.lock().unwrap();
            m.active = false;
            m.transcribing = true;
        }
        assert_eq!(
            call(&core, "set_setting", &[json!("speech_model"), json!(PARAKEET)]),
            Some(json!(false))
        );
        core.meeting.lock().unwrap().transcribing = false;
        assert_eq!(
            call(&core, "set_setting", &[json!("speech_model"), json!(PARAKEET)]),
            Some(json!(true))
        );

        // resummarize_meeting RPC validates missing IDs and rejects sample mode cleanly.
        assert_eq!(call(&core, "resummarize_meeting", &[]).unwrap()["ok"], false);
        assert_eq!(
            call(&core, "resummarize_meeting", &[json!(1)]).unwrap()["ok"],
            false
        );
    }

    #[test]
    fn app_tone_rule_dedup_validation_and_max_cap() {
        let mut initial = vec![crate::app_tone::AppToneRule {
            app: "Discord".into(),
            tone: crate::app_tone::TonePreset::Casual,
        }];
        for i in 1..MAX_APP_TONE_RULES {
            initial.push(crate::app_tone::AppToneRule {
                app: format!("App{i}"),
                tone: crate::app_tone::TonePreset::Default,
            });
        }
        assert_eq!(initial.len(), MAX_APP_TONE_RULES);

        let (core, _) = Core::new(
            Settings {
                app_tone_rules: initial,
                ..Default::default()
            },
            true,
        );

        // ".exe" alone is rejected by validate_rule.
        assert_eq!(
            call(&core, "add_app_tone", &[json!(".exe"), json!("casual")]).unwrap()["ok"],
            false
        );
        assert_eq!(
            call(&core, "add_app_tone", &[json!("  .EXE "), json!("polished")]).unwrap()["ok"],
            false
        );

        // At MAX_APP_TONE_RULES (100), updating "Discord" via "Discord.exe" deduplicates in place.
        let upd = call(&core, "add_app_tone", &[json!("Discord.exe"), json!("technical")]).unwrap();
        assert_eq!(upd["ok"], true, "updating existing rule at cap via .exe variant must succeed");
        assert_eq!(upd["rules"].as_array().unwrap().len(), MAX_APP_TONE_RULES);
        assert_eq!(upd["rules"][0]["app"], "Discord.exe");
        assert_eq!(upd["rules"][0]["tone"], "technical");

        // Adding a 101st distinct rule is rejected with cap message.
        let overflow = call(&core, "add_app_tone", &[json!("Notion"), json!("polished")]).unwrap();
        assert_eq!(overflow["ok"], false);
        assert!(
            overflow["message"].as_str().unwrap_or("").contains("100"),
            "expected cap error mentioning 100, got: {overflow}"
        );
        assert_eq!(core.settings.lock().unwrap().app_tone_rules.len(), MAX_APP_TONE_RULES);

        // Deleting by non-.exe variant removes the "Discord.exe" rule via same_app_rule.
        let del = call(&core, "delete_app_tone", &[json!("discord")]).unwrap();
        assert_eq!(del["ok"], true);
        assert_eq!(del["rules"].as_array().unwrap().len(), MAX_APP_TONE_RULES - 1);

        // Direct upsert_app_tone_rule helper parity (the exact function called by app.rs).
        let mut direct: Vec<crate::app_tone::AppToneRule> = (0..MAX_APP_TONE_RULES)
            .map(|i| crate::app_tone::AppToneRule {
                app: if i == 0 { "Discord".into() } else { format!("Rule{i}") },
                tone: crate::app_tone::TonePreset::Default,
            })
            .collect();
        assert!(upsert_app_tone_rule(&mut direct, ".exe", crate::app_tone::TonePreset::Casual).is_err());
        assert!(upsert_app_tone_rule(&mut direct, "Discord.exe", crate::app_tone::TonePreset::Polished).is_ok());
        assert_eq!(direct.len(), MAX_APP_TONE_RULES);
        assert_eq!(direct[0].app, "Discord.exe");
        assert_eq!(direct[0].tone, crate::app_tone::TonePreset::Polished);
        assert!(upsert_app_tone_rule(&mut direct, "NewApp101", crate::app_tone::TonePreset::Technical).is_err());
        assert_eq!(direct.len(), MAX_APP_TONE_RULES);
    }
}
