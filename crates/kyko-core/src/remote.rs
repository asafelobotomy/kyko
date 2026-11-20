//! `kyko @`: commands for a running kyko window from a shell or a script, over the window's
//! control socket (`$KYKO_LISTEN_ON`). One JSON request per connection, one JSON reply.
//!
//! The words a user types (`kyko @ send-text --match pane:3 ls\n`) become a [`Request`] here;
//! the window carries it out and answers with [`Reply`].

use serde::{Deserialize, Serialize};

/// Which panes a command is for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Match {
    /// The pane the command was typed in (`$KYKO_PANE_ID`), else the focused one.
    #[default]
    Here,
    Pane(u32),
    /// Every pane of a tab.
    Tab(u32),
    /// Every pane of the window.
    All,
}

impl Match {
    /// `pane:3`, `tab:2`, `all`; also a bare number as a pane.
    pub fn parse(s: &str) -> Result<Match, String> {
        let bad = || format!("{s:?}: use pane:N, tab:N or all");
        if s == "all" {
            return Ok(Match::All);
        }
        if let Ok(n) = s.parse() {
            return Ok(Match::Pane(n));
        }
        let (kind, n) = s.split_once(':').ok_or_else(bad)?;
        let n: u32 = n.parse().map_err(|_| bad())?;
        match kind {
            "pane" | "id" => Ok(Match::Pane(n)),
            "tab" => Ok(Match::Tab(n)),
            _ => Err(bad()),
        }
    }
}

/// Where `launch` puts its pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    #[default]
    Tab,
    SplitSide,
    SplitStacked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    /// The tabs and their panes.
    Ls,
    /// Types `text` into the panes, as if from the keyboard.
    SendText {
        #[serde(default)]
        target: Match,
        text: String,
    },
    /// The text of a pane: its screen, or its whole scrollback.
    GetText {
        #[serde(default)]
        target: Match,
        #[serde(default)]
        all: bool,
    },
    /// A new pane running `args` (the shell when empty).
    Launch {
        #[serde(default)]
        place: Place,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        /// Keep the pane open after the command ends.
        #[serde(default)]
        hold: bool,
    },
    FocusPane {
        pane: u32,
    },
    FocusTab {
        tab: u32,
    },
    ClosePane {
        #[serde(default)]
        target: Match,
    },
    SetTabTitle {
        #[serde(default)]
        tab: Option<u32>,
        title: String,
    },
    /// The focused pane fills its tab, or goes back.
    ToggleZoom,
    /// Typing in the active tab reaches all its panes, or one again.
    ToggleBroadcast,
}

/// The window's answer: `ok` with data, or an error for the user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

impl Reply {
    pub fn ok(data: serde_json::Value) -> Reply {
        Reply {
            ok: true,
            error: None,
            data,
        }
    }

    pub fn err(message: impl Into<String>) -> Reply {
        Reply {
            ok: false,
            error: Some(message.into()),
            data: serde_json::Value::Null,
        }
    }
}

/// The help `kyko @` prints.
pub const USAGE: &str = "\
usage: kyko @ COMMAND [OPTIONS]

  ls                                 the tabs and panes, as JSON
  send-text [--match M] [--stdin] TEXT
                                     type TEXT (\\n, \\t, \\e, \\xHH, \\\\ are understood)
  get-text [--match M] [--all]       a pane's screen, or with --all its scrollback
  launch [--type tab|split-side|split-stacked] [--cwd DIR] [--hold] [COMMAND...]
                                     a new pane; prints its id
  focus-pane ID | focus-tab ID
  close-pane [--match M]
  set-tab-title [--tab ID] TITLE     an empty TITLE goes back to the program's name
  zoom | broadcast                   toggle pane zoom / typing into every pane

  M is pane:ID, tab:ID or all; without it, the pane the command runs in (or the focused one).
  The window is the one in $KYKO_LISTEN_ON, or --to unix:PATH.";

/// Backslash escapes in `send-text`: `\n`, `\r`, `\t`, `\e`, `\a`, `\0`, `\\`, `\xHH`.
pub fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('a') => out.push('\x07'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('x') => {
                let hex: String = it.by_ref().take(2).collect();
                let v = u8::from_str_radix(&hex, 16)
                    .ok()
                    .filter(|_| hex.len() == 2)
                    .ok_or_else(|| format!("\\x{hex}: two hex digits expected"))?;
                out.push(char::from(v));
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Ok(out)
}

/// What the words after `kyko @` ask for, plus `--to` and whether the text comes from stdin.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub request: Request,
    pub to: Option<String>,
    /// `send-text --stdin`: the text is read from standard input and added to `text`.
    pub stdin: bool,
}

