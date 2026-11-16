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
    e.replay(b"new\r\n");
    assert_eq!(row(&snap(&e), 0), "new");
}

#[test]
fn replay_feeds_everything() {
    // Cutting a wrapped ring is the holder's job now (protocol version 2): nothing is skipped.
    let mut e = emu();
    e.replay(b"\x1b[?1049hfirst\r\nsecond");
    assert!(e.modes().alt_screen);
    assert_eq!(row(&snap(&e), 0), "first");
    assert_eq!(row(&snap(&e), 1), "second");
}

#[test]
fn modes_follow_what_was_fed() {
    let mut e = emu();
    let m = e.modes();
    assert!(m.line_wrap && m.show_cursor && m.alternate_scroll && !m.app_cursor);
    e.feed(b"\x1b[?1h\x1b=\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b[?1004h\x1b[?25l\x1b[5 q");
    let m = e.modes();
    assert!(m.app_cursor && m.app_keypad && m.mouse_drag && m.sgr_mouse);
    assert!(m.bracketed_paste && m.focus_reporting && !m.show_cursor);
    assert_eq!(
        (m.cursor_shape, m.cursor_blinking),
        (kyko_term::Shape::Beam, true)
    );
}

#[test]
fn kitty_keyboard_is_off_unless_asked_for() {
    let mut e = emu();
    e.feed(b"\x1b[>1u");
    assert_eq!(e.modes().kitty_flags, 0);
    e.feed(b"\x1b[?u");
    assert!(e.take_replies().is_empty(), "the query was answered");
    e.set_kitty_keyboard(true);
    e.feed(b"\x1b[>1u");
    assert_eq!(e.modes().kitty_flags, 1);
    e.feed(b"\x1b[?u");
    assert_eq!(e.take_replies(), b"\x1b[?1u");
    // And it survives a replay.
    e.replay(b"\x1b[>5u");
    assert_eq!(e.modes().kitty_flags, 5);
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

#[test]
fn replay_discards_replies_to_old_queries() {
    let mut e = emu();
    e.replay(b"\x1b[6n\r\nprompt");
    assert!(e.take_replies().is_empty());
    assert_eq!(row(&snap(&e), 1), "prompt");
}

#[test]
fn queries_after_a_replay_are_still_answered() {
    let mut e = emu();
    e.replay(b"\x1b[6n\r\nprompt");
    e.feed(b"\x1b[6n");
    assert!(!e.take_replies().is_empty());
}

#[test]
fn scrolling_moves_the_view_into_the_history_and_back() {
    let mut e = Emulator::new(20, 3, 100);
    for n in 0..10 {
        e.feed(format!("line {n}\r\n").as_bytes());
    }
    assert_eq!(e.display_offset(), 0);
    e.scroll(2);
    assert_eq!(e.display_offset(), 2);
    let top = e.snapshot(&GRUVBOX).cells[..6]
        .iter()
        .map(|c| c.ch)
        .collect::<String>();
    assert!(top.starts_with("line"), "{top:?}");
    e.scroll(1000);
    assert_eq!(e.display_offset(), e.history_len());
    e.scroll_to_bottom();
    assert_eq!(e.display_offset(), 0);
}

#[test]
fn a_drag_selects_text_and_a_double_click_selects_a_word() {
    let mut e = Emulator::new(30, 3, 100);
    e.feed(b"hello brave world");
    e.select_start(6, 0, kyko_term::SelectKind::Simple, false);
    e.select_update(10, 0, true);
    assert_eq!(e.selection_text().as_deref(), Some("brave"));
    // The selected cells are drawn the other way round.
    let snap = e.snapshot(&GRUVBOX);
    assert_eq!(snap.cells[6].bg, GRUVBOX.fg);
    assert_eq!(snap.cells[0].bg, GRUVBOX.bg);
    e.select_clear();
    assert_eq!(e.selection_text(), None);
    e.select_start(13, 0, kyko_term::SelectKind::Word, false);
    assert_eq!(e.selection_text().as_deref(), Some("world"));
    e.select_start(2, 0, kyko_term::SelectKind::Line, false);
    assert!(e.selection_text().unwrap().starts_with("hello brave world"));
}

#[test]
fn search_finds_matches_in_the_history_and_steps_through_them() {
    let mut e = Emulator::new(30, 3, 100);
    for n in 0..12 {
        e.feed(format!("row {n} needle\r\n").as_bytes());
    }
    e.feed(b"the Needle again");
    // Smart case: all lower case finds every spelling, a capital only exact ones.
    assert_eq!(e.search_find("needle"), 13);
    assert_eq!(e.search_find("Needle"), 1);
    assert_eq!(e.search_find("needle"), 13);
    // The nearest match to the live screen comes first and is selected.
    assert_eq!(e.selection_text().as_deref(), Some("Needle"));
    assert_eq!(e.search_state(), Some((12, 13)));
    // Stepping back reaches an old match, which scrolls the view into the history.
    for _ in 0..12 {
        e.search_step(false);
    }
    assert_eq!(e.search_state(), Some((0, 13)));
    assert!(e.display_offset() > 0);
    assert_eq!(e.selection_text().as_deref(), Some("needle"));
    // Past the first it wraps to the last.
    assert_eq!(e.search_step(false), Some((12, 13)));
    assert_eq!(e.search_find("absent"), 0);
    assert_eq!(e.search_step(true), None);
    e.search_clear();
    assert_eq!(e.selection_text(), None);
}
