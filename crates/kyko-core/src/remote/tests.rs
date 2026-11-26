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