/// Parses `kyko @` words. `Err` carries a message (or the usage for `help`).
pub fn parse_args(words: &[String]) -> Result<Parsed, String> {
    let mut to = None;
    let mut rest: Vec<&str> = Vec::new();
    let mut i = 0;
    // `--to` may come before the command.
    while i < words.len() {
        let w = words[i].as_str();
        if w == "--to" {
            to = Some(words.get(i + 1).ok_or("--to needs a value")?.clone());
            i += 2;
            continue;
        }
        if let Some(v) = w.strip_prefix("--to=") {
            to = Some(v.to_string());
            i += 1;
            continue;
        }
        rest.extend(words[i..].iter().map(String::as_str));
        break;
    }
    let Some((&cmd, args)) = rest.split_first() else {
        return Err(USAGE.to_string());
    };
    let mut target = Match::Here;
    let mut all = false;
    let mut stdin = false;
    let mut hold = false;
    let mut place = Place::Tab;
    let mut cwd = None;
    let mut tab = None;
    let mut free: Vec<String> = Vec::new();
    let mut i = 0;
    // Options stop at the first free word for `launch` (the command keeps its own flags) and at
    // `--` everywhere.
    let mut options = true;
    while i < args.len() {
        let w = args[i];
        let value = |i: usize| -> Result<&str, String> {
            args.get(i + 1)
                .copied()
                .ok_or_else(|| format!("{w} needs a value"))
        };
        if options && w == "--" {
            options = false;
        } else if options && (w == "--match" || w == "-m") {
            target = Match::parse(value(i)?)?;
            i += 1;
        } else if options && w == "--all" {
            all = true;
        } else if options && w == "--stdin" {
            stdin = true;
        } else if options && w == "--hold" {
            hold = true;
        } else if options && w == "--type" {
            place = match value(i)? {
                "tab" => Place::Tab,
                "split-side" | "vsplit" => Place::SplitSide,
                "split-stacked" | "hsplit" => Place::SplitStacked,
                other => {
                    return Err(format!(
                        "--type {other}: use tab, split-side or split-stacked"
                    ));
                }
            };
            i += 1;
        } else if options && w == "--cwd" {
            cwd = Some(value(i)?.to_string());
            i += 1;
        } else if options && w == "--tab" {
            tab = Some(value(i)?.parse().map_err(|_| "--tab needs a tab id")?);
            i += 1;
        } else if options && w.starts_with("--") {
            return Err(format!("{cmd}: unknown option {w}"));
        } else {
            free.push(w.to_string());
            if cmd == "launch" {
                options = false;
            }
        }
        i += 1;
    }
    let id = |what: &str| -> Result<u32, String> {
        match free.as_slice() {
            [one] => one
                .parse()
                .map_err(|_| format!("{cmd}: {one:?} is not a {what} id")),
            _ => Err(format!("{cmd}: give one {what} id")),
        }
    };
    let request = match cmd {
        "ls" => Request::Ls,
        "send-text" => {
            if free.is_empty() && !stdin {
                return Err("send-text: give the text, or --stdin".into());
            }
            Request::SendText {
                target,
                text: unescape(&free.join(" "))?,
            }
        }
        "get-text" => Request::GetText { target, all },
        "launch" => Request::Launch {
            place,
            cwd,
            args: free,
            hold,
        },
        "focus-pane" => Request::FocusPane { pane: id("pane")? },
        "focus-tab" => Request::FocusTab { tab: id("tab")? },
        "close-pane" => Request::ClosePane { target },
        "set-tab-title" => Request::SetTabTitle {
            tab,
            title: free.join(" "),
        },
        "zoom" => Request::ToggleZoom,
        "broadcast" => Request::ToggleBroadcast,
        "help" | "--help" | "-h" => return Err(USAGE.to_string()),
        other => return Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    Ok(Parsed { request, to, stdin })
}

/// A line split into words as a shell would: blanks separate them, `'...'` keeps everything,
/// `"..."` keeps all but `\"` and `\\`, and a backslash outside quotes keeps the next character.
pub fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match it.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("a ' is not closed".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match it.next() {
                        Some('"') => break,
                        Some('\\') => match it.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err("a \" is not closed".into()),
                        },
                        Some(c) => word.push(c),
                        None => return Err("a \" is not closed".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.push(it.next().unwrap_or('\\'));
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// A session file (`kyko --session FILE`): one `kyko @` command per line, run in order when
/// the window opens; blank lines and `#` comments are skipped. `~/` in a `--cwd` is the home
/// folder. Errors name the line.
pub fn parse_session(text: &str, home: &str) -> Result<Vec<Request>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |e: String| format!("line {}: {}", n + 1, e.lines().next().unwrap_or(""));
        let words = split_words(line).map_err(at)?;
        let parsed = parse_args(&words).map_err(at)?;
        if parsed.to.is_some() || parsed.stdin {
            return Err(at("--to and --stdin have no place in a session file".into()));
        }
        let mut request = parsed.request;
        if let Request::Launch { cwd: Some(c), .. } = &mut request
            && let Some(rest) = c.strip_prefix("~/")
        {
            *c = format!("{home}/{rest}");
        }
        out.push(request);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn req(s: &str) -> Request {
        parse_args(&words(s)).unwrap().request
    }

    #[test]
    fn commands_parse() {
        assert_eq!(req("ls"), Request::Ls);
        assert_eq!(
            req("send-text --match pane:3 ls\\n"),
            Request::SendText {
                target: Match::Pane(3),
                text: "ls\n".into()
            }
        );
        assert_eq!(
            req("get-text -m tab:2 --all"),
            Request::GetText {
                target: Match::Tab(2),
                all: true
            }
        );
        assert_eq!(
            req("launch --type split-side --cwd /tmp htop -d 5"),
            Request::Launch {
                place: Place::SplitSide,
                cwd: Some("/tmp".into()),
                args: words("htop -d 5"),
                hold: false
            }
        );
        assert_eq!(req("focus-tab 4"), Request::FocusTab { tab: 4 });
        assert_eq!(
            req("set-tab-title --tab 2 build logs"),
            Request::SetTabTitle {
                tab: Some(2),
                title: "build logs".into()
            }
        );
        let p = parse_args(&words("--to unix:/x zoom")).unwrap();
        assert_eq!(p.to.as_deref(), Some("unix:/x"));
        assert_eq!(p.request, Request::ToggleZoom);
    }

    #[test]
    fn mistakes_say_what_is_wrong() {
        assert!(parse_args(&words("frobnicate")).is_err());
        assert!(parse_args(&words("focus-pane")).is_err());
        assert!(parse_args(&words("focus-pane x")).is_err());
        assert!(parse_args(&words("send-text")).is_err());
        assert!(parse_args(&words("get-text --match window:1")).is_err());
        assert!(parse_args(&words("ls --bogus")).is_err());
        assert!(parse_args(&[]).unwrap_err().starts_with("usage"));
    }

    #[test]
    fn words_split_like_a_shell() {
        assert_eq!(
            split_words(r#"launch --cwd "~/my dir" sh -c 'echo "hi"; ls' a\ b"#).unwrap(),
            [
                "launch",
                "--cwd",
                "~/my dir",
                "sh",
                "-c",
                "echo \"hi\"; ls",
                "a b"
            ]
        );
        assert!(split_words("echo 'open").is_err());
        assert_eq!(split_words("  ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn session_files_are_launch_lines() {
        let text =
            "# my day\nlaunch --cwd ~/src htop\n\nlaunch --type split-side\nset-tab-title work\n";
        let got = parse_session(text, "/home/u").unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(
            got[0],
            Request::Launch {
                place: Place::Tab,
                cwd: Some("/home/u/src".into()),
                args: vec!["htop".into()],
                hold: false
            }
        );
        let e = parse_session("ls\nfrob\n", "/h").unwrap_err();
        assert!(e.starts_with("line 2:"), "{e}");
        assert!(parse_session("send-text --stdin", "/h").is_err());
    }

    #[test]
    fn escapes() {
        assert_eq!(unescape(r"a\tb\e[1m\x41\\").unwrap(), "a\tb\x1b[1mA\\");
        assert_eq!(unescape(r"keep \q").unwrap(), "keep \\q");
        assert!(unescape(r"\x4").is_err());
    }

    #[test]
    fn requests_round_trip_as_json() {
        for r in [
            Request::Ls,
            req("send-text --match all hi"),
            req("launch --hold make"),
            req("close-pane -m 7"),
        ] {
            let text = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), r, "{text}");
        }
        assert_eq!(
            serde_json::to_string(&Request::Ls).unwrap(),
            r#"{"cmd":"ls"}"#
        );
    }
}
