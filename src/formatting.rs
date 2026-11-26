//! Presentation checks based on the current Listen reference (MIT, Sczitzo).

use regex::Regex;
use std::sync::LazyLock;

static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}_]+(?:['’][\p{L}\p{N}_]+)?").unwrap());
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+(?:[.,:]\d+)*").unwrap());
static TECHNICAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"(?:[A-Za-z]:)?[\\/]*(?:[\w.-]+[\\/]+)+[\w.-]+|-{2,}[A-Za-z0-9][\w-]*|\b[A-Za-z_]\w*_\w+\b|\b[A-Za-z_][\w-]*\.[A-Za-z0-9][\w.-]*"
).unwrap()
});
static ARROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[-=]+>|[-=]+>|<[-=]+|[→←↔↑↓]").unwrap());
static OPERATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":=|[+*/=^%<>|&!~\\()\[\]{}$£€¥−×÷≤≥≠∧∨¬-]+").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[-*+]\s+(\S.*)$").unwrap());
static ORDERED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)([0-9]+|[A-Za-z])([.)])\s+(\S.*)$").unwrap());
static DIVIDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:?-{3,}:?$").unwrap());
static BORDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\+(?:[-=]+\+)+\s*$").unwrap());
static SPOKEN_FILLER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:you know|i mean|like|basically|actually|literally|sort of|kind of)\s*$")
        .unwrap()
});
static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z][\p{L}\p{N}_]*\b").unwrap());
static NAMED_FILLER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:um+|uh+|erm|hmm)$").unwrap());
static PREAMBLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"(?i)^(?:sure\b|certainly\b|here(?:'s| is| are)\b|as an ai\b|i (?:can|will|cannot|can't) help\b)"
).unwrap()
});
static BACKTICKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`+").unwrap());
static URL_SPAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:https?://|www\.)[^\s<>"'`)\]]+"#).unwrap());
static EMAIL_SPAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap());
static FENCED_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```.*?```").unwrap());
static NEWLINE_CMD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?P<pre>[ \t,]*)\b(?P<cmd>new\s+(?:paragraph|line))\b(?P<post>[ \t,.]*)")
        .unwrap()
});
static SCRATCH_CMD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?P<sep>(?:\s*[,;—]\s*|\s+--?\s+|\s*[.!?]+\s+|\s+))(?P<cmd>scratch\s+that|delete\s+that)\b(?P<after>[ \t]*(?:[.!?]+|[,;])?[ \t]*)",
    )
    .unwrap()
});

fn words(text: &str) -> Vec<String> {
    WORD.find_iter(text)
        .map(|word| word.as_str().to_lowercase().replace('’', "'"))
        .collect()
}

fn tokens<'a>(pattern: &'static Regex, text: &'a str) -> impl Iterator<Item = &'a str> {
    pattern.find_iter(text).map(|m| m.as_str())
}

fn technical_literal_matches(text: &str) -> Vec<(usize, usize, &str)> {
    TECHNICAL
        .find_iter(text)
        .filter_map(|m| {
            let value = m.as_str();
            let previous = text[..m.start()].chars().next_back();
            let preceded_by_word = previous.is_some_and(|c| c.is_alphanumeric() || c == '_');
            let flag = value.starts_with("--") && !value.contains(['/', '\\']);
            if preceded_by_word || (flag && previous == Some('-')) {
                return None;
            }
            let trimmed = value.trim_end_matches(['.', ',']);
            Some((m.start(), m.start() + trimmed.len(), trimmed))
        })
        .collect()
}

fn technical_literals(text: &str) -> impl Iterator<Item = &str> {
    technical_literal_matches(text)
        .into_iter()
        .map(|(_, _, value)| value)
}

fn inline_code_ranges(text: &str) -> Vec<(usize, usize)> {
    let runs: Vec<_> = BACKTICKS.find_iter(text).collect();
    let mut ranges = Vec::new();
    let mut position = 0;
    while let Some(open) = runs.get(position) {
        if let Some((end, close)) = runs
            .iter()
            .enumerate()
            .skip(position + 1)
            .take_while(|(_, close)| !text[open.end()..close.start()].contains('\n'))
            .find(|(_, close)| close.len() == open.len())
        {
            ranges.push((open.end(), close.start()));
            position = end + 1;
        } else {
            position += 1;
        }
    }
    ranges
}

fn inline_code_full_ranges(text: &str) -> Vec<(usize, usize)> {
    let runs: Vec<_> = BACKTICKS.find_iter(text).collect();
    let mut ranges = Vec::new();
    let mut position = 0;
    while let Some(open) = runs.get(position) {
        if let Some((end, close)) = runs
            .iter()
            .enumerate()
            .skip(position + 1)
            .take_while(|(_, close)| !text[open.end()..close.start()].contains('\n'))
            .find(|(_, close)| close.len() == open.len())
        {
            ranges.push((open.start(), close.end()));
            position = end + 1;
        } else {
            position += 1;
        }
    }
    ranges
}

fn technical_literals_match(raw: &str, formatted: &str) -> bool {
    let raw_lits = technical_literal_matches(raw);
    let fmt_lits = technical_literal_matches(formatted);
    if raw_lits.len() != fmt_lits.len() {
        return false;
    }
    if raw_lits.is_empty() {
        return true;
    }
    let code_ranges = inline_code_ranges(formatted);
    raw_lits.into_iter().zip(fmt_lits).all(|((_, _, r), (fs, fe, f))| {
        r == f
            || (r.to_lowercase() == f.to_lowercase()
                && code_ranges.iter().any(|&(cs, ce)| fs >= cs && fe <= ce))
    })
}

fn is_negative_word(word: &str) -> bool {
    matches!(
        word,
        "no" | "not" | "never" | "neither" | "nor" | "without" | "cannot"
    ) || word.ends_with("n't")
}

fn filler(word: &str) -> bool {
    matches!(word, "um" | "uh" | "erm" | "hmm")
}

fn ordered_words(words: &[String]) -> Vec<&str> {
    let mut result = Vec::new();
    for word in words
        .iter()
        .map(String::as_str)
        .filter(|word| !filler(word))
    {
        if result.last() != Some(&word) {
            result.push(word);
        }
    }
    result
}

fn without_comma_fillers(raw: &str) -> String {
    let mut parts = raw.split(',').peekable();
    let mut result = parts.next().unwrap_or_default().to_owned();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() || !SPOKEN_FILLER.is_match(part) {
            result.push(',');
            result.push_str(part);
        }
    }
    result
}

fn same_words(raw: &str, cleaned: &str, vocabulary: &str) -> bool {
    let before = words(raw);
    let after = words(cleaned);
    if before.is_empty() || after.is_empty() || after.len() > before.len() * 5 / 4 + 2 {
        return false;
    }
    if !tokens(&NUMBER, raw).eq(tokens(&NUMBER, cleaned))
        || !technical_literals_match(raw, cleaned)
        || (PREAMBLE.is_match(cleaned.trim()) && !PREAMBLE.is_match(raw.trim()))
    {
        return false;
    }
    if !before
        .iter()
        .map(String::as_str)
        .filter(|w| is_negative_word(w))
        .eq(after.iter().map(String::as_str).filter(|w| is_negative_word(w)))
    {
        return false;
    }
    let vocab_entries = crate::storage::vocab_words(vocabulary);
    let names = tokens(&NAME, raw).filter(|name| !NAMED_FILLER.is_match(name));
    for term in names.chain(vocab_entries.iter().map(String::as_str)) {
        let terms = words(term);
        let count =
            |in_words: &[String], word: &str| in_words.iter().filter(|v| *v == word).count();
        if terms
            .iter()
            .all(|word| count(&before, word) >= count(&terms, word))
            && terms
                .iter()
                .any(|word| count(&after, word) < count(&terms, word))
        {
            return false;
        }
    }
    let check = |source: &[String]| {
        ordered_words(source) == ordered_words(&after)
            && after.iter().all(|word| source.contains(word))
    };
    check(&before) || check(&words(&without_comma_fillers(raw)))
}

fn operator_context(text: &str) -> Vec<(Vec<String>, &str)> {
    let mut context = Vec::new();
    let mut position = 0;
    let content = |part: &str| {
        let mut words = words(part);
        words.retain(|word| !filler(word));
        words.dedup();
        words
    };
    for operator in OPERATOR.find_iter(text) {
        context.push((
            content(&text[position..operator.start()]),
            operator.as_str(),
        ));
        position = operator.end();
    }
    context.push((content(&text[position..]), ""));
    context
}

fn inline_code(text: &str) -> Vec<(&str, &str)> {
    let runs: Vec<_> = BACKTICKS.find_iter(text).collect();
    let mut spans = Vec::new();
    let mut position = 0;
    while let Some(open) = runs.get(position) {
        if let Some((end, close)) = runs
            .iter()
            .enumerate()
            .skip(position + 1)
            .take_while(|(_, close)| !text[open.end()..close.start()].contains('\n'))
            .find(|(_, close)| close.len() == open.len())
        {
            spans.push((open.as_str(), &text[open.end()..close.start()]));
            position = end + 1;
        } else {
            position += 1;
        }
    }
    spans
}

