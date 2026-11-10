use kyko_term::*;

const GRUVBOX: Colors = Colors {
    fg: 0xebdbb2,
    bg: 0x282828,
    cursor: 0xebdbb2,
    ansi: [
        0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374,
        0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
    ],
};

fn emu() -> Emulator {
    Emulator::new(80, 24, 1000)
}

fn snap(e: &Emulator) -> Snapshot {
    e.snapshot(&GRUVBOX)
}

fn screen(bytes: &[u8]) -> Snapshot {
    let mut e = emu();
    e.feed(bytes);
    snap(&e)
}

/// Row `r` as text: wide-char spacers skipped, trailing blanks trimmed.
fn row(s: &Snapshot, r: usize) -> String {
    let cols = s.cols as usize;
    let text: String = s.cells[r * cols..(r + 1) * cols]
        .iter()
        .filter(|c| c.flags & WIDE_SPACER == 0)
        .map(|c| c.ch)
        .collect();
    text.trim_end().to_string()
}

#[test]
fn plain_text() {
    assert_eq!(row(&screen(b"hi there\r\n"), 0), "hi there");
}

#[test]
fn ansi_red_uses_palette() {
    let s = screen(b"\x1b[31mX");
    assert_eq!((s.cells[0].ch, s.cells[0].fg), ('X', 0xcc241d));
}

#[test]
fn true_colour() {
    assert_eq!(screen(b"\x1b[38;2;1;2;3mX").cells[0].fg, 0x010203);
}

#[test]
fn colour_cube_and_grey() {
    assert_eq!(screen(b"\x1b[38;5;196mX").cells[0].fg, 0xff0000);
    assert_eq!(screen(b"\x1b[38;5;232mX").cells[0].fg, 0x080808);
}

#[test]
fn inverse_swaps() {
    let s = screen(b"\x1b[7mX");
    assert_eq!((s.cells[0].fg, s.cells[0].bg), (0x282828, 0xebdbb2));
}

#[test]
fn styles_set_flags() {
    let s = screen(b"\x1b[1;3;4;9mX");
    assert_eq!(
        s.cells[0].flags & (BOLD | ITALIC | UNDERLINE | STRIKE),
        BOLD | ITALIC | UNDERLINE | STRIKE
    );
}

#[test]
fn wide_char_two_cells() {
    let s = screen("日x".as_bytes());
    assert!(s.cells[0].flags & WIDE != 0 && s.cells[1].flags & WIDE_SPACER != 0);
    assert_eq!(s.cells[2].ch, 'x');
}

#[test]
fn invalid_utf8_shows_replacement() {
    let s = screen(b"\xff\xfeok");
    assert_eq!(row(&s, 0), "\u{fffd}\u{fffd}ok");
}

#[test]
fn cursor_position() {
    assert_eq!(screen(b"ab").cursor, Some((2, 0)));
}

#[test]
fn cursor_hidden() {
    assert_eq!(screen(b"\x1b[?25l").cursor, None);
}

#[test]
fn alternate_screen_and_back() {
    let mut e = emu();
    e.feed(b"main");
    e.feed(b"\x1b[?1049h\x1b[HALT"); // xterm keeps the cursor column on switching
    assert_eq!(row(&snap(&e), 0), "ALT");
    e.feed(b"\x1b[?1049l");
    assert_eq!(row(&snap(&e), 0), "main");
}

#[test]
fn resize_clamps() {
    let mut e = emu();
    e.resize(0, 0);
    let s = snap(&e);
    assert_eq!((s.cols, s.rows), (2, 1));
    e.resize(120, 40);
    assert_eq!(snap(&e).cols, 120);
}

#[test]
fn scrollback_is_bounded() {
    let mut e = Emulator::new(80, 24, 100);
    for i in 0..1000 {
        e.feed(format!("{i}\r\n").as_bytes());
    }
    assert_eq!(e.history_len(), 100);
}

#[test]
fn device_status_report_is_answered() {
    let mut e = emu();
    e.feed(b"\x1b[6n");
    assert_eq!(e.take_replies(), b"\x1b[1;1R");
    assert!(e.take_replies().is_empty());
}

#[test]
fn title_is_kept() {
    let mut e = emu();
    e.feed(b"\x1b]0;hello\x07");
    assert_eq!(e.title(), Some("hello"));
}

#[test]
fn replay_resets_first() {
    let mut e = emu();
    e.feed(b"old");
    e.replay(b"new\r\n", false);
    assert_eq!(row(&snap(&e), 0), "new");
}

#[test]
fn truncated_replay_skips_the_partial_first_line() {
    // The ring wrapped inside an escape sequence: "8;5;196mjunk\n" is the tail of a colour sequence.
    let mut e = emu();
    e.replay(b"8;5;196mjunk\r\nclean\r\n", true);
    assert_eq!(row(&snap(&e), 0), "clean");
}

#[test]
fn truncated_replay_with_no_newline_shows_nothing() {
    let mut e = emu();
    e.replay(b"partial", true);
    assert_eq!(row(&snap(&e), 0), "");
}

#[test]
fn truncated_replay_starting_mid_character_is_clean() {
    let mut e = emu();
    e.replay(&[0x97, 0xa5, b'\n', b'o', b'k'], true); // tail of a 3-byte character, then a line
    assert_eq!(row(&snap(&e), 0), "ok");
}

#[test]
fn synchronised_update_is_held_until_flushed() {
    let mut e = emu();
    e.feed(b"\x1b[?2026hhidden");
    assert!(e.pending_sync());
    assert_eq!(row(&snap(&e), 0), "");
    e.flush_sync();
    assert_eq!(row(&snap(&e), 0), "hidden");
}
