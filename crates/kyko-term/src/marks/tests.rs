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
