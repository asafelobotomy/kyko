//! Opening links by keyboard: every link on screen gets a short label, and typing a label picks
//! that link (lower case opens it, a capital last letter copies it).

use crate::{LinkSpans, Snapshot};

/// The letters labels are made of, the home row first.
const ALPHABET: &str = "asdfghjklqwertyuiopzxcvbnm";

/// Labels for `n` links: single letters while they last, else two letters each, so no label is
/// the start of another.
pub fn labels(n: usize) -> Vec<String> {
    let letters: Vec<char> = ALPHABET.chars().collect();
    if n <= letters.len() {
        return letters[..n].iter().map(char::to_string).collect();
    }
    letters
        .iter()
        .flat_map(|a| letters.iter().map(move |b| format!("{a}{b}")))
        .take(n)
        .collect()
}

pub struct Hint {
    pub label: String,
    pub url: String,
    pub spans: LinkSpans,
}

/// What a key did to the labels.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// More letters are needed.
    Pending,
    Open(String),
    Copy(String),
    /// Escape, or a label nothing has: the labels go away.
    Cancel,
}

pub struct Hints {
    pub items: Vec<Hint>,
    pub typed: String,
}

impl Hints {
    /// Labels for the links in `snap`, or `None` when there are none.
    pub fn new(snap: &Snapshot) -> Option<Hints> {
        let links = snap.all_links();
        if links.is_empty() {
            return None;
        }
        let items = labels(links.len())
            .into_iter()
            .zip(links)
            .map(|(label, (url, spans))| Hint { label, url, spans })
            .collect();
        Some(Hints {
            items,
            typed: String::new(),
        })
    }

    /// A typed character; `None` for Escape, `'\u{8}'` for Backspace.
    pub fn key(&mut self, ch: Option<char>) -> Outcome {
        let Some(ch) = ch else {
            return Outcome::Cancel;
        };
        if ch == '\u{8}' {
            self.typed.pop();
            return Outcome::Pending;
        }
        if !ch.is_ascii_alphabetic() {
            return Outcome::Pending;
        }
        let copy = ch.is_ascii_uppercase();
        self.typed.push(ch.to_ascii_lowercase());
        if let Some(h) = self.items.iter().find(|h| h.label == self.typed) {
            return if copy {
                Outcome::Copy(h.url.clone())
            } else {
                Outcome::Open(h.url.clone())
            };
        }
        if self.items.iter().any(|h| h.label.starts_with(&self.typed)) {
            Outcome::Pending
        } else {
            Outcome::Cancel
        }
    }

    /// The labels still in play, with the letters left to type: `(row, col, text)` at the
    /// start of each link.
    pub fn shown(&self) -> Vec<(u16, u16, &str)> {
        self.items
            .iter()
            .filter(|h| h.label.starts_with(&self.typed))
            .filter_map(|h| {
                let &(r, a, _) = h.spans.first()?;
                Some((r, a, &h.label[self.typed.len()..]))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_never_prefix_each_other() {
        assert_eq!(labels(3), ["a", "s", "d"]);
        let many = labels(40);
        assert_eq!(many.len(), 40);
        assert!(many.iter().all(|l| l.len() == 2));
        for (i, a) in many.iter().enumerate() {
            for (j, b) in many.iter().enumerate() {
                assert!(i == j || !b.starts_with(a.as_str()));
            }
        }
    }

    fn hints(n: usize) -> Hints {
        let items = labels(n)
            .into_iter()
            .enumerate()
            .map(|(i, label)| Hint {
                label,
                url: format!("https://e.x/{i}"),
                spans: vec![(i as u16, 0, 10)],
            })
            .collect();
        Hints {
            items,
            typed: String::new(),
        }
    }

    #[test]
    fn typing_a_label_opens_or_copies() {
        let mut h = hints(3);
        assert_eq!(h.key(Some('s')), Outcome::Open("https://e.x/1".into()));
        let mut h = hints(3);
        assert_eq!(h.key(Some('D')), Outcome::Copy("https://e.x/2".into()));
        let mut h = hints(3);
        assert_eq!(h.key(Some('z')), Outcome::Cancel);
        assert_eq!(hints(3).key(None), Outcome::Cancel);
    }

    #[test]
    fn two_letter_labels_narrow_then_pick() {
        let mut h = hints(30);
        assert_eq!(h.key(Some('a')), Outcome::Pending);
        assert_eq!(h.shown().len(), 26);
        assert_eq!(h.shown()[1], (1, 0, "s"));
        assert_eq!(h.key(Some('\u{8}')), Outcome::Pending);
        assert_eq!(h.shown().len(), 30);
        h.key(Some('s'));
        assert_eq!(h.key(Some('a')), Outcome::Open("https://e.x/26".into()));
    }
}