fn cells(line: &str) -> Option<Vec<&str>> {
    if !line.contains('|') {
        return None;
    }
    let mut cells = Vec::new();
    let mut start = 0;
    let mut ticks = 0;
    let mut characters = line.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        match character {
            '\\' => {
                characters.next();
            }
            '`' => {
                let mut run = 1;
                while characters.next_if(|(_, c)| *c == '`').is_some() {
                    run += 1;
                }
                if ticks == 0 {
                    ticks = run;
                } else if ticks == run {
                    ticks = 0;
                }
            }
            '|' if ticks == 0 => {
                cells.push(line[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if ticks != 0 {
        return None;
    }
    cells.push(line[start..].trim());
    if line.trim_start().starts_with('|') {
        cells.remove(0);
    }
    if line.trim_end().ends_with('|') && cells.last() == Some(&"") {
        cells.pop();
    }
    (cells.len() >= 2).then_some(cells)
}

fn flatten_tables(raw: &str, formatted: &str) -> Option<Vec<String>> {
    let raw_words = words(raw);
    let mut lines = formatted.lines().peekable();
    let mut result = Vec::new();
    while let Some(line) = lines.next() {
        let header = cells(line).filter(|header| {
            lines
                .peek()
                .and_then(|line| cells(line))
                .is_some_and(|divider| {
                    header.len() == divider.len()
                        && divider.iter().all(|cell| DIVIDER.is_match(cell))
                })
        });
        let Some(header) = header else {
            result.push(line.to_owned());
            continue;
        };
        lines.next();
        let neutral = ["item", "detail", "details", "value", "category", "measure"];
        let kept: Vec<_> = header
            .iter()
            .copied()
            .filter(|cell| {
                let lower = cell.to_lowercase();
                !neutral.contains(&lower.as_str()) || raw_words.contains(&lower)
            })
            .collect();
        result.push(kept.join(" "));
        lines.peek().and_then(|line| cells(line))?;
        while let Some(row) = lines.peek().and_then(|line| cells(line)) {
            if row.len() != header.len() || row.contains(&"") {
                return None;
            }
            result.push(row.join(" "));
            lines.next();
        }
    }
    Some(result)
}

fn marker_value(marker: &str, start: &str) -> Option<usize> {
    if start == "1" {
        marker.parse().ok()
    } else if marker.len() == 1 && marker.as_bytes()[0].is_ascii_alphabetic() {
        Some(marker.as_bytes()[0] as usize)
    } else {
        None
    }
}

fn strip_lists(lines: &mut [String]) {
    let mut index = 0;
    while index < lines.len() {
        if let Some(bullet) = BULLET.captures(&lines[index]) {
            lines[index] = bullet[1].to_owned();
        } else if let Some(first) = ORDERED.captures(&lines[index]) {
            let start = &first[2];
            if matches!(start, "1" | "A" | "a") {
                let mut expected = marker_value(start, start).unwrap();
                let mut replacements = Vec::new();
                for (offset, line) in lines[index..].iter().enumerate() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let Some(item) = ORDERED.captures(line) else {
                        break;
                    };
                    if item[1] != first[1]
                        || item[3] != first[3]
                        || marker_value(&item[2], start) != Some(expected)
                    {
                        break;
                    }
                    replacements.push((index + offset, item[4].to_owned()));
                    expected += 1;
                }
                if replacements.len() >= 2 {
                    index = replacements.last().unwrap().0;
                    for (position, item) in replacements {
                        lines[position] = item;
                    }
                }
            }
        }
        index += 1;
    }
}

fn has_whole_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    let matched = &text[start..end];
    let Some(first) = matched.chars().next() else {
        return false;
    };
    let Some(last) = matched.chars().next_back() else {
        return false;
    };
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if let Some(prev) = text[..start].chars().next_back() {
        if is_word(first) {
            if is_word(prev) {
                return false;
            }
        } else if is_word(prev) || prev == first {
            return false;
        }
    }
    if let Some(next) = text[end..].chars().next() {
        if is_word(last) {
            if is_word(next) {
                return false;
            }
        } else if is_word(next) || next == last {
            return false;
        }
    }
    true
}

fn split_structured_vocab(term: &str) -> Option<Vec<String>> {
    let alnum_count = term.chars().filter(|c| c.is_alphanumeric()).count();
    if alnum_count < 4 {
        return None;
    }
    let chars: Vec<char> = term.chars().collect();
    let mut has_structure = term.contains(['.', '-', '_']);
    let mut parts = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if matches!(c, '.' | '-' | '_') || c.is_whitespace() {
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if !cur.is_empty() {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied();
            let is_camel = (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_uppercase()
                    && c.is_uppercase()
                    && next.is_some_and(char::is_lowercase));
            if is_camel {
                has_structure = true;
                parts.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    (has_structure && parts.len() >= 2).then_some(parts)
}

fn build_exact_vocab_pattern(term: &str) -> String {
    let pieces: Vec<String> = term.split_whitespace().map(regex::escape).collect();
    format!("(?i){}", pieces.join(r"[ \t]+"))
}

fn apply_vocab_postpass(text: &str, vocabulary: &str) -> (String, bool) {
    if text.is_empty() || text.len() > 2_000_000 || vocabulary.trim().is_empty() {
        return (text.to_owned(), false);
    }
    let raw_entries = crate::storage::vocab_words(vocabulary);
    if raw_entries.is_empty() {
        return (text.to_owned(), false);
    }
    let mut entries: Vec<String> = Vec::with_capacity(raw_entries.len());
    let mut seen = std::collections::HashSet::new();
    for entry in raw_entries {
        if seen.insert(entry.to_lowercase()) {
            entries.push(entry);
        }
    }

    let mut backtick_ranges = inline_code_full_ranges(text);
    for m in FENCED_CODE.find_iter(text) {
        backtick_ranges.push((m.start(), m.end()));
    }
    let url_ranges: Vec<(usize, usize)> = URL_SPAN
        .find_iter(text)
        .map(|m| (m.start(), m.end()))
        .collect();
    let email_ranges: Vec<(usize, usize)> = EMAIL_SPAN
        .find_iter(text)
        .map(|m| (m.start(), m.end()))
        .collect();
    let tech_ranges: Vec<(usize, usize)> = technical_literal_matches(text)
        .into_iter()
        .map(|(s, e, _)| (s, e))
        .collect();

    let overlaps = |ranges: &[(usize, usize)], s: usize, e: usize| {
        ranges.iter().any(|&(rs, re)| s < re && e > rs)
    };

    // (start, end, replacement, is_exact)
    let mut candidates: Vec<(usize, usize, &str, bool)> = Vec::new();
    for term in &entries {
        let exact_pat = build_exact_vocab_pattern(term);
        if let Ok(re) = Regex::new(&exact_pat) {
            for m in re.find_iter(text) {
                let (s, e) = (m.start(), m.end());
                if !has_whole_word_boundaries(text, s, e)
                    || overlaps(&backtick_ranges, s, e)
                    || overlaps(&email_ranges, s, e)
                {
                    continue;
                }
                if overlaps(&url_ranges, s, e)
                    && !url_ranges.iter().any(|&(us, ue)| us == s && ue == e)
                {
                    continue;
                }
                if overlaps(&tech_ranges, s, e)
                    && !tech_ranges.iter().any(|&(ts, te)| ts == s && te == e)
                {
                    continue;
                }
                candidates.push((s, e, term.as_str(), true));
            }
        }
        if let Some(parts) = split_structured_vocab(term) {
            let escaped: Vec<String> = parts.iter().map(|p| regex::escape(p)).collect();
            let struct_pat = format!("(?i){}", escaped.join(r"[ \t._-]*"));
            if let Ok(re) = Regex::new(&struct_pat) {
                for m in re.find_iter(text) {
                    let (s, e) = (m.start(), m.end());
                    if !has_whole_word_boundaries(text, s, e)
                        || overlaps(&backtick_ranges, s, e)
                        || overlaps(&url_ranges, s, e)
                        || overlaps(&email_ranges, s, e)
                    {
                        continue;
                    }
                    if overlaps(&tech_ranges, s, e)
                        && !tech_ranges.iter().any(|&(ts, te)| ts == s && te == e)
                    {
                        continue;
                    }
                    candidates.push((s, e, term.as_str(), false));
                }
            }
        }
    }

    if candidates.is_empty() {
        return (text.to_owned(), false);
    }

    // Longest matched span first, then longest configured term, then exact over collapsed, then earliest.
    candidates.sort_by(|a, b| {
        (b.1 - b.0)
            .cmp(&(a.1 - a.0))
            .then_with(|| b.2.len().cmp(&a.2.len()))
            .then_with(|| b.3.cmp(&a.3))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut chosen: Vec<(usize, usize, &str)> = Vec::new();
    for (s, e, repl, _) in candidates {
        if chosen.iter().any(|&(cs, ce, _)| s < ce && e > cs) {
            continue;
        }
        chosen.push((s, e, repl));
    }
    chosen.sort_by_key(|&(s, _, _)| s);

    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut changed = false;
    for (s, e, repl) in chosen {
        out.push_str(&text[pos..s]);
        if &text[s..e] != repl {
            changed = true;
        }
        out.push_str(repl);
        pos = e;
    }
    out.push_str(&text[pos..]);
    (out, changed)
}

fn vocab_norm(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '.' | '-' | '_' | '\'' | '’'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn damerau_levenshtein(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let n = ac.len();
    let m = bc.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(ac[i - 1] != bc[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && ac[i - 1] == bc[j - 2] && ac[i - 2] == bc[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[n][m]
}

fn consonant_skeleton(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        for lc in c.to_lowercase() {
            if lc.is_alphabetic() && !matches!(lc, 'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h') {
                if out.chars().last() != Some(lc) {
                    out.push(lc);
                }
            }
        }
    }
    out
}

fn is_vocab_stop_or_modal(w: &str) -> bool {
    matches!(
        w,
        "a" | "an"
            | "the"
            | "and"
            | "or"
            | "but"
            | "nor"
            | "for"
            | "yet"
            | "so"
            | "in"
            | "on"
            | "at"
            | "to"
            | "of"
            | "by"
            | "as"
            | "is"
            | "am"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "have"
            | "has"
            | "had"
            | "do"
            | "does"
            | "did"
            | "done"
            | "can"
            | "could"
            | "will"
            | "would"
            | "shall"
            | "should"
            | "may"
            | "might"
            | "must"
            | "just"
            | "only"
            | "also"
            | "very"
            | "even"
            | "still"
            | "then"
            | "than"
            | "that"
            | "this"
            | "these"
            | "those"
            | "with"
            | "from"
            | "into"
            | "onto"
            | "over"
            | "under"
            | "about"
            | "after"
            | "before"
            | "between"
            | "through"
            | "during"
            | "without"
            | "because"
            | "while"
            | "where"
            | "when"
            | "what"
            | "which"
            | "who"
            | "whom"
            | "whose"
            | "how"
            | "why"
            | "there"
            | "their"
            | "they"
            | "them"
            | "here"
            | "your"
            | "yours"
            | "our"
            | "ours"
            | "mine"
            | "his"
            | "her"
            | "hers"
            | "him"
            | "she"
            | "he"
            | "it"
            | "its"
            | "we"
            | "us"
            | "you"
            | "me"
            | "my"
            | "i"
            | "some"
            | "any"
            | "all"
            | "both"
            | "each"
            | "every"
            | "many"
            | "much"
            | "more"
            | "most"
            | "other"
            | "such"
            | "same"
            | "well"
            | "really"
            | "actually"
            | "basically"
            | "literally"
            | "never"
            | "ever"
            | "not"
            | "no"
            | "yes"
            | "yeah"
            | "okay"
            | "ok"
            | "sure"
            | "please"
            | "sorry"
            | "get"
            | "got"
            | "put"
            | "let"
            | "set"
            | "run"
            | "use"
            | "try"
            | "see"
            | "saw"
            | "say"
            | "said"
            | "make"
            | "made"
            | "take"
            | "took"
            | "give"
            | "gave"
            | "come"
            | "came"
            | "want"
            | "need"
            | "know"
            | "like"
            | "good"
            | "best"
            | "next"
            | "last"
            | "first"
            | "back"
            | "down"
            | "out"
            | "off"
            | "now"
            | "too"
    )
}

fn can_reconcile_vocab(raw_span: &str, vocab_term: &str) -> Option<(bool, usize)> {
    if raw_span.contains(['\n', '\r']) {
        return None;
    }
    let rn = vocab_norm(raw_span);
    let vn = vocab_norm(vocab_term);
    if rn.is_empty() || vn.is_empty() {
        return None;
    }
    if technical_literals(raw_span).next().is_some()
        && technical_literals(vocab_term).next().is_none()
    {
        return None;
    }
    if rn == vn {
        let vocab_has_leading_dash = vocab_term.split_whitespace().any(|t| t.starts_with('-'));
        let vocab_has_trailing_dash = vocab_term.split_whitespace().any(|t| t.ends_with('-'));
        if raw_span.split_whitespace().any(|t| {
            (t.starts_with('-') && !vocab_has_leading_dash)
                || (t.ends_with('-') && !vocab_has_trailing_dash)
        }) || ARROW.is_match(raw_span)
            || (raw_span.contains("--") && !vocab_term.contains("--"))
            || OPERATOR.find_iter(raw_span).any(|m| {
                m.as_str().chars().any(|c| !matches!(c, '.' | '-' | '_'))
            })
        {
            return None;
        }
        return Some((true, 0));
    }
    let vlen = vn.chars().count();
    if vlen < 4 {
        return None;
    }
    if ARROW.is_match(raw_span) || OPERATOR.is_match(raw_span) {
        return None;
    }
    let r_digits: String = raw_span.chars().filter(|c| c.is_ascii_digit()).collect();
    let v_digits: String = vocab_term.chars().filter(|c| c.is_ascii_digit()).collect();
    if (!r_digits.is_empty() || !v_digits.is_empty()) && r_digits != v_digits {
        return None;
    }
    let rw = words(raw_span);
    if rw.is_empty() {
        return None;
    }
    if rw.iter().any(|w| is_negative_word(w)) {
        return None;
    }
    if (rw.len() == 1 && is_vocab_stop_or_modal(&rw[0]))
        || (rw.len() > 1 && rw.iter().all(|w| is_vocab_stop_or_modal(w)))
    {
        return None;
    }
    let max_dist = match vlen {
        4..=5 => 1,
        6..=9 => 2,
        _ => 3,
    };
    let dist = damerau_levenshtein(&rn, &vn);
    if dist > max_dist {
        return None;
    }
    let max_len = vlen.max(rn.chars().count()) as f64;
    let sim = 1.0 - (dist as f64 / max_len);
    let is_multi_word = rw.len() >= 2;
    let first_match = rn.chars().next() == vn.chars().next();
    let skel_r = consonant_skeleton(&rn);
    let skel_v = consonant_skeleton(&vn);
    let skeleton_match =
        is_multi_word && first_match && skel_v.len() >= 2 && skel_r == skel_v;
    (sim >= 0.65 || skeleton_match).then_some((false, dist))
}

#[derive(Clone, Copy, Debug)]
struct RawTok<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn tokenize_raw_spans(text: &str) -> Vec<RawTok<'_>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && text[i..].chars().next().is_some_and(char::is_whitespace) {
            i += text[i..].chars().next().unwrap().len_utf8();
        }
        if i >= bytes.len() {
            break;
        }
        let chunk_start = i;
        while i < bytes.len() && !text[i..].chars().next().is_some_and(char::is_whitespace) {
            i += text[i..].chars().next().unwrap().len_utf8();
        }
        let mut s = chunk_start;
        let mut e = i;
        while s < e {
            let ch = text[s..e].chars().next().unwrap();
            if matches!(ch, '(' | '[' | '{' | '"' | '“' | '‘' | '\'' | '`') {
                s += ch.len_utf8();
            } else {
                break;
            }
        }
        while s < e {
            let ch = text[s..e].chars().next_back().unwrap();
            if matches!(
                ch,
                ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '”' | '’' | '\'' | '`' | '.'
            ) {
                e -= ch.len_utf8();
            } else {
                break;
            }
        }
        if s < e {
            out.push(RawTok {
                start: s,
                end: e,
                text: &text[s..e],
            });
        }
    }
    out
}

enum FmtItem {
    Word(String),
    Vocab(String),
}

fn reconcile_raw_with_vocab(raw: &str, formatted: &str, vocabulary: &str) -> String {
    if vocabulary.trim().is_empty() || raw.is_empty() || formatted.is_empty() {
        return raw.to_owned();
    }
    let entries = crate::storage::vocab_words(vocabulary);
    if entries.is_empty() {
        return raw.to_owned();
    }

    // Locate exact configured vocab occurrences in `formatted` (longest-first, non-overlapping).
    let mut hits: Vec<(usize, usize, &str)> = Vec::new();
    for term in &entries {
        if term.is_empty() {
            continue;
        }
        let mut search_from = 0;
        while let Some(rel) = formatted[search_from..].find(term.as_str()) {
            let s = search_from + rel;
            let e = s + term.len();
            if has_whole_word_boundaries(formatted, s, e) {
                hits.push((s, e, term.as_str()));
            }
            let step = formatted[s..].chars().next().map_or(1, char::len_utf8);
            search_from = s + step;
        }
    }
    if hits.is_empty() {
        return raw.to_owned();
    }
    hits.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then_with(|| a.0.cmp(&b.0)));
    let mut chosen_hits: Vec<(usize, usize, &str)> = Vec::new();
    for (s, e, term) in hits {
        if chosen_hits.iter().any(|&(cs, ce, _)| s < ce && e > cs) {
            continue;
        }
        chosen_hits.push((s, e, term));
    }
    chosen_hits.sort_by_key(|&(s, _, _)| s);

    // Build ordered item stream of words and exact vocab hits from `formatted`.
    let mut fmt_items: Vec<FmtItem> = Vec::new();
    let mut pos = 0;
    for &(s, e, term) in &chosen_hits {
        for w in words(&formatted[pos..s]) {
            if !filler(&w) {
                fmt_items.push(FmtItem::Word(w));
            }
        }
        fmt_items.push(FmtItem::Vocab(term.to_owned()));
        pos = e;
    }
    for w in words(&formatted[pos..]) {
        if !filler(&w) {
            fmt_items.push(FmtItem::Word(w));
        }
    }

    let raw_tokens = tokenize_raw_spans(raw);
    if raw_tokens.is_empty() {
        return raw.to_owned();
    }

    let tok_matches_word = |tok: &RawTok<'_>, w: &str| {
        tok.text.to_lowercase().replace('’', "'") == w || words(tok.text).iter().any(|tw| tw == w)
    };

    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    let mut raw_cursor = 0usize;

    for (idx, item) in fmt_items.iter().enumerate() {
        match item {
            FmtItem::Word(w) => {
                let search_end = (raw_cursor + 12).min(raw_tokens.len());
                if let Some(rel) = raw_tokens[raw_cursor..search_end]
                    .iter()
                    .position(|tok| tok_matches_word(tok, w))
                {
                    raw_cursor += rel + 1;
                }
            }
            FmtItem::Vocab(term) => {
                let next_anchor = fmt_items[idx + 1..].iter().find_map(|it| match it {
                    FmtItem::Word(w) => Some(w.as_str()),
                    FmtItem::Vocab(_) => None,
                });
                let max_span = (words(term).len() + 2).max(3).min(5);
                let start_limit = (raw_cursor + 12).min(raw_tokens.len());

                // Best candidate: (anchor_preserved, is_exact_norm, neg_dist, neg_start, span_len, s, e)
                let mut best: Option<(bool, bool, usize, usize, usize, usize, usize)> = None;
                for s in raw_cursor..start_limit {
                    let e_limit = (s + max_span).min(raw_tokens.len());
                    for e in (s + 1)..=e_limit {
                        let span_start = raw_tokens[s].start;
                        let span_end = raw_tokens[e - 1].end;
                        let raw_span = &raw[span_start..span_end];
                        let Some((is_exact, dist)) = can_reconcile_vocab(raw_span, term) else {
                            continue;
                        };
                        let anchor_ok = next_anchor.is_none_or(|anc| {
                            let anc_end = (e + 12).min(raw_tokens.len());
                            raw_tokens[e..anc_end]
                                .iter()
                                .any(|tok| tok_matches_word(tok, anc))
                        });
                        let key = (
                            anchor_ok,
                            is_exact,
                            usize::MAX - dist,
                            usize::MAX - s,
                            e - s,
                        );
                        let better = best.is_none_or(|b| {
                            let bk = (b.0, b.1, b.2, b.3, b.4);
                            key > bk
                        });
                        if better {
                            best = Some((key.0, key.1, key.2, key.3, key.4, s, e));
                        }
                    }
                }
                if let Some((_, _, _, _, _, s, e)) = best {
                    let span_start = raw_tokens[s].start;
                    let span_end = raw_tokens[e - 1].end;
                    if &raw[span_start..span_end] != term.as_str() {
                        replacements.push((span_start, span_end, term.clone()));
                    }
                    raw_cursor = e;
                }
            }
        }
    }

    if replacements.is_empty() {
        return raw.to_owned();
    }
    replacements.sort_by_key(|&(s, _, _)| s);
    let mut out = String::with_capacity(raw.len() + 16);
    let mut cur = 0;
    for (s, e, repl) in replacements {
        if s < cur {
            continue;
        }
        out.push_str(&raw[cur..s]);
        out.push_str(&repl);
        cur = e;
    }
    out.push_str(&raw[cur..]);
    out
}

fn is_safe_strict(raw: &str, formatted: &str, vocabulary: &str) -> bool {
    if !tokens(&ARROW, raw).eq(tokens(&ARROW, formatted))
        || !technical_literals_match(raw, formatted)
    {
        return false;
    }
    let Some(mut lines) = flatten_tables(raw, formatted) else {
        return false;
    };
    strip_lists(&mut lines);
    let content = lines
        .iter()
        .filter(|line| !BORDER.is_match(line))
        .map(|line| {
            let line = line.trim();
            line.strip_prefix('|')
                .and_then(|inner| inner.strip_suffix('|'))
                .filter(|inner| !inner.is_empty() && !inner.contains('|'))
                .map_or(line, str::trim)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let context = operator_context(&content);
    if operator_context(raw) != context && operator_context(&without_comma_fillers(raw)) != context
    {
        return false;
    }
    if ARROW.is_match(raw)
        && !ARROW
            .split(raw)
            .map(words)
            .eq(ARROW.split(&content).map(words))
    {
        return false;
    }
    let final_code = inline_code(formatted);
    inline_code(raw)
        .iter()
        .all(|span| final_code.contains(span))
        && final_code
            .iter()
            .all(|(_, value)| raw.to_lowercase().contains(&value.to_lowercase()))
        && same_words(raw, &content, vocabulary)
}

pub fn is_safe(raw: &str, formatted: &str, vocabulary: &str) -> bool {
    if vocabulary.trim().is_empty() {
        return is_safe_strict(raw, formatted, "");
    }
    let reconciled = reconcile_raw_with_vocab(raw, formatted, vocabulary);
    is_safe_strict(&reconciled, formatted, vocabulary)
}

fn expand_contractions(input: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(input.len() + 4);
    for w in input {
        match w.as_str() {
            "don't" => out.extend(["do".into(), "not".into()]),
            "doesn't" => out.extend(["does".into(), "not".into()]),
            "didn't" => out.extend(["did".into(), "not".into()]),
            "can't" | "cannot" => out.extend(["can".into(), "not".into()]),
            "won't" => out.extend(["will".into(), "not".into()]),
            "wouldn't" => out.extend(["would".into(), "not".into()]),
            "couldn't" => out.extend(["could".into(), "not".into()]),
            "shouldn't" => out.extend(["should".into(), "not".into()]),
            "isn't" => out.extend(["is".into(), "not".into()]),
            "aren't" => out.extend(["are".into(), "not".into()]),
            "wasn't" => out.extend(["was".into(), "not".into()]),
            "weren't" => out.extend(["were".into(), "not".into()]),
            "hasn't" => out.extend(["has".into(), "not".into()]),
            "haven't" => out.extend(["have".into(), "not".into()]),
            "hadn't" => out.extend(["had".into(), "not".into()]),
            "mustn't" => out.extend(["must".into(), "not".into()]),
            "needn't" => out.extend(["need".into(), "not".into()]),
            "shan't" => out.extend(["shall".into(), "not".into()]),
            "i'm" => out.extend(["i".into(), "am".into()]),
            "i'll" => out.extend(["i".into(), "will".into()]),
            "i've" => out.extend(["i".into(), "have".into()]),
            "you're" => out.extend(["you".into(), "are".into()]),
            "you'll" => out.extend(["you".into(), "will".into()]),
            "you've" => out.extend(["you".into(), "have".into()]),
            "we're" => out.extend(["we".into(), "are".into()]),
            "we'll" => out.extend(["we".into(), "will".into()]),
            "we've" => out.extend(["we".into(), "have".into()]),
            "they're" => out.extend(["they".into(), "are".into()]),
            "they'll" => out.extend(["they".into(), "will".into()]),
            "they've" => out.extend(["they".into(), "have".into()]),
            "let's" => out.extend(["let".into(), "us".into()]),
            _ => out.push(w.clone()),
        }
    }
    out
}

fn is_ordered_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut hi = 0usize;
    for w in needle {
        let Some(rel) = haystack[hi..].iter().position(|h| h == w) else {
            return false;
        };
        hi += rel + 1;
    }
    true
}

fn count_contiguous_subseq(haystack: &[String], needle: &[String]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if haystack[i..i + needle.len()] == *needle {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    count
}

fn preserves_triggers(raw: &str, formatted: &str, preserved_triggers: &[&str]) -> bool {
    if preserved_triggers.is_empty() {
        return true;
    }
    let before = words(raw);
    let after = words(formatted);
    for trigger in preserved_triggers {
        let tw = words(trigger);
        if tw.is_empty() {
            continue;
        }
        let raw_count = count_contiguous_subseq(&before, &tw);
        if raw_count > 0 && count_contiguous_subseq(&after, &tw) < raw_count {
            return false;
        }
    }
    true
}

fn is_sentence_exclamation(text: &str, m: regex::Match<'_>) -> bool {
    if m.as_str() != "!" {
        return false;
    }
    let prev = text[..m.start()].chars().next_back();
    let next = text[m.end()..].chars().next();
    let prev_ok = prev.is_some_and(|c| {
        c.is_alphanumeric() || matches!(c, ')' | ']' | '"' | '\'' | '”' | '’')
    });
    let next_ok = next.is_none_or(char::is_whitespace);
    prev_ok && next_ok
}

fn operator_segments_match_tighten(source: &str, content: &str) -> bool {
    let src_ops: Vec<_> = OPERATOR.find_iter(source).collect();
    let cnt_ops: Vec<_> = OPERATOR.find_iter(content).collect();
    if src_ops.len() != cnt_ops.len()
        || !src_ops
            .iter()
            .zip(cnt_ops.iter())
            .all(|(a, b)| a.as_str() == b.as_str())
    {
        return false;
    }
    let seg_words = |seg: &str| -> Vec<String> {
        let mut w = expand_contractions(&words(seg));
        w.retain(|tok| !filler(tok));
        w
    };
    let split_segs = |text: &str, ops: &[regex::Match<'_>]| -> Vec<Vec<String>> {
        let mut out = Vec::with_capacity(ops.len() + 1);
        let mut pos = 0;
        for op in ops {
            out.push(seg_words(&text[pos..op.start()]));
            pos = op.end();
        }
        out.push(seg_words(&text[pos..]));
        out
    };
    let src_segs = split_segs(source, &src_ops);
    let cnt_segs = split_segs(content, &cnt_ops);
    for (i, (s, c)) in src_segs.iter().zip(cnt_segs.iter()).enumerate() {
        if !is_ordered_subsequence(c, s) {
            return false;
        }
        if !s.is_empty() && c.is_empty() {
            return false;
        }
        if i < src_ops.len()
            && !s.is_empty()
            && !(is_sentence_exclamation(source, src_ops[i])
                && is_sentence_exclamation(content, cnt_ops[i]))
        {
            if c.last() != s.last() {
                return false;
            }
        }
        if i > 0 && !s.is_empty() {
            if is_sentence_exclamation(source, src_ops[i - 1])
                && is_sentence_exclamation(content, cnt_ops[i - 1])
            {
                let s_trimmed_empty = s.iter().all(|w| is_tighten_filler(w));
                if !s_trimmed_empty && c.is_empty() {
                    return false;
                }
            } else if c.first() != s.first() {
                return false;
            }
        }
    }
    true
}

fn is_tighten_filler(w: &str) -> bool {
    matches!(
        w,
        "um" | "umm"
            | "uh"
            | "uhh"
            | "er"
            | "erm"
            | "hmm"
            | "ah"
            | "like"
            | "basically"
            | "actually"
            | "literally"
            | "just"
            | "so"
            | "well"
            | "honestly"
            | "seriously"
            | "obviously"
            | "anyway"
            | "okay"
            | "ok"
            | "really"
            | "very"
    )
}

fn strip_tighten_fillers(input: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if i + 1 < input.len() {
            let pair = (input[i].as_str(), input[i + 1].as_str());
            if matches!(
                pair,
                ("you", "know") | ("i", "mean") | ("sort", "of") | ("kind", "of")
            ) {
                i += 2;
                continue;
            }
        }
        if !is_tighten_filler(&input[i]) {
            out.push(input[i].clone());
        }
        i += 1;
    }
    out
}

fn dedup_stutters(input: &[String]) -> Vec<String> {
    let mut out = input.to_vec();
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        let mut next = Vec::with_capacity(out.len());
        while i < out.len() {
            let max_k = ((out.len() - i) / 2).min(6);
            let mut matched = 0;
            for k in (1..=max_k).rev() {
                if out[i..i + k] == out[i + k..i + 2 * k] {
                    matched = k;
                    break;
                }
            }
            if matched > 0 {
                let phrase = &out[i..i + matched];
                next.extend_from_slice(phrase);
                i += matched;
                while i + matched <= out.len() && &out[i..i + matched] == phrase {
                    i += matched;
                }
                changed = true;
            } else {
                next.push(out[i].clone());
                i += 1;
            }
        }
        out = next;
    }
    out
}

fn is_tighten_content_word(w: &str) -> bool {
    !is_tighten_filler(w) && !is_non_content_word(w)
}

fn is_safe_tighten(
    raw: &str,
    formatted: &str,
    vocabulary: &str,
    preserved_triggers: &[&str],
) -> bool {
    let reconciled_buf;
    let src = if vocabulary.trim().is_empty() {
        raw
    } else {
        reconciled_buf = reconcile_raw_with_vocab(raw, formatted, vocabulary);
        reconciled_buf.as_str()
    };

    if !tokens(&ARROW, src).eq(tokens(&ARROW, formatted))
        || !technical_literals_match(src, formatted)
    {
        return false;
    }
    let Some(mut lines) = flatten_tables(src, formatted) else {
        return false;
    };
    strip_lists(&mut lines);
    let content = lines
        .iter()
        .filter(|line| !BORDER.is_match(line))
        .map(|line| {
            let line = line.trim();
            line.strip_prefix('|')
                .and_then(|inner| inner.strip_suffix('|'))
                .filter(|inner| !inner.is_empty() && !inner.contains('|'))
                .map_or(line, str::trim)
        })
        .collect::<Vec<_>>()
        .join("\n");

    if !tokens(&OPERATOR, src).eq(tokens(&OPERATOR, &content)) {
        return false;
    }
    if OPERATOR.is_match(src) {
        let clean_src = without_comma_fillers(src);
        if !operator_segments_match_tighten(src, &content)
            && !operator_segments_match_tighten(&clean_src, &content)
        {
            return false;
        }
    }
    if ARROW.is_match(src) {
        let src_segs: Vec<Vec<String>> = ARROW
            .split(src)
            .map(|seg| expand_contractions(&words(seg)))
            .collect();
        let cnt_segs: Vec<Vec<String>> = ARROW
            .split(&content)
            .map(|seg| expand_contractions(&words(seg)))
            .collect();
        if src_segs.len() != cnt_segs.len()
            || !src_segs.iter().zip(cnt_segs.iter()).all(|(s, c)| {
                is_ordered_subsequence(c, s) && (s.is_empty() || !c.is_empty())
            })
        {
            return false;
        }
    }

    let final_code = inline_code(formatted);
    if !inline_code(src)
        .iter()
        .all(|span| final_code.contains(span))
        || !final_code
            .iter()
            .all(|(_, value)| src.to_lowercase().contains(&value.to_lowercase()))
    {
        return false;
    }

    let before = words(src);
    let after = words(&content);
    if before.is_empty() || after.is_empty() || after.len() > before.len() + 2 {
        return false;
    }
    if !tokens(&NUMBER, src).eq(tokens(&NUMBER, &content))
        || !technical_literals_match(src, &content)
        || (PREAMBLE.is_match(content.trim()) && !PREAMBLE.is_match(src.trim()))
    {
        return false;
    }

    if !preserves_triggers(src, &content, preserved_triggers) {
        return false;
    }

    let before_exp = expand_contractions(&before);
    let after_exp = expand_contractions(&after);

    if !before_exp
        .iter()
        .map(String::as_str)
        .filter(|w| is_negative_word(w))
        .eq(after_exp
            .iter()
            .map(String::as_str)
            .filter(|w| is_negative_word(w)))
    {
        return false;
    }

    // Preserve proper names (skipping fillers and sentence-initial non-content words) and configured vocab.
    let vocab_entries = crate::storage::vocab_words(vocabulary);
    let count = |in_words: &[String], word: &str| in_words.iter().filter(|v| *v == word).count();
    for m in NAME.find_iter(src) {
        let name = m.as_str();
        let lower = name.to_lowercase();
        if NAMED_FILLER.is_match(name) || is_tighten_filler(&lower) {
            continue;
        }
        let at_sent_start = m.start() == find_sentence_start(src, m.start())
            || src[find_sentence_start(src, m.start())..m.start()]
                .trim()
                .is_empty();
        if at_sent_start && is_non_content_word(name) {
            continue;
        }
        let terms = expand_contractions(&words(name));
        if terms
            .iter()
            .all(|word| count(&before_exp, word) >= count(&terms, word))
            && terms
                .iter()
                .any(|word| count(&after_exp, word) < count(&terms, word))
        {
            return false;
        }
    }
    for term in &vocab_entries {
        let terms = expand_contractions(&words(term));
        if terms
            .iter()
            .all(|word| count(&before_exp, word) >= count(&terms, word))
            && terms
                .iter()
                .any(|word| count(&after_exp, word) < count(&terms, word))
        {
            return false;
        }
    }

    // Strict ordered-subsequence condensation (zero invented words, preserves order).
    if !is_ordered_subsequence(&after_exp, &before_exp) {
        return false;
    }

    // Compute >= 55% content-word retention after removing fillers/hedges and multi-word stutters.
    let before_base = dedup_stutters(&strip_tighten_fillers(&before_exp));
    let before_content = before_base
        .iter()
        .filter(|w| is_tighten_content_word(w))
        .count();
    if before_content > 0 {
        let after_content = after_exp
            .iter()
            .filter(|w| is_tighten_content_word(w))
            .count();
        if after_content * 100 < before_content * 55 {
            return false;
        }
    } else if !before_base.is_empty() {
        let after_non_filler = after_exp.iter().filter(|w| !is_tighten_filler(w)).count();
        if after_non_filler * 100 < before_base.len() * 55 {
            return false;
        }
    }

    // If `src` itself contains list lines, forbid list-line regrouping.
    if src.lines().any(|line| list_item(line).is_some())
        && !keeps_list_lines_for_style(
            src,
            formatted,
            vocabulary,
            crate::storage::Style::Tighten,
            preserved_triggers,
        )
    {
        return false;
    }

    true
}

pub fn is_safe_for_style(
    raw: &str,
    formatted: &str,
    vocabulary: &str,
    style: crate::storage::Style,
    preserved_triggers: &[&str],
) -> bool {
    match style {
        crate::storage::Style::Clean => {
            is_safe(raw, formatted, vocabulary)
                && preserves_triggers(raw, formatted, preserved_triggers)
        }
        crate::storage::Style::Tighten => {
            is_safe_tighten(raw, formatted, vocabulary, preserved_triggers)
        }
    }
}

const COUNTS: [&str; 20] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
];
const ORDINALS: [&str; 20] = [
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
    "twentieth",
];

static LIST_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(?:I\s+(?:need|want|would\s+like)\s+(?:to\s+)?(?:(?:make|create|write)\s+)?",
        r"(?:a\s+|my\s+|the\s+)?|(?:please\s+)?(?:make|create|write)\s+(?:me\s+)?",
        r"(?:a\s+|my\s+|the\s+)?)(?P<kind>(?:shopping|grocery|number(?:ed)?|letter(?:ed)?|",
        r"bullet(?:ed)?)\s+list\b|list\s+for\s+(?:the\s+)?groceries\b)"
    ))
    .unwrap()
});
static LIST_COUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s+(?:with|of)\s+([a-z]+|[0-9]{1,2})\s+(items|options)\b").unwrap()
});
static LIST_SEPARATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([,.:!?])?\s*").unwrap());
static LIST_QUALIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s+or\s+so\b").unwrap());
static LIST_INTRO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(?:it['’]s\s+going\s+to\s+be|it\s+is\s+going\s+to\s+be|",
        r"the\s+items\s+are|they\s+are|here\s+are\s+the\s+items)\b"
    ))
    .unwrap()
});
static INCOMPLETE_INTRO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:it['’]s|it\s+is|the\s+items|they|here)\b").unwrap());
static SENTENCE_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?](?:\s|$)").unwrap());
static ORDINAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\b({})\s*,\s*", ORDINALS.join("|"))).unwrap());
static CONJUNCTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s+(?:and|or)\s+").unwrap());
static LIST_AMBIGUITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)["“”\x60]|(?:^|[^\w])['‘’]|['‘’](?:[^\w]|$)|\bscratch\s+that\b"#).unwrap()
});
static FOLLOWUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\boh\s*,\s*and\s+").unwrap());
static LIST_PREFIX_REFUSAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(?:not|never|no|without|don['’]t|doesn['’]t|cannot|can['’]t|explain|example|",
        r"quoted|quote|said|says?|reads?|mentioned|mentions?|wrote|written|whether|how|if|why|",
        r"word|phrase|label|called|named)\b"
    ))
    .unwrap()
});
static ITEM_START_REFUSAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:or|and|between|about|approximately|roughly|at\s+least|at\s+most|maybe)\b")
        .unwrap()
});
static ORDINAL_REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(?:word|term|phrase|label|called|named|says?|reads?|marked|printed|means?|",
        r"mentions?|mentioned)\s*[:,]?\s*$"
    ))
    .unwrap()
});

