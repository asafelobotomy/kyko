//! Prompt marks: the gutter, jumping between prompts, and selecting the last command's output,
//! driven by the byte stream a marked-up shell prints.
use kyko_term::*;

const COLORS: Colors = Colors {
    fg: 0xebdbb2,
    bg: 0x282828,
    cursor: 0xebdbb2,
    ansi: [0; 16],
};

const A: &str = "\x1b]133;A\x07";
const C: &str = "\x1b]133;C\x07";
fn d(n: u8) -> String {
    format!("\x1b]133;D;{n}\x07")
}

/// A shell session: `ls` printing two lines and succeeding, `false` printing nothing and failing,
/// then a fresh prompt.
fn session() -> String {
    format!(
        "{A}$ ls\r\n{C}one\r\ntwo\r\n{}{A}$ false\r\n{C}{}{A}$ ",
        d(0),
        d(1)
    )
}

fn gutter(e: &Emulator) -> Vec<u8> {
    e.snapshot(&COLORS).gutter
}

#[test]
fn a_finished_command_is_green_or_red_and_the_open_prompt_is_not_marked() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(session().as_bytes());
    let g = gutter(&e);
    // $ ls, one, two: green. $ false: red. The prompt waiting for input: nothing.
    assert_eq!(
        &g[..6],
        [
            GUTTER_OK,
            GUTTER_OK,
            GUTTER_OK,
            GUTTER_FAILED,
            GUTTER_NONE,
            GUTTER_NONE
        ]
    );
}

#[test]
fn a_running_command_is_shown_as_running() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(format!("{A}$ sleep 9\r\n{C}waiting\r\n").as_bytes());
    let g = gutter(&e);
    assert_eq!(&g[..3], [GUTTER_RUNNING; 3]);
    assert_eq!(g[3], GUTTER_RUNNING);
}

#[test]
fn marks_split_across_pieces_work_and_a_shell_without_marks_shows_nothing() {
    let mut e = Emulator::new(20, 12, 100);
    for piece in session().as_bytes().chunks(3) {
        e.feed(piece);
    }
    assert_eq!(gutter(&e)[0], GUTTER_OK);
    let mut plain = Emulator::new(20, 12, 100);
    plain.feed(b"$ ls\r\none\r\n$ ");
    assert!(gutter(&plain).iter().all(|g| *g == GUTTER_NONE));
}

#[test]
fn marks_are_not_drawn() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(session().as_bytes());
    let s = e.snapshot(&COLORS);
    assert_eq!(s.cells[0].ch, '$');
    assert!(
        s.cells
            .iter()
            .all(|c| !c.ch.is_control() && c.ch != '\u{F0000}')
    );
    assert_eq!(e.selection_text(), None);
}

#[test]
fn the_last_output_is_selected_and_copied_without_the_prompt() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(format!("{A}$ ls\r\n{C}one\r\ntwo\r\n{}{A}$ ", d(0)).as_bytes());
    assert!(e.select_last_output());
    assert_eq!(e.selection_text().unwrap().trim_end(), "one\ntwo");
}

#[test]
fn a_command_without_output_has_none_to_select() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(format!("{A}$ true\r\n{C}{}{A}$ ", d(0)).as_bytes());
    assert!(!e.select_last_output());
    let mut none = Emulator::new(20, 12, 100);
    none.feed(b"$ ");
    assert!(!none.select_last_output());
}

#[test]
fn output_that_ends_without_a_newline_includes_its_last_line() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(format!("{A}$ printf hi\r\n{C}hi{}\r\n{A}$ ", d(0)).as_bytes());
    assert!(e.select_last_output());
    assert_eq!(e.selection_text().unwrap().trim_end(), "hi");
}

fn long_session() -> String {
    // Thirty commands, each with a line of output: far more than 6 rows.
    (0..30)
        .map(|i| format!("{A}$ cmd{i}\r\n{C}out{i}\r\n{}", d(0)))
        .collect::<String>()
        + A
        + "$ "
}

#[test]
fn jumping_walks_back_and_forth_between_prompts() {
    let mut e = Emulator::new(20, 6, 1000);
    e.feed(long_session().as_bytes());
    assert_eq!(e.display_offset(), 0);
    let mut seen = Vec::new();
    for _ in 0..4 {
        assert!(e.jump_prompt(-1));
        seen.push(e.display_offset());
    }
    // Each jump goes back to an earlier prompt: two lines each.
    assert!(seen.windows(2).all(|w| w[1] > w[0]), "{seen:?}");
    assert_eq!(seen[1] - seen[0], 2);
    let back = e.display_offset();
    assert!(e.jump_prompt(1));
    assert!(e.display_offset() < back);
    // All the way up, then no further.
    while e.jump_prompt(-1) {}
    assert!(!e.jump_prompt(-1));
    // And all the way down to the live screen.
    while e.jump_prompt(1) {}
    assert_eq!(e.display_offset(), 0);
}

