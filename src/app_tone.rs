//! App-Aware Tone & Style: per-application formatting presets, user rules,
//! smart built-in app matching, and `formatting::is_safe`-compatible prompt suffixes.

use serde::{Deserialize, Serialize};

/// Maximum allowed length (in Unicode scalar values) for a rule's app matcher,
/// aligned with `winput::app_label`.
pub const MAX_APP_NAME_CHARS: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TonePreset {
    #[default]
    Default,
    Casual,
    Polished,
    Technical,
}

impl TonePreset {
    pub const ALL: [TonePreset; 4] = [
        TonePreset::Default,
        TonePreset::Casual,
        TonePreset::Polished,
        TonePreset::Technical,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TonePreset::Default => "default",
            TonePreset::Casual => "casual",
            TonePreset::Polished => "polished",
            TonePreset::Technical => "technical",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TonePreset::Default => "Default",
            TonePreset::Casual => "Casual",
            TonePreset::Polished => "Polished",
            TonePreset::Technical => "Technical",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "default" => Some(TonePreset::Default),
            "casual" => Some(TonePreset::Casual),
            "polished" => Some(TonePreset::Polished),
            "technical" => Some(TonePreset::Technical),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppToneRule {
    pub app: String,
    pub tone: TonePreset,
}

/// Trims and validates a user-configured per-app tone rule.
pub fn validate_rule(app: &str, tone: TonePreset) -> Result<AppToneRule, String> {
    let trimmed = app.trim();
    if trimmed.is_empty() {
        return Err("App name cannot be empty.".into());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("App name cannot contain control characters.".into());
    }
    if trimmed.chars().count() > MAX_APP_NAME_CHARS {
        return Err(format!(
            "App name must be at most {MAX_APP_NAME_CHARS} characters."
        ));
    }
    Ok(AppToneRule {
        app: trimmed.to_owned(),
        tone,
    })
}

fn normalize_app_name(name: &str) -> String {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .unwrap_or(&lower)
        .trim()
        .to_owned()
}

fn is_alnum_ascii(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// Checks whether `needle` appears in `haystack` bounded on both sides by start/end
/// or non-alphanumeric ASCII characters.
fn contains_bounded_phrase(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut offset = 0;
    while let Some(pos) = haystack[offset..].find(needle) {
        let start = offset + pos;
        let end = start + needle.len();
        let left_ok = start == 0 || !is_alnum_ascii(bytes[start - 1]);
        let right_ok = end == bytes.len() || !is_alnum_ascii(bytes[end]);
        if left_ok && right_ok {
            return true;
        }
        offset = start + 1;
    }
    false
}

/// Splits a normalized lowercase app name into ASCII alphanumeric tokens.
fn ascii_tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
}

/// True when a token equals `base` or equals `base` followed purely by ASCII digits
/// (e.g., `idea64`, `pycharm64`, `rider64`, `iterm2`).
fn token_matches_base_or_digits(token: &str, base: &str) -> bool {
    token
        .strip_prefix(base)
        .is_some_and(|rest| rest.is_empty() || rest.bytes().all(|b| b.is_ascii_digit()))
}

fn matches_chat_app(norm: &str) -> bool {
    const BRAND_SUBSTR: &[&str] = &["discord", "slack", "telegram", "whatsapp"];
    if BRAND_SUBSTR.iter().any(|k| norm.contains(k)) {
        return true;
    }
    ascii_tokens(norm).any(|tok| {
        matches!(tok, "signal" | "signaldesktop" | "messages" | "imessages" | "teams")
            || tok.ends_with("teams")
    })
}

fn matches_email_or_docs_app(norm: &str) -> bool {
    if contains_bounded_phrase(norm, "google docs") || norm.contains("googledocs") {
        return true;
    }
    const BRAND_SUBSTR: &[&str] = &["outlook", "thunderbird", "gmail", "notion", "obsidian"];
    if BRAND_SUBSTR.iter().any(|k| norm.contains(k)) {
        return true;
    }
    ascii_tokens(norm).any(|tok| {
        matches!(tok, "word" | "winword" | "wordpad" | "olk" | "mail")
            || tok.ends_with("mail")
            || tok.starts_with("mail")
    })
}

fn matches_code_or_terminal_app(norm: &str) -> bool {
    const BRAND_SUBSTR: &[&str] = &[
        "cursor",
        "antigravity",
        "windsurf",
        "terminal",
        "powershell",
        "pycharm",
    ];
    if BRAND_SUBSTR.iter().any(|k| norm.contains(k)) {
        return true;
    }
    if norm == "wt" {
        return true;
    }
    ascii_tokens(norm).any(|tok| {
        tok == "vscode"
            || tok == "cmd"
            || tok == "pwsh"
            || tok == "zed"
            || token_matches_base_or_digits(tok, "code")
            || token_matches_base_or_digits(tok, "iterm")
            || token_matches_base_or_digits(tok, "idea")
            || token_matches_base_or_digits(tok, "rider")
    })
}

/// Resolves the active [`TonePreset`] for `app_name`.
///
/// Resolution order:
/// 1. Exact case-insensitive user rule match (ignoring optional `.exe` suffix).
/// 2. Case-insensitive substring user rule match (longest matching rule wins;
///    ties keep the earlier rule). Empty rules are ignored.
/// 3. When `auto_enabled` is true and no user rule matches, smart built-in defaults
///    classify chat apps as [`TonePreset::Casual`], email/docs apps as
///    [`TonePreset::Polished`], and code/terminal apps as [`TonePreset::Technical`].
/// 4. Everything else resolves to [`TonePreset::Default`].
pub fn resolve_tone(app_name: &str, auto_enabled: bool, rules: &[AppToneRule]) -> TonePreset {
    let norm = normalize_app_name(app_name);
    if norm.is_empty() {
        return TonePreset::Default;
    }

    // Pass 1: exact case-insensitive user rule match.
    for rule in rules {
        let rule_norm = normalize_app_name(&rule.app);
        if !rule_norm.is_empty() && norm == rule_norm {
            return rule.tone;
        }
    }

    // Pass 2: case-insensitive substring user rule match (most specific / longest wins).
    let mut best_match: Option<(usize, TonePreset)> = None;
    for rule in rules {
        let rule_norm = normalize_app_name(&rule.app);
        if !rule_norm.is_empty() && norm.contains(&rule_norm) {
            let len = rule_norm.len();
            if best_match.is_none_or(|(best_len, _)| len > best_len) {
                best_match = Some((len, rule.tone));
            }
        }
    }
    if let Some((_, tone)) = best_match {
        return tone;
    }

    if !auto_enabled {
        return TonePreset::Default;
    }

    if matches_chat_app(&norm) {
        TonePreset::Casual
    } else if matches_email_or_docs_app(&norm) {
        TonePreset::Polished
    } else if matches_code_or_terminal_app(&norm) {
        TonePreset::Technical
    } else {
        TonePreset::Default
    }
}

/// Returns a concise, additive formatting instruction for `tone` that stays
/// strictly compatible with `formatting::is_safe` (adjusting only punctuation,
/// capitalization, paragraphing, and technical literal preservation without
/// adding, dropping, or substituting content words).
///
/// Returns `""` for [`TonePreset::Default`], or a trimmed sentence (no leading
/// space) so callers can append it with a single separating space.
pub fn tone_prompt_suffix(tone: TonePreset) -> &'static str {
    match tone {
        TonePreset::Default => "",
        TonePreset::Casual => "Tone (Casual chat): use light, natural punctuation and relaxed casing suited to chat; omit stiff trailing periods on short single-line remarks and prefer concise line breaks over formal paragraphs. Keep every spoken word, contraction, name, and number verbatim in order — never add, drop, or substitute words.",
        TonePreset::Polished => "Tone (Polished writing): use crisp sentence capitalization, complete terminal punctuation, clean commas, and clear paragraph breaks for longer thoughts. Keep every spoken word, name, and number verbatim in order — never add greetings, sign-offs, or extra words, and never drop or substitute content words.",
        TonePreset::Technical => "Tone (Technical): preserve exact casing and literal spelling for identifiers, file paths, CLI flags, commands, numbers, and symbols; wrap unambiguous literal commands or identifiers in backticks only when verbatim, and use minimal, crisp punctuation and structured lines. Never add, drop, or alter any spoken words or operators.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_preset_metadata_and_serde_roundtrip() {
        assert_eq!(TonePreset::default(), TonePreset::Default);
        assert_eq!(
            TonePreset::ALL,
            [
                TonePreset::Default,
                TonePreset::Casual,
                TonePreset::Polished,
                TonePreset::Technical,
            ]
        );
        for preset in TonePreset::ALL {
            assert_eq!(TonePreset::from_id(preset.id()), Some(preset));
            assert_eq!(
                TonePreset::from_id(&format!("  {}  ", preset.id().to_uppercase())),
                Some(preset)
            );
            assert!(!preset.label().is_empty());

            let json = serde_json::to_string(&preset).expect("serialize preset");
            assert_eq!(json, format!("\"{}\"", preset.id()));
            let parsed: TonePreset = serde_json::from_str(&json).expect("deserialize preset");
            assert_eq!(parsed, preset);
        }
        assert_eq!(TonePreset::from_id("unknown"), None);
        assert_eq!(TonePreset::from_id(""), None);
    }

    #[test]
    fn validate_rule_trims_and_rejects_invalid_app_names() {
        let valid = validate_rule("  Discord  ", TonePreset::Casual).expect("valid rule");
        assert_eq!(
            valid,
            AppToneRule {
                app: "Discord".into(),
                tone: TonePreset::Casual,
            }
        );

        assert!(validate_rule("", TonePreset::Polished).is_err());
        assert!(validate_rule("   \t  ", TonePreset::Polished).is_err());
        assert!(validate_rule("Bad\0App", TonePreset::Technical).is_err());
        assert!(validate_rule("Line1\nLine2", TonePreset::Technical).is_err());

        let exact_max = "a".repeat(MAX_APP_NAME_CHARS);
        assert!(validate_rule(&exact_max, TonePreset::Default).is_ok());
        let too_long = "a".repeat(MAX_APP_NAME_CHARS + 1);
        assert!(validate_rule(&too_long, TonePreset::Default).is_err());
    }

    #[test]
    fn resolve_tone_builtin_categories_when_auto_enabled() {
        for app in [
            "Discord",
            "Discord.exe",
            "DiscordPTB",
            "Slack",
            "Telegram",
            "WhatsApp",
            "Signal",
            "Messages",
            "Teams",
            "ms-teams.exe",
            "Microsoft Teams",
        ] {
            assert_eq!(
                resolve_tone(app, true, &[]),
                TonePreset::Casual,
                "expected Casual for {app:?}"
            );
        }

        for app in [
            "Outlook",
            "OUTLOOK.EXE",
            "Thunderbird",
            "Mail",
            "ProtonMail",
            "Gmail",
            "Word",
            "WINWORD.EXE",
            "Microsoft Word",
            "Notion",
            "Obsidian",
            "Google Docs",
            "Google Docs - Chrome",
        ] {
            assert_eq!(
                resolve_tone(app, true, &[]),
                TonePreset::Polished,
                "expected Polished for {app:?}"
            );
        }

        for app in [
            "Code",
            "Code.exe",
            "VS Code",
            "vscode",
            "Cursor",
            "Antigravity",
            "Windsurf",
            "Zed",
            "Terminal",
            "WindowsTerminal.exe",
            "wt.exe",
            "PowerShell",
            "pwsh.exe",
            "cmd.exe",
            "iTerm",
            "iTerm2",
            "idea",
            "idea64.exe",
            "IntelliJ IDEA",
            "PyCharm",
            "pycharm64",
            "Rider",
            "rider64.exe",
        ] {
            assert_eq!(
                resolve_tone(app, true, &[]),
                TonePreset::Technical,
                "expected Technical for {app:?}"
            );
        }
    }

    #[test]
    fn resolve_tone_avoids_false_positives_and_respects_auto_disabled() {
        for app in [
            "",
            "   ",
            "Notepad",
            "Google Chrome",
            "Firefox",
            "Spotify",
            "1Password",
            "Synchronized",
            "Barcode Scanner",
            "File Explorer",
        ] {
            assert_eq!(
                resolve_tone(app, true, &[]),
                TonePreset::Default,
                "expected Default for {app:?}"
            );
        }

        // Auto disabled returns Default when no user rule matches.
        assert_eq!(resolve_tone("Discord", false, &[]), TonePreset::Default);
        assert_eq!(resolve_tone("Outlook", false, &[]), TonePreset::Default);
        assert_eq!(resolve_tone("VS Code", false, &[]), TonePreset::Default);
    }

    #[test]
    fn resolve_tone_prioritizes_user_rules_exact_then_longest_substring() {
        let rules = vec![
            AppToneRule { app: "Discord".into(), tone: TonePreset::Polished },
            AppToneRule { app: "Code".into(), tone: TonePreset::Casual },
            AppToneRule { app: "VS Code".into(), tone: TonePreset::Default },
            AppToneRule { app: "Outlook".into(), tone: TonePreset::Technical },
            AppToneRule { app: "".into(), tone: TonePreset::Technical }, // empty ignored
        ];

        // Overrides built-in defaults even when auto_enabled is true or false.
        assert_eq!(resolve_tone("discord.exe", true, &rules), TonePreset::Polished);
        assert_eq!(resolve_tone("Discord", false, &rules), TonePreset::Polished);
        assert_eq!(resolve_tone("OUTLOOK.EXE", true, &rules), TonePreset::Technical);

        // Exact rule beats shorter substring rule.
        assert_eq!(resolve_tone("VS Code", true, &rules), TonePreset::Default);
        // Substring rule matches when exact doesn't.
        assert_eq!(resolve_tone("Code - Insiders", true, &rules), TonePreset::Casual);
        // Empty rule never matches unrelated apps.
        assert_eq!(resolve_tone("Notepad", true, &rules), TonePreset::Default);

        // Longest substring wins when multiple substring rules match.
        let sub_rules = vec![
            AppToneRule { app: "Mail".into(), tone: TonePreset::Casual },
            AppToneRule { app: "Proton Mail".into(), tone: TonePreset::Polished },
        ];
        assert_eq!(
            resolve_tone("Proton Mail - Inbox", true, &sub_rules),
            TonePreset::Polished
        );
    }

    #[test]
    fn tone_prompt_suffix_is_safe_concise_and_empty_for_default() {
        assert_eq!(tone_prompt_suffix(TonePreset::Default), "");

        for preset in [TonePreset::Casual, TonePreset::Polished, TonePreset::Technical] {
            let suffix = tone_prompt_suffix(preset);
            assert!(!suffix.is_empty());
            assert_eq!(suffix, suffix.trim(), "suffix should have no outer whitespace");
            assert!(
                suffix.to_ascii_lowercase().contains("never add")
                    && suffix.to_ascii_lowercase().contains("drop"),
                "suffix for {preset:?} must guard against adding or dropping words for formatting::is_safe"
            );
        }
        assert_ne!(
            tone_prompt_suffix(TonePreset::Casual),
            tone_prompt_suffix(TonePreset::Polished)
        );
        assert_ne!(
            tone_prompt_suffix(TonePreset::Polished),
            tone_prompt_suffix(TonePreset::Technical)
        );
    }
}
