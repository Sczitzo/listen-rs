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
    if normalize_app_name(trimmed).is_empty() {
        return Err("App name cannot be empty.".into());
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

/// Returns `true` when `a` and `b` normalize to the same non-empty app rule key
/// (trimmed, case-insensitive, ignoring an optional `.exe` suffix).
pub fn same_app_rule(a: &str, b: &str) -> bool {
    let norm_a = normalize_app_name(a);
    !norm_a.is_empty() && norm_a == normalize_app_name(b)
}

/// Returns `true` when `app_name` matches a known web browser executable or
/// display name. Window-title web-app inspection is strictly gated on this
/// allowlist so native apps never suffer title-based false positives.
pub fn is_browser_app(app_name: &str) -> bool {
    let norm = normalize_app_name(app_name);
    if norm.is_empty() {
        return false;
    }
    let cleaned: String = norm
        .replace('\u{200b}', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    matches!(
        cleaned.as_str(),
        "chrome"
            | "google chrome"
            | "google chrome beta"
            | "google chrome dev"
            | "google chrome canary"
            | "chrome beta"
            | "chrome canary"
            | "msedge"
            | "microsoft edge"
            | "microsoft edge beta"
            | "microsoft edge dev"
            | "microsoft edge canary"
            | "firefox"
            | "mozilla firefox"
            | "firefox nightly"
            | "firefox developer edition"
            | "brave"
            | "brave browser"
            | "vivaldi"
            | "opera"
            | "opera gx"
            | "opera_gx"
            | "opera browser"
            | "arc"
            | "arc browser"
            | "zen"
            | "zen browser"
            | "chromium"
            | "waterfox"
            | "floorp"
            | "librewolf"
    )
}

fn is_alnum_ascii(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
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

/// Like [`contains_bounded_phrase`], but also treats `_` as a word character so
/// identifiers/filenames like `gmail_client.rs` or `slack_notes.docx` do not match.
fn contains_word_phrase(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut offset = 0;
    while let Some(pos) = haystack[offset..].find(needle) {
        let start = offset + pos;
        let end = start + needle.len();
        let left_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let right_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        if left_ok && right_ok {
            return true;
        }
        offset = start + 1;
    }
    false
}

/// Strips leading browser notification indicators such as `(3) `, `(99+) `,
/// `[2] `, `* `, or `• ` from a lowercase title segment.
fn strip_notification_prefix(mut s: &str) -> &str {
    loop {
        let trimmed = s.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix('*')
            .or_else(|| trimmed.strip_prefix('\u{2022}'))
            .or_else(|| trimmed.strip_prefix('\u{00b7}'))
        {
            s = rest;
            continue;
        }
        if let Some(after_open) = trimmed.strip_prefix('(') {
            if let Some(close) = after_open.find(')') {
                let inner = &after_open[..close];
                if !inner.is_empty()
                    && inner.bytes().any(|b| b.is_ascii_digit())
                    && inner.bytes().all(|b| b.is_ascii_digit() || b == b'+')
                {
                    s = &after_open[close + 1..];
                    continue;
                }
            }
        }
        if let Some(after_open) = trimmed.strip_prefix('[') {
            if let Some(close) = after_open.find(']') {
                let inner = &after_open[..close];
                if !inner.is_empty()
                    && inner.bytes().any(|b| b.is_ascii_digit())
                    && inner.bytes().all(|b| b.is_ascii_digit() || b == b'+')
                {
                    s = &after_open[close + 1..];
                    continue;
                }
            }
        }
        return trimmed;
    }
}

/// Splits a browser window title into trimmed segments across standard title
/// separators (`" - "`, `" – "`, `" — "`, `" | "`, `" · "`, `" • "`).
fn split_title_segments(title: &str) -> Vec<&str> {
    const SEPS: &[&str] = &[
        " - ",
        " \u{2013} ",
        " \u{2014} ",
        " | ",
        " \u{00b7} ",
        " \u{2022} ",
    ];
    let mut segments = Vec::new();
    let mut rest = title;
    while !rest.is_empty() {
        let next = SEPS
            .iter()
            .filter_map(|sep| rest.find(sep).map(|idx| (idx, sep.len())))
            .min_by_key(|&(idx, _)| idx);
        match next {
            Some((idx, sep_len)) => {
                let seg = rest[..idx].trim();
                if !seg.is_empty() {
                    segments.push(seg);
                }
                rest = &rest[idx + sep_len..];
            }
            None => {
                let seg = rest.trim();
                if !seg.is_empty() {
                    segments.push(seg);
                }
                break;
            }
        }
    }
    segments
}

fn classify_browser_segment(raw_seg: &str) -> Option<TonePreset> {
    let lower = raw_seg.replace('\u{200b}', "").trim().to_ascii_lowercase();
    if lower.is_empty() || is_browser_app(&lower) {
        return None;
    }
    let clean = strip_notification_prefix(&lower);

    // Technical web apps
    if contains_word_phrase(&lower, "github")
        || contains_word_phrase(&lower, "gitlab")
        || contains_word_phrase(&lower, "stack overflow")
        || contains_word_phrase(&lower, "stackoverflow")
        || contains_word_phrase(&lower, "codepen")
        || contains_word_phrase(&lower, "replit")
        || contains_word_phrase(&lower, "localhost")
        || contains_word_phrase(&lower, "127.0.0.1")
        || contains_word_phrase(&lower, "swagger")
        || contains_word_phrase(&lower, "bitbucket")
        || contains_word_phrase(&lower, "codesandbox")
        || contains_word_phrase(&lower, "stackblitz")
    {
        return Some(TonePreset::Technical);
    }

    // Polished web apps (email, docs, issue tracking, professional networks)
    if contains_word_phrase(&lower, "gmail")
        || lower.contains("mail.google.com")
        || contains_word_phrase(&lower, "outlook")
        || contains_word_phrase(&lower, "google docs")
        || lower.contains("docs.google.com")
        || contains_word_phrase(&lower, "notion")
        || clean == "linear"
        || lower.contains("linear.app")
        || contains_word_phrase(&lower, "linkedin")
        || contains_word_phrase(&lower, "proton mail")
        || contains_word_phrase(&lower, "protonmail")
        || lower.contains("mail.proton.me")
        || contains_word_phrase(&lower, "fastmail")
        || contains_word_phrase(&lower, "yahoo mail")
        || contains_word_phrase(&lower, "icloud mail")
        || contains_word_phrase(&lower, "confluence")
    {
        return Some(TonePreset::Polished);
    }

    // Casual chat web apps
    if contains_word_phrase(&lower, "discord")
        || contains_word_phrase(&lower, "slack")
        || contains_word_phrase(&lower, "whatsapp")
        || clean == "messenger"
        || clean == "messenger web"
        || contains_word_phrase(&lower, "facebook messenger")
        || lower.contains("messenger.com")
        || contains_word_phrase(&lower, "telegram")
        || contains_word_phrase(&lower, "microsoft teams")
        || contains_word_phrase(&lower, "teams chat")
        || clean == "teams"
        || lower.contains("teams.microsoft.com")
        || lower.contains("teams.live.com")
    {
        return Some(TonePreset::Casual);
    }

    None
}

fn classify_browser_title(title: &str) -> TonePreset {
    // Walk right-to-left so trailing site names ("... · GitHub - Google Chrome",
    // "... - Google Docs - Google Chrome") win over page-title text.
    for seg in split_title_segments(title).into_iter().rev() {
        if let Some(tone) = classify_browser_segment(seg) {
            return tone;
        }
    }
    TonePreset::Default
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

/// Resolves the active [`TonePreset`] for `app_name` and an optional transient
/// `browser_title`.
///
/// Resolution order:
/// 1. Exact case-insensitive user rule match via [`same_app_rule`].
/// 2. Case-insensitive substring user rule match on `app_name` (longest wins;
///    ties keep the earlier rule). Empty rules are ignored.
/// 3. When `auto_enabled` is true:
///    - If [`is_browser_app(app_name)`] is true, inspects `browser_title` for
///      known web apps (Gmail, Outlook, Google Docs, Notion, Linear, LinkedIn,
///      Discord, Slack, WhatsApp, Messenger, Telegram, Teams, GitHub, GitLab,
///      Stack Overflow, CodePen, Replit, localhost, Swagger, etc.).
///    - For non-browser native apps, `browser_title` is NEVER inspected; built-in
///      defaults resolve strictly by `app_name`.
/// 4. Everything else resolves to [`TonePreset::Default`].
pub fn resolve_tone_for_target(
    app_name: &str,
    browser_title: Option<&str>,
    rules: &[AppToneRule],
    auto_enabled: bool,
) -> TonePreset {
    let norm = normalize_app_name(app_name);
    if norm.is_empty() {
        return TonePreset::Default;
    }

    // Pass 1: exact case-insensitive user rule match.
    for rule in rules {
        if same_app_rule(&norm, &rule.app) {
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

    if is_browser_app(app_name) {
        return browser_title
            .map(classify_browser_title)
            .unwrap_or(TonePreset::Default);
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

/// Backward-compatible wrapper around [`resolve_tone_for_target`] without a
/// browser title.
pub fn resolve_tone(app_name: &str, auto_enabled: bool, rules: &[AppToneRule]) -> TonePreset {
    resolve_tone_for_target(app_name, None, rules, auto_enabled)
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

    #[test]
    fn browser_title_matches_web_apps_without_false_positives_on_native_apps() {
        // 1. Browser allowlist recognizes executables and display names cleanly.
        for browser in [
            "chrome",
            "chrome.exe",
            "Google Chrome",
            "msedge",
            "msedge.exe",
            "Microsoft Edge",
            "firefox",
            "firefox.exe",
            "Mozilla Firefox",
            "brave",
            "brave.exe",
            "Brave Browser",
            "vivaldi",
            "vivaldi.exe",
            "opera",
            "opera.exe",
            "arc",
            "arc.exe",
            "zen",
            "zen.exe",
            "Zen Browser",
            "chromium",
            "waterfox",
            "floorp",
            "librewolf",
        ] {
            assert!(
                is_browser_app(browser),
                "expected {browser:?} to be recognized as a browser"
            );
        }

        for non_browser in [
            "",
            ".exe",
            "Code.exe",
            "VS Code",
            "WINWORD.EXE",
            "Word",
            "Microsoft Word",
            "Discord.exe",
            "Slack",
            "Notepad",
            "msedgewebview2.exe",
            "Barcode Scanner",
            "Archive Manager",
            "ZenDesk",
            "Operator",
        ] {
            assert!(
                !is_browser_app(non_browser),
                "expected {non_browser:?} NOT to be recognized as a browser"
            );
        }

        // 2. Non-browser native apps NEVER inspect browser_title.
        assert_eq!(
            resolve_tone_for_target(
                "Code.exe",
                Some("gmail_client.rs - listen-rs - Visual Studio Code"),
                &[],
                true
            ),
            TonePreset::Technical
        );
        assert_eq!(
            resolve_tone_for_target(
                "VS Code",
                Some("Inbox - Gmail - Visual Studio Code"),
                &[],
                true
            ),
            TonePreset::Technical
        );
        assert_eq!(
            resolve_tone_for_target(
                "WINWORD.EXE",
                Some("slack_notes.docx - Word"),
                &[],
                true
            ),
            TonePreset::Polished
        );
        assert_eq!(
            resolve_tone_for_target(
                "Microsoft Word",
                Some("Discord chat notes - Word"),
                &[],
                true
            ),
            TonePreset::Polished
        );
        assert_eq!(
            resolve_tone_for_target(
                "Notepad",
                Some("Inbox - Gmail - Notepad"),
                &[],
                true
            ),
            TonePreset::Default
        );

        // 3. Known browsers resolve web apps from transient browser_title when auto_enabled.
        for (app, title, expected) in [
            ("chrome.exe", "Inbox - Gmail - Google Chrome", TonePreset::Polished),
            ("Google Chrome", "Compose - user@example.com - Gmail - Google Chrome", TonePreset::Polished),
            ("msedge.exe", "Mail - Alice - Outlook - Microsoft Edge", TonePreset::Polished),
            ("Google Chrome", "Q4 Plan - Google Docs - Google Chrome", TonePreset::Polished),
            ("Mozilla Firefox", "Roadmap - Notion — Mozilla Firefox", TonePreset::Polished),
            ("Arc", "ENG-42 Fix auth - Linear - Arc", TonePreset::Polished),
            ("Brave Browser", "Feed | LinkedIn - Brave", TonePreset::Polished),
            ("chrome.exe", "#general | Dev Server - Discord - Google Chrome", TonePreset::Casual),
            ("firefox.exe", "engineering - Acme - Slack — Mozilla Firefox", TonePreset::Casual),
            ("vivaldi.exe", "(3) WhatsApp - Vivaldi", TonePreset::Casual),
            ("opera.exe", "(1) Messenger | Facebook - Opera", TonePreset::Casual),
            ("zen.exe", "Telegram Web - Zen Browser", TonePreset::Casual),
            ("msedge.exe", "Chat | Microsoft Teams - Microsoft Edge", TonePreset::Casual),
            ("chrome.exe", "listen-rs/src/app_tone.rs · user/listen-rs · GitHub - Google Chrome", TonePreset::Technical),
            ("firefox.exe", "Merge Requests · GitLab — Mozilla Firefox", TonePreset::Technical),
            ("brave.exe", "rust - Borrow checker - Stack Overflow - Brave", TonePreset::Technical),
            ("chromium.exe", "Animated Button - CodePen - Chromium", TonePreset::Technical),
            ("waterfox.exe", "my-app - Replit - Waterfox", TonePreset::Technical),
            ("floorp.exe", "http://localhost:3000/api - Floorp", TonePreset::Technical),
            ("librewolf.exe", "Swagger UI - LibreWolf", TonePreset::Technical),
            // Right-to-left site precedence avoids page-title cross-talk:
            ("chrome.exe", "PR #42: Fix Gmail sync · org/repo · GitHub - Google Chrome", TonePreset::Technical),
            ("chrome.exe", "Slack RFC draft - Google Docs - Google Chrome", TonePreset::Polished),
            ("chrome.exe", "Fix Discord webhook bug - Linear - Google Chrome", TonePreset::Polished),
            // Generic browser tabs stay Default:
            ("chrome.exe", "New Tab - Google Chrome", TonePreset::Default),
            ("chrome.exe", "Linear algebra - Wikipedia - Google Chrome", TonePreset::Default),
            ("firefox.exe", "YouTube — Mozilla Firefox", TonePreset::Default),
        ] {
            assert_eq!(
                resolve_tone_for_target(app, Some(title), &[], true),
                expected,
                "expected {expected:?} for app={app:?}, title={title:?}"
            );
        }

        // Browser without title or with auto_enabled=false resolves to Default.
        assert_eq!(
            resolve_tone_for_target("chrome.exe", None, &[], true),
            TonePreset::Default
        );
        assert_eq!(
            resolve_tone_for_target("chrome.exe", Some("Inbox - Gmail - Google Chrome"), &[], false),
            TonePreset::Default
        );

        // Explicit user rule overrides browser title detection (including .exe normalization).
        let browser_rules = vec![AppToneRule {
            app: "Chrome".into(),
            tone: TonePreset::Technical,
        }];
        assert_eq!(
            resolve_tone_for_target(
                "chrome.exe",
                Some("Inbox - Gmail - Google Chrome"),
                &browser_rules,
                true
            ),
            TonePreset::Technical
        );

        // 4. validate_rule rejects rules whose normalized app name is empty.
        assert!(validate_rule(".exe", TonePreset::Casual).is_err());
        assert!(validate_rule("   .exe", TonePreset::Polished).is_err());
        assert!(validate_rule("  .EXE  ", TonePreset::Technical).is_err());

        // 5. same_app_rule deduplicates normalized app names cleanly.
        assert!(same_app_rule("Discord", "Discord.exe"));
        assert!(same_app_rule("  DISCORD  ", "discord.exe"));
        assert!(same_app_rule("chrome.exe", "Chrome"));
        assert!(!same_app_rule("Discord", "Slack"));
        assert!(!same_app_rule("", ".exe"));
        assert!(!same_app_rule(".exe", "   .exe"));
    }
}