#[test]
fn marks_survive_the_scrollback_limit() {
    let mut e = Emulator::new(20, 6, 10);
    e.feed(long_session().as_bytes());
    // Only the last few commands are still in the history, and their statuses are right.
    assert!(e.jump_prompt(-1));
    let g = gutter(&e);
    assert!(g.iter().all(|g| *g == GUTTER_OK || *g == GUTTER_NONE));
    assert!(g.contains(&GUTTER_OK));
}

#[test]
fn a_full_screen_program_does_not_get_marks() {
    let mut e = Emulator::new(20, 6, 100);
    e.feed(format!("{A}$ vim\r\n{C}\x1b[?1049h{A}hello").as_bytes());
    // The mark inside the alternate screen is ignored; leaving it shows the shell's screen again.
    assert!(gutter(&e).iter().all(|g| *g == GUTTER_NONE));
    e.feed(format!("\x1b[?1049l{}{A}$ ", d(0)).as_bytes());
    assert_eq!(gutter(&e)[0], GUTTER_OK);
}

#[test]
fn replay_starts_clean() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(session().as_bytes());
    e.replay(b"plain\r\n");
    assert!(gutter(&e).iter().all(|g| *g == GUTTER_NONE));
}

#[test]
fn a_flood_of_output_keeps_the_history_at_its_limit_and_the_marks_true() {
    let mut e = Emulator::new(20, 6, 100);
    let mut text = String::new();
    // Far more lines than the history holds, in one feed, with a prompt and its status last.
    text.push_str(&format!("{A}$ yes\r\n{C}"));
    for i in 0..20_000 {
        text.push_str(&format!("line {i}\r\n"));
    }
    text.push_str(&d(0));
    text.push_str(&format!("{A}$ "));
    e.feed(text.as_bytes());
    assert_eq!(e.history_len(), 100);
    // The big command's own prompt is long gone; only the last prompt is left, with no status.
    assert!(!e.jump_prompt(-1) || e.display_offset() <= 100);
    assert!(e.select_last_output());
    let out = e.selection_text().unwrap();
    // Its output ran past the history, so what is left of it starts mid-way, but it ends with the
    // last line printed.
    assert!(out.trim_end().ends_with("line 19999"), "{out}");
}

#[test]
fn a_resize_before_anything_wraps_keeps_the_marks_and_a_rewrap_drops_them() {
    let mut e = Emulator::new(80, 24, 100);
    e.feed(format!("{A}$ ls\r\n{C}one\r\n{}{A}$ ", d(0)).as_bytes());
    e.resize(40, 24);
    assert_eq!(gutter(&e)[0], GUTTER_OK);
    // A line longer than the new width is re-wrapped: the numbering is no longer trusted.
    let long = "x".repeat(70);
    let mut w = Emulator::new(80, 24, 100);
    w.feed(format!("{A}$ ls\r\n{C}{long}\r\n{}{A}$ ", d(0)).as_bytes());
    w.resize(20, 24);
    assert!(gutter(&w).iter().all(|g| *g == GUTTER_NONE));
}

#[test]
fn finished_commands_are_reported_with_their_status() {
    let mut e = Emulator::new(20, 12, 100);
    e.feed(session().as_bytes());
    let done: Vec<u8> = e.take_finished().into_iter().map(|f| f.status).collect();
    assert_eq!(done, [0, 1]);
    assert!(e.take_finished().is_empty());
    // A D with no C before it (an empty command line) is not a finished command.
    e.feed(format!("{}{A}$ ", d(0)).as_bytes());
    assert!(e.take_finished().is_empty());
}

#[test]
fn a_shell_that_marks_twice_reports_one_command_with_its_line() {
    // fish 4 marks on its own and kyko's script marks too: each sequence comes twice.
    let mut e = Emulator::new(40, 12, 100);
    let a = "\x1b]133;A;click_events=1\x1b\\";
    let c = "\x1b]133;C;cmdline_url=cargo%20build\x1b\\";
    e.feed(
        format!(
            "{A}{a}$ cargo build\r\n{c}{C}done\r\n{}{}{A}{a}$ ",
            d(2),
            d(2)
        )
        .as_bytes(),
    );
    let f = e.take_finished();
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].status, 2);
    assert_eq!(f[0].command.as_deref(), Some("cargo build"));
    assert_eq!(e.command_at(0), Some("cargo build"));
    assert_eq!(e.command_at(1), Some("cargo build"));
    assert_eq!(e.command_at(2), None);
    assert_eq!(gutter(&e)[0], GUTTER_FAILED);
}

#[test]
fn termcap_and_version_queries_are_answered() {
    let mut e = Emulator::new(40, 12, 100);
    e.feed(b"\x1bP+q696e646e;6e6f7065\x1b\\\x1b[>0q");
    let r = String::from_utf8(e.take_replies()).unwrap();
    // indn = ESC [ %p1%d S; `nope` is unknown.
    assert!(
        r.starts_with("\x1bP1+r696E646E=1B5B257031256453\x1b\\"),
        "{r:?}"
    );
    assert!(r.contains("\x1bP0+r6E6F7065\x1b\\"), "{r:?}");
    assert!(
        r.contains(&format!("\x1bP>|kyko {}\x1b\\", env!("CARGO_PKG_VERSION"))),
        "{r:?}"
    );
}

