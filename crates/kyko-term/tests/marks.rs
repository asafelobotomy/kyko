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
