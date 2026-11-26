//! Loading from a file, key presses, the changes between two configs, and random input.

use super::*;

#[test]
fn load() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("kyko-core-config-load");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Missing: the defaults, no warning.
    let l = Config::load(&dir.join("absent.toml")).unwrap();
    assert_eq!(l.config, Config::default());
    assert!(l.warnings.is_empty());
    // A directory.
    assert!(matches!(
        Config::load(&dir),
        Err(LoadError::Unreadable { .. })
    ));
    // Not UTF-8.
    let bad = dir.join("latin1.toml");
    std::fs::write(&bad, b"[terminal]\nfont = \"Caf\xe9\"\n").unwrap();
    match Config::load(&bad) {
        Err(LoadError::Unreadable { message, path }) => {
            assert!(message.contains("UTF-8"));
            assert_eq!(path, bad);
        }
        other => panic!("{other:?}"),
    }
    // A syntax error.
    let broken = dir.join("broken.toml");
    std::fs::write(&broken, "[window\n").unwrap();
    assert!(matches!(Config::load(&broken), Err(LoadError::Syntax(_))));
    // A good file.
    let good = dir.join("config.toml");
    std::fs::write(&good, FULL).unwrap();
    assert_eq!(Config::load(&good).unwrap(), parse(FULL));
}

#[test]
fn action_for_a_key_press() {
    let k = Keys::default();
    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };
    assert_eq!(
        k.action_for(ctrl_shift, Key::Char('t')),
        Some(Action::NewTab)
    );
    assert_eq!(
        k.action_for(ctrl_shift, Key::Char('+')),
        Some(Action::FontBigger)
    );
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    assert_eq!(k.action_for(alt, Key::Char('3')), Some(Action::Tab(3)));
    assert_eq!(k.action_for(alt, Key::Char('0')), None);
}

#[test]
fn changes_by_group() {
    let d = Config::default();
    assert!(d.changes(&d).is_empty());
    let cases = [
        ("[hotkey]\ntoggle = \"F11\"", Changes::HOTKEY),
        ("[window]\nopacity = 0.8", Changes::WINDOW),
        ("[theme]\nsource = \"uq\"", Changes::THEME),
        ("[theme.custom]\ncursor = \"#ffffff\"", Changes::THEME),
        ("[terminal]\nfont = \"Iosevka\"", Changes::FONT),
        ("[terminal]\nfont_size = 14", Changes::FONT),
        ("[terminal]\nline_height = 1.5", Changes::FONT),
        ("[terminal]\nbell = \"none\"", Changes::TERMINAL),
        ("[terminal]\nshell = \"/bin/zsh\"", Changes::TERMINAL),
        ("[keys]\ncopy = \"Ctrl+Insert\"", Changes::KEYS),
        ("[sessions]\nring_kib = 512", Changes::SESSIONS),
        ("[advanced]\nallow_run = false", Changes::ADVANCED),
    ];
    for (text, group) in cases {
        let c = parse(text).config;
        assert_eq!(c.changes(&d), group, "{text}");
        assert_eq!(d.changes(&c), group, "{text}");
    }
    let both = parse("[terminal]\nfont = \"X\"\nbell = \"none\"").config;
    let ch = both.changes(&d);
    assert!(ch.contains(Changes::FONT) && ch.contains(Changes::TERMINAL));
    assert!(!ch.contains(Changes::KEYS));
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng::new(0x00C0_FFEE);
    for _ in 0..cases(10_000) {
        let bytes = rng.bytes(200);
        let _ = Config::parse(&String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn random_mutations_never_panic() {
    let mut rng = Rng::new(0xFEED);
    let full = FULL.as_bytes();
    for _ in 0..cases(10_000) {
        let mut b = full.to_vec();
        for _ in 0..=rng.below(8) {
            let i = rng.below(b.len());
            match rng.below(3) {
                0 => b[i] = rng.next() as u8,
                1 => {
                    b.remove(i);
                }
                _ => b.insert(i, *rng.pick(b"=[]\"{},.#\n+-0 ")),
            }
        }
        let _ = Config::parse(&String::from_utf8_lossy(&b));
    }
}

#[test]
fn structured_mutations_cost_one_key() {
    let mut rng = Rng::new(7);
    for _ in 0..cases(500) {
        let (section, key, value) = rng.pick(BAD);
        let Some(text) = replace(FULL, section, key, value) else {
            continue;
        };
        let l = parse(&text);
        assert_eq!(l.warnings.len(), 1, "{section}.{key}");
    }
}

#[test]
fn each_position_keeps_its_own_size() {
    let mut w = Config::default().window;
    for (p, size) in [
        (Position::Top, (88, 45)),
        (Position::Bottom, (88, 45)),
        (Position::Left, (40, 100)),
        (Position::Right, (40, 100)),
        (Position::Center, (60, 60)),
    ] {
        w.position = p;
        assert_eq!(w.size_pct(), size, "{}", p.name());
        let (wk, hk) = p.size_keys();
        assert!(wk.ends_with("width_pct") && hk.ends_with("height_pct"));
    }
}
