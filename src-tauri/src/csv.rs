//! Minimal RFC-4180-ish CSV helpers: a line parser handling `"`-quoted fields
//! with escaped `""` and embedded commas, and the matching field quoter.
//! Sufficient for our 3-column dictionary export.

/// Parse one CSV line into its fields. A trailing separator yields a trailing
/// empty field; unterminated quotes are tolerated (everything up to end of
/// line stays inside the quoted field).
pub(crate) fn parse_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// Quote a CSV field if it contains a comma, quote, or newline.
pub(crate) fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{field, parse_line};

    #[test]
    fn parses_quoted_fields_with_escapes_and_commas() {
        let line = "word,\"say \"\"hi\"\", loudly\",1";
        assert_eq!(
            parse_line(line),
            vec!["word".to_string(), "say \"hi\", loudly".to_string(), "1".to_string()]
        );
    }

    #[test]
    fn empty_and_trailing_fields_round_trip() {
        assert_eq!(parse_line("a,,b,"), vec!["a", "", "b", ""]);
        assert_eq!(field(""), "");
        assert_eq!(parse_line(&field("")), vec![""]);
    }

    #[test]
    fn plain_field_is_left_untouched() {
        assert_eq!(field("hello world"), "hello world");
    }

    /// Deterministic property-style check: a seeded LCG generates random fields
    /// containing commas, quotes, newlines, carriage returns, tabs, and unicode;
    /// serializing each with [`field`] and re-parsing the joined row must
    /// reproduce the original fields exactly.
    #[test]
    fn round_trip_preserves_random_fields() {
        struct Lcg(u64);
        impl Lcg {
            fn next(&mut self) -> u64 {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                self.0 >> 16
            }
        }
        let alphabet = [
            ',', '"', '\n', '\r', '\t', ' ', 'a', 'Z', '0', 'é', 'ß', '日', '🦀',
        ];
        let mut rng = Lcg(0xDEC49E5F);
        for _ in 0..200 {
            let field_count = 1 + (rng.next() % 8) as usize;
            let fields: Vec<String> = (0..field_count)
                .map(|_| {
                    let len = (rng.next() % 24) as usize;
                    (0..len)
                        .map(|_| alphabet[(rng.next() as usize) % alphabet.len()])
                        .collect()
                })
                .collect();
            let row = fields
                .iter()
                .map(|f| field(f))
                .collect::<Vec<_>>()
                .join(",");
            assert_eq!(parse_line(&row), fields);
        }
    }
}
