use super::*;

fn modes(bytes: &[u8]) -> Modes {
    let mut m = Modes::new();
    m.feed(bytes);
    m
}

fn preamble(bytes: &[u8]) -> String {
    String::from_utf8(modes(bytes).preamble()).unwrap()
}

#[test]
fn defaults_need_no_preamble() {
    assert_eq!(preamble(b"plain text\r\n\x1b[1;31mred\x1b[0m"), "");
}

#[test]
fn each_private_mode_round_trips() {
    assert_eq!(preamble(b"\x1b[?1h"), "\x1b[?1h");
    assert_eq!(preamble(b"\x1b[?7l"), "\x1b[?7l");
    assert_eq!(preamble(b"\x1b[?25l"), "\x1b[?25l");
    assert_eq!(preamble(b"\x1b[?1007l"), "\x1b[?1007l");
    assert_eq!(preamble(b"\x1b[?2004h"), "\x1b[?2004h");
    assert_eq!(preamble(b"\x1b[?1004h"), "\x1b[?1004h");
    assert_eq!(preamble(b"\x1b[4h\x1b[20h"), "\x1b[4h\x1b[20h");
    assert_eq!(preamble(b"\x1b="), "\x1b=");
    assert_eq!(preamble(b"\x1b=\x1b>"), "");
}

#[test]
fn only_1049_switches_screens() {
    assert_eq!(preamble(b"\x1b[?47h\x1b[?1047h"), "");
    assert_eq!(preamble(b"\x1b[?1049h"), "\x1b[?1049h");
    assert_eq!(preamble(b"\x1b[?1049h\x1b[?1049l"), "");
}

#[test]
fn mouse_modes_exclude_each_other() {
    assert_eq!(preamble(b"\x1b[?1000h\x1b[?1003h"), "\x1b[?1003h");
    // Unsetting another mode leaves the active one.
    assert_eq!(preamble(b"\x1b[?1003h\x1b[?1000l"), "\x1b[?1003h");
    assert_eq!(preamble(b"\x1b[?1003h\x1b[?1003l"), "");
    assert_eq!(
        preamble(b"\x1b[?1006h\x1b[?1005h\x1b[?1006l"),
        "\x1b[?1005h"
    );
    assert_eq!(preamble(b"\x1b[?1015h"), "");
}

#[test]
fn cursor_style_merges_shape_and_blink() {
    assert_eq!(preamble(b"\x1b[5 q"), "\x1b[5 q");
    assert_eq!(preamble(b"\x1b[6 q"), "\x1b[6 q");
    assert_eq!(preamble(b"\x1b[5 q\x1b[?12l"), "\x1b[6 q");
    // With no style yet, 12 starts from a steady block.
    assert_eq!(preamble(b"\x1b[?12h"), "\x1b[1 q");
    assert_eq!(preamble(b"\x1b[?12l"), "\x1b[2 q");
    assert_eq!(preamble(b"\x1b[3 q\x1b[0 q"), "");
    assert_eq!(preamble(b"\x1b[3 q\x1b[7 q"), "\x1b[3 q");
}

#[test]
fn ris_resets_everything() {
    assert_eq!(
        preamble(b"\x1b[?1049h\x1b[>3u\x1b[?1h\x1b=\x1b[5 q\x1bc"),
        ""
    );
}

#[test]
fn soft_reset_is_not_a_reset() {
    assert_eq!(preamble(b"\x1b[?1h\x1b[!p"), "\x1b[?1h");
}

#[test]
fn kitty_stacks_are_per_screen() {
    assert_eq!(
        preamble(b"\x1b[>1u\x1b[?1049h\x1b[>5u"),
        "\x1b[>1u\x1b[?1049h\x1b[>5u"
    );
    // Leaving the alternate screen keeps its stack for next time.
    assert_eq!(
        preamble(b"\x1b[>1u\x1b[?1049h\x1b[>5u\x1b[?1049l"),
        "\x1b[>1u\x1b[?1049h\x1b[>5u\x1b[?1049l"
    );
    assert_eq!(preamble(b"\x1b[>1u\x1b[>3u\x1b[<u"), "\x1b[>1u");
    assert_eq!(preamble(b"\x1b[>1u\x1b[>3u\x1b[<5u"), "");
}

#[test]
fn kitty_set_changes_only_the_active_flags() {
    // fish: `CSI = 5 u` with an empty stack.
    assert_eq!(preamble(b"\x1b[=5u"), "\x1b[=5;1u");
    assert_eq!(preamble(b"\x1b[>1u\x1b[=4;2u"), "\x1b[>1u\x1b[=5;1u");
    assert_eq!(preamble(b"\x1b[>7u\x1b[=2;3u"), "\x1b[>7u\x1b[=5;1u");
    // A screen switch reloads from the stack, losing the `=` change.
    assert_eq!(preamble(b"\x1b[=5u\x1b[?1049h\x1b[?1049l"), "");
}

#[test]
fn the_kitty_stack_drops_its_oldest_entry() {
    let mut stream = b"\x1b[>2u".to_vec();
    for _ in 0..KITTY_STACK {
        stream.extend_from_slice(b"\x1b[>1u");
    }
    let m = modes(&stream);
    assert_eq!(m.tracked.kitty[0].len(), KITTY_STACK);
    assert!(m.tracked.kitty[0].iter().all(|&f| f == 1));
}

