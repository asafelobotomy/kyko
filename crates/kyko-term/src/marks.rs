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
    /// The prompt on this row said the shell moves its cursor to a click (`click_events=1`).
    pub click_events: bool,
}

/// The marks, by row number.
pub type Table = BTreeMap<u64, Row>;

/// What the shell reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// OSC 133;A. `click_events` is fish's and kitty's `click_events=1`: the shell moves its
    /// cursor to a mouse click on its command line. A secondary prompt (`k=s`, a continuation
    /// line) is not reported.
    Prompt { click_events: bool },
    /// OSC 133;B
    Input,
    /// OSC 133;C, with the command line when the shell sent it (`cmdline_url=`, percent-encoded,
    /// as fish and kyko's scripts do, or kitty's plain `cmdline=`).
    Output { command: Option<String> },
    /// OSC 133;D, with the exit status when there was one.
    Done(Option<u8>),
    /// A desktop notification a program asked for: OSC 9 (text), OSC 777 (`notify;title;body`)
    /// or a finished OSC 99 (kitty's protocol), as title and body.
    Notify(String, String),
    /// A kitty graphics command (`ESC _ G ... ESC \`): what follows the `G`.
    Graphics(Vec<u8>),
    /// `CSI 16 t`: the size of a cell in pixels, which image programs ask for.
    CellSizeRequest,
    /// A sixel image (`ESC P ... q DATA ESC \`): the data after the `q`.
    Sixel(Vec<u8>),
    /// XTGETTCAP (`ESC P + q NAME;NAME ESC \`): the capability names asked for, hex-decoded.
    Termcap(Vec<String>),
    /// XTVERSION (`CSI > 0 q`): the terminal's name and version.
    VersionRequest,
}

/// Longest OSC body that is looked at: room for a notification. Only bodies that start like
/// one of the sequences here are kept; anything else is dropped after a few bytes.
const LIMIT: usize = 4096;
/// Longest OSC 133 body: a longer one is not a mark, except a `C` carrying a command line, which
/// is kept up to `LIMIT` and cut there (the mark still counts).
const MARK_LIMIT: usize = 48;
/// The longest command line passed on, in characters.
const COMMAND_SHOWN: usize = 1024;
/// The longest XTGETTCAP query looked at.
const TCAP_LIMIT: usize = 512;
/// The longest notification title and body passed on, in characters.
const SHOWN: usize = 512;
const PREFIXES: [&[u8]; 4] = [b"133;", b"9;", b"777;", b"99;"];

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
    /// Inside `ESC _ G`, a graphics command.
    Apc,
    /// Inside an APC, saw ESC.
    ApcEsc,
    /// Inside `ESC [`: only `16 t` is looked for.
    Csi,
    /// Inside `ESC P`: only sixel images (`params q data`) are kept.
    Dcs,
    /// Inside a DCS, saw ESC.
    DcsEsc,
    /// Inside `ESC P +`: an XTGETTCAP query once `q` follows.
    Tcap,
    /// Inside an XTGETTCAP query, saw ESC.
    TcapEsc,
}

/// The longest graphics command kept; programs send big images in chunks of 4096.
const APC_LIMIT: usize = 8 << 20;

/// Finds OSC 133 sequences in a byte stream that arrives in pieces.
#[derive(Default)]
pub struct Scanner {
    state: State,
    body: Vec<u8>,
    /// OSC 99 notifications still arriving in chunks, by id: title and body so far.
    pending: Vec<(String, String, String)>,
}

fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(SHOWN).collect()
}

fn base64(text: &str) -> Option<Vec<u8>> {
    base64_bytes(text.as_bytes())
}

