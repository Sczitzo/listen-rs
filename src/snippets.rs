//! Deterministic voice snippet validation and spoken-trigger expansion.

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Maximum allowed length (in Unicode scalar values) for a snippet trigger or expansion.
pub const MAX_SNIPPET_LEN: usize = 4000;

/// A spoken trigger phrase and the text it expands into.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub expansion: String,
}

/// Detailed result of expanding spoken snippets in an utterance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnippetResult {
    pub text: String,
    pub matched: bool,
    pub standalone: bool,
}

/// Returns `true` if two snippet triggers are equivalent after collapsing whitespace
/// and Unicode case-folding to lowercase.
pub fn triggers_equal(a: &str, b: &str) -> bool {
    let mut ia = a.split_whitespace();
    let mut ib = b.split_whitespace();
    loop {
        match (ia.next(), ib.next()) {
            (Some(wa), Some(wb)) => {
                if wa.to_lowercase() != wb.to_lowercase() {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// Validates and normalizes a voice snippet definition.
///
/// Trims and collapses internal whitespace in `trigger` and rejects empty triggers,
/// empty/whitespace-only expansions, NUL bytes, or values longer than [`MAX_SNIPPET_LEN`] chars.
pub fn validate_snippet(trigger: &str, expansion: &str) -> Result<Snippet, String> {
    if trigger.contains('\0') || expansion.contains('\0') {
        return Err("Snippets cannot contain NUL bytes.".into());
    }
    let trigger = trigger.split_whitespace().collect::<Vec<_>>().join(" ");
    if trigger.is_empty() {
        return Err("Snippet trigger cannot be empty.".into());
    }
    if expansion.trim().is_empty() {
        return Err("Snippet expansion cannot be empty.".into());
    }
    if trigger.chars().count() > MAX_SNIPPET_LEN {
        return Err(format!(
            "Snippet trigger is too long (maximum {MAX_SNIPPET_LEN} characters)."
        ));
    }
    if expansion.chars().count() > MAX_SNIPPET_LEN {
        return Err(format!(
            "Snippet expansion is too long (maximum {MAX_SNIPPET_LEN} characters)."
        ));
    }
    Ok(Snippet {
        trigger,
        expansion: expansion.to_owned(),
    })
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’')
}

fn is_terminal_punct(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…')
}

/// Checks whole-phrase boundaries around `text[start..end]`.
///
/// Prevents matching inside larger words (e.g. `"cat"` inside `"category"` or `"cat's"`,
/// `"my email"` inside `"dummy email"`, or `"/sig"` inside `"foo/sig"`).
fn has_valid_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before = &text[..start];
    let after = &text[end..];

    if let Some(prev) = before.chars().next_back() {
        if is_word_char(prev) {
            return false;
        }
        if is_apostrophe(prev) {
            let before_apos = &before[..before.len() - prev.len_utf8()];
            if before_apos.chars().next_back().is_some_and(is_word_char) {
                return false;
            }
        }
    }

    if let Some(next) = after.chars().next() {
        if is_word_char(next) {
            return false;
        }
        if is_apostrophe(next) {
            let after_apos = &after[next.len_utf8()..];
            if after_apos.chars().next().is_some_and(is_word_char) {
                return false;
            }
        }
    }

    true
}

fn compile_trigger_regex(trigger: &str) -> Option<Regex> {
    let parts: Vec<String> = trigger.split_whitespace().map(regex::escape).collect();
    if parts.is_empty() {
        return None;
    }
    let pattern = format!("(?i){}", parts.join(r"(?:\s+|-)"));
    Regex::new(&pattern).ok()
}

/// Expands spoken snippet triggers inside `text` deterministically.
///
/// Returns `(expanded_text, did_expand)`. Call [`expand_snippets_detailed`] when you also
/// need to know whether the entire utterance was a single standalone snippet trigger.
pub fn expand_snippets(text: &str, snippets: &[Snippet]) -> (String, bool) {
    let res = expand_snippets_detailed(text, snippets);
    (res.text, res.matched)
}

/// Expands spoken snippet triggers inside `text` deterministically, returning both whether
/// any snippet matched and whether the entire utterance was a single standalone trigger.
///
/// - Matches triggers case-insensitively on whole-phrase boundaries (allowing whitespace or
///   hyphens between multi-word trigger tokens).
/// - Resolves overlapping triggers longest-first (tie-breaking by slice order).
/// - Performs a single non-recursive pass so expansions never re-trigger snippets.
/// - Strips automatic ASR trailing periods/commas on standalone triggers
///   (e.g. `"my email."` or `"my email ."` -> `"cam@example.com"`), and preserves inline
///   trailing punctuation unless the expansion is multiline or already ends with terminal
///   punctuation.
pub fn expand_snippets_detailed(text: &str, snippets: &[Snippet]) -> SnippetResult {
    if text.is_empty() || snippets.is_empty() {
        return SnippetResult {
            text: text.to_owned(),
            matched: false,
            standalone: false,
        };
    }

    // Collect valid snippets and sort longest trigger first (stable sort keeps original order on ties).
    let mut ordered: Vec<(usize, &str, &str)> = snippets
        .iter()
        .filter_map(|s| {
            let trigger = s.trigger.trim();
            if trigger.is_empty() || s.expansion.trim().is_empty() {
                return None;
            }
            let norm_len: usize = trigger
                .split_whitespace()
                .map(|part| part.chars().count())
                .sum::<usize>()
                + trigger.split_whitespace().count().saturating_sub(1);
            Some((norm_len, trigger, s.expansion.as_str()))
        })
        .collect();

    if ordered.is_empty() {
        return SnippetResult {
            text: text.to_owned(),
            matched: false,
            standalone: false,
        };
    }

    ordered.sort_by(|a, b| b.0.cmp(&a.0));

    // Find all non-overlapping matches against the original text, longest trigger first.
    let mut matches: Vec<(usize, usize, &str)> = Vec::new();
    for &(_, trigger, expansion) in &ordered {
        let Some(re) = compile_trigger_regex(trigger) else {
            continue;
        };
        for m in re.find_iter(text) {
            let (start, end) = (m.start(), m.end());
            if !has_valid_boundaries(text, start, end) {
                continue;
            }
            let overlaps = matches
                .iter()
                .any(|&(ms, me, _)| start < me && end > ms);
            if !overlaps {
                matches.push((start, end, expansion));
            }
        }
    }

    if matches.is_empty() {
        return SnippetResult {
            text: text.to_owned(),
            matched: false,
            standalone: false,
        };
    }

    matches.sort_by_key(|&(start, _, _)| start);

    // Check if the entire utterance is a single standalone trigger (plus optional ASR trailing `.`/`,`).
    if matches.len() == 1 {
        let (start, end, expansion) = matches[0];
        let before = &text[..start];
        let after = &text[end..];
        let after_trimmed = after.trim();
        let standalone_asr_punct = matches!(after_trimmed, "" | "." | ",");
        let suppress_terminal =
            expansion.contains('\n') || expansion.trim_end().chars().next_back().is_some_and(is_terminal_punct);
        if before.trim().is_empty() && standalone_asr_punct {
            let mut out = String::with_capacity(before.len() + expansion.len());
            out.push_str(before);
            out.push_str(expansion);
            if after_trimmed.is_empty() {
                out.push_str(after);
            } else {
                // Preserve any trailing whitespace after the stripped ASR period/comma.
                let stripped = after.trim_start().trim_start_matches(['.', ',']);
                out.push_str(stripped);
            }
            return SnippetResult {
                text: out,
                matched: true,
                standalone: true,
            };
        }
        if before.trim().is_empty()
            && suppress_terminal
            && !after_trimmed.is_empty()
            && after_trimmed.chars().all(is_terminal_punct)
        {
            let mut out = String::with_capacity(before.len() + expansion.len());
            out.push_str(before);
            out.push_str(expansion);
            let stripped = after.trim_start().trim_start_matches(is_terminal_punct);
            out.push_str(stripped);
            return SnippetResult {
                text: out,
                matched: true,
                standalone: true,
            };
        }
    }

    let mut out = String::with_capacity(text.len() + 32);
    let mut cursor = 0usize;
    for (start, end, expansion) in matches {
        if start < cursor {
            continue;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(expansion);

        let suppress_terminal = expansion.contains('\n')
            || expansion
                .trim_end()
                .chars()
                .next_back()
                .is_some_and(is_terminal_punct);

        let mut next_cursor = end;
        if suppress_terminal {
            let rest = &text[end..];
            let ws_bytes: usize = rest
                .chars()
                .take_while(|&c| matches!(c, ' ' | '\t'))
                .map(char::len_utf8)
                .sum();
            let after_ws = &rest[ws_bytes..];
            let punct_bytes: usize = after_ws
                .chars()
                .take_while(|&c| is_terminal_punct(c))
                .map(char::len_utf8)
                .sum();
            if punct_bytes > 0 {
                next_cursor += ws_bytes + punct_bytes;
            }
        }
        cursor = next_cursor;
    }
    out.push_str(&text[cursor..]);
    SnippetResult {
        text: out,
        matched: true,
        standalone: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippet(trigger: &str, expansion: &str) -> Snippet {
        validate_snippet(trigger, expansion).expect("valid test snippet")
    }

    #[test]
    fn validate_snippet_trims_trigger_and_rejects_invalid_inputs() {
        let s = validate_snippet("  my email  ", "cam@example.com").unwrap();
        assert_eq!(s.trigger, "my email");
        assert_eq!(s.expansion, "cam@example.com");

        // Preserves multiline formatting in expansion.
        let multi = validate_snippet("sig", "Best regards,\n  Cam").unwrap();
        assert_eq!(multi.expansion, "Best regards,\n  Cam");

        // Rejects empty/whitespace trigger or expansion.
        assert!(validate_snippet("", "abc").is_err());
        assert!(validate_snippet("   \t\n ", "abc").is_err());
        assert!(validate_snippet("my email", "").is_err());
        assert!(validate_snippet("my email", "   \n\t ").is_err());

        // Rejects NUL bytes.
        assert!(validate_snippet("my\0email", "cam@example.com").is_err());
        assert!(validate_snippet("my email", "cam\0@example.com").is_err());

        // Length boundary: 4000 ok, 4001 rejected (counted in Unicode chars).
        let exact = "é".repeat(MAX_SNIPPET_LEN);
        assert!(validate_snippet(&exact, &exact).is_ok());
        let too_long = "é".repeat(MAX_SNIPPET_LEN + 1);
        assert!(validate_snippet(&too_long, "ok").is_err());
        assert!(validate_snippet("ok", &too_long).is_err());
    }

    #[test]
    fn expands_standalone_and_inline_case_insensitively() {
        let snippets = vec![snippet("my email", "cam@example.com")];

        assert_eq!(
            expand_snippets("my email.", &snippets),
            ("cam@example.com".into(), true)
        );
        assert_eq!(
            expand_snippets("My Email.", &snippets),
            ("cam@example.com".into(), true)
        );
        assert_eq!(
            expand_snippets("MY EMAIL", &snippets),
            ("cam@example.com".into(), true)
        );
        assert_eq!(
            expand_snippets("send it to my email please", &snippets),
            ("send it to cam@example.com please".into(), true)
        );
        assert_eq!(
            expand_snippets("Send it to MY   EMAIL, please.", &snippets),
            ("Send it to cam@example.com, please.".into(), true)
        );
        assert_eq!(
            expand_snippets("Send it to my email.", &snippets),
            ("Send it to cam@example.com.".into(), true)
        );
        assert_eq!(
            expand_snippets("Is that my email?", &snippets),
            ("Is that cam@example.com?".into(), true)
        );
    }

    #[test]
    fn respects_whole_word_boundaries_and_contractions() {
        let snippets = vec![
            snippet("cat", "feline"),
            snippet("my email", "cam@example.com"),
            snippet("i", "myself"),
        ];

        // Must not corrupt partial words or contractions.
        assert_eq!(
            expand_snippets("category scatter concatenate cat's", &snippets),
            ("category scatter concatenate cat's".into(), false)
        );
        assert_eq!(
            expand_snippets("dummy email and my emailed note", &snippets),
            ("dummy email and my emailed note".into(), false)
        );
        assert_eq!(
            expand_snippets("I'm sure I'll go.", &snippets),
            ("I'm sure I'll go.".into(), false)
        );

        // Quoted or parenthesized whole words still expand cleanly.
        assert_eq!(
            expand_snippets("The 'cat' (cat) sat.", &snippets),
            ("The 'feline' (feline) sat.".into(), true)
        );
    }

    #[test]
    fn resolves_overlapping_triggers_longest_first_without_recursion() {
        let snippets = vec![
            snippet("my email", "personal@example.com"),
            snippet("my email address", "full@example.com"),
            snippet("email", "my email"),
        ];

        assert_eq!(
            expand_snippets("Use my email address or my email.", &snippets),
            ("Use full@example.com or personal@example.com.".into(), true)
        );
        // Single-pass: "email" -> "my email" must NOT re-trigger "my email".
        assert_eq!(
            expand_snippets("Check email now.", &snippets),
            ("Check my email now.".into(), true)
        );
    }

    #[test]
    fn handles_trailing_punctuation_for_terminal_and_multiline_expansions() {
        let snippets = vec![
            snippet("my sig", "Best regards,\nCam"),
            snippet("cheers", "Thank you so much!"),
            snippet("zoom link", "https://zoom.us/j/12345"),
        ];

        // Multiline suppresses trailing terminal punctuation.
        assert_eq!(
            expand_snippets("Thanks for the update. my sig.", &snippets),
            ("Thanks for the update. Best regards,\nCam".into(), true)
        );
        assert_eq!(
            expand_snippets("my sig.", &snippets),
            ("Best regards,\nCam".into(), true)
        );

        // Terminal punctuation in expansion suppresses duplicate terminal punctuation.
        assert_eq!(
            expand_snippets("Please tell them cheers.", &snippets),
            ("Please tell them Thank you so much!".into(), true)
        );
        assert_eq!(
            expand_snippets("cheers?", &snippets),
            ("Thank you so much!".into(), true)
        );
        // Non-terminal inline punctuation is preserved.
        assert_eq!(
            expand_snippets("Send the zoom link, please.", &snippets),
            ("Send the https://zoom.us/j/12345, please.".into(), true)
        );
        // Standalone URL drops ASR period.
        assert_eq!(
            expand_snippets("Zoom link.", &snippets),
            ("https://zoom.us/j/12345".into(), true)
        );
    }

    #[test]
    fn handles_empty_inputs_special_regex_chars_and_unicode() {
        let snippets = vec![
            snippet("c++", "C++20"),
            snippet("café", "Blue Bottle Café"),
        ];
        assert_eq!(expand_snippets("", &snippets), ("".into(), false));
        assert_eq!(expand_snippets("hello", &[]), ("hello".into(), false));
        assert_eq!(
            expand_snippets("We write c++ at the CAFÉ.", &snippets),
            ("We write C++20 at the Blue Bottle Café.".into(), true)
        );
        // Does not match inside C++17.
        assert_eq!(
            expand_snippets("Use c++17 here.", &snippets),
            ("Use c++17 here.".into(), false)
        );
    }

    #[test]
    fn punctuation_hyphenation_dedup_and_standalone_flag() {
        let snippets = vec![
            snippet("my email", "cam@example.com"),
            snippet("my sig", "Best regards,\nCam"),
            snippet("follow up", "follow-up action item"),
            snippet("greet", "hello"),
            snippet("full greeting", "hello world"),
            snippet("zoom link", "https://zoom.us/j/12345"),
        ];

        // Space before trailing punctuation on standalone triggers strips space + punctuation cleanly.
        let email_res = expand_snippets_detailed("my email .", &snippets);
        assert_eq!(
            email_res,
            SnippetResult {
                text: "cam@example.com".into(),
                matched: true,
                standalone: true,
            }
        );
        let sig_res = expand_snippets_detailed("my sig !", &snippets);
        assert_eq!(
            sig_res,
            SnippetResult {
                text: "Best regards,\nCam".into(),
                matched: true,
                standalone: true,
            }
        );
        // Trailing whitespace after stripped punctuation is still preserved.
        assert_eq!(
            expand_snippets("my email . ", &snippets),
            ("cam@example.com ".into(), true)
        );
        assert_eq!(
            expand_snippets("my sig ! ", &snippets),
            ("Best regards,\nCam ".into(), true)
        );

        // Hyphenated spoken trigger matches multi-word trigger.
        let follow_res = expand_snippets_detailed("Send a follow-up tomorrow.", &snippets);
        assert_eq!(
            follow_res,
            SnippetResult {
                text: "Send a follow-up action item tomorrow.".into(),
                matched: true,
                standalone: false,
            }
        );

        // Internal whitespace normalized in validate_snippet and triggers_equal handles whitespace + Unicode case.
        let s = validate_snippet("my  \t email", "cam@example.com").unwrap();
        assert_eq!(s.trigger, "my email");
        assert!(triggers_equal("my  email", "my email"));
        assert!(triggers_equal("  my\temail  ", "MY EMAIL"));
        assert!(triggers_equal("CAFÉ", "café"));
        assert!(!triggers_equal("my email", "my other email"));

        // Non-standalone utterance whose expanded text happens to equal another snippet's expansion
        // MUST report standalone == false (avoiding false AI-cleanup skip).
        let collision_res = expand_snippets_detailed("greet world", &snippets);
        assert_eq!(
            collision_res,
            SnippetResult {
                text: "hello world".into(),
                matched: true,
                standalone: false,
            }
        );

        // Inline multiline and URL snippets expand with standalone == false (staying post-Ollama).
        let inline_sig = expand_snippets_detailed("Thanks for the update. my sig !", &snippets);
        assert_eq!(
            inline_sig,
            SnippetResult {
                text: "Thanks for the update. Best regards,\nCam".into(),
                matched: true,
                standalone: false,
            }
        );
        let inline_url = expand_snippets_detailed("Join via zoom link now.", &snippets);
        assert_eq!(
            inline_url,
            SnippetResult {
                text: "Join via https://zoom.us/j/12345 now.".into(),
                matched: true,
                standalone: false,
            }
        );
    }
}
