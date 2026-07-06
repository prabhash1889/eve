//! Deterministic text transforms applied around the LLM polish step (Phase 2).
//!
//! These are intentionally rule-based (no LLM) so they're fast, free, and
//! predictable. The pipeline runs them in two slots:
//!
//! - **Before** the LLM: [`course_correct`] strips spoken retractions
//!   ("scratch that") so the model never sees the discarded clause.
//! - **After** the LLM: [`finalize`] applies spoken punctuation ("new line",
//!   "period"), formats clearly-enumerated spoken lists, and normalizes
//!   whitespace — doing this *after* the model means it can't reflow the
//!   structure we just inserted.

/// Spoken phrases that mean "discard what I just said". When one appears we
/// delete from the start of the current clause up to and including the phrase.
const RETRACTION_MARKERS: &[&str] = &[
    "scratch that",
    "strike that",
    "delete that",
    "ignore that",
    "cancel that",
    "scrap that",
];

/// Run the post-LLM deterministic pass: spoken punctuation → list formatting →
/// whitespace cleanup. Optionally applies CJK autocorrect spacing.
pub fn finalize(text: &str, cjk_autocorrect: bool, language: &str) -> String {
    let punctuated = apply_spoken_punctuation(text);
    let listed = format_lists(&punctuated);
    let normalized = normalize_whitespace(&listed);

    if cjk_autocorrect && is_cjk_language(language, &normalized) {
        autocorrect::format(&normalized)
    } else {
        normalized
    }
}

fn is_cjk_language(language: &str, text: &str) -> bool {
    language.starts_with("zh")
        || language.starts_with("ja")
        || language.starts_with("ko")
        || (language == "auto" && has_cjk_chars(text))
}

fn has_cjk_chars(text: &str) -> bool {
    text.chars().any(|c| {
        // CJK Unified Ideographs, Hiragana, Katakana, Hangul Syllables
        matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3040}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}')
    })
}

// ---------------------------------------------------------------------------
// Course correction (pre-LLM)
// ---------------------------------------------------------------------------

/// Remove spoken retractions and the clause they cancel.
///
/// "send it at five, scratch that, send it at six" → "send it at six".
/// "Buy milk. Get eggs. Delete that. Get bread." → "Buy milk. Get bread."
///
/// The marker's own clause is deleted up to and including the marker. If the
/// marker stands alone in its clause (e.g. its own sentence, or "…, scratch
/// that, …"), the preceding clause is the thing being retracted, so it's deleted
/// too. Clause boundaries are `.`/`!`/`?`/`,`/newline or the start of the text.
pub fn course_correct(text: &str) -> String {
    let boundaries = ['.', '!', '?', '\n', ','];
    let mut result = text.to_string();

    // Find and excise the earliest retraction marker (ASCII case-insensitive),
    // repeating until none remain.
    while let Some((pos, marker_len)) = RETRACTION_MARKERS
        .iter()
        .filter_map(|m| find_ci(&result, m).map(|p| (p, m.len())))
        .min_by_key(|&(p, _)| p)
    {
        // Extend the cut past the marker's trailing punctuation/spaces.
        let mut end = pos + marker_len;
        while end < result.len()
            && matches!(result.as_bytes()[end], b'.' | b',' | b'!' | b'?' | b' ')
        {
            end += 1;
        }

        // Start of the marker's own clause.
        let mut clause_start = result[..pos].rfind(boundaries).map(|i| i + 1).unwrap_or(0);

        // If nothing precedes the marker within its clause, the user is
        // retracting the *previous* clause — extend the cut back over it.
        let in_clause_prefix = result[clause_start..pos].trim();
        if in_clause_prefix.is_empty() && clause_start > 0 {
            clause_start = result[..clause_start - 1]
                .rfind(boundaries)
                .map(|i| i + 1)
                .unwrap_or(0);
        }

        let mut next = String::with_capacity(result.len());
        next.push_str(result[..clause_start].trim_end());
        if !next.is_empty() {
            // Re-join the kept halves with a single space.
            next.push(' ');
        }
        next.push_str(result[end..].trim_start());
        result = next.trim().to_string();
    }

    result
}

