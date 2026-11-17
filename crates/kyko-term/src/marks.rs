//! Prompt marks: the shell's OSC 133 sequences (`A` prompt starts, `B` input starts, `C` the
//! command's output starts, `D;<status>` it ended) found in the output on its way to the
//! emulator. The scanner only finds them; the emulator then records each against the row the
//! cursor is on. Rows are numbered from the first line the terminal ever scrolled off the top, so
//! a number keeps meaning the same line as the screen scrolls (see `Emulator::row_id`).

use std::collections::BTreeMap;

/// What was marked on one row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row {
    /// A prompt starts here.
    pub prompt: bool,
    /// A command's output starts here.
    pub output: bool,
    /// A command ended on this row: `Some(true)` when the row holds the last of its output,
    /// `Some(false)` when the output stopped above it.
    pub end: Option<bool>,
    /// The exit status of the command whose prompt is on this row.
    pub status: Option<u8>,
}

/// The marks, by row number.
pub type Table = BTreeMap<u64, Row>;

/// What the shell reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// OSC 133;A
    Prompt,
    /// OSC 133;B
    Input,
    /// OSC 133;C
    Output,
    /// OSC 133;D, with the exit status when there was one.
    Done(Option<u8>),
}

/// Longest OSC body that is looked at; a longer one is not a mark.
const LIMIT: usize = 48;

#[derive(Default)]
enum State {
    #[default]
    Idle,
    /// Saw ESC.
    Esc,
    /// Inside `ESC ]`, collecting the body.
    Osc,
    /// Inside an OSC, saw ESC: `\` ends it.
    OscEsc,
}

/// Finds OSC 133 sequences in a byte stream that arrives in pieces.
#[derive(Default)]
pub struct Scanner {
    state: State,
    body: Vec<u8>,
}

fn parse(body: &[u8]) -> Option<Event> {
    let rest = body.strip_prefix(b"133;")?;
    let (kind, params) = match rest.iter().position(|&b| b == b';') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &rest[rest.len()..]),
    };
    match kind {
        b"A" => Some(Event::Prompt),
        b"B" => Some(Event::Input),
        b"C" => Some(Event::Output),
        b"D" => {
            // Extra `key=value` parameters may follow the status.
            let first = params.split(|&b| b == b';').next().unwrap_or(&[]);
            let code = std::str::from_utf8(first)
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .map(|n| n.clamp(0, 255) as u8);
            Some(Event::Done(code))
        }
        _ => None,
    }
}

impl Scanner {
    /// The marks in `bytes`, each with the offset just after its terminator, where it applies.
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, Event)> {
        let mut found = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            match self.state {
                State::Idle => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    }
                }
                State::Esc => {
                    if b == b']' {
                        self.state = State::Osc;
                        self.body.clear();
                    } else if b != 0x1b {
                        self.state = State::Idle;
                    }
                }
                State::Osc => match b {
                    0x07 => self.end(i + 1, &mut found),
                    0x1b => self.state = State::OscEsc,
                    _ if self.body.len() >= LIMIT => self.state = State::Idle,
                    _ => self.body.push(b),
                },
                State::OscEsc => {
                    if b == b'\\' {
                        self.end(i + 1, &mut found);
                    } else {
                        // A new escape sequence began: this OSC never ended.
                        self.state = if b == b']' { State::Osc } else { State::Idle };
                        self.body.clear();
                    }
                }
            }
        }
        found
    }

    fn end(&mut self, at: usize, found: &mut Vec<(usize, Event)>) {
        if let Some(e) = parse(&self.body) {
            found.push((at, e));
        }
        self.body.clear();
        self.state = State::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_found_with_either_terminator() {
        let mut s = Scanner::default();
        let got = s.scan(b"x\x1b]133;A\x07yy\x1b]133;D;3\x1b\\z\x1b]133;C\x07");
        assert_eq!(
            got,
            [
                (9, Event::Prompt),
                (22, Event::Done(Some(3))),
                (31, Event::Output)
            ]
        );
    }

    #[test]
    fn a_mark_split_across_pieces_is_found_once_complete() {
        let mut s = Scanner::default();
        assert!(s.scan(b"abc\x1b]13").is_empty());
        assert!(s.scan(b"3;D;").is_empty());
        assert_eq!(s.scan(b"12\x07rest"), [(3, Event::Done(Some(12)))]);
        assert!(s.scan(b"\x1b").is_empty());
        assert!(s.scan(b"]133;B").is_empty());
        assert_eq!(s.scan(b"\x07"), [(1, Event::Input)]);
    }

    #[test]
    fn other_sequences_and_long_junk_are_ignored() {
        let mut s = Scanner::default();
        assert!(
            s.scan(b"\x1b]7;file:///tmp\x07\x1b]0;title\x07\x1b[1m")
                .is_empty()
        );
        let mut long = b"\x1b]133;D;".to_vec();
        long.extend(std::iter::repeat_n(b'1', 100));
        long.push(0x07);
        assert!(s.scan(&long).is_empty());
        assert_eq!(s.scan(b"\x1b]133;A\x07"), [(8, Event::Prompt)]);
    }

    #[test]
    fn a_done_without_a_status_or_with_a_wild_one() {
        let mut s = Scanner::default();
        assert_eq!(s.scan(b"\x1b]133;D\x07"), [(8, Event::Done(None))]);
        assert_eq!(
            s.scan(b"\x1b]133;D;999\x07"),
            [(12, Event::Done(Some(255)))]
        );
        assert_eq!(s.scan(b"\x1b]133;D;x\x07"), [(10, Event::Done(None))]);
        assert_eq!(
            s.scan(b"\x1b]133;D;0;aid=1\x07"),
            [(16, Event::Done(Some(0)))]
        );
    }
}
