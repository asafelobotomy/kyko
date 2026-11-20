//! Links in the grid: OSC 8 hyperlinks a program wrote, and URLs found in the text.

/// The schemes kyko opens. Anything else (an `ssh:` or a custom handler a program might name in
/// an OSC 8 link) is left alone.
pub const SCHEMES: [&str; 6] = [
    "https://",
    "http://",
    "ftp://",
    "file://",
    "mailto:",
    "gemini://",
];

/// Whether `url` names a scheme kyko opens.
pub fn openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    SCHEMES.iter().any(|s| lower.starts_with(s))
        && url.len() <= 8192
        && !url.chars().any(char::is_control)
}

fn url_char(c: char) -> bool {
    c.is_alphanumeric() || "-._~:/?#[]@!$&'*+,;=%()".contains(c)
}

/// The URL in `line` that covers column `col`, as the half-open column range `start..end`.
/// One `char` per column. Trailing punctuation that usually ends a sentence is not part of it,
/// and a closing bracket only when the URL opened one.
pub fn url_at(line: &[char], col: usize) -> Option<(usize, usize)> {
    if col >= line.len() {
        return None;
    }
    let text: String = line.iter().collect();
    let lower = text.to_ascii_lowercase();
    // Byte offset to column, for the char-indexed result.
    let col_of = |byte: usize| text[..byte].chars().count();
    let mut found = None;
    for scheme in SCHEMES {
        let mut from = 0;
        while let Some(i) = lower[from..].find(scheme) {
            let start = from + i;
            from = start + scheme.len();
            let sc = col_of(start);
            // A scheme must not be glued to a word before it ("xhttps://").
            if sc > 0 && line[sc - 1].is_alphanumeric() {
                continue;
            }
            let mut end = sc;
            while end < line.len() && url_char(line[end]) {
                end += 1;
            }
            // Drop sentence punctuation and unbalanced closing brackets at the end.
            while let Some(&last) = line[sc..end].last() {
                let opens = |o: char| line[sc..end].iter().filter(|c| **c == o).count();
                let unbalanced = match last {
                    ')' => opens(')') > opens('('),
                    ']' => opens(']') > opens('['),
                    _ => false,
                };
                if ".,;:!?'".contains(last) || unbalanced {
                    end -= 1;
                } else {
                    break;
                }
            }
            if end - sc <= scheme.chars().count() {
                continue;
            }
            if (sc..end).contains(&col) {
                found = Some((sc, end));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str, col: usize) -> Option<String> {
        let line: Vec<char> = s.chars().collect();
        url_at(&line, col).map(|(a, b)| line[a..b].iter().collect())
    }

    #[test]
    fn finds_the_url_under_the_column() {
        let s = "see https://example.com/a?b=1 for more";
        assert_eq!(at(s, 10).as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(at(s, 2), None);
        assert_eq!(at(s, 30), None);
    }

    #[test]
    fn trims_sentence_punctuation_and_unbalanced_brackets() {
        assert_eq!(
            at("(https://a.org/x).", 3).as_deref(),
            Some("https://a.org/x")
        );
        assert_eq!(
            at("https://en.wikipedia.org/wiki/Foo_(bar)", 3).as_deref(),
            Some("https://en.wikipedia.org/wiki/Foo_(bar)")
        );
        assert_eq!(
            at("go to http://x.io, then", 8).as_deref(),
            Some("http://x.io")
        );
    }

    #[test]
    fn needs_more_than_a_scheme_and_a_word_boundary() {
        assert_eq!(at("https:// alone", 2), None);
        assert_eq!(at("xhttps://a.b", 3), None);
        assert_eq!(at("mailto:me@x.org", 3).as_deref(), Some("mailto:me@x.org"));
    }

    #[test]
    fn only_known_schemes_open() {
        assert!(openable("https://a.b"));
        assert!(openable("FILE:///tmp/x"));
        assert!(!openable("ssh://host"));
        assert!(!openable("javascript:alert(1)"));
        assert!(!openable("https://a.b/\u{7}"));
    }
}