// ---------------------------------------------------------------------------
// Dictionary corrections (post-transcription, pre-course-correct)
// ---------------------------------------------------------------------------

/// Apply dictionary misspelling→correction mappings as whole-word,
/// case-insensitive substitutions.
///
/// `dict` is a list of `(from, to)` pairs (e.g. `("tail wind", "Tailwind")`).
/// Each `from` may be multiple words; matches must sit on word boundaries so a
/// term never rewrites inside a larger word. Callers should pass the longest
/// `from` first so multi-word terms win over their substrings.
pub fn apply_corrections(text: &str, dict: &[(String, String)]) -> String {
    let mut result = text.to_string();
    for (from, to) in dict {
        if from.trim().is_empty() {
            continue;
        }
        result = replace_word_ci(&result, from, to);
    }
    result
}

/// Replace every whole-word, ASCII-case-insensitive occurrence of `needle` in
/// `haystack` with `replacement`. A match counts only when both ends fall on a
/// word boundary (start/end of string or a non-alphanumeric neighbour).
fn replace_word_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    let nlen = needle.len();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        let end = i + nlen;
        let is_match = end <= haystack.len()
            && haystack.is_char_boundary(end)
            && haystack.as_bytes()[i..end].eq_ignore_ascii_case(needle.as_bytes())
            && is_word_boundary(haystack, i, end);
        if is_match {
            out.push_str(replacement);
            i = end;
        } else {
            // Advance by a full char so `i` stays on a UTF-8 boundary.
            let ch = haystack[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// True when `[start, end)` is flanked by non-alphanumeric chars (or text edges).
fn is_word_boundary(s: &str, start: usize, end: usize) -> bool {
    let before_ok = s[..start]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric());
    let after_ok = s[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
    before_ok && after_ok
}

// ---------------------------------------------------------------------------
// Snippet expansion (post-finalize, pre-injection)
// ---------------------------------------------------------------------------

/// Trigger phrases at or below this length (in chars, normalized) are matched
/// with a 1-edit fuzzy tolerance to absorb minor mistranscriptions. Anything
/// shorter than 3 chars is never fuzzy-matched (it would match almost anything).
const FUZZY_TRIGGER_RANGE: std::ops::RangeInclusive<usize> = 3..=6;

/// Expand spoken snippet triggers into their long-form text.
///
/// `snippets` is a list of `(trigger, expansion)` pairs (e.g.
/// `("my email", "bob@example.com")`). Matching is whole-phrase and
/// case-insensitive; short triggers also match within 1 edit (Levenshtein) so a
/// slightly-misheard trigger still fires. Callers should pass the longest
/// trigger first so multi-word phrases win over their substrings.
///
/// Surrounding punctuation and whitespace are preserved — only the matched word
/// span is replaced, so "send my email." becomes "send bob@example.com.".
pub fn expand_snippets(text: &str, snippets: &[(String, String)]) -> String {
    let mut result = text.to_string();
    for (trigger, expansion) in snippets {
        if trigger.trim().is_empty() {
            continue;
        }
        result = replace_phrase_ci(&result, trigger, expansion);
    }
    result
}

/// A whitespace token reduced to its alphanumeric core, with the byte span of
/// that core within the source string.
struct Token {
    start: usize,
    end: usize,
    lower: String,
}

/// Split `text` into word tokens, recording each token's lowercased
/// alphanumeric core and its byte span (surrounding punctuation excluded).
/// Pure-punctuation tokens are dropped so they sit in the gaps between matches.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    let push = |s: usize, e: usize, tokens: &mut Vec<Token>| {
        let raw = &text[s..e];
        let front = raw.trim_start_matches(|c: char| !c.is_alphanumeric());
        let core = front.trim_end_matches(|c: char| !c.is_alphanumeric());
        if core.is_empty() {
            return;
        }
        let core_start = s + (raw.len() - front.len());
        tokens.push(Token {
            start: core_start,
            end: core_start + core.len(),
            lower: core.to_lowercase(),
        });
    };
    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = start.take() {
                push(s, idx, &mut tokens);
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }
    if let Some(s) = start {
        push(s, text.len(), &mut tokens);
    }
    tokens
}