/// Standard base64, padding optional; `None` for anything else.
pub(crate) fn base64_bytes(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in text {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

impl Scanner {
    /// kitty's OSC 99: `99;key=value:key=value;payload`. `d=0` says more chunks follow for the
    /// same `i`; `p` names the part (`title`, the default, or `body`); `e=1` means base64.
    fn osc99(&mut self, rest: &[u8]) -> Option<Event> {
        let rest = std::str::from_utf8(rest).ok()?;
        let (meta, payload) = rest.split_once(';')?;
        let (mut id, mut done, mut part, mut b64) = ("", true, "title", false);
        for kv in meta.split(':') {
            match kv.split_once('=') {
                Some(("i", v)) => id = v,
                Some(("d", v)) => done = v != "0",
                Some(("p", v)) => part = v,
                Some(("e", v)) => b64 = v == "1",
                _ => {}
            }
        }
        if !matches!(part, "title" | "body") {
            // Queries (`p=?`), icons, buttons and closing are not supported; ignored.
            return None;
        }
        let text = if b64 {
            String::from_utf8_lossy(&base64(payload)?).into_owned()
        } else {
            payload.to_string()
        };
        let i = match self.pending.iter().position(|(pid, _, _)| pid == id) {
            Some(i) => i,
            None => {
                if self.pending.len() >= 8 {
                    self.pending.remove(0);
                }
                self.pending
                    .push((id.to_string(), String::new(), String::new()));
                self.pending.len() - 1
            }
        };
        let entry = &mut self.pending[i];
        let field = if part == "title" {
            &mut entry.1
        } else {
            &mut entry.2
        };
        if field.len() < LIMIT {
            field.push_str(&text);
        }
        if !done {
            return None;
        }
        let (_, title, body) = self.pending.remove(i);
        Some(Event::Notify(clip(&title), clip(&body)))
    }

    fn parse(&mut self, body: &[u8]) -> Option<Event> {
        if let Some(rest) = body.strip_prefix(b"99;") {
            return self.osc99(rest);
        }
        if let Some(rest) = body.strip_prefix(b"777;notify;") {
            let text = String::from_utf8_lossy(rest);
            let (title, body) = text.split_once(';').unwrap_or((&text, ""));
            return Some(Event::Notify(clip(title), clip(body)));
        }
        if let Some(rest) = body.strip_prefix(b"9;") {
            // ConEmu's OSC 9;N;... (progress and the like) is not a notification.
            if rest.first().is_some_and(u8::is_ascii_digit)
                && rest.get(1).is_none_or(|b| *b == b';')
            {
                return None;
            }
            return Some(Event::Notify(
                clip(&String::from_utf8_lossy(rest)),
                String::new(),
            ));
        }
        parse_mark(body)
    }
}

fn parse_mark(body: &[u8]) -> Option<Event> {
    let rest = body.strip_prefix(b"133;")?;
    let (kind, params) = match rest.iter().position(|&b| b == b';') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &rest[rest.len()..]),
    };
    let pairs = || {
        params.split(|&b| b == b';').filter_map(|kv| {
            let i = kv.iter().position(|&b| b == b'=')?;
            Some((&kv[..i], &kv[i + 1..]))
        })
    };
    match kind {
        b"A" => {
            if pairs().any(|(k, v)| k == b"k" && v != b"i") {
                // A secondary or right-side prompt: the same command goes on.
                return None;
            }
            let click_events = pairs().any(|(k, v)| k == b"click_events" && v == b"1");
            Some(Event::Prompt { click_events })
        }
        b"B" => Some(Event::Input),
        b"C" => {
            let command = pairs().find_map(|(k, v)| match k {
                b"cmdline_url" => Some(command_text(&percent_decode(v))),
                b"cmdline" => Some(command_text(v)),
                _ => None,
            });
            Some(Event::Output {
                command: command.filter(|c| !c.is_empty()),
            })
        }
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

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `%XX` escapes decoded; a broken one (the text was cut short) is kept as it is.
fn percent_decode(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'%'
            && let (Some(h), Some(l)) = (
                text.get(i + 1).copied().and_then(hex_digit),
                text.get(i + 2).copied().and_then(hex_digit),
            )
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

/// A command line as one line of text: line breaks become spaces, other control characters go.
fn command_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim()
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .take(COMMAND_SHOWN)
        .collect()
}

/// The names in an XTGETTCAP query (`NAME;NAME`, each hex-encoded); `None` if any is not hex.
fn termcap_names(body: &[u8]) -> Option<Vec<String>> {
    body.split(|&b| b == b';')
        .map(|hex| {
            if hex.len() % 2 != 0 {
                return None;
            }
            let bytes = hex
                .chunks(2)
                .map(|p| Some(hex_digit(p[0])? << 4 | hex_digit(p[1])?))
                .collect::<Option<Vec<u8>>>()?;
            String::from_utf8(bytes).ok()
        })
        .collect()
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
                    } else if b == b'_' {
                        self.state = State::Apc;
                        self.body.clear();
                    } else if b == b'[' {
                        self.state = State::Csi;
                        self.body.clear();
                    } else if b == b'P' {
                        self.state = State::Dcs;
                        self.body.clear();
                    } else if b != 0x1b {
                        self.state = State::Idle;
                    }
                }
                State::Osc => match b {
                    0x07 => self.end(i + 1, &mut found),
                    0x1b => self.state = State::OscEsc,
                    // A command line past the limit is cut, and the mark kept.
                    _ if self.body.len() >= LIMIT && self.body.starts_with(b"133;C;") => {}
                    _ if self.body.len() >= LIMIT
                        || (self.body.len() >= MARK_LIMIT
                            && self.body.starts_with(b"133;")
                            && !self.body.starts_with(b"133;C;")) =>
                    {
                        self.state = State::Idle
                    }
                    _ => {
                        self.body.push(b);
                        // Only the OSCs looked at here are worth keeping.
                        if self.body.len() <= 4
                            && !PREFIXES.iter().any(|p| {
                                let n = self.body.len().min(p.len());
                                p[..n] == self.body[..n]
                            })
                        {
                            self.state = State::Idle;
                        }
                    }
                },
                State::Csi => match b {
                    b'0'..=b'9' | b';' if self.body.len() < 4 => self.body.push(b),
                    b'>' if self.body.is_empty() => self.body.push(b),
                    b't' if self.body == b"16" => {
                        found.push((i + 1, Event::CellSizeRequest));
                        self.state = State::Idle;
                    }
                    b'q' if self.body == b">" || self.body == b">0" => {
                        found.push((i + 1, Event::VersionRequest));
                        self.state = State::Idle;
                    }
                    0x1b => self.state = State::Esc,
                    _ => self.state = State::Idle,
                },
                State::Dcs => match b {
                    0x1b => self.state = State::DcsEsc,
                    b'+' if self.body.is_empty() => self.state = State::Tcap,
                    _ if self.body.len() >= APC_LIMIT => self.state = State::Idle,
                    _ => {
                        // Before the `q`, only a sixel's numeric parameters may come.
                        if !self.body.contains(&b'q') && !matches!(b, b'0'..=b'9' | b';' | b'q') {
                            self.state = State::Idle;
                        } else {
                            self.body.push(b);
                        }
                    }
                },
                State::Tcap => match b {
                    0x1b => self.state = State::TcapEsc,
                    _ if self.body.len() >= TCAP_LIMIT => self.state = State::Idle,
                    _ => self.body.push(b),
                },
                State::TcapEsc => {
                    let body = std::mem::take(&mut self.body);
                    if b == b'\\'
                        && let Some(names) = body.strip_prefix(b"q").and_then(termcap_names)
                    {
                        found.push((i + 1, Event::Termcap(names)));
                    }
                    // Otherwise a new escape sequence began: this one never ended.
                    self.state = match b {
                        b']' => State::Osc,
                        b'_' => State::Apc,
                        b'P' => State::Dcs,
                        b'[' => State::Csi,
                        _ => State::Idle,
                    };
                }
                State::DcsEsc => {
                    if b == b'\\' {
                        let body = std::mem::take(&mut self.body);
                        let q = body.iter().position(|&c| c == b'q').map_or(0, |q| q + 1);
                        found.push((i + 1, Event::Sixel(body[q..].to_vec())));
                        self.state = State::Idle;
                    } else {
                        self.body.clear();
                        self.state = match b {
                            b']' => State::Osc,
                            b'_' => State::Apc,
                            b'P' => State::Dcs,
                            b'[' => State::Csi,
                            _ => State::Idle,
                        };
                    }
                }
                State::Apc => match b {
                    0x1b => self.state = State::ApcEsc,
                    _ if self.body.len() >= APC_LIMIT => self.state = State::Idle,
                    _ => {
                        // Only kitty's graphics commands are looked at.
                        if self.body.is_empty() && b != b'G' {
                            self.state = State::Idle;
                        } else {
                            self.body.push(b);
                        }
                    }
                },
                State::ApcEsc => {
                    if b == b'\\' {
                        let mut body = std::mem::take(&mut self.body);
                        body.remove(0);
                        found.push((i + 1, Event::Graphics(body)));
                        self.state = State::Idle;
                    } else {
                        // A new escape sequence began: this one never ended.
                        self.body.clear();
                        self.state = match b {
                            b']' => State::Osc,
                            b'_' => State::Apc,
                            _ => State::Idle,
                        };
                    }
                }
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
        let body = std::mem::take(&mut self.body);
        if let Some(e) = self.parse(&body) {
            found.push((at, e));
        }
        self.body.clear();
        self.state = State::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(bytes: &[u8]) -> Vec<Event> {
        Scanner::default()
            .scan(bytes)
            .into_iter()
            .map(|(_, e)| e)
            .collect()
    }

    #[test]
    fn notifications_in_three_dialects() {
        let n = |t: &str, b: &str| Event::Notify(t.into(), b.into());
        assert_eq!(notes(b"\x1b]9;Build done\x07"), [n("Build done", "")]);
        assert_eq!(notes(b"\x1b]9;4;1;50\x07"), []);
        assert_eq!(
            notes(b"\x1b]777;notify;Title;Body text\x1b\\"),
            [n("Title", "Body text")]
        );
        assert_eq!(
            notes(b"\x1b]99;i=1:d=0;Hello\x1b\\\x1b]99;i=1:p=body;World\x1b\\"),
            [n("Hello", "World")]
        );
        assert_eq!(notes(b"\x1b]99;e=1;SGk=\x07"), [n("Hi", "")]);
        assert_eq!(notes(b"\x1b]99;p=?;\x07"), []);
        // Control characters are taken out.
        assert_eq!(notes(b"\x1b]9;a\x01b\x07"), [n("ab", "")]);
    }

    #[test]
    fn other_long_oscs_are_dropped_early() {
        let mut long = b"\x1b]52;c;".to_vec();
        long.extend(std::iter::repeat_n(b'A', 10_000));
        long.extend(b"\x07\x1b]133;A\x07");
        assert_eq!(
            notes(&long),
            [Event::Prompt {
                click_events: false
            }]
        );
    }

    #[test]
    fn marks_are_found_with_either_terminator() {
        let mut s = Scanner::default();
        let got = s.scan(b"x\x1b]133;A\x07yy\x1b]133;D;3\x1b\\z\x1b]133;C\x07");
        assert_eq!(
            got,
            [
                (
                    9,
                    Event::Prompt {
                        click_events: false
                    }
                ),
                (22, Event::Done(Some(3))),
                (31, Event::Output { command: None })
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
        assert_eq!(
            s.scan(b"\x1b]133;A\x07"),
            [(
                8,
                Event::Prompt {
                    click_events: false
                }
            )]
        );
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

    #[test]
    fn fish_prompt_options_and_command_lines() {
        assert_eq!(
            notes(b"\x1b]133;A;click_events=1\x1b\\"),
            [Event::Prompt { click_events: true }]
        );
        // A continuation prompt is not a new command.
        assert_eq!(notes(b"\x1b]133;A;k=s\x07"), []);
        assert_eq!(
            notes(b"\x1b]133;C;cmdline_url=echo%20hi%3B%20ls\x1b\\"),
            [Event::Output {
                command: Some("echo hi; ls".into())
            }]
        );
        assert_eq!(
            notes(b"\x1b]133;C;cmdline=make -j8\x07"),
            [Event::Output {
                command: Some("make -j8".into())
            }]
        );
        // Line breaks in a multi-line command become spaces; escapes are dropped.
        assert_eq!(
            notes(b"\x1b]133;C;cmdline_url=for%20x%0Aend%1B\x07"),
            [Event::Output {
                command: Some("for x end".into())
            }]
        );
    }

    #[test]
    fn a_long_command_line_is_cut_and_the_mark_kept() {
        let mut long = b"\x1b]133;C;cmdline_url=".to_vec();
        long.extend(std::iter::repeat_n(b'x', 20_000));
        long.push(0x07);
        let got = notes(&long);
        let [Event::Output { command: Some(c) }] = got.as_slice() else {
            panic!("{got:?}")
        };
        assert!(c.len() <= COMMAND_SHOWN && c.len() > 1000);
    }

    #[test]
    fn termcap_and_version_queries() {
        // fish asks for `indn` and `query-os-name`, then XTVERSION.
        assert_eq!(
            notes(b"\x1bP+q696e646e;71756572792d6f732d6e616d65\x1b\\\x1b[>0q\x1b[>q"),
            [
                Event::Termcap(vec!["indn".into(), "query-os-name".into()]),
                Event::VersionRequest,
                Event::VersionRequest
            ]
        );
        assert_eq!(notes(b"\x1bP+qzz\x1b\\"), []);
        // Sixel still works next to it.
        assert_eq!(notes(b"\x1bPq#0\x1b\\"), [Event::Sixel(b"#0".to_vec())]);
    }
}
