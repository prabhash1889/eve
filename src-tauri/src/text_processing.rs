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
/// whitespace cleanup.
pub fn finalize(text: &str) -> String {
    let punctuated = apply_spoken_punctuation(text);
    let listed = format_lists(&punctuated);
    normalize_whitespace(&listed)
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
        let mut clause_start = result[..pos]
            .rfind(boundaries)
            .map(|i| i + 1)
            .unwrap_or(0);

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
        if let Some(ord) =
            LIST_MARKERS.iter().position(|aliases| aliases.contains(&cleaned.as_str()))
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
    fn finalize_combines_passes() {
        assert_eq!(
            finalize("hello   world period"),
            "hello world."
        );
    }
}
