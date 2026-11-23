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

/// A file named in a program's output: compiler errors (`src/main.rs:12:5`), grep (`a/b.txt:3:`),
/// `ls` and `find` lines. The path as written (relative to the shell's folder, `~/` or absolute)
/// and the line and column after it, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTarget {
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

fn path_char(c: char) -> bool {
    !c.is_whitespace() && !"\"'`<>|()[]{},;".contains(c) && !c.is_control()
}

/// What a path-looking word names: it must have a `/` or a file extension, so a bare word
/// (`error`, `main`) is not a link. A trailing `:LINE` and `:LINE:COL` are taken off and kept.
pub fn file_target(word: &str) -> Option<FileTarget> {
    let mut path = word.trim_end_matches(['.', ':', '!', '?']);
    let mut nums = Vec::new();
    while nums.len() < 2 {
        match path.rsplit_once(':') {
            Some((p, n))
                if !n.is_empty() && n.len() <= 7 && n.bytes().all(|b| b.is_ascii_digit()) =>
            {
                nums.push(n.parse::<u32>().ok()?);
                path = p;
            }
            _ => break,
        }
    }
    nums.reverse();
    if path.is_empty() || path.contains("://") || path.starts_with('-') || path.len() > 4096 {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let has_ext = name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && (1..=10).contains(&ext.len())
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
            && ext.chars().any(|c| c.is_ascii_alphabetic())
    });
    let pathy = path.contains('/') && path.chars().any(|c| c.is_alphanumeric());
    if !(has_ext || pathy) {
        return None;
    }
    Some(FileTarget {
        path: path.to_string(),
        line: nums.first().copied(),
        column: nums.get(1).copied(),
    })
}

/// The path-looking word in `line` covering column `col`, as `start..end` and what it names.
pub fn path_at(line: &[char], col: usize) -> Option<(usize, usize, FileTarget)> {
    if col >= line.len() || !path_char(line[col]) {
        return None;
    }
    let mut a = col;
    while a > 0 && path_char(line[a - 1]) {
        a -= 1;
    }
    let mut b = col + 1;
    while b < line.len() && path_char(line[b]) {
        b += 1;
    }
    // grep's `path:12:text`: the word ends after the line (and column) number.
    let mut i = a;
    while i < b {
        if line[i] == ':' && i > a {
            let digits = |from: usize| (from..b).take_while(|&k| line[k].is_ascii_digit()).count();
            let n = digits(i + 1);
            if n > 0 {
                let mut end = i + 1 + n;
                if end < b && line[end] == ':' {
                    let m = digits(end + 1);
                    if m > 0 {
                        end += 1 + m;
                    }
                }
                b = end;
                break;
            }
        }
        i += 1;
    }
    // Sentence punctuation after the word is not part of it.
    while b > a && ".:!?".contains(line[b - 1]) {
        b -= 1;
    }
    if col >= b {
        return None;
    }
    let word: String = line[a..b].iter().collect();
    let t = file_target(&word)?;
    Some((a, b, t))
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

    fn target(p: &str, line: Option<u32>, column: Option<u32>) -> Option<FileTarget> {
        Some(FileTarget {
            path: p.into(),
            line,
            column,
        })
    }

    #[test]
    fn file_targets_with_lines_and_columns() {
        assert_eq!(
            file_target("src/main.rs:12:5"),
            target("src/main.rs", Some(12), Some(5))
        );
        assert_eq!(file_target("a/b.txt:3:"), target("a/b.txt", Some(3), None));
        assert_eq!(file_target("~/notes.md"), target("~/notes.md", None, None));
        assert_eq!(file_target("/etc/hosts"), target("/etc/hosts", None, None));
        assert_eq!(file_target("Cargo.toml."), target("Cargo.toml", None, None));
        // Words, numbers, versions, options and URLs are not files.
        for w in [
            "error",
            "main",
            "1.2.3",
            "3.14",
            "--help",
            "https://x.org/a.html",
            "/",
        ] {
            assert_eq!(file_target(w), None, "{w}");
        }
    }

    #[test]
    fn the_path_under_a_column() {
        let line: Vec<char> = "error: --> src/lib.rs:40:9 here".chars().collect();
        let (a, b, t) = path_at(&line, 15).unwrap();
        assert_eq!((a, b), (11, 26));
        assert_eq!(t, target("src/lib.rs", Some(40), Some(9)).unwrap());
        assert!(path_at(&line, 2).is_none());
        let grep: Vec<char> = "docs/a.md:3:some text".chars().collect();
        let (_, _, t) = path_at(&grep, 1).unwrap();
        assert_eq!(t.path, "docs/a.md");
        assert_eq!(t.line, Some(3));
    }
}