fn valid_item(text: &str) -> bool {
    WORD.is_match(text)
        && !matches!(
            text.trim_end_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase()
                .as_str(),
            "and" | "or"
        )
}

fn comma_items(body: &str, count: Option<usize>) -> Option<Vec<&str>> {
    let mut items: Vec<_> = body.split(',').map(str::trim).collect();
    if count == Some(items.len() + 1) {
        let last = items.pop()?;
        let mut boundaries = CONJUNCTION.find_iter(last);
        let split = boundaries.next()?.start();
        if boundaries.next().is_some() {
            return None;
        }
        items.extend([last[..split].trim_end(), last[split..].trim_start()]);
    }
    ((2..=20).contains(&items.len())
        && count.is_none_or(|count| count == items.len())
        && items.iter().all(|item| valid_item(item)))
    .then_some(items)
}

fn ordinal_items(body: &str, count: Option<usize>) -> Option<Vec<&str>> {
    let markers: Vec<_> = ORDINAL.captures_iter(body).collect();
    if !(2..=ORDINALS.len()).contains(&markers.len())
        || count.is_some_and(|count| count != markers.len())
        || markers.first()?.get(0)?.start() != 0
    {
        return None;
    }
    markers
        .iter()
        .enumerate()
        .map(|(index, marker)| {
            let range = marker.get(0)?;
            let start = range.start();
            let end = markers
                .get(index + 1)
                .and_then(|next| next.get(0))
                .map_or(body.len(), |next| next.start());
            let content = body[range.end()..end]
                .trim()
                .trim_end_matches(',')
                .trim_end();
            (marker[1].eq_ignore_ascii_case(ORDINALS[index])
                && !ORDINAL_REFERENCE.is_match(&body[..start])
                && valid_item(content))
            .then_some(body[start..end].trim().trim_end_matches(',').trim_end())
        })
        .collect()
}