#[test]
fn escape_ends_strings_and_sequences() {
    // An unterminated OSC, then a mode change: the OSC ends at the ESC.
    assert_eq!(preamble(b"\x1b]0;title\x1b[?1049h"), "\x1b[?1049h");
    assert_eq!(preamble(b"\x1bPq#0;1\x1b[?2004h"), "\x1b[?2004h");
    assert_eq!(preamble(b"\x1b_apc text\x1b[?2004h"), "\x1b[?2004h");
    // Inside a string, a mode change is text.
    assert_eq!(preamble(b"\x1b]0;\x9b?1049h\x07"), "");
    // BEL ends an OSC but not a DCS.
    assert_eq!(preamble(b"\x1bPq\x07\x1b[?1h"), "\x1b[?1h");
    let m = modes(b"\x1bPq\x07");
    assert_eq!(m.scan.state, State::DcsPassthrough);
    assert_eq!(modes(b"\x1bPq\x9c").scan.state, State::Ground);
    assert_eq!(modes(b"\x1b]0;x\x9c").scan.state, State::Osc);
}

#[test]
fn can_and_sub_abort() {
    assert_eq!(preamble(b"\x1b[?1049\x18h"), "");
    assert_eq!(preamble(b"\x1b]0;x\x1a\x1b[?1h"), "\x1b[?1h");
}

#[test]
fn controls_inside_a_csi_do_not_end_it() {
    assert_eq!(preamble(b"\x1b[?20\n04h"), "\x1b[?2004h");
}

#[test]
fn too_many_parameters_drop_the_sequence() {
    let mut s = b"\x1b[?".to_vec();
    s.extend_from_slice(&b"2004;".repeat(40));
    s.extend_from_slice(b"2004h");
    assert_eq!(preamble(&s), "");
    // 32 values exactly is still fine.
    let mut s = b"\x1b[?".to_vec();
    s.extend_from_slice(&b"1;".repeat(31));
    s.extend_from_slice(b"2004h");
    assert_eq!(preamble(&s), "\x1b[?1h\x1b[?2004h");
}

#[test]
fn subparameters_count_but_only_the_first_value_is_read() {
    assert_eq!(preamble(b"\x1b[?2004:5h"), "\x1b[?2004h");
}

#[test]
fn too_many_intermediates_drop_the_sequence() {
    assert_eq!(preamble(b"\x1b[?  1h"), "");
}

#[test]
fn chunking_does_not_matter() {
    let corpus: &[u8] = "héllo \u{1F600} wörld\r\n\x1b[?1049h\x1b[>1u\x1b]0;tïtle\x07\
                          \x1bPq#0\x1b\\\x1b[?1;2004h\x1b[=3;2u\x1b[5 q\x1b=\x1b[?1002h\
                          \x1b[?1006h text \x1b[38;2;1;2;3m\x1b[?12l\x1b[4h\x1b_x\x1b\\"
        .as_bytes();
    let whole = modes(corpus);
    for i in 0..=corpus.len() {
        let mut m = Modes::new();
        m.feed(&corpus[..i]);
        m.feed(&corpus[i..]);
        assert_eq!(m, whole, "split at {i}");
    }
    let mut m = Modes::new();
    for b in corpus {
        m.feed(std::slice::from_ref(b));
    }
    assert_eq!(m, whole);
}

fn start(bytes: &[u8]) -> (usize, Modes) {
    let mut m = Modes::new();
    let p = m.find_start(bytes);
    (p, m)
}

#[test]
fn the_start_follows_a_line_break() {
    assert_eq!(start(b"partial line\nwhole line\n").0, 13);
}

#[test]
fn the_start_skips_a_cut_sequence() {
    // The ring was cut inside `CSI ? 1049 h`; the rest is its tail.
    let mut m = Modes::new();
    m.feed(b"\x1b[?10");
    let p = m.find_start(b"49hscreen\x1b[2;1Hmore");
    assert_eq!(p, 3);
    assert!(m.tracked.alt_screen);
}

#[test]
fn the_start_skips_continuation_bytes() {
    // Cut inside "é" (C3 A9) and "😀" (F0 9F 98 80), with no line break.
    assert_eq!(start(b"\xa9x").0, 1);
    assert_eq!(start(b"\x9f\x98\x80x").0, 3);
}

#[test]
fn the_start_is_after_a_string_cut_in_two() {
    let mut m = Modes::new();
    m.feed(b"\x1b]0;a long ti");
    let p = m.find_start(b"tle\x07\x1b[?1hdraw");
    assert_eq!(p, 4);
    assert_eq!(m.scan.state, State::Ground);
    // The mode after the start is not read: the window will read it.
    assert_eq!(m.preamble(), b"");
}

#[test]
fn with_no_line_break_the_start_is_the_first_clean_place() {
    assert_eq!(start(b"\x1b[3;4Hdraw\x1b[5;6Hmore").0, 0);
}

#[test]
fn a_line_break_after_the_search_window_is_not_waited_for() {
    let mut s = vec![b'x'; LINE_SEARCH + 10];
    s.extend_from_slice(b"\nrest");
    assert_eq!(start(&s).0, 0);
}

#[test]
fn a_line_break_at_the_very_end_is_not_a_start() {
    assert_eq!(start(b"text\n").0, 0);
}

#[test]
fn nothing_clean_means_nothing_to_replay() {
    let mut m = Modes::new();
    m.feed(b"\x1b]0;");
    assert_eq!(m.find_start(b"never ends"), 10);
}

#[test]
fn find_start_leaves_the_state_at_the_start() {
    let bytes = b"\x1b[?1h\x1b[?2004";
    let mut m = Modes::new();
    let p = m.find_start(bytes);
    assert_eq!(p, 0);
    assert_eq!(m, Modes::new());
    let mut m = Modes::new();
    m.feed(b"\x1b[?25l");
    let p = m.find_start(b"ab\ncd");
    assert_eq!(p, 3);
    assert_eq!(m.preamble(), b"\x1b[?25l");
}