/// Replace every non-overlapping whole-phrase occurrence of `trigger` in `text`
/// with `expansion`, case-insensitively (plus 1-edit fuzz for short triggers).
fn replace_phrase_ci(text: &str, trigger: &str, expansion: &str) -> String {
    let trigger_words: Vec<String> = trigger.split_whitespace().map(str::to_lowercase).collect();
    if trigger_words.is_empty() {
        return text.to_string();
    }
    let trigger_norm = trigger_words.join(" ");
    let fuzzy = FUZZY_TRIGGER_RANGE.contains(&trigger_norm.chars().count());
    let k = trigger_words.len();

    let tokens = tokenize(text);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0; // bytes of `text` already emitted
    let mut i = 0;
    while i < tokens.len() {
        if i + k <= tokens.len() {
            let candidate = tokens[i..i + k]
                .iter()
                .map(|t| t.lower.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let is_match =
                candidate == trigger_norm || (fuzzy && levenshtein(&candidate, &trigger_norm) <= 1);
            if is_match {
                out.push_str(&text[cursor..tokens[i].start]);
                out.push_str(expansion);
                cursor = tokens[i + k - 1].end;
                i += k;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Levenshtein edit distance between two strings (over Unicode scalar values).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ---------------------------------------------------------------------------
// Vibe-coding (post-finalize, code editors only — Phase 8)
// ---------------------------------------------------------------------------

/// Wrap spoken "backtick X backtick" spans in literal backticks for code
/// editors: "set backtick is active backtick to true" → "set `is active` to
/// true". An unterminated `backtick` (no closing marker) is left as the plain
/// word so dictation isn't swallowed. Applied per line so finalize's newlines
/// (lists, paragraphs) survive.
pub fn apply_vibe_coding(text: &str) -> String {
    text.split('\n')
        .map(wrap_backticks_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap_backticks_line(line: &str) -> String {
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        if strip_word(words[i]) == "backtick" {
            // Find the matching closing "backtick".
            if let Some(close) = (i + 1..words.len()).find(|&j| strip_word(words[j]) == "backtick")
            {
                let inner = words[i + 1..close].join(" ");
                // Skip empty pairs ("backtick backtick") entirely.
                if !inner.is_empty() {
                    out.push(format!("`{inner}`"));
                }
                i = close + 1;
                continue;
            }
        }
        out.push(words[i].to_string());
        i += 1;
    }
    out.join(" ")
}

// ---------------------------------------------------------------------------
// Edit counting (Insights — Phase 8)
// ---------------------------------------------------------------------------

/// Approximate the number of word-level edits (insertions, deletions,
/// substitutions) between the raw transcript and the final injected text. Used
/// as the Insights "corrections" metric — a cheap proxy for how much cleanup a
/// dictation needed. Comparison is case-insensitive over whitespace tokens.
pub fn count_edits(before: &str, after: &str) -> usize {
    let a: Vec<String> = before
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect();
    let b: Vec<String> = after.split_whitespace().map(|w| w.to_lowercase()).collect();
    word_levenshtein(&a, &b)
}

/// Levenshtein edit distance over a sequence of words (token-level).
fn word_levenshtein(a: &[String], b: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, wa) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, wb) in b.iter().enumerate() {
            let cost = if wa == wb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ---------------------------------------------------------------------------
// Spoken punctuation (post-LLM)
// ---------------------------------------------------------------------------

/// What a spoken-punctuation command expands to and how it spaces.
enum Punct {
    /// Attaches to the preceding word (no leading space): `.` `,` `?` etc.
    Trailing(&'static str),
    /// A line break (`\n` or `\n\n`).
    Break(&'static str),
}

/// Replace spoken punctuation commands with their symbols.
///
/// "hello world period new line bye" → "hello world.\nbye".
/// Matching is case-insensitive and only fires on standalone words, so ordinary
/// prose ("a comma-separated list") is left alone.
pub fn apply_spoken_punctuation(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < words.len() {
        // Try a two-word command first ("new line", "full stop", ...).
        if i + 1 < words.len() {
            let bigram = format!("{} {}", strip_word(words[i]), strip_word(words[i + 1]));
            if let Some(p) = match_command(&bigram) {
                push_punct(&mut out, p);
                i += 2;
                continue;
            }
        }
        if let Some(p) = match_command(&strip_word(words[i])) {
            push_punct(&mut out, p);
            i += 1;
            continue;
        }
        push_word(&mut out, words[i]);
        i += 1;
    }

    out
}

fn match_command(s: &str) -> Option<Punct> {
    Some(match s {
        "new paragraph" => Punct::Break("\n\n"),
        "new line" | "newline" => Punct::Break("\n"),
        "period" | "full stop" => Punct::Trailing("."),
        "comma" => Punct::Trailing(","),
        "question mark" => Punct::Trailing("?"),
        "exclamation mark" | "exclamation point" => Punct::Trailing("!"),
        "colon" => Punct::Trailing(":"),
        "semicolon" => Punct::Trailing(";"),
        _ => return None,
    })
}

/// Lowercase a word and strip surrounding punctuation so "Period." matches
/// "period". Keeps inner characters (e.g. won't mangle "new-line").
fn strip_word(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

fn push_punct(out: &mut String, p: Punct) {
    match p {
        Punct::Trailing(sym) => {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push_str(sym);
        }
        Punct::Break(sym) => {
            while out.ends_with(' ') || out.ends_with('\n') {
                out.pop();
            }
            out.push_str(sym);
        }
    }
}

fn push_word(out: &mut String, word: &str) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push(' ');
    }
    out.push_str(word);
}

// ---------------------------------------------------------------------------
// List auto-formatting (post-LLM)
// ---------------------------------------------------------------------------

/// List-item markers, in order. The first alias in each group is the *ordinal*
/// form ("first", "second", …); the rest are cardinals/digits. Ordinals are a
/// strong list signal and qualify anywhere; cardinals only qualify at a clause
/// boundary, so ordinary prose ("one of two or three") isn't reformatted.
const LIST_MARKERS: &[&[&str]] = &[
    &["first", "one", "1"],
    &["second", "two", "2"],
    &["third", "three", "3"],
    &["fourth", "four", "4"],
    &["fifth", "five", "5"],
    &["sixth", "six", "6"],
    &["seventh", "seven", "7"],
    &["eighth", "eight", "8"],
    &["ninth", "nine", "9"],
    &["tenth", "ten", "10"],
];

/// Convert a clearly-enumerated spoken list into a numbered list.
///
/// "first buy milk second buy eggs third buy bread" →
/// "1. buy milk\n2. buy eggs\n3. buy bread".
///
/// Conservative on purpose: it only triggers when at least three *sequential*
/// markers (first/second/third…) appear as clause starts, so ordinary prose
/// like "one of the two or three" won't be reflowed. Text that already contains
/// line breaks is assumed pre-formatted and left untouched.
pub fn format_lists(text: &str) -> String {
    if text.contains('\n') {
        return text.to_string();
    }

    let words: Vec<&str> = text.split_whitespace().collect();
    // Record (word_index, marker_ordinal) for every qualifying marker.
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let cleaned = strip_word(w);
        if let Some(ord) = LIST_MARKERS
            .iter()
            .position(|aliases| aliases.contains(&cleaned.as_str()))
        {
            let is_ordinal = cleaned == LIST_MARKERS[ord][0];
            let at_clause_start = i == 0
                || words[i - 1]
                    .chars()
                    .last()
                    .is_some_and(|c| matches!(c, '.' | ',' | '!' | '?' | ':' | ';'));
            // Ordinals are unambiguous list markers; cardinals must sit at a
            // clause boundary to count (avoids reflowing counting-prose).
            if is_ordinal || at_clause_start {
                hits.push((i, ord));
            }
        }
    }

    // Require a run of ≥3 strictly sequential markers starting at ordinal 0 (first/one).
    let run_start = hits.iter().position(|&(_, ord)| ord == 0);
    let Some(run_start) = run_start else {
        return text.to_string();
    };
    let mut run: Vec<usize> = vec![hits[run_start].0];
    let mut expected = 1;
    for &(idx, ord) in &hits[run_start + 1..] {
        if ord == expected {
            run.push(idx);
            expected += 1;
        }
    }
    if run.len() < 3 {
        return text.to_string();
    }

    // Build the numbered list from the spans between markers.
    let mut out = String::new();
    // Any words before the first marker become a lead-in line.
    if run[0] > 0 {
        let lead = words[..run[0]].join(" ");
        let lead = lead.trim_end_matches([' ', ',']).trim();
        if !lead.is_empty() {
            out.push_str(lead);
            out.push('\n');
        }
    }
    for (n, &start) in run.iter().enumerate() {
        let end = run.get(n + 1).copied().unwrap_or(words.len());
        // Skip the marker word itself; drop a leading filler comma if present.
        let item = words[start + 1..end]
            .join(" ")
            .trim_start_matches([',', ' '])
            .trim()
            .to_string();
        out.push_str(&format!("{}. {}", n + 1, item));
        if n + 1 < run.len() {
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collapse runs of spaces/tabs to a single space and trim each line, while
/// preserving intentional line breaks.
fn normalize_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let collapsed: Vec<&str> = line.split_whitespace().collect();
        out.push_str(&collapsed.join(" "));
    }
    // Trim leading/trailing blank lines but keep interior blanks (paragraphs).
    out.trim_matches('\n').to_string()
}

/// ASCII case-insensitive substring search returning the byte offset in
/// `haystack`. `needle` must be ASCII; the returned index is a valid char
/// boundary because ASCII bytes are always boundaries.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    for start in 0..=h.len() - n.len() {
        if h[start..start + n.len()]
            .iter()
            .zip(n)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_punctuation_basic() {
        assert_eq!(
            apply_spoken_punctuation("hello world period new line goodbye"),
            "hello world.\ngoodbye"
        );
    }

    #[test]
    fn spoken_punctuation_handles_case_and_variants() {
        assert_eq!(
            apply_spoken_punctuation("yes Comma absolutely Full Stop"),
            "yes, absolutely."
        );
        assert_eq!(
            apply_spoken_punctuation("really question mark wow exclamation point"),
            "really? wow!"
        );
    }

    #[test]
    fn spoken_punctuation_new_paragraph() {
        assert_eq!(
            apply_spoken_punctuation("intro new paragraph body"),
            "intro\n\nbody"
        );
    }

    #[test]
    fn course_correct_removes_cancelled_clause() {
        assert_eq!(
            course_correct("send it at five, scratch that, send it at six"),
            "send it at six"
        );
    }

    #[test]
    fn course_correct_at_sentence_boundary() {
        // A standalone "delete that" retracts the clause before it ("Get eggs").
        assert_eq!(
            course_correct("Buy milk. Get eggs. Delete that. Get bread."),
            "Buy milk. Get bread."
        );
    }

    #[test]
    fn course_correct_keeps_in_clause_prefix() {
        // Here the marker has content before it in its own clause, so only that
        // clause (from its start) is retracted.
        assert_eq!(
            course_correct("let's meet Monday scratch that Tuesday"),
            "Tuesday"
        );
    }

    #[test]
    fn course_correct_noop_without_marker() {
        let s = "just a normal sentence";
        assert_eq!(course_correct(s), s);
    }

    #[test]
    fn format_lists_numbers_sequential_markers() {
        assert_eq!(
            format_lists("first buy milk second buy eggs third buy bread"),
            "1. buy milk\n2. buy eggs\n3. buy bread"
        );
    }

    #[test]
    fn format_lists_keeps_lead_in() {
        assert_eq!(
            format_lists("my tasks: first email Sam second call Lee third ship it"),
            "my tasks:\n1. email Sam\n2. call Lee\n3. ship it"
        );
    }

    #[test]
    fn format_lists_ignores_short_or_nonsequential() {
        // "one ... two" is only two markers — not enough to be a list.
        let s = "one of two options";
        assert_eq!(format_lists(s), s);
    }

    #[test]
    fn corrections_replace_whole_words_case_insensitively() {
        let dict = vec![("tail wind".to_string(), "Tailwind".to_string())];
        assert_eq!(
            apply_corrections("I styled it with Tail Wind today", &dict),
            "I styled it with Tailwind today"
        );
    }

    #[test]
    fn corrections_respect_word_boundaries() {
        // "cat" must not rewrite inside "category".
        let dict = vec![("cat".to_string(), "dog".to_string())];
        assert_eq!(
            apply_corrections("the cat in the category", &dict),
            "the dog in the category"
        );
    }

    #[test]
    fn corrections_noop_when_absent() {
        let dict = vec![("foo".to_string(), "bar".to_string())];
        let s = "nothing to change here";
        assert_eq!(apply_corrections(s, &dict), s);
    }

    #[test]
    fn snippets_expand_multiword_trigger() {
        let snips = vec![("my email".to_string(), "bob@example.com".to_string())];
        assert_eq!(
            expand_snippets("please send it to my email today", &snips),
            "please send it to bob@example.com today"
        );
    }

    #[test]
    fn snippets_preserve_trailing_punctuation() {
        let snips = vec![("my email".to_string(), "bob@example.com".to_string())];
        assert_eq!(
            expand_snippets("reach me at my email.", &snips),
            "reach me at bob@example.com."
        );
    }

    #[test]
    fn snippets_fuzzy_match_short_trigger() {
        // "addr" (4 chars) tolerates a 1-edit mishearing → "adr".
        let snips = vec![("addr".to_string(), "1 Infinite Loop".to_string())];
        assert_eq!(
            expand_snippets("ship to adr please", &snips),
            "ship to 1 Infinite Loop please"
        );
    }

    #[test]
    fn snippets_respect_word_boundaries() {
        // A long trigger never matches inside a larger word.
        let snips = vec![("cat".to_string(), "dog".to_string())];
        assert_eq!(
            expand_snippets("the category is fixed", &snips),
            "the category is fixed"
        );
    }

    #[test]
    fn snippets_noop_when_absent() {
        let snips = vec![("sig".to_string(), "Best, Bob".to_string())];
        let s = "nothing to expand here";
        assert_eq!(expand_snippets(s, &snips), s);
    }

    #[test]
    fn vibe_wraps_backtick_spans() {
        assert_eq!(
            apply_vibe_coding("set backtick is active backtick to true"),
            "set `is active` to true"
        );
    }

    #[test]
    fn vibe_leaves_unterminated_backtick() {
        // A lone "backtick" with no closing marker is left as a plain word.
        assert_eq!(apply_vibe_coding("the backtick key"), "the backtick key");
    }

    #[test]
    fn vibe_preserves_newlines() {
        assert_eq!(
            apply_vibe_coding("call backtick foo backtick\nthen backtick bar backtick"),
            "call `foo`\nthen `bar`"
        );
    }

    #[test]
    fn count_edits_measures_word_changes() {
        // One filler removed + one word substituted = 2 edits.
        assert_eq!(count_edits("um send it now", "send it later"), 2);
        assert_eq!(count_edits("same text here", "same text here"), 0);
    }

    #[test]
    fn finalize_combines_passes() {
        assert_eq!(finalize("hello   world period", false, "en"), "hello world.");
    }
}