fn split_list_body(body: &str, numbered: bool) -> (&str, &str, &str) {
    let mut markers = ORDINAL.captures_iter(body).peekable();
    let mut seen = 0;
    for stop in SENTENCE_END.find_iter(body) {
        let end = stop.start() + 1;
        if numbered {
            while markers
                .peek()
                .is_some_and(|marker| marker.get(0).unwrap().start() < end)
            {
                markers.next();
                seen += 1;
            }
            let rest = body[end..].trim_start();
            if let (Some(expected), Some(marker)) = (ORDINALS.get(seen), markers.peek()) {
                if marker.get(0).unwrap().start() == body.len() - rest.len()
                    && marker[1].eq_ignore_ascii_case(expected)
                {
                    continue;
                }
            }
        }
        return (&body[..end - 1], &body[end - 1..end], &body[end..]);
    }
    (body, "", "")
}

pub fn spoken_list(raw: &str, vocabulary: &str) -> Option<String> {
    // shortcut: handles explicit lists without corrections, until original-source span handling is ported.
    if raw.len() > 2_000_000
        || LIST_AMBIGUITY.is_match(raw)
        || raw.lines().any(|line| list_item(line).is_some())
    {
        return None;
    }
    let mut requests = LIST_REQUEST.captures_iter(raw);
    let request = requests.next()?;
    if requests.next().is_some() {
        return None;
    }
    let kind = request["kind"].to_ascii_lowercase();
    let numbered = kind.starts_with("number");
    let lettered = kind.starts_with("letter");
    let bulleted = kind.starts_with("bullet");
    let after_request = &raw[request.get(0)?.end()..];
    let (count, tail) = if let Some(declared) = LIST_COUNT.captures(after_request) {
        if !numbered && !lettered && !declared[2].eq_ignore_ascii_case("items") {
            return None;
        }
        let word = &declared[1];
        let count = word.parse::<usize>().ok().or_else(|| {
            COUNTS
                .iter()
                .position(|count| word.eq_ignore_ascii_case(count))
                .map(|n| n + 1)
        })?;
        if !(2..=20).contains(&count) {
            return None;
        }
        let tail = &after_request[declared.get(0)?.end()..];
        let qualifier_end = if !numbered && !lettered {
            LIST_QUALIFIER
                .find(tail)
                .map_or(0, |qualifier| qualifier.end())
        } else {
            0
        };
        (Some(count), &tail[qualifier_end..])
    } else {
        (None, after_request)
    };
    let separator = LIST_SEPARATOR.captures(tail)?;
    if count.is_none() {
        match separator.get(1).map(|stop| stop.as_str()) {
            _ if lettered => return None,
            Some(",") => return None,
            None if !numbered && !bulleted => return None,
            _ => {}
        }
    }
    let mut rest = &tail[separator.get(0)?.end()..];
    let heading = raw[..raw.len() - rest.len()].trim_end();
    let mut formatted = heading.to_owned();
    if count.is_some() && !numbered && !lettered {
        if let Some(intro) = LIST_INTRO.find(rest).filter(|intro| intro.start() == 0) {
            let intro_tail = &rest[intro.end()..];
            let end = intro.end() + LIST_SEPARATOR.find(intro_tail)?.end();
            formatted.push('\n');
            formatted.push_str(rest[..end].trim_end());
            rest = &rest[end..];
            if LIST_INTRO.is_match(rest) {
                return None;
            }
        } else if INCOMPLETE_INTRO.is_match(rest) {
            return None;
        }
    }
    if LIST_PREFIX_REFUSAL.is_match(&formatted) || ITEM_START_REFUSAL.is_match(rest) {
        return None;
    }
    let (body, stop, suffix, followup) = if count.is_some() && !numbered && !lettered {
        let mut cues = FOLLOWUP.find_iter(rest);
        if let Some(cue) = cues.next() {
            let (body, stop, between) = split_list_body(&rest[..cue.start()], false);
            let (added, ending, suffix) = split_list_body(&rest[cue.start()..], false);
            let content = added.get(cue.len()..)?.trim();
            if cues.next().is_some()
                || !between.trim().is_empty()
                || !valid_item(content)
                || content.contains(',')
                || OPERATOR.is_match(added)
                || technical_literals(added).next().is_some()
            {
                return None;
            }
            (body, stop, suffix, Some((added.trim_end(), ending)))
        } else {
            let (body, stop, suffix) = split_list_body(rest, false);
            (body, stop, suffix, None)
        }
    } else {
        let (body, stop, suffix) = split_list_body(rest, numbered);
        (body, stop, suffix, None)
    };
    if stop.is_empty() && count.is_none() && !numbered && !bulleted {
        return None;
    }
    if OPERATOR.is_match(body) || technical_literals(body).next().is_some() {
        return None;
    }
    let items = if numbered {
        ordinal_items(body.trim(), count)?
    } else {
        comma_items(body, count)?
    };
    for (index, item) in items.iter().enumerate() {
        formatted.push('\n');
        if numbered {
            formatted.push_str(&format!("{}. ", index + 1));
        } else if lettered {
            formatted.push(char::from(b'A' + index as u8));
            formatted.push_str(". ");
        } else {
            formatted.push_str("- ");
        }
        formatted.push_str(item);
    }
    formatted.push_str(stop);
    if let Some((added, ending)) = followup {
        formatted.push_str("\n- ");
        formatted.push_str(added);
        formatted.push_str(ending);
    }
    if !suffix.trim().is_empty() {
        formatted.push_str("\n\n");
        formatted.push_str(suffix);
    }
    is_safe(raw, &formatted, vocabulary).then_some(formatted)
}

fn list_item(line: &str) -> Option<&str> {
    if let Some(bullet) = BULLET.captures(line) {
        return bullet.get(1).map(|item| item.as_str());
    }
    ORDERED.captures(line)?.get(4).map(|item| item.as_str())
}

pub fn keeps_list_lines(local: &str, candidate: &str, vocabulary: &str) -> bool {
    keeps_list_lines_for_style(
        local,
        candidate,
        vocabulary,
        crate::storage::Style::Clean,
        &[],
    )
}

pub(crate) fn keeps_list_lines_for_style(
    local: &str,
    candidate: &str,
    vocabulary: &str,
    style: crate::storage::Style,
    preserved_triggers: &[&str],
) -> bool {
    let lines = |text: &str| text.lines().filter(|line| !line.trim().is_empty()).count();
    lines(local) == lines(candidate)
        && local
            .lines()
            .filter(|line| !line.trim().is_empty())
            .zip(candidate.lines().filter(|line| !line.trim().is_empty()))
            .all(
                |(before, after)| match (list_item(before), list_item(after)) {
                    (Some(item), Some(edited)) => {
                        is_safe_for_style(item, edited, vocabulary, style, preserved_triggers)
                    }
                    (None, None) => true,
                    _ => false,
                },
            )
}

/// Result of deterministic pre-formatting applied to a raw transcript before optional AI cleanup.
///
/// Callers (such as `engine.rs`) should feed [`Self::model_input`] to AI cleanup and validate
/// any model output with [`Self::is_safe_output`] (which validates against the self-corrected
/// source rather than the uncorrected raw transcript).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedDictation {
    /// Transcript after deterministic spoken self-corrections (equal to `raw` when none applied).
    pub corrected: String,
    /// Optional locally formatted spoken list derived from [`Self::corrected`].
    pub local_list: Option<String>,
    /// True when [`apply_self_corrections`] resolved at least one spoken self-correction.
    pub self_corrected: bool,
    /// True when any deterministic local transform (self-correction, spoken command, or vocab post-pass) modified the transcript.
    pub locally_edited: bool,
    /// True when a spoken scratch/delete command discarded the entire utterance.
    pub discarded: bool,
    vocabulary: String,
}

impl PreparedDictation {
    /// Text to pass into the AI cleanup model.
    pub fn model_input(&self) -> &str {
        self.local_list.as_deref().unwrap_or(&self.corrected)
    }

    /// Validates a model-cleaned candidate against the self-corrected source and any local list.
    pub fn is_safe_output(&self, candidate: &str, vocabulary: &str) -> bool {
        if self.discarded {
            return false;
        }
        is_safe(&self.corrected, candidate, vocabulary)
            && self
                .local_list
                .as_deref()
                .is_none_or(|list| keeps_list_lines(list, candidate, vocabulary))
    }

    /// Validates a model-cleaned candidate under the requested cleanup style while preserving
    /// any raw snippet trigger words matched before cleanup.
    pub fn is_safe_output_for_style(
        &self,
        formatted: &str,
        style: crate::storage::Style,
        preserved_triggers: &[&str],
    ) -> bool {
        if self.discarded {
            return false;
        }
        is_safe_for_style(
            &self.corrected,
            formatted,
            &self.vocabulary,
            style,
            preserved_triggers,
        ) && self.local_list.as_deref().is_none_or(|list| {
            keeps_list_lines_for_style(
                list,
                formatted,
                &self.vocabulary,
                style,
                preserved_triggers,
            )
        })
    }

    /// Local fallback text when AI cleanup is disabled, fails, or is rejected by `is_safe`.
    pub fn local_fallback(&self) -> Option<&str> {
        if self.discarded {
            return None;
        }
        self.local_list
            .as_deref()
            .or_else(|| self.locally_edited.then_some(self.corrected.as_str()))
    }
}

/// Runs deterministic spoken self-corrections, spoken commands, vocab post-pass, and explicit spoken-list formatting on `raw`.
pub fn prepare_dictation(raw: &str, vocabulary: &str) -> PreparedDictation {
    let after_self = apply_self_corrections(raw);
    let self_corrected = after_self != raw;
    let (after_cmds, cmds_changed, discarded) = apply_spoken_commands(&after_self);
    if discarded {
        return PreparedDictation {
            corrected: String::new(),
            local_list: None,
            self_corrected,
            locally_edited: true,
            discarded: true,
            vocabulary: vocabulary.to_owned(),
        };
    }
    let (corrected, vocab_changed) = apply_vocab_postpass(&after_cmds, vocabulary);
    let locally_edited = self_corrected || cmds_changed || vocab_changed;
    let local_list = spoken_list(&corrected, vocabulary);
    PreparedDictation {
        corrected,
        local_list,
        self_corrected,
        locally_edited,
        discarded: false,
        vocabulary: vocabulary.to_owned(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PivotStrength {
    /// Unambiguous multi-word correction cues (`actually make that`, `make that`, `no wait`, `wait no`).
    Explicit,
    /// Contextual cues (`sorry,`, `I mean`, `actually`) requiring stricter boundary and span verification.
    Contextual,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PivotKind {
    ActuallyMakeThat,
    MakeThat,
    NoWait,
    WaitNo,
    Sorry,
    IMean,
    Actually,
}

impl PivotKind {
    fn strength(self) -> PivotStrength {
        match self {
            Self::ActuallyMakeThat | Self::MakeThat | Self::NoWait | Self::WaitNo => {
                PivotStrength::Explicit
            }
            Self::Sorry | Self::IMean | Self::Actually => PivotStrength::Contextual,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PivotMatch {
    kind: PivotKind,
    /// Start of the separator + pivot region (where target must end).
    sep_start: usize,
    /// End of the pivot phrase + any immediately following comma/whitespace.
    repl_start: usize,
    /// Whether a clause/sentence separator preceded the pivot.
    has_comma_sep: bool,
    /// Whether a sentence-ending period preceded the pivot.
    has_period_sep: bool,
}

#[derive(Clone, Copy, Debug)]
struct SpanToken<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

static SELF_CORRECTION_PIVOT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)",
        r"(?P<sep>(?:\s*[,;—]\s*|\s+--?\s+|\s*\.\s+|\s+))",
        r"(?:",
        r"(?P<amt>actually\s*,?\s*make\s+that)\b|",
        r"(?P<mt>make\s+that)\b|",
        r"(?P<nw>no\s*,?\s*wait)\b|",
        r"(?P<wn>wait\s*,?\s*no)\b|",
        r"(?P<sr>sorry\s*,)|",
        r"(?P<im>i\s+mean)\b|",
        r"(?P<ac>actually)\b",
        r")",
        r"(?P<after_sep>\s*,?\s*)"
    ))
    .unwrap()
});

static FAST_PIVOT_HINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:actually|make\s+that|wait|sorry|i\s+mean)\b").unwrap()
});

static TIME_COMPACT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:[01]?\d|2[0-3]):[0-5]\d(?:\s*(?:a\.?m\.?|p\.?m\.?))?$|^(?:1[0-2]|0?[1-9])(?:a\.?m\.?|p\.?m\.?)$")
        .unwrap()
});

static NUMBER_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[$£€¥]?\d+(?:[,._]\d+)*(?:%|st|nd|rd|th)?$").unwrap()
});

static DATE_NUMERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{1,4}[/-]\d{1,2}(?:[/-]\d{1,4})?$").unwrap()
});

static TECHNICAL_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[^\s@]+@[^\s@]+\.[^\s@]+|https?://\S+|--[A-Za-z0-9][\w-]*|[A-Za-z0-9_-]+\.[A-Za-z0-9]{1,8}|[A-Za-z_]\w*_\w+)$")
        .unwrap()
});

