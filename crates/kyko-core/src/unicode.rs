//! Ctrl+Shift+U: finds characters to type by code point (`1f600`, `U+00E9`) or, when the
//! Unicode character database is installed, by name (`smiling face`).
use std::path::Path;

/// Where distributions put `UnicodeData.txt`.
const DATABASES: [&str; 3] = [
    "/usr/share/unicode/UnicodeData.txt",
    "/usr/share/unicode-data/UnicodeData.txt",
    "/usr/share/unicode/ucd/UnicodeData.txt",
];

/// A character offered: its code point and its name (empty when unknown).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub ch: char,
    pub name: String,
}

/// The names, from the first database found; empty when none is installed.
pub fn load_names() -> Vec<(char, String)> {
    DATABASES
        .iter()
        .find_map(|p| std::fs::read_to_string(Path::new(p)).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// `UnicodeData.txt`'s `code;NAME;...` lines, without the control characters and ranges.
pub fn parse(text: &str) -> Vec<(char, String)> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split(';');
            let code = u32::from_str_radix(f.next()?, 16).ok()?;
            let name = f.next()?;
            let ch = char::from_u32(code)?;
            (!name.starts_with('<') && !ch.is_control()).then(|| (ch, name.to_string()))
        })
        .collect()
}

/// A code point typed as hex, with or without `U+` or `0x`.
fn code_point(query: &str) -> Option<char> {
    let q = query.trim();
    let hex = q
        .strip_prefix("U+")
        .or_else(|| q.strip_prefix("u+"))
        .or_else(|| q.strip_prefix("0x"))
        .unwrap_or(q);
    if hex.is_empty() || hex.len() > 6 {
        return None;
    }
    char::from_u32(u32::from_str_radix(hex, 16).ok()?).filter(|c| !c.is_control())
}

/// Up to `limit` characters for `query`: the code point it spells first, then the names that
/// hold every word of it, those that start with the first word ahead.
pub fn search(query: &str, names: &[(char, String)], limit: usize) -> Vec<Found> {
    let mut out = Vec::new();
    let name_of = |ch: char| {
        names
            .iter()
            .find(|(c, _)| *c == ch)
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    };
    if let Some(ch) = code_point(query) {
        out.push(Found {
            ch,
            name: name_of(ch),
        });
    }
    let words: Vec<String> = query
        .split_whitespace()
        .map(str::to_ascii_uppercase)
        .collect();
    if words.is_empty() {
        return out;
    }
    let mut hits: Vec<(bool, &(char, String))> = names
        .iter()
        .filter(|(_, n)| {
            words
                .iter()
                .all(|w| n.split(' ').any(|part| part.starts_with(w.as_str())))
        })
        .map(|e| (!e.1.starts_with(words[0].as_str()), e))
        .collect();
    hits.sort_by_key(|(later, (_, n))| (*later, n.len()));
    for (_, (ch, name)) in hits {
        if out.len() >= limit {
            break;
        }
        if out.iter().all(|f| f.ch != *ch) {
            out.push(Found {
                ch: *ch,
                name: name.clone(),
            });
        }
    }
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: &str = "0007;<control>;Cc;0;BN;;;;;N;BELL;;;;\n\
                        00E9;LATIN SMALL LETTER E WITH ACUTE;Ll;0;L;0065 0301;;;;N;;;00C9;;00C9\n\
                        2192;RIGHTWARDS ARROW;Sm;0;ON;;;;;N;RIGHT ARROW;;;;\n\
                        1F600;GRINNING FACE;So;0;ON;;;;;N;;;;;\n\
                        1F642;SLIGHTLY SMILING FACE;So;0;ON;;;;;N;;;;;\n\
                        263A;WHITE SMILING FACE;So;0;ON;;;;;N;;;;;\n";

    #[test]
    fn code_points_and_names_are_found() {
        let names = parse(DATA);
        assert_eq!(names.len(), 5);
        let r = search("U+00e9", &names, 10);
        assert_eq!(r[0].ch, 'é');
        assert_eq!(r[0].name, "LATIN SMALL LETTER E WITH ACUTE");
        // Hex that is also a word: the code point first, then names.
        let r = search("face", &names, 10);
        assert_eq!(r[0].ch, '\u{face}');
        assert_eq!(r.len(), 4);
        let r = search("smil fa", &names, 10);
        let chars: Vec<char> = r.iter().map(|f| f.ch).collect();
        assert_eq!(chars, vec!['☺', '🙂']);
        assert_eq!(search("arrow", &names, 10)[0].ch, '→');
        assert!(search("7", &names, 10).is_empty());
        assert!(search("", &names, 10).is_empty());
        assert_eq!(search("smiling", &names, 1).len(), 1);
    }
}
