use super::*;

fn p(s: &str) -> KeySeq {
    s.parse().unwrap_or_else(|e| panic!("{s:?}: {e}"))
}

fn mods(ctrl: bool, alt: bool, shift: bool, super_: bool) -> Mods {
    Mods {
        ctrl,
        alt,
        shift,
        super_,
    }
}

#[test]
fn defaults_parse_and_print_canonically() {
    for s in [
        "F12",
        "Ctrl+Shift+T",
        "Ctrl+Shift+W",
        "Ctrl+PgUp",
        "Ctrl+PgDn",
        "Alt+1",
        "Alt+9",
        "Ctrl+Shift+D",
        "Ctrl+Shift+E",
        "Alt+Left",
        "Alt+Right",
        "Alt+Up",
        "Alt+Down",
        "Ctrl+Shift+C",
        "Ctrl+Shift+V",
        "Ctrl+Shift+F",
        "Ctrl++",
        "Ctrl+-",
        "Ctrl+0",
    ] {
        assert_eq!(p(s).to_string(), s);
    }
    assert_eq!(p("F12").mods, Mods::default());
}

#[test]
fn aliases_and_case() {
    for (input, canonical) in [
        ("ctrl+pageup", "Ctrl+PgUp"),
        ("Control+PageDown", "Ctrl+PgDn"),
        ("escape", "Esc"),
        ("Shift+Return", "Shift+Enter"),
        ("Del", "Delete"),
        ("INS", "Insert"),
        ("meta+t", "Super+T"),
        ("Win+x", "Super+X"),
        ("logo+Space", "Super+Space"),
        ("shift+ctrl+t", "Ctrl+Shift+T"),
        ("super+shift+alt+ctrl+f1", "Ctrl+Alt+Shift+Super+F1"),
        ("f24", "F24"),
        ("+", "+"),
        ("Alt++", "Alt++"),
    ] {
        assert_eq!(p(input).to_string(), canonical, "{input}");
    }
}

#[test]
fn non_ascii_keeps_its_case() {
    assert_eq!(p("Ctrl+ß").key, Key::Char('ß'));
    assert_eq!(p("Ctrl+ß").to_string(), "Ctrl+ß");
    assert_eq!(p("Alt+é").key, Key::Char('é'));
    assert_eq!(p("Alt+É").key, Key::Char('É'));
    assert_eq!(p("Alt+ö").to_string(), "Alt+ö");
}

#[test]
fn errors() {
    for bad in [
        "",
        "Ctrl+",
        "Ctrl+Shift+",
        "Ctrl+Bogus",
        "Hyper+T",
        "Ctrl+A+B",
        "Ctrl+Ctrl+T",
        "Control+Ctrl+T",
        "F0",
        "F25",
        "F01",
        "Ctrl+\u{7}",
        "Ctrl+\t",
        "Ctrl+ ",
        "Ctrl++T",
        "+T",
        "++",
        "Ctrl+\u{1F600}x",
    ] {
        assert!(bad.parse::<KeySeq>().is_err(), "{bad:?} parsed");
    }
    let long = format!("Ctrl+{}", "x".repeat(60));
    assert_eq!(long.len(), 65);
    assert!(long.parse::<KeySeq>().unwrap_err().0.contains("64"));
}

#[test]
fn round_trip_every_key_with_every_modifier_set() {
    let chars = [
        'A', 'Z', '0', '9', '+', '-', '=', ',', 'ß', 'é', '/', '\\', '\'',
    ];
    let keys: Vec<Key> = Key::named()
        .chain(chars.iter().map(|c| Key::Char(*c)))
        .collect();
    for key in keys {
        for bits in 0..16u8 {
            let seq = KeySeq::new(
                mods(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0),
                key,
            );
            let text = seq.to_string();
            assert_eq!(p(&text), seq, "{text}");
            assert_eq!(p(&text).to_string(), text);
        }
    }
}

#[test]
fn matching() {
    let ctrl = mods(true, false, false, false);
    let ctrl_shift = mods(true, false, true, false);
    // `+` and `-`: Shift does not count on either side.
    assert!(p("Ctrl++").matches(ctrl_shift, Key::Char('+')));
    assert!(p("Ctrl++").matches(ctrl, Key::Char('+')));
    assert!(p("Ctrl+Shift++").matches(ctrl, Key::Char('+')));
    assert!(p("Ctrl+-").matches(ctrl_shift, Key::Char('-')));
    assert!(!p("Ctrl++").matches(Mods::default(), Key::Char('+')));
    // Letters and named keys: Shift counts.
    assert!(p("Ctrl+Shift+T").matches(ctrl_shift, Key::Char('T')));
    assert!(p("Ctrl+Shift+T").matches(ctrl_shift, Key::Char('t')));
    assert!(!p("Ctrl+Shift+T").matches(ctrl, Key::Char('T')));
    assert!(!p("Ctrl+T").matches(ctrl_shift, Key::Char('T')));
    assert!(p("Ctrl+PgUp").matches(ctrl, Key::PgUp));
    assert!(!p("Ctrl+PgUp").matches(ctrl_shift, Key::PgUp));
    assert!(!p("Ctrl+PgUp").matches(ctrl, Key::PgDn));
    // Clashes are symmetric.
    assert!(p("Ctrl++").clashes(&p("Ctrl+Shift++")));
    assert!(p("Ctrl+Shift++").clashes(&p("Ctrl++")));
    assert!(!p("Ctrl+T").clashes(&p("Ctrl+Shift+T")));
}

#[test]
fn a_qt_key_press_becomes_a_sequence() {
    // Ctrl+Shift+T: Qt reports Key_T (0x54) with Ctrl and Shift.
    let seq = KeySeq::from_qt(0x54, 0x0400_0000 | 0x0200_0000, "\u{14}").unwrap();
    assert_eq!(seq.to_string(), "Ctrl+Shift+T");
    assert_eq!(seq, "ctrl+shift+t".parse().unwrap());
    let seq = KeySeq::from_qt(0x0100_003b, 0x1000_0000, "").unwrap();
    assert_eq!(seq.to_string(), "Super+F12");
    assert_eq!(
        KeySeq::from_qt(0x0100_0016, 0x0400_0000, "")
            .unwrap()
            .to_string(),
        "Ctrl+PgUp"
    );
}

#[test]
fn a_modifier_alone_is_no_sequence() {
    for k in [0x0100_0020, 0x0100_0021, 0x0100_0022, 0x0100_0023] {
        assert_eq!(KeySeq::from_qt(k, 0x0400_0000, ""), None);
    }
}