fn tokenize_slice<'a>(text: &'a str, base_offset: usize) -> Vec<SpanToken<'a>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && text[i..].chars().next().is_some_and(char::is_whitespace) {
            i += text[i..].chars().next().unwrap().len_utf8();
        }
        if i >= bytes.len() {
            break;
        }
        let chunk_start = i;
        while i < bytes.len() && !text[i..].chars().next().is_some_and(char::is_whitespace) {
            i += text[i..].chars().next().unwrap().len_utf8();
        }
        let mut s = chunk_start;
        let mut e = i;
        // Trim leading opening punctuation.
        while s < e {
            let ch = text[s..e].chars().next().unwrap();
            if matches!(ch, '(' | '[' | '{' | '"' | '“' | '‘' | '\'') {
                s += ch.len_utf8();
            } else {
                break;
            }
        }
        // Trim trailing punctuation (keep trailing '.' for a.m./p.m. or Dr./Mr./Ms./Mrs.).
        while s < e {
            let ch = text[s..e].chars().next_back().unwrap();
            if matches!(ch, ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '”' | '’' | '\'') {
                e -= ch.len_utf8();
            } else if ch == '.' {
                let candidate = &text[s..e];
                let keep_dot = candidate.eq_ignore_ascii_case("a.m.")
                    || candidate.eq_ignore_ascii_case("p.m.")
                    || candidate.eq_ignore_ascii_case("dr.")
                    || candidate.eq_ignore_ascii_case("mr.")
                    || candidate.eq_ignore_ascii_case("ms.")
                    || candidate.eq_ignore_ascii_case("mrs.");
                if keep_dot {
                    break;
                }
                e -= 1;
            } else {
                break;
            }
        }
        if s < e {
            out.push(SpanToken {
                start: base_offset + s,
                end: base_offset + e,
                text: &text[s..e],
            });
        }
    }
    out
}

fn is_meridiem(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "am" | "pm" | "a.m." | "p.m." | "a.m" | "p.m" | "o'clock" | "o’clock"
    )
}

fn is_number_word(w: &str) -> bool {
    let lower = w.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "zero"
            | "one"
            | "two"
            | "three"
            | "four"
            | "five"
            | "six"
            | "seven"
            | "eight"
            | "nine"
            | "ten"
            | "eleven"
            | "twelve"
            | "thirteen"
            | "fourteen"
            | "fifteen"
            | "sixteen"
            | "seventeen"
            | "eighteen"
            | "nineteen"
            | "twenty"
            | "thirty"
            | "forty"
            | "fifty"
            | "sixty"
            | "seventy"
            | "eighty"
            | "ninety"
            | "hundred"
            | "thousand"
            | "million"
            | "billion"
            | "dozen"
            | "half"
            | "quarter"
            | "couple"
            | "first"
            | "second"
            | "third"
            | "fourth"
            | "fifth"
            | "sixth"
            | "seventh"
            | "eighth"
            | "ninth"
            | "tenth"
    )
}

fn is_number_like(w: &str) -> bool {
    NUMBER_TOKEN.is_match(w) || is_number_word(w)
}

fn is_hour_like(w: &str) -> bool {
    if let Ok(n) = w.parse::<u32>() {
        return (1..=12).contains(&n);
    }
    matches!(
        w.to_ascii_lowercase().as_str(),
        "one"
            | "two"
            | "three"
            | "four"
            | "five"
            | "six"
            | "seven"
            | "eight"
            | "nine"
            | "ten"
            | "eleven"
            | "twelve"
    )
}

fn is_day_word(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "monday"
            | "tuesday"
            | "wednesday"
            | "thursday"
            | "friday"
            | "saturday"
            | "sunday"
            | "mon"
            | "tue"
            | "tues"
            | "wed"
            | "thu"
            | "thur"
            | "thurs"
            | "fri"
            | "sat"
            | "sun"
            | "today"
            | "tomorrow"
            | "yesterday"
            | "tonight"
    )
}

fn is_month_word(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "january"
            | "february"
            | "march"
            | "april"
            | "may"
            | "june"
            | "july"
            | "august"
            | "september"
            | "october"
            | "november"
            | "december"
            | "jan"
            | "feb"
            | "mar"
            | "apr"
            | "jun"
            | "jul"
            | "aug"
            | "sep"
            | "sept"
            | "oct"
            | "nov"
            | "dec"
    )
}

fn is_time_modifier(w: &str) -> bool {
    matches!(w.to_ascii_lowercase().as_str(), "this" | "next" | "last")
}

fn is_anchor_word(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "at" | "on"
            | "in"
            | "to"
            | "for"
            | "from"
            | "with"
            | "by"
            | "of"
            | "into"
            | "about"
            | "over"
            | "under"
            | "after"
            | "before"
            | "between"
            | "through"
            | "the"
            | "a"
            | "an"
            | "my"
            | "your"
            | "our"
            | "his"
            | "her"
            | "their"
            | "this"
            | "that"
            | "these"
            | "those"
    )
}

fn is_non_content_word(w: &str) -> bool {
    let lower = w.to_ascii_lowercase().replace('’', "'");
    matches!(
        lower.as_str(),
        "i" | "i'm"
            | "i'll"
            | "i've"
            | "i'd"
            | "we"
            | "we're"
            | "we'll"
            | "we've"
            | "we'd"
            | "you"
            | "you're"
            | "you'll"
            | "you've"
            | "you'd"
            | "he"
            | "he's"
            | "she"
            | "she's"
            | "it"
            | "it's"
            | "they"
            | "they're"
            | "that"
            | "that's"
            | "this"
            | "these"
            | "those"
            | "there"
            | "there's"
            | "here"
            | "here's"
            | "the"
            | "a"
            | "an"
            | "and"
            | "or"
            | "but"
            | "so"
            | "if"
            | "when"
            | "while"
            | "because"
            | "though"
            | "although"
            | "however"
            | "then"
            | "than"
            | "as"
            | "at"
            | "on"
            | "in"
            | "to"
            | "for"
            | "from"
            | "with"
            | "by"
            | "of"
            | "about"
            | "into"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "am"
            | "have"
            | "has"
            | "had"
            | "do"
            | "does"
            | "did"
            | "don't"
            | "doesn't"
            | "didn't"
            | "can"
            | "can't"
            | "cannot"
            | "could"
            | "will"
            | "won't"
            | "would"
            | "should"
            | "may"
            | "might"
            | "must"
            | "let's"
            | "just"
            | "only"
            | "also"
            | "too"
            | "very"
            | "really"
            | "actually"
            | "basically"
            | "literally"
            | "honestly"
            | "seriously"
            | "obviously"
            | "probably"
            | "maybe"
            | "perhaps"
            | "anyway"
            | "well"
            | "okay"
            | "ok"
            | "sure"
            | "please"
            | "thanks"
            | "thank"
            | "sorry"
            | "wait"
            | "mean"
            | "know"
            | "think"
            | "said"
            | "say"
            | "says"
            | "told"
            | "tell"
            | "asked"
            | "ask"
            | "right"
            | "yes"
            | "yeah"
            | "no"
            | "not"
            | "never"
            | "fine"
            | "good"
            | "great"
            | "what"
            | "who"
            | "where"
            | "why"
            | "how"
            | "which"
            | "my"
            | "your"
            | "our"
            | "his"
            | "her"
            | "their"
            | "me"
            | "us"
            | "him"
            | "them"
    )
}

fn is_proper_name_token(w: &str) -> bool {
    let Some(first) = w.chars().next() else {
        return false;
    };
    if !first.is_uppercase() {
        return false;
    }
    if is_non_content_word(w) || is_day_word(w) || is_month_word(w) || is_meridiem(w) {
        return false;
    }
    // Require at least two letters or a title like "Dr." so lone "A"/"I" don't qualify.
    w.chars().filter(|c| c.is_alphabetic()).count() >= 2
}