const B: &str = "\x1b]133;B\x07";

fn cl(cmd: &str) -> String {
    format!("\x1b]133;C;cmdline_url={}\x07", cmd.replace(' ', "%20"))
}

#[test]
fn blocks_know_their_prompt_output_status_and_command() {
    let mut e = Emulator::new(30, 12, 100);
    e.feed(
        format!(
            "{A}$ {B}ls\r\n{}one\r\ntwo\r\n{}{A}$ {B}false\r\n{}{}{A}$ {B}",
            cl("ls"),
            d(0),
            cl("false"),
            d(1)
        )
        .as_bytes(),
    );
    let b = e.block_at(2).unwrap();
    assert_eq!(b.prompt, 0);
    assert_eq!(b.output, Some((1, 2)));
    assert_eq!(b.status, Some(0));
    assert_eq!(b.command.as_deref(), Some("ls"));
    let f = e.block_at(3).unwrap();
    assert_eq!((f.prompt, f.output, f.status), (3, None, Some(1)));
    assert!(e.select_output_at(1));
    assert_eq!(e.selection_text().as_deref(), Some("one\ntwo\n"));
    assert!(!e.select_output_at(3));
}

#[test]
fn a_running_commands_output_runs_to_the_cursor() {
    let mut e = Emulator::new(30, 12, 100);
    e.feed(format!("{A}$ {B}make\r\n{}a\r\nb\r\nc", cl("make")).as_bytes());
    assert_eq!(e.block_at(0).unwrap().output, Some((1, 3)));
    assert_eq!(e.block_at(0).unwrap().status, None);
}

#[test]
fn jumping_between_failed_commands_and_the_pinned_header() {
    let mut e = Emulator::new(30, 5, 200);
    let mut s = String::new();
    for (i, code) in [(0, 0), (1, 3), (2, 0), (3, 1)] {
        s += &format!("{A}$ {B}c{i}\r\n{}", cl(&format!("c{i}")));
        for k in 0..6 {
            s += &format!("out{i}.{k}\r\n");
        }
        s += &d(code);
    }
    s += &format!("{A}$ {B}");
    e.feed(s.as_bytes());
    assert!(e.jump_failed(-1));
    assert_eq!(e.block_at(0).unwrap().command.as_deref(), Some("c3"));
    assert!(e.jump_failed(-1));
    assert_eq!(e.block_at(0).unwrap().command.as_deref(), Some("c1"));
    assert!(!e.jump_failed(-1));
    // At c1's prompt nothing is pinned; two rows down into its output it is.
    assert_eq!(e.sticky_header(), None);
    e.scroll(-2);
    assert_eq!(e.sticky_header().as_deref(), Some("c1"));
    assert!(e.jump_failed(1));
    assert_eq!(e.block_at(0).unwrap().command.as_deref(), Some("c3"));
}

#[test]
fn a_click_on_the_command_line_moves_the_cursor_with_arrows() {
    let mut e = Emulator::new(20, 6, 100);
    e.feed(format!("{A}$ {B}echo hello").as_bytes());
    // Cursor after "hello" (column 12). A click on the "h" of hello (column 7): 5 lefts.
    assert_eq!(e.click_to_cursor(7, 0), Some(b"\x1b[D".repeat(5)));
    // Past the end of the text: the cursor is already there.
    assert_eq!(e.click_to_cursor(18, 0), Some(Vec::new()));
    // On the prompt itself, or below the command line: not a move.
    assert_eq!(e.click_to_cursor(0, 0), None);
    assert_eq!(e.click_to_cursor(3, 4), None);
    // Back to the start, then a click on the end: rights, across a wide character.
    e.feed(b"\x1b[10D");
    let r = e.click_to_cursor(12, 0).unwrap();
    assert_eq!(r, b"\x1b[C".repeat(10));
    // A running command takes the screen: clicks are left alone.
    e.feed(format!("\r\n{}", cl("echo hello")).as_bytes());
    assert_eq!(e.click_to_cursor(1, 1), None);
}

#[test]
fn a_prompt_that_asks_for_clicks_gets_a_mouse_click() {
    let mut e = Emulator::new(20, 6, 100);
    e.feed(format!("\x1b]133;A;click_events=1\x07> {B}ls -la").as_bytes());
    assert_eq!(
        e.click_to_cursor(3, 0),
        Some(b"\x1b[<0;4;1M\x1b[<0;4;1m".to_vec())
    );
}

#[test]
fn a_wide_character_is_one_arrow() {
    let mut e = Emulator::new(20, 6, 100);
    e.feed(format!("{A}$ {B}a界b").as_bytes());
    // Columns: a=2, 界=3-4, b=5, cursor at 6. Clicking a: three lefts (b, 界, a).
    assert_eq!(e.click_to_cursor(2, 0), Some(b"\x1b[D".repeat(3)));
}