fn is_proper_name_span(tokens: &[SpanToken<'_>]) -> bool {
    (1..=3).contains(&tokens.len()) && tokens.iter().all(|t| is_proper_name_token(t.text))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SpanKind {
    Time,
    Day,
    MonthOrDate,
    Number,
    Technical,
    ProperName,
}

fn classify_span(tokens: &[SpanToken<'_>]) -> Option<SpanKind> {
    match tokens.len() {
        1 => {
            let t = tokens[0].text;
            let lower = t.to_ascii_lowercase();
            if TIME_COMPACT.is_match(t) || matches!(lower.as_str(), "noon" | "midnight") {
                return Some(SpanKind::Time);
            }
            if is_day_word(t) {
                return Some(SpanKind::Day);
            }
            if is_month_word(t) || DATE_NUMERIC.is_match(t) {
                return Some(SpanKind::MonthOrDate);
            }
            if is_number_like(t) {
                return Some(SpanKind::Number);
            }
            if TECHNICAL_TOKEN.is_match(t) {
                return Some(SpanKind::Technical);
            }
            if is_proper_name_token(t) {
                return Some(SpanKind::ProperName);
            }
            None
        }
        2 => {
            let (a, b) = (tokens[0].text, tokens[1].text);
            if (is_hour_like(a) || TIME_COMPACT.is_match(a)) && is_meridiem(b) {
                return Some(SpanKind::Time);
            }
            if is_time_modifier(a) && is_day_word(b) {
                return Some(SpanKind::Day);
            }
            if (is_month_word(a) && is_number_like(b))
                || (is_time_modifier(a) && is_month_word(b))
            {
                return Some(SpanKind::MonthOrDate);
            }
            if is_number_word(a) && is_number_word(b) {
                return Some(SpanKind::Number);
            }
            if is_proper_name_span(tokens) {
                return Some(SpanKind::ProperName);
            }
            None
        }
        3 => {
            let (a, b, c) = (tokens[0].text, tokens[1].text, tokens[2].text);
            if matches!(a.to_ascii_lowercase().as_str(), "half" | "quarter")
                && matches!(b.to_ascii_lowercase().as_str(), "past" | "to" | "after")
                && is_hour_like(c)
            {
                return Some(SpanKind::Time);
            }
            if is_number_like(a) && b.eq_ignore_ascii_case("of") && is_month_word(c) {
                return Some(SpanKind::MonthOrDate);
            }
            if is_proper_name_span(tokens) {
                return Some(SpanKind::ProperName);
            }
            None
        }
        _ => None,
    }
}

fn is_contrast_pair(a: &str, b: &str) -> bool {
    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    if a == b {
        return false;
    }
    const PAIRS: &[(&str, &str)] = &[
        ("left", "right"),
        ("up", "down"),
        ("top", "bottom"),
        ("north", "south"),
        ("east", "west"),
        ("before", "after"),
        ("start", "end"),
        ("open", "close"),
        ("on", "off"),
        ("in", "out"),
        ("yes", "no"),
        ("true", "false"),
        ("more", "less"),
        ("bigger", "smaller"),
        ("earlier", "later"),
        ("higher", "lower"),
        ("faster", "slower"),
        ("plus", "minus"),
        ("add", "remove"),
        ("enable", "disable"),
        ("dark", "light"),
        ("black", "white"),
        ("morning", "afternoon"),
        ("morning", "evening"),
        ("afternoon", "evening"),
        ("am", "pm"),
    ];
    if PAIRS
        .iter()
        .any(|&(x, y)| (a == x && b == y) || (a == y && b == x))
    {
        return true;
    }
    const COLORS: &[&str] = &[
        "red", "blue", "green", "yellow", "orange", "purple", "pink", "black", "white", "gray",
        "grey", "brown",
    ];
    COLORS.contains(&a.as_str()) && COLORS.contains(&b.as_str())
}

fn same_head_noun(a: &str, b: &str) -> bool {
    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    if a == b {
        return true;
    }
    let singular_plural = |short: &str, long: &str| {
        long.strip_prefix(short)
            .is_some_and(|suffix| matches!(suffix, "s" | "es"))
    };
    (a.len() >= 2 && singular_plural(&a, &b)) || (b.len() >= 2 && singular_plural(&b, &a))
}

fn is_valid_modifier_token(w: &str) -> bool {
    is_number_like(w) || is_proper_name_token(w) || !is_non_content_word(w)
}

fn spans_match(
    target: &[SpanToken<'_>],
    repl: &[SpanToken<'_>],
    pivot: PivotMatch,
    before_total_tokens: usize,
    repl_is_full_clause: bool,
) -> bool {
    if target.is_empty() || repl.is_empty() {
        return false;
    }
    // Reject if the target starts at sentence position 0 and is a generic non-content sentence starter.
    let target_at_sentence_start = target.len() == before_total_tokens;
    if target_at_sentence_start && is_non_content_word(target[0].text) {
        return false;
    }

    // 1. Direct typed value match (Time, Day, MonthOrDate, Number, Technical, ProperName).
    if let (Some(tk), Some(rk)) = (classify_span(target), classify_span(repl)) {
        if tk == rk {
            // ProperName without a separator is too risky unless explicit pivot.
            if tk == SpanKind::ProperName
                && !pivot.has_comma_sep
                && !pivot.has_period_sep
                && pivot.kind.strength() != PivotStrength::Explicit
            {
                return false;
            }
            // Never treat a sentence-initial capitalized verb/word as part of a ProperName target
            // (e.g. in "Call Alex, I mean Jordan Lee", "Call" is the verb, not part of "Alex").
            if tk == SpanKind::ProperName && target_at_sentence_start {
                if target.len() >= 2 || !repl_is_full_clause {
                    return false;
                }
            }
            return true;
        }
    }

    // 2. Shared leading anchor (e.g. "at 4 PM" <-> "at 3 PM", "to Alex" <-> "to Jordan", "the red car" <-> "the blue car").
    if target.len() >= 2
        && repl.len() >= 2
        && is_anchor_word(target[0].text)
        && target[0].text.eq_ignore_ascii_case(repl[0].text)
    {
        let inner_target = &target[1..];
        let inner_repl = &repl[1..];
        if spans_match(
            inner_target,
            inner_repl,
            pivot,
            before_total_tokens.saturating_sub(1),
            repl_is_full_clause,
        ) {
            return true;
        }
        // Allow short single-word content replacement inside identical anchor ("in the kitchen" <-> "in the garage").
        if (pivot.has_comma_sep || pivot.kind.strength() == PivotStrength::Explicit)
            && inner_target.len() == 1
            && inner_repl.len() == 1
            && !is_non_content_word(inner_target[0].text)
            && !is_non_content_word(inner_repl[0].text)
        {
            return true;
        }
    }

    // 3. Shared trailing head word(s) with compatible leading modifier(s):
    //    e.g. "5 boxes" <-> "8 boxes", "1 box" <-> "3 boxes", "red apples" <-> "green apples".
    if target.len() >= 2
        && repl.len() >= 2
        && (pivot.has_comma_sep || pivot.has_period_sep || pivot.kind.strength() == PivotStrength::Explicit)
    {
        let mut shared = 0usize;
        while shared < target.len() - 1 && shared < repl.len() - 1 {
            let tw = target[target.len() - 1 - shared].text;
            let rw = repl[repl.len() - 1 - shared].text;
            let eq = if shared == 0 {
                same_head_noun(tw, rw) && !is_non_content_word(tw)
            } else {
                tw.eq_ignore_ascii_case(rw)
            };
            if eq {
                shared += 1;
            } else {
                break;
            }
        }
        if shared > 0 {
            let t_mod = &target[..target.len() - shared];
            let r_mod = &repl[..repl.len() - shared];
            let same_number_kind =
                classify_span(t_mod) == Some(SpanKind::Number)
                    && classify_span(r_mod) == Some(SpanKind::Number);
            let same_len_modifiers = t_mod.len() == r_mod.len()
                && t_mod.len() <= 2
                && t_mod.iter().all(|t| is_valid_modifier_token(t.text))
                && r_mod.iter().all(|t| is_valid_modifier_token(t.text));
            // Do not swallow an unrelated sentence-initial verb when modifiers differ in length.
            if same_number_kind || (same_len_modifiers && (!target_at_sentence_start || t_mod.len() == 1)) {
                return true;
            }
        }
    }

    // 4. Explicit contrast/antonym pair ("left" <-> "right", "red" <-> "blue", etc.).
    if target.len() == 1
        && repl.len() == 1
        && (pivot.has_comma_sep || pivot.kind.strength() == PivotStrength::Explicit)
        && is_contrast_pair(target[0].text, repl[0].text)
    {
        return true;
    }

    // 5. Explicit-pivot single content-word replacement when the replacement is the entire clause:
    //    e.g. "Pick up apples, sorry, oranges." or "Buy coffee, no wait tea."
    if target.len() == 1
        && repl.len() == 1
        && repl_is_full_clause
        && (pivot.kind.strength() == PivotStrength::Explicit || pivot.kind == PivotKind::Sorry)
        && (pivot.has_comma_sep || pivot.has_period_sep)
        && !target[0].text.eq_ignore_ascii_case(repl[0].text)
        && !is_non_content_word(target[0].text)
        && !is_non_content_word(repl[0].text)
    {
        return true;
    }

    false
}

fn find_sentence_start(text: &str, before_pos: usize) -> usize {
    let slice = &text[..before_pos];
    let mut last_boundary = 0usize;
    let bytes = slice.as_bytes();
    for (idx, ch) in slice.char_indices() {
        if matches!(ch, '\n' | '!' | '?') {
            last_boundary = idx + ch.len_utf8();
        } else if ch == '.' {
            // Only treat '.' as a sentence boundary if followed by whitespace (not in "4.5", "a.m.", "foo.rs", "Dr.").
            let next_is_ws = bytes
                .get(idx + 1)
                .is_some_and(|b| b.is_ascii_whitespace());
            if next_is_ws {
                let before_dot = slice[..idx].rsplit(|c: char| c.is_whitespace()).next().unwrap_or("");
                let is_abbrev = matches!(
                    before_dot.to_ascii_lowercase().as_str(),
                    "dr" | "mr" | "ms" | "mrs" | "a.m" | "p.m"
                );
                if !is_abbrev {
                    last_boundary = idx + 1;
                }
            }
        }
    }
    last_boundary
}

fn find_after_clause_end(text: &str, from: usize) -> usize {
    let slice = &text[from..];
    let mut end = slice.len();

    // Stop at any clause/sentence terminator or newline.
    for (idx, ch) in slice.char_indices() {
        if matches!(ch, ',' | ';' | '!' | '?' | '\n' | '—') {
            end = idx;
            break;
        }
        if ch == '.' {
            let next_byte = slice.as_bytes().get(idx + 1).copied();
            let at_end_or_ws = next_byte.is_none_or(|b| b.is_ascii_whitespace());
            if at_end_or_ws {
                let word = slice[..idx]
                    .rsplit(|c: char| c.is_whitespace())
                    .next()
                    .unwrap_or("");
                let is_abbrev = matches!(
                    word.to_ascii_lowercase().as_str(),
                    "dr" | "mr" | "ms" | "mrs" | "a.m" | "p.m"
                );
                if !is_abbrev {
                    end = idx;
                    break;
                }
            }
        }
    }

    // Also stop before a subsequent self-correction pivot in the same clause.
    if let Some(next_pivot) = SELF_CORRECTION_PIVOT.find(&slice[..end]) {
        end = next_pivot.start();
    }

    from + end
}

fn pivot_from_capture(text: &str, caps: &regex::Captures<'_>) -> Option<PivotMatch> {
    let sep = caps.name("sep")?;
    let sep_str = sep.as_str();
    let has_period_sep = sep_str.contains('.');
    let has_comma_sep = sep_str.contains([',', ';', '—']) || sep_str.contains('-');
    let (kind, pivot_m) = if let Some(m) = caps.name("amt") {
        (PivotKind::ActuallyMakeThat, m)
    } else if let Some(m) = caps.name("mt") {
        (PivotKind::MakeThat, m)
    } else if let Some(m) = caps.name("nw") {
        (PivotKind::NoWait, m)
    } else if let Some(m) = caps.name("wn") {
        (PivotKind::WaitNo, m)
    } else if let Some(m) = caps.name("sr") {
        (PivotKind::Sorry, m)
    } else if let Some(m) = caps.name("im") {
        (PivotKind::IMean, m)
    } else if let Some(m) = caps.name("ac") {
        (PivotKind::Actually, m)
    } else {
        return None;
    };

    let full = caps.get(0)?;
    let repl_start = full.end();

    // Guard 1: "make that" without a separator can be normal grammar ("5 boxes make that heavy").
    if kind == PivotKind::MakeThat && !has_comma_sep && !has_period_sep {
        return None;
    }

    // Guard 2: "sorry," requires a comma/period separator before it and must not be an apology ("I'm sorry,").
    if kind == PivotKind::Sorry {
        if !has_comma_sep && !has_period_sep {
            return None;
        }
        let prev_word = text[..sep.start()]
            .rsplit(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
            .find(|w| !w.is_empty())
            .unwrap_or("")
            .to_ascii_lowercase()
            .replace('’', "'");
        if matches!(
            prev_word.as_str(),
            "i'm"
                | "im"
                | "am"
                | "is"
                | "are"
                | "was"
                | "were"
                | "be"
                | "feel"
                | "felt"
                | "so"
                | "very"
                | "really"
                | "truly"
                | "deeply"
                | "we're"
                | "you're"
        ) {
            return None;
        }
    }

    // Guard 3: "I mean" requires a separator (not "what I mean" or "I mean well").
    if kind == PivotKind::IMean {
        if !has_comma_sep && !has_period_sep {
            return None;
        }
        let prev_word = text[..sep.start()]
            .rsplit(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
            .find(|w| !w.is_empty())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(prev_word.as_str(), "what" | "you" | "do" | "does" | "did") {
            return None;
        }
    }

    // Guard 4: "no wait" must not be the verb "wait for/until/here/a second".
    if kind == PivotKind::NoWait {
        let after_sep = caps.name("after_sep").map_or("", |m| m.as_str());
        if !after_sep.contains(',') {
            let next_word = text[repl_start..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            if matches!(next_word.as_str(), "for" | "until" | "here" | "there" | "up") {
                return None;
            }
        }
    }

    // Guard 5: "wait no" must not match idioms like "wait, no problem / no one / no worries".
    if kind == PivotKind::WaitNo {
        let after_sep = caps.name("after_sep").map_or("", |m| m.as_str());
        if !after_sep.contains(',') {
            let next_word = text[repl_start..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            if matches!(
                next_word.as_str(),
                "problem"
                    | "worries"
                    | "rush"
                    | "hurry"
                    | "need"
                    | "one"
                    | "way"
                    | "thanks"
                    | "idea"
                    | "chance"
                    | "doubt"
                    | "longer"
                    | "more"
                    | "matter"
            ) {
                return None;
            }
        }
    }

    // Guard 6: Skip pivots inside quotes or inline code backticks.
    let prefix = &text[..pivot_m.start()];
    if prefix.chars().filter(|&c| c == '`').count() % 2 == 1
        || prefix.chars().filter(|&c| c == '"').count() % 2 == 1
    {
        return None;
    }

    Some(PivotMatch {
        kind,
        sep_start: sep.start(),
        repl_start,
        has_comma_sep,
        has_period_sep,
    })
}

fn is_continuation_starter(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "at" | "on"
            | "in"
            | "by"
            | "before"
            | "after"
            | "until"
            | "to"
            | "for"
            | "from"
            | "with"
            | "around"
            | "about"
            | "near"
            | "through"
            | "during"
            | "today"
            | "tomorrow"
            | "tonight"
            | "now"
            | "soon"
            | "early"
            | "late"
            | "please"
            | "instead"
            | "right"
            | "and"
            | "or"
            | "but"
            | "if"
            | "when"
            | "because"
            | "so"
            | "then"
    )
}

fn try_apply_one_self_correction(text: &str) -> Option<String> {
    for caps in SELF_CORRECTION_PIVOT.captures_iter(text) {
        let Some(pivot) = pivot_from_capture(text, &caps) else {
            continue;
        };

        let sent_start = find_sentence_start(text, pivot.sep_start);
        let before_slice = &text[sent_start..pivot.sep_start];
        let before_tokens = tokenize_slice(before_slice, sent_start);
        if before_tokens.is_empty() {
            continue;
        }

        // Ensure the last token in `before` ends cleanly at the separator (no closing paren/quote/bracket in between).
        let last_before = *before_tokens.last().unwrap();
        if !text[last_before.end..pivot.sep_start].trim().is_empty() {
            continue;
        }

        let after_end = find_after_clause_end(text, pivot.repl_start);
        let after_slice = &text[pivot.repl_start..after_end];
        let after_tokens = tokenize_slice(after_slice, pivot.repl_start);
        if after_tokens.is_empty() {
            continue;
        }

        // For contextual pivots (`actually`, `I mean`, `sorry,`) preceded by a period,
        // require the replacement to be a compact value fragment (<= 3 tokens) matching the full clause.
        if pivot.has_period_sep
            && pivot.kind.strength() == PivotStrength::Contextual
            && after_tokens.len() > 3
        {
            continue;
        }

        let max_repl = after_tokens.len().min(5);
        let max_target = before_tokens.len().min(5);

        // Search longest replacement span first, then longest target suffix.
        let mut best: Option<(usize, usize)> = None;
        'search: for repl_len in (1..=max_repl).rev() {
            let repl = &after_tokens[..repl_len];
            let repl_is_full_clause = repl_len == after_tokens.len();
            // If more words remain in the same clause after the replacement, only allow clean
            // adverbial/prepositional/conjunction continuations so we never splice a subject verb
            // clause (e.g. "4 PM, actually 3 PM works better" or "Alex, I mean Jordan knows").
            if !repl_is_full_clause && !is_continuation_starter(after_tokens[repl_len].text) {
                continue;
            }
            for target_len in (1..=max_target).rev() {
                let target = &before_tokens[before_tokens.len() - target_len..];
                if spans_match(target, repl, pivot, before_tokens.len(), repl_is_full_clause) {
                    // For separator-less `actually`, require strict non-name typed value match (Time/Day/Month/Number).
                    if pivot.kind == PivotKind::Actually && !pivot.has_comma_sep && !pivot.has_period_sep {
                        let strict_typed = matches!(
                            (classify_span(target), classify_span(repl)),
                            (
                                Some(SpanKind::Time | SpanKind::Day | SpanKind::MonthOrDate | SpanKind::Number),
                                Some(SpanKind::Time | SpanKind::Day | SpanKind::MonthOrDate | SpanKind::Number)
                            )
                        );
                        if !strict_typed {
                            continue;
                        }
                    }
                    best = Some((target_len, repl_len));
                    break 'search;
                }
            }
        }

        let Some((target_len, repl_len)) = best else {
            continue;
        };

        let target_start = before_tokens[before_tokens.len() - target_len].start;
        let repl_span_start = after_tokens[0].start;
        let repl_span_end = after_tokens[repl_len - 1].end;
        let mut replacement = text[repl_span_start..repl_span_end].to_owned();

        // Preserve sentence-start capitalization when replacing the very first word of a sentence.
        if target_len == before_tokens.len()
            && text[target_start..].chars().next().is_some_and(char::is_uppercase)
            && replacement.chars().next().is_some_and(char::is_lowercase)
        {
            let mut chars = replacement.chars();
            if let Some(first) = chars.next() {
                replacement = format!("{}{}", first.to_uppercase(), chars.as_str());
            }
        }

        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..target_start]);
        out.push_str(&replacement);
        out.push_str(&text[repl_span_end..]);
        return Some(out);
    }
    None
}

/// Resolves natural mid-sentence spoken self-corrections deterministically using verified span matching.
///
/// Handles pivots such as `"actually make that"`, `"make that"`, `"no wait"`, `"no, wait"`,
/// `"wait no"`, `"sorry,"`, `"I mean"`, and `"actually"` only when preceded by a matching target
/// value/clause and followed by a clear replacement. Leaves sentence-initial pivots, normal adverb
/// uses, apologies, and incomplete pivots untouched.
pub fn apply_self_corrections(raw: &str) -> String {
    if raw.is_empty() || raw.len() > 2_000_000 || !FAST_PIVOT_HINT.is_match(raw) {
        return raw.to_owned();
    }
    let mut current = raw.to_owned();
    for _ in 0..16 {
        let Some(next) = try_apply_one_self_correction(&current) else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn is_inside_quotes_or_backticks(text: &str, cmd_start: usize, cmd_end: usize) -> bool {
    let prefix = &text[..cmd_start];
    if prefix.chars().filter(|&c| c == '`').count() % 2 == 1
        || prefix.chars().filter(|&c| c == '"').count() % 2 == 1
    {
        return true;
    }
    let open_curly = prefix.chars().filter(|&c| c == '“').count();
    let close_curly = prefix.chars().filter(|&c| c == '”').count();
    if open_curly > close_curly {
        return true;
    }
    let left = prefix.trim_end().chars().next_back();
    let right = text[cmd_end..].trim_start().chars().next();
    if matches!(left, Some('\'' | '‘')) && matches!(right, Some('\'' | '’')) {
        return true;
    }
    false
}

fn capitalize_leading_word(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => {
            format!("{}{}", first.to_uppercase(), chars.as_str())
        }
        _ => s.to_owned(),
    }
}

fn prev_word_in_clause(before: &str) -> Option<String> {
    let trimmed = before.trim_end_matches(|c: char| c == ' ' || c == '\t' || c == ',');
    if trimmed.ends_with(['.', '!', '?', ';', '—', '\n', '\r']) {
        return None;
    }
    trimmed
        .rsplit(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .find(|w| !w.is_empty())
        .map(|w| w.to_lowercase().replace('’', "'"))
}

fn next_immediate_word(after: &str) -> Option<String> {
    let trimmed = after.trim_start_matches([' ', '\t']);
    let first_ch = trimmed.chars().next()?;
    if !first_ch.is_alphanumeric() {
        return None;
    }
    trimmed
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .next()
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase().replace('’', "'"))
}

fn apply_newline_commands(text: &str) -> (String, bool) {
    if !NEWLINE_CMD.is_match(text) {
        return (text.to_owned(), false);
    }
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut changed = false;
    for caps in NEWLINE_CMD.captures_iter(text) {
        let full = caps.get(0).unwrap();
        if full.start() < pos {
            continue;
        }
        let cmd = caps.name("cmd").unwrap();
        let pre = caps.name("pre").unwrap();
        let post = caps.name("post").unwrap();
        let is_paragraph = cmd.as_str().to_ascii_lowercase().contains("paragraph");

        if is_inside_quotes_or_backticks(text, cmd.start(), cmd.end()) {
            continue;
        }

        // Reject when preceded by determiners/quantifiers.
        if let Some(prev) = prev_word_in_clause(&text[..pre.start()]) {
            if matches!(
                prev.as_str(),
                "a" | "an"
                    | "the"
                    | "this"
                    | "that"
                    | "my"
                    | "your"
                    | "our"
                    | "one"
                    | "each"
                    | "every"
                    | "another"
                    | "next"
                    | "last"
                    | "same"
            ) {
                continue;
            }
        }

        // Reject when immediately followed by noun-complements.
        if let Some(next) = next_immediate_word(&text[cmd.end()..]) {
            if matches!(
                next.as_str(),
                "of" | "in"
                    | "for"
                    | "about"
                    | "character"
                    | "break"
                    | "is"
                    | "was"
                    | "are"
                    | "starts"
                    | "begins"
                    | "here"
                    | "there"
                    | "at"
                    | "to"
                    | "with"
                    | "from"
                    | "after"
                    | "before"
            ) {
                continue;
            }
        }

        let after_rest = &text[post.end()..];
        let trailing_period = after_rest.trim().is_empty() && post.as_str().contains('.');
        let consume_end = post.end();

        let left_slice = &text[pos..pre.start()];
        out.push_str(left_slice);
        if trailing_period {
            let left_trimmed = left_slice.trim_end();
            if !left_trimmed.is_empty() && !left_trimmed.ends_with(['.', '!', '?']) {
                out.push('.');
            }
        }
        if is_paragraph {
            out.push_str("\n\n");
        } else {
            out.push('\n');
        }
        changed = true;
        pos = consume_end;

        if is_paragraph {
            let rem = &text[pos..];
            if let Some(first_ch) = rem.chars().next() {
                if first_ch.is_lowercase() {
                    for uc in first_ch.to_uppercase() {
                        out.push(uc);
                    }
                    pos += first_ch.len_utf8();
                }
            }
        }
    }
    if !changed {
        return (text.to_owned(), false);
    }
    out.push_str(&text[pos..]);
    (out, true)
}

fn try_targeted_scratch_replacement(
    text: &str,
    sep_start: usize,
    repl_start: usize,
    has_comma_sep: bool,
    has_period_sep: bool,
) -> Option<String> {
    let pivot = PivotMatch {
        kind: PivotKind::ActuallyMakeThat,
        sep_start,
        repl_start,
        has_comma_sep,
        has_period_sep,
    };
    let sent_start = find_sentence_start(text, sep_start);
    let before_slice = &text[sent_start..sep_start];
    let before_tokens = tokenize_slice(before_slice, sent_start);
    if before_tokens.is_empty() {
        return None;
    }
    let last_before = *before_tokens.last().unwrap();
    if !text[last_before.end..sep_start].trim().is_empty() {
        return None;
    }
    let after_end = find_after_clause_end(text, repl_start);
    let after_slice = &text[repl_start..after_end];
    let after_tokens = tokenize_slice(after_slice, repl_start);
    if after_tokens.is_empty() {
        return None;
    }

    let max_repl = after_tokens.len().min(5);
    let max_target = before_tokens.len().min(5);
    let mut best: Option<(usize, usize)> = None;
    'search: for repl_len in (1..=max_repl).rev() {
        let repl = &after_tokens[..repl_len];
        let repl_is_full_clause = repl_len == after_tokens.len();
        if !repl_is_full_clause && !is_continuation_starter(after_tokens[repl_len].text) {
            continue;
        }
        for target_len in (1..=max_target).rev() {
            let target = &before_tokens[before_tokens.len() - target_len..];
            if spans_match(target, repl, pivot, before_tokens.len(), repl_is_full_clause) {
                best = Some((target_len, repl_len));
                break 'search;
            }
        }
    }
    let (target_len, repl_len) = best?;
    let target_start = before_tokens[before_tokens.len() - target_len].start;
    let repl_span_start = after_tokens[0].start;
    let repl_span_end = after_tokens[repl_len - 1].end;
    let mut replacement = text[repl_span_start..repl_span_end].to_owned();
    if target_len == before_tokens.len()
        && text[target_start..].chars().next().is_some_and(char::is_uppercase)
        && replacement.chars().next().is_some_and(char::is_lowercase)
    {
        replacement = capitalize_leading_word(&replacement);
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..target_start]);
    out.push_str(&replacement);
    out.push_str(&text[repl_span_end..]);
    Some(out)
}

fn try_apply_one_scratch_command(text: &str) -> Option<String> {
    for caps in SCRATCH_CMD.captures_iter(text) {
        let sep = caps.name("sep")?;
        let cmd = caps.name("cmd")?;
        let after = caps.name("after")?;
        let is_delete = cmd.as_str().to_ascii_lowercase().starts_with("delete");

        if is_inside_quotes_or_backticks(text, cmd.start(), cmd.end()) {
            continue;
        }

        // Must have some spoken content before the command to scratch/delete.
        if !WORD.is_match(&text[..sep.start()]) {
            continue;
        }

        // Reject when preceded by modal/aux/infinitive/negation/subject words in the same clause.
        if let Some(prev) = prev_word_in_clause(&text[..cmd.start()]) {
            if matches!(
                prev.as_str(),
                "to" | "don't"
                    | "not"
                    | "never"
                    | "can"
                    | "can't"
                    | "cannot"
                    | "could"
                    | "should"
                    | "would"
                    | "will"
                    | "won't"
                    | "must"
                    | "please"
                    | "want"
                    | "need"
                    | "let's"
                    | "you"
                    | "we"
                    | "i"
                    | "they"
            ) {
                continue;
            }
        }

        let sep_str = sep.as_str();
        let has_period_sep = sep_str.contains(['.', '!', '?']);
        let has_comma_sep = sep_str.contains([',', ';', '—']) || sep_str.contains('-');
        let remainder = &text[after.end()..];
        let at_end = remainder.trim().is_empty();
        let after_has_punct = after
            .as_str()
            .contains([',', '.', ';', '!', '?']);

        // Extra guard for `delete that`: reject demonstrative objects like "delete that file",
        // questions like "Did Bob delete that?", and mid-clause prose like "clicks delete that, it breaks."
        if is_delete {
            if after.as_str().contains('?') && !has_comma_sep && !has_period_sep {
                continue;
            }
            if !at_end && !after_has_punct {
                let next = next_immediate_word(&text[cmd.end()..]).unwrap_or_default();
                let is_clause_restarter = matches!(
                    next.as_str(),
                    "i" | "i'm"
                        | "i'll"
                        | "i've"
                        | "i'd"
                        | "we"
                        | "we're"
                        | "we'll"
                        | "let's"
                        | "set"
                        | "send"
                        | "call"
                        | "make"
                        | "meet"
                        | "order"
                        | "book"
                        | "email"
                        | "put"
                        | "run"
                        | "check"
                        | "use"
                        | "add"
                        | "write"
                        | "tell"
                        | "ask"
                        | "try"
                        | "actually"
                );
                if !is_clause_restarter {
                    continue;
                }
            } else if !at_end && !has_comma_sep && !has_period_sep {
                continue;
            }
        }

        // 1. Try targeted self-correction span replacement first.
        if let Some(targeted) = try_targeted_scratch_replacement(
            text,
            sep.start(),
            after.end(),
            has_comma_sep,
            has_period_sep,
        ) {
            return Some(targeted);
        }

        // 2. Otherwise drop back to the last sentence boundary (. ! ? \n).
        let sent_start = find_sentence_start(text, sep.start());
        let dropped_segment = text[sent_start..sep.start()].trim_start();
        let was_capitalized = dropped_segment
            .chars()
            .next()
            .is_some_and(char::is_uppercase);
        let prefix = &text[..sent_start];
        let mut rest = remainder.trim_start().to_owned();
        if !rest.is_empty() && was_capitalized {
            rest = capitalize_leading_word(&rest);
        }

        let out = if prefix.trim().is_empty() {
            rest
        } else if rest.is_empty() {
            prefix.trim_end().to_owned()
        } else if prefix.ends_with('\n') {
            format!("{prefix}{rest}")
        } else {
            format!("{} {rest}", prefix.trim_end())
        };
        return Some(out);
    }
    None
}

fn apply_spoken_commands(raw: &str) -> (String, bool, bool) {
    if raw.is_empty() || raw.len() > 2_000_000 {
        return (raw.to_owned(), false, false);
    }
    let (mut current, mut changed) = apply_newline_commands(raw);
    let mut scratch_applied = false;
    for _ in 0..16 {
        let Some(next) = try_apply_one_scratch_command(&current) else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
        changed = true;
        scratch_applied = true;
        if current.trim().is_empty() {
            return (String::new(), true, true);
        }
    }
    let discarded = scratch_applied && current.trim().is_empty();
    (current, changed, discarded)
}

#[cfg(test)]
mod list_tests {
    use super::*;

    #[test]
    fn explicit_lists_match_the_current_reference() {
        for (spoken, expected) in [
            (
                "I need a shopping list with 4 items, 2 apples, 3 bananas, 1 carton of milk, and 6 eggs",
                "I need a shopping list with 4 items,\n- 2 apples\n- 3 bananas\n- 1 carton of milk\n- and 6 eggs",
            ),
            (
                "Make a grocery list: apples, pears, mac and cheese.",
                "Make a grocery list:\n- apples\n- pears\n- mac and cheese.",
            ),
            (
                "Make a numbered list First, wash. Second, dry. Third, put everything away. Keep this sentence.",
                "Make a numbered list\n1. First, wash.\n2. Second, dry.\n3. Third, put everything away.\n\n Keep this sentence.",
            ),
            (
                "Make a letter list of three options. Take the bus, ride a bicycle, or walk.",
                "Make a letter list of three options.\nA. Take the bus\nB. ride a bicycle\nC. or walk.",
            ),
            (
                "I need a shopping list with three items or so. It’s going to be café apples, pears, and milk.",
                "I need a shopping list with three items or so.\nIt’s going to be\n- café apples\n- pears\n- and milk.",
            ),
            (
                "I need a shopping list with three items: apples, bananas, and milk. Oh, and some ice cream. Keep the receipt.",
                "I need a shopping list with three items:\n- apples\n- bananas\n- and milk.\n- Oh, and some ice cream.\n\n Keep the receipt.",
            ),
        ] {
            assert_eq!(spoken_list(spoken, "").as_deref(), Some(expected), "{spoken}");
            assert_eq!(spoken_list(expected, ""), None, "{expected}");
        }
    }

    #[test]
    fn explicit_lists_leave_ambiguous_or_technical_dictation_alone() {
        for raw in [
            "She said make a shopping list: apples, bananas, milk.",
            "I don’t want to make a shopping list: apples, bananas, milk.",
            "I need a shopping list with three items or four apples, bananas, milk.",
            "Make a shopping list with four items apples, bananas, pears and",
            "I need a shopping list with three items. The items were apples, bananas, milk.",
            "Make a numbered list First, wash Third, dry",
            "Make a numbered list First, wash Second, discuss the term third, before leaving.",
            "Make a shopping list with three items: apples, bananas, scratch that pears, and milk.",
            "Make a shopping list with three items: apples, bananas. Oh, and bread.",
            r"Make a shopping list: apples, C:\work\cart.txt, milk.",
        ] {
            assert_eq!(spoken_list(raw, ""), None, "{raw}");
        }
    }

    #[test]
    fn self_corrections_resolve_natural_spoken_pivots() {
        for (raw, expected) in [
            // Required core examples
            ("Let's meet at 4 PM, actually 3 PM", "Let's meet at 3 PM"),
            (
                "Let's meet at 4 PM, actually make that 3 PM",
                "Let's meet at 3 PM",
            ),
            (
                "Send it on Tuesday, no wait Wednesday",
                "Send it on Wednesday",
            ),
            ("Order 5 boxes, make that 8 boxes", "Order 8 boxes"),
            ("Call Alex, I mean Jordan", "Call Jordan"),
            // Additional pivot variants & punctuation
            (
                "Send it on Tuesday, no, wait, Wednesday.",
                "Send it on Wednesday.",
            ),
            (
                "Send it on Tuesday, wait no, Wednesday.",
                "Send it on Wednesday.",
            ),
            ("Call Alex, sorry, Jordan!", "Call Jordan!"),
            (
                "Let's meet at 4 PM. Actually make that 3 PM.",
                "Let's meet at 3 PM.",
            ),
            (
                "Let's meet at 4 PM, actually at 3 PM, in room 2.",
                "Let's meet at 3 PM, in room 2.",
            ),
            (
                "Send it on Tuesday, no wait Wednesday at noon.",
                "Send it on Wednesday at noon.",
            ),
            (
                "Order 1 box, make that 3 boxes, please.",
                "Order 3 boxes, please.",
            ),
            (
                "Order 5 big boxes, make that 8 big boxes.",
                "Order 8 big boxes.",
            ),
            ("Five boxes, make that eight boxes.", "Eight boxes."),
            ("Call Alex, I mean Jordan Lee.", "Call Jordan Lee."),
            ("Call Alex Smith, I mean Jordan Lee.", "Call Jordan Lee."),
            (
                "Meet at 2 PM, no wait 3 PM, actually make that 4 PM.",
                "Meet at 4 PM.",
            ),
            (
                "Book it for March 5, actually April 8.",
                "Book it for April 8.",
            ),
            ("Pay $50, make that $75.", "Pay $75."),
            ("Turn left, wait no, right.", "Turn right."),
            ("Pick up apples, sorry, oranges.", "Pick up oranges."),
            (
                "Email alice@example.com, sorry, bob@example.com.",
                "Email bob@example.com.",
            ),
            (
                "Buy the red shirt, no wait, the blue shirt.",
                "Buy the blue shirt.",
            ),
            (
                "Meet at 4 PM, actually 3 PM. Call Alex, I mean Jordan.",
                "Meet at 3 PM. Call Jordan.",
            ),
        ] {
            assert_eq!(apply_self_corrections(raw), expected, "{raw}");
        }
    }

    #[test]
    fn self_corrections_preserve_normal_prose_and_safety_invariants() {
        for untouched in [
            // Sentence-initial pivots
            "Actually, let's meet tomorrow.",
            "Actually let's meet tomorrow",
            "I mean, it's fine.",
            "I mean it's fine",
            "No, wait, don't go.",
            "No wait, let me check.",
            "Wait no, that's wrong.",
            "Sorry, I missed your call.",
            "Make that a double.",
            // Normal adverb / verb uses
            "It actually works well.",
            "I actually like this.",
            "I mean well.",
            "Do you know what I mean?",
            "You know what I mean, right?",
            "We should make that happen.",
            "5 boxes make that heavy.",
            // Apologies & idioms
            "I'm sorry, Alex, I can't make it.",
            "I am sorry, 3 PM doesn't work.",
            "We are sorry, this happened.",
            "She said no, wait for me.",
            "We can wait, no problem.",
            "Please wait, no one is here.",
            // Missing or non-matching replacement
            "Let's meet at 4 PM, actually.",
            "Let's meet at 4 PM, actually",
            "Let's meet at 4 PM, no wait.",
            "Let's meet at 4 PM, make that",
            "Call Alex, I mean.",
            "Let's meet at 4 PM, actually, never mind.",
            "Let's meet at 4 PM, actually 3 PM works better.",
            "Call Alex, I mean, he knows the code.",
            "Call Alex, I mean, Jordan knows the code.",
            "We finished the sprint. Actually, let's meet tomorrow.",
            "We met Alex. Actually, Jordan called me.",
            // Quoted / code spans
            "Use `4 PM, actually 3 PM` verbatim.",
            "She wrote \"Order 5 boxes, make that 8 boxes\" in the doc.",
        ] {
            assert_eq!(apply_self_corrections(untouched), untouched, "{untouched}");
        }
    }

    #[test]
    fn prepare_dictation_integrates_self_corrections_with_is_safe_and_lists() {
        let prepared = prepare_dictation("Let's meet at 4 PM, actually make that 3 PM", "");
        assert!(prepared.self_corrected);
        assert_eq!(prepared.corrected, "Let's meet at 3 PM");
        assert_eq!(prepared.model_input(), "Let's meet at 3 PM");
        assert_eq!(prepared.local_fallback(), Some("Let's meet at 3 PM"));
        // AI cleanup adding punctuation validates safely against the self-corrected source.
        assert!(prepared.is_safe_output("Let's meet at 3 PM.", ""));
        // AI cleanup hallucinating a different hour is still rejected by is_safe.
        assert!(!prepared.is_safe_output("Let's meet at 2 PM.", ""));

        // Unchanged dictation reports self_corrected = false and no local fallback.
        let plain = prepare_dictation("It actually works well.", "");
        assert!(!plain.self_corrected);
        assert_eq!(plain.corrected, "It actually works well.");
        assert_eq!(plain.local_fallback(), None);

        // Self-correction before spoken list formatting works cleanly together.
        let list_prepared = prepare_dictation(
            "Make a grocery list: apples, pears, sorry, peaches.",
            "",
        );
        assert!(list_prepared.self_corrected);
        assert_eq!(
            list_prepared.local_list.as_deref(),
            Some("Make a grocery list:\n- apples\n- peaches.")
        );
        assert!(list_prepared.is_safe_output(
            "Make a grocery list:\n- apples\n- peaches.",
            ""
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Style;

    #[test]
    fn vocab_deterministic_postpass_and_reconciled_is_safe() {
        let vocab = "Ollama, CLAUDE.md, OpenCode, listen-rs, Next.js, Rust, Git, Claude";

        // Deterministic post-pass fixes exact case-insensitive and structured separator-collapsed matches.
        let cases = [
            ("run ollama locally", "run Ollama locally"),
            ("update claude md first", "update CLAUDE.md first"),
            ("launch open code now", "launch OpenCode now"),
            ("build listen rs", "build listen-rs"),
            ("deploy next.js app", "deploy Next.js app"),
            // Exact casing inside backticks or URLs is skipped.
            ("Run `ollama` and check ollama", "Run `ollama` and check Ollama"),
            (
                "Visit https://ollama.com for ollama",
                "Visit https://ollama.com for Ollama",
            ),
        ];
        for (raw, expected) in cases {
            let prep = prepare_dictation(raw, vocab);
            assert_eq!(prep.corrected, expected, "raw={raw}");
            assert!(prep.locally_edited, "expected locally_edited for {raw}");
            assert!(!prep.discarded);
        }

        // Deterministic pass NEVER fuzzy-matches common words like must -> Rust, get -> Git, olama -> Ollama.
        for untouched in ["must fix this", "get it done", "check olama logs"] {
            let prep = prepare_dictation(untouched, vocab);
            assert_eq!(prep.corrected, untouched, "must not touch {untouched}");
            assert!(!prep.locally_edited);
        }

        // Reconciled-Raw is_safe accepts verified vocab corrections (fuzzy >=4 chars & exact norm).
        for (raw, formatted) in [
            ("check olama logs", "Check Ollama logs."),
            ("check Olama logs", "Check Ollama logs."),
            ("check oh lama logs", "Check Ollama logs."),
            ("update claude md", "Update CLAUDE.md."),
            ("launch open code", "Launch OpenCode."),
            ("build listen rs", "Build listen-rs."),
        ] {
            assert!(
                is_safe(raw, formatted, vocab),
                "expected is_safe to accept {raw:?} -> {formatted:?}"
            );
        }

        // Anti-hallucination rejections remain strictly enforced.
        for (raw, formatted) in [
            ("I like dogs", "I like Ollama"),
            ("send the mail", "send the Claude"),
            ("get it", "Git it"),
            ("must fix", "Rust fix"),
            ("just fix", "Rust fix"),
            (r"check olama in C:\a\b.py", "check Ollama in C:/a/b.py"),
        ] {
            assert!(
                !is_safe(raw, formatted, vocab),
                "expected is_safe to reject {raw:?} -> {formatted:?}"
            );
        }

        // Empty vocabulary preserves 100% strict behavior.
        assert!(!is_safe("check olama logs", "Check Ollama logs.", ""));
    }

    #[test]
    fn spoken_commands_apply_safely_and_preserve_prose() {
        // Accepted spoken commands.
        let p1 = prepare_dictation("the build passed new paragraph now let's test the UI", "");
        assert!(p1.locally_edited);
        assert!(!p1.discarded);
        assert_eq!(p1.corrected, "the build passed\n\nNow let's test the UI");
        assert!(p1.is_safe_output("The build passed.\n\nNow let's test the UI.", ""));

        let p2 = prepare_dictation("set the port to 8080 scratch that set the port to 3000", "");
        assert!(p2.locally_edited);
        assert!(!p2.discarded);
        assert_eq!(p2.corrected, "set the port to 3000");
        assert!(p2.is_safe_output("Set the port to 3000.", ""));

        let p3 = prepare_dictation("Hello new line world.", "");
        assert!(p3.locally_edited);
        assert!(!p3.discarded);
        assert_eq!(p3.corrected, "Hello\nworld.");

        let p4 = prepare_dictation("Hi Alice. Send at 4 PM, scratch that, send at 5 PM.", "");
        assert!(p4.locally_edited);
        assert!(!p4.discarded);
        assert_eq!(p4.corrected, "Hi Alice. Send at 5 PM.");

        // Prose guards: all of these MUST remain untouched.
        for prose in [
            "Add a new line of code.",
            "Write a new paragraph about testing.",
            "The new line is faster.",
            "Don't scratch that.",
            "Please delete that file.",
            "We should not delete that.",
            "Use `new line` here.",
        ] {
            let p = prepare_dictation(prose, "");
            assert_eq!(p.corrected, prose, "prose must stay untouched: {prose}");
            assert!(!p.locally_edited, "locally_edited must be false for: {prose}");
            assert!(!p.discarded, "discarded must be false for: {prose}");
        }

        // Full-utterance scratch sets discarded = true and never falls back to raw.
        let discarded = prepare_dictation("Send at 4 PM scratch that", "");
        assert!(discarded.discarded);
        assert!(discarded.locally_edited);
        assert!(discarded.corrected.is_empty());
        assert_eq!(discarded.local_fallback(), None);
        assert!(!discarded.is_safe_output("Send at 4 PM scratch that", ""));

        // Preserve formatting.rs:1936 contract: spoken_list returns None on ambiguous scratch that.
        assert_eq!(
            spoken_list(
                "Make a shopping list with three items: apples, bananas, scratch that pears, and milk.",
                ""
            ),
            None
        );
    }

    #[test]
    fn style_tighten_condenses_fillers_but_enforces_hard_invariants() {
        // Accepts filler/hedge removal and multi-word stutter dedup.
        assert!(is_safe_for_style(
            "Um so I basically just want to ship it",
            "I want to ship it.",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(is_safe_for_style(
            "I want to I want to ship",
            "I want to ship.",
            "",
            Style::Tighten,
            &[]
        ));
        // Safe contraction mapping preserving negation count.
        assert!(is_safe_for_style(
            "Do not delete the file",
            "Don't delete the file.",
            "",
            Style::Tighten,
            &[]
        ));

        // Hard invariants remain strictly rejected under Style::Tighten.
        assert!(!is_safe_for_style("3 PM", "2 PM", "", Style::Tighten, &[]));
        assert!(!is_safe_for_style(
            "Do not delete",
            "Delete",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(!is_safe_for_style(
            "I like it",
            "I like this.",
            "",
            Style::Tighten,
            &[]
        ));

        // Rejects list line regrouping (`- apples mac`).
        let list_prep = prepare_dictation("Make a grocery list: apples, pears, mac and cheese.", "");
        assert!(!list_prep.is_safe_output_for_style(
            "Make a grocery list:\n- apples mac\n- pears and cheese.",
            Style::Tighten,
            &[]
        ));
        assert!(!is_safe_for_style(
            "- apples\n- pears\n- mac and cheese",
            "- apples mac\n- pears and cheese",
            "",
            Style::Tighten,
            &[]
        ));

        // Rejects dropping preserved snippet trigger words.
        assert!(!is_safe_for_style(
            "Please send it to my email soon",
            "Send it to email soon.",
            "",
            Style::Tighten,
            &["my email"]
        ));
        assert!(is_safe_for_style(
            "Um basically send it to my email soon",
            "Send it to my email soon.",
            "",
            Style::Tighten,
            &["my email"]
        ));

        // Style::Clean stays strict.
        assert!(!is_safe_for_style(
            "Um so I basically just want to ship it",
            "I want to ship it.",
            "",
            Style::Clean,
            &[]
        ));
    }

    #[test]
    fn technical_tone_backtick_casing_passes_is_safe() {
        assert!(is_safe("Run Cargo Test", "Run `cargo test`", ""));
        assert!(is_safe("Edit readme.md", "Edit `README.md`", ""));

        // Unbackticked path/literal corruption or case change outside vocab is still rejected.
        assert!(!is_safe("Edit readme.md", "Edit README.md", ""));
        assert!(!is_safe(r"open C:\work\app.py", "open C:/work/app.py", ""));
        assert!(!is_safe(
            r"open C:\work\app.py",
            "open `C:/work/app.py`",
            ""
        ));
        assert!(!is_safe("run --dry-run", "run --dry_run", ""));
        // Already-backticked spans in raw still require exact case preservation.
        assert!(!is_safe("Edit `readme.md`", "Edit `README.md`", ""));
    }

    #[test]
    fn vocab_postpass_preserves_flags_paths_filenames_and_emails() {
        let vocab = "Ollama, Config, CLAUDE.md, listen-rs";

        // CLI flags, file paths, dotted filenames, and emails must NOT be partially rewritten.
        for untouched in [
            "Pass --ollama-host to the CLI",
            r"Open C:\work\ollama\app.py now",
            "Edit config.json before restarting",
            "Email alice@ollama.com about it",
        ] {
            let prep = prepare_dictation(untouched, vocab);
            assert_eq!(prep.corrected, untouched, "must preserve: {untouched}");
            assert!(!prep.locally_edited, "must not mark edited: {untouched}");
        }

        // Standalone vocab words and full-literal vocab entries still rewrite cleanly.
        let standalone = prepare_dictation("check ollama and --ollama-host", vocab);
        assert_eq!(standalone.corrected, "check Ollama and --ollama-host");
        assert!(standalone.locally_edited);

        let full_lit = prepare_dictation("update claude.md and listen-rs", vocab);
        assert_eq!(full_lit.corrected, "update CLAUDE.md and listen-rs");
        assert!(full_lit.locally_edited);

        let full_lit_hyphen = prepare_dictation("build Listen-RS now", vocab);
        assert_eq!(full_lit_hyphen.corrected, "build listen-rs now");
        assert!(full_lit_hyphen.locally_edited);
    }

    #[test]
    fn scratch_command_handles_question_and_exclamation_boundaries() {
        let q = prepare_dictation("Are you coming? Scratch that.", "");
        assert!(q.discarded, "expected discarded for question scratch");
        assert_eq!(q.corrected, "");

        let ex = prepare_dictation("Wait for me! Scratch that.", "");
        assert!(ex.discarded, "expected discarded for exclamation scratch");
        assert_eq!(ex.corrected, "");

        let keep_prev = prepare_dictation("Hi Alice. Are you coming? Scratch that.", "");
        assert!(!keep_prev.discarded);
        assert!(keep_prev.locally_edited);
        assert_eq!(keep_prev.corrected, "Hi Alice.");

        let replace_q = prepare_dictation("What time is it? Scratch that, what day is it?", "");
        assert!(!replace_q.discarded);
        assert!(replace_q.locally_edited);
        assert_eq!(replace_q.corrected, "What day is it?");
    }

    #[test]
    fn style_tighten_preserves_operator_operands() {
        // Dropping operands bordering operators must be rejected under Style::Tighten.
        assert!(!is_safe_for_style(
            "Check the value of x + y",
            "Check the value of x +",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(!is_safe_for_style(
            "Compute a = b + c and print result",
            "Compute a = + c and print result",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(!is_safe_for_style(
            "Check if x <= y before shipping",
            "Check if x <= before shipping",
            "",
            Style::Tighten,
            &[]
        ));

        // Valid filler tightening around operators remains accepted.
        assert!(is_safe_for_style(
            "um basically check the value of x + y",
            "Check the value of x + y",
            "",
            Style::Tighten,
            &[]
        ));
    }

    #[test]
    fn newline_commands_at_end_do_not_emit_orphan_period() {
        let para = prepare_dictation("The build passed new paragraph.", "");
        assert!(para.locally_edited);
        assert!(!para.discarded);
        assert_eq!(para.corrected, "The build passed.\n\n");
        assert!(!para.corrected.ends_with('.'));

        let line = prepare_dictation("Hello new line.", "");
        assert!(line.locally_edited);
        assert!(!line.discarded);
        assert_eq!(line.corrected, "Hello.\n");
        assert!(!line.corrected.ends_with('.'));

        // Already punctuated sentence before trailing newline command must not double-dot.
        let already = prepare_dictation("The build passed. New paragraph.", "");
        assert_eq!(already.corrected, "The build passed.\n\n");
    }

    #[test]
    fn delete_that_prose_and_question_guards() {
        for prose in [
            "If someone clicks delete that, it breaks.",
            "Did Bob delete that?",
        ] {
            let p = prepare_dictation(prose, "");
            assert_eq!(p.corrected, prose, "prose must remain untouched: {prose}");
            assert!(!p.locally_edited);
            assert!(!p.discarded);
        }

        let exec_end = prepare_dictation("Send it to Bob. Delete that.", "");
        assert!(exec_end.discarded);
        assert_eq!(exec_end.corrected, "");

        let exec_mid = prepare_dictation("Send it to Bob, delete that, send it to Alice.", "");
        assert!(!exec_mid.discarded);
        assert!(exec_mid.locally_edited);
        assert_eq!(exec_mid.corrected, "Send it to Alice.");
    }

    #[test]
    fn style_tighten_allows_filler_removal_next_to_operators_and_exclamations() {
        assert!(is_safe_for_style(
            "Great! Um, let's ship it",
            "Great! Let's ship it.",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(is_safe_for_style(
            "It costs, um, $50",
            "It costs $50.",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(is_safe_for_style(
            "It costs, you know, $50",
            "It costs $50.",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(is_safe_for_style(
            "Check (um, the logs)",
            "Check (the logs)",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(is_safe_for_style(
            "Check if x <= uh y",
            "Check if x <= y",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(is_safe_for_style(
            "We shipped it! So I basically want to celebrate",
            "We shipped it! I want to celebrate.",
            "",
            Style::Tighten,
            &[]
        ));
        // Unary !ident and != still pin their adjacent operands.
        assert!(!is_safe_for_style(
            "Check !flag before exit",
            "Check ! before exit",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(!is_safe_for_style(
            "Check if x != y before exit",
            "Check if x != before exit",
            "",
            Style::Tighten,
            &[]
        ));
    }

    #[test]
    fn vocab_reconciliation_never_bypasses_flags_operators_or_technical_literals() {
        assert!(!is_safe("pass --config", "pass Config", "Config"));
        assert!(!is_safe("compute x - y", "compute xy", "xy"));
        assert!(!is_safe("open config.json", "open ConfigJSON", "ConfigJSON"));
        assert!(!is_safe("use -v5", "use v5", "v5"));
        // Legitimate vocab normalization remains accepted.
        assert!(is_safe("open claude.md", "open CLAUDE.md", "CLAUDE.md"));
        assert!(is_safe("open claude md", "open CLAUDE.md", "CLAUDE.md"));
        assert!(is_safe("use open code", "use OpenCode", "OpenCode"));
    }

    #[test]
    fn scratch_command_handles_repeated_terminal_punctuation() {
        let exclam = prepare_dictation("No way!! Scratch that.", "");
        assert!(exclam.discarded);
        assert_eq!(exclam.corrected, "");

        let interrobang = prepare_dictation("Are you sure?! Scratch that.", "");
        assert!(interrobang.discarded);
        assert_eq!(interrobang.corrected, "");

        let second = prepare_dictation("Hi Alice. Wait?! Scratch that.", "");
        assert!(!second.discarded);
        assert!(second.locally_edited);
        assert_eq!(second.corrected, "Hi Alice.");

        // Sentences ending in guarded prose words before sentence punctuation must still scratch.
        for discarded_case in [
            "Where are you? Scratch that.",
            "I can't. Scratch that.",
            "Yes, please! Scratch that.",
            "No, I don't. Scratch that.",
            "No way!! Scratch that!!",
            "Send at 4 PM. Scratch that...",
        ] {
            let p = prepare_dictation(discarded_case, "");
            assert!(p.discarded, "expected discard for: {discarded_case}");
            assert_eq!(p.corrected, "", "expected empty for: {discarded_case}");
        }

        let after_guarded_prev = prepare_dictation("Hi Alice. What do you want? Scratch that.", "");
        assert!(!after_guarded_prev.discarded);
        assert_eq!(after_guarded_prev.corrected, "Hi Alice.");

        // Multi-token flags, negative numbers, and fuzzy technical literals must not reconcile.
        assert!(!is_safe("run git -c", "run GitC", "GitC"));
        assert!(!is_safe("set offset -5", "set Offset5", "Offset5"));
        assert!(!is_safe("open config.js", "open ConfigJSON", "ConfigJSON"));
        assert!(!is_safe("edit user_name", "edit Usernames", "Usernames"));

        // Sentence exclamation check must hold on BOTH source and content (no factorial/unary repurpose).
        assert!(!is_safe_for_style(
            "Compute n! for k",
            "Compute ! for k",
            "",
            Style::Tighten,
            &[]
        ));
        assert!(!is_safe_for_style(
            "We want it! Check flag",
            "We want !Check flag",
            "",
            Style::Tighten,
            &[]
        ));
    }
}
