mod common;

use common::{Rng, cases};
use kyko_core::config::*;
use kyko_core::keys::{Key, KeySeq, Mods};
use kyko_core::theme::{CustomColors, ThemeSource};

fn parse(text: &str) -> Loaded {
    Config::parse(text).unwrap_or_else(|e| panic!("{text}\n{e}"))
}

fn seq(s: &str) -> KeySeq {
    s.parse().unwrap()
}

/// Every key set to a valid value that is not its default.
const FULL: &str = r##"
[hotkey]
toggle = "Ctrl+Alt+T"

[window]
shape = "sheet"
tabs = "bottom"
height_pct = 60
width_pct = 70
opacity_pct = 85
monitor = "DP-1"
above_fullscreen = false
hide_on_focus_loss = false
animations = false
opacity = 0.9

[theme]
source = "custom"

[theme.custom]
base = "dracula-light"
foreground = "#101010"
background = "#202020"
cursor = "#303030"
selection = "#404040"
selection_text = "#505050"
ansi = ["#000001", "#000002"]

[terminal]
font = "Iosevka"
font_size = 13
line_height = 1.2
padding = 4
cursor = "beam"
cursor_blink = true
new_tab_dir = "home"
bell = "sound"
notify_on_finish = "unfocused"
notify_after = 30
copy_on_select = true
ligatures = false
shell_integration = false
confirm_multiline_paste = false
scrollback = 50000
shell = "/usr/bin/fish"

[sessions]
ring_kib = 4096

[keys]
new_tab = "Ctrl+Alt+N"
close_pane = "Ctrl+Alt+W"
prev_tab = "Ctrl+Shift+PgUp"
next_tab = "Ctrl+Shift+PgDn"
tab_1 = "Super+1"
tab_9 = "Super+9"
split_side = "Ctrl+Alt+D"
split_stacked = "Ctrl+Alt+E"
focus_left = "Ctrl+Alt+Left"
focus_right = "Ctrl+Alt+Right"
focus_up = "Ctrl+Alt+Up"
focus_down = "Ctrl+Alt+Down"
copy = "Ctrl+Alt+C"
paste = "Ctrl+Alt+V"
search = "Ctrl+Alt+F"
font_bigger = "Ctrl+="
font_smaller = "Ctrl+_"
font_reset = "Ctrl+Alt+0"

[advanced]
programs_write_clipboard = true
allow_run = false
"##;

#[test]
fn default_is_the_table_in_the_spec() {
    let d = Config::default();
    assert_eq!(d.hotkey, Some(seq("F12")));
    let w = &d.window;
    assert_eq!(w.shape, Shape::Island);
    assert_eq!(w.tabs, TabsAt::Top);
    assert_eq!(w.height_pct, 45);
    assert_eq!(w.width_pct, 88);
    assert_eq!(w.monitor, Monitor::Focused);
    assert!(w.above_fullscreen);
    assert!(w.hide_on_focus_loss);
    assert!(w.animations);
    assert_eq!(w.opacity, 1.0);
    assert_eq!(d.theme, ThemeSource::Auto);
    assert_eq!(d.custom, CustomColors::default());
    assert_eq!(d.custom.base, "gruvbox-dark");
    let t = &d.terminal;
    assert_eq!(t.font, None);
    assert_eq!(t.font_size, None);
    assert_eq!(t.line_height, 1.0);
    assert_eq!(t.padding, 8);
    assert_eq!(t.cursor, Cursor::Block);
    assert!(!t.cursor_blink);
    assert_eq!(t.new_tab_dir, NewTabDir::Current);
    assert_eq!(t.bell, Bell::Visual);
    assert!(!t.copy_on_select);
    assert!(t.ligatures);
    assert!(t.shell_integration);
    assert!(t.confirm_multiline_paste);
    assert_eq!(t.scrollback, 10_000);
    assert_eq!(t.shell, None);
    assert_eq!(d.sessions.ring_kib, 2048);
    assert!(!d.advanced.programs_write_clipboard);
    assert!(d.advanced.allow_run);
    // §2.3's default keys.
    let want = [
        ("new_tab", "Ctrl+Shift+T"),
        ("close_pane", "Ctrl+Shift+W"),
        ("prev_tab", "Ctrl+PgUp"),
        ("next_tab", "Ctrl+PgDn"),
        ("tab_1", "Alt+1"),
        ("tab_5", "Alt+5"),
        ("tab_9", "Alt+9"),
        ("split_side", "Ctrl+Shift+D"),
        ("split_stacked", "Ctrl+Shift+E"),
        ("focus_left", "Alt+Left"),
        ("focus_right", "Alt+Right"),
        ("focus_up", "Alt+Up"),
        ("focus_down", "Alt+Down"),
        ("copy", "Ctrl+Shift+C"),
        ("paste", "Ctrl+Shift+V"),
        ("search", "Ctrl+Shift+F"),
        ("font_bigger", "Ctrl++"),
        ("font_smaller", "Ctrl+-"),
        ("font_reset", "Ctrl+0"),
        ("prev_prompt", "Ctrl+Shift+Up"),
        ("next_prompt", "Ctrl+Shift+Down"),
        ("copy_last_output", "Ctrl+Shift+O"),
        ("show_scrollback", "Ctrl+Shift+H"),
        ("open_link", "Ctrl+Shift+L"),
        ("zoom_pane", "Ctrl+Shift+Z"),
        ("broadcast", "Ctrl+Shift+B"),
        ("unicode_input", "Ctrl+Shift+U"),
    ];
    for (name, s) in want {
        let action = Action::all().find(|a| a.key_name() == name).unwrap();
        assert_eq!(d.keys.get(action), Some(seq(s)), "{name}");
        assert_eq!(action.default_seq(), seq(s), "{name}");
    }
    assert_eq!(Action::all().count(), 33);
}

#[test]
fn empty_file_is_the_defaults() {
    for text in ["", "\n", "# only a comment\n"] {
        let l = parse(text);
        assert_eq!(l.config, Config::default());
        assert!(l.warnings.is_empty());
    }
}

#[test]
fn every_key_set() {
    let l = parse(FULL);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    let c = &l.config;
    assert_eq!(c.hotkey, Some(seq("Ctrl+Alt+T")));
    assert_eq!(
        c.window,
        WindowConfig {
            shape: Shape::Sheet,
            tabs: TabsAt::Bottom,
            height_pct: 60,
            width_pct: 70,
            opacity_pct: 85,
            monitor: Monitor::Output("DP-1".into()),
            above_fullscreen: false,
            hide_on_focus_loss: false,
            animations: false,
            opacity: 0.9,
        }
    );
    assert_eq!(c.theme, ThemeSource::Custom);
    let mut ansi = [None; 16];
    ansi[0] = Some(1);
    ansi[1] = Some(2);
    assert_eq!(
        c.custom,
        CustomColors {
            base: "dracula-light".into(),
            foreground: Some(0x101010),
            background: Some(0x202020),
            cursor: Some(0x303030),
            selection: Some(0x404040),
            selection_text: Some(0x505050),
            ansi,
        }
    );
    assert_eq!(
        c.terminal,
        TerminalConfig {
            font: Some("Iosevka".into()),
            font_size: Some(13),
            line_height: 1.2,
            padding: 4,
            cursor: Cursor::Beam,
            cursor_blink: true,
            new_tab_dir: NewTabDir::Home,
            bell: Bell::Sound,
            notify_on_finish: NotifyOnFinish::Unfocused,
            notify_after: 30,
            copy_on_select: true,
            ligatures: false,
            shell_integration: false,
            confirm_multiline_paste: false,
            scrollback: 50_000,
            shell: Some("/usr/bin/fish".into()),
        }
    );
    assert_eq!(c.sessions.ring_kib, 4096);
    assert_eq!(c.keys.get(Action::Tab(1)), Some(seq("Super+1")));
    assert_eq!(c.keys.get(Action::Tab(2)), Some(seq("Alt+2")));
    assert_eq!(c.keys.get(Action::FontSmaller), Some(seq("Ctrl+_")));
    assert!(c.advanced.programs_write_clipboard);
    assert!(!c.advanced.allow_run);
    for monitor in [("primary", Monitor::Primary), ("focused", Monitor::Focused)] {
        let l = parse(&format!("[window]\nmonitor = {:?}", monitor.0));
        assert_eq!(l.config.window.monitor, monitor.1);
    }
    for (src, want) in [
        ("uq", ThemeSource::Uq),
        (
            "builtin:velvet-light",
            ThemeSource::Builtin("velvet-light".into()),
        ),
    ] {
        let l = parse(&format!("[theme]\nsource = {src:?}"));
        assert_eq!(l.config.theme, want);
    }
}

/// (section, key, a bad value as TOML).
const BAD: &[(&str, &str, &str)] = &[
    ("hotkey", "toggle", "\"Ctrl+Nope\""),
    ("hotkey", "toggle", "12"),
    ("window", "shape", "\"round\""),
    ("window", "shape", "true"),
    ("window", "tabs", "\"left\""),
    ("window", "height_pct", "19"),
    ("window", "height_pct", "101"),
    ("window", "height_pct", "45.0"),
    ("window", "height_pct", "\"45\""),
    ("window", "width_pct", "39"),
    ("window", "opacity_pct", "29"),
    ("window", "width_pct", "-1"),
    ("window", "monitor", "\"\""),
    ("window", "monitor", "1"),
    ("window", "above_fullscreen", "\"yes\""),
    ("window", "hide_on_focus_loss", "1"),
    ("window", "animations", "[]"),
    ("window", "opacity", "0.49"),
    ("window", "opacity", "1.01"),
    ("window", "opacity", "nan"),
    ("window", "opacity", "inf"),
    ("window", "opacity", "-inf"),
    ("window", "opacity", "\"1\""),
    ("theme", "source", "\"solarized\""),
    ("theme", "source", "\"builtin:\""),
    ("theme", "source", "3"),
    ("terminal", "font", "\"\""),
    ("terminal", "font", "12"),
    ("terminal", "font_size", "5"),
    ("terminal", "font_size", "49"),
    ("terminal", "font_size", "12.5"),
    ("terminal", "line_height", "0.79"),
    ("terminal", "line_height", "2.01"),
    ("terminal", "line_height", "nan"),
    ("terminal", "line_height", "+inf"),
    ("terminal", "padding", "33"),
    ("terminal", "cursor", "\"bar\""),
    ("terminal", "cursor_blink", "0"),
    ("terminal", "new_tab_dir", "\"/tmp\""),
    ("terminal", "bell", "\"loud\""),
    ("terminal", "notify_on_finish", "\"sometimes\""),
    ("terminal", "notify_after", "86401"),
    ("terminal", "copy_on_select", "\"true\""),
    ("terminal", "confirm_multiline_paste", "{}"),
    ("terminal", "scrollback", "100001"),
    ("terminal", "scrollback", "-5"),
    ("terminal", "scrollback", "9223372036854775807"),
    ("terminal", "shell", "\"fish\""),
    ("terminal", "shell", "\"\""),
    ("terminal", "shell", "1"),
    ("sessions", "ring_kib", "255"),
    ("sessions", "ring_kib", "16385"),
    ("keys", "new_tab", "\"Ctrl+Shift+Shift+T\""),
    ("keys", "copy", "7"),
    ("keys", "tab_3", "\"F25\""),
    ("advanced", "allow_run", "\"no\""),
    ("advanced", "programs_write_clipboard", "1979-05-27"),
];

#[test]
fn a_bad_value_costs_only_that_key() {
    for (section, key, value) in BAD {
        let text = format!("[{section}]\n{key} = {value}\n");
        let l = parse(&text);
        assert_eq!(l.warnings.len(), 1, "{text}{:?}", l.warnings);
        assert_eq!(l.warnings[0].key, format!("{section}.{key}"), "{text}");
        assert_eq!(l.config, Config::default(), "{text}");
    }
}

#[test]
fn bad_values_in_a_full_file_cost_only_their_key() {
    let full = parse(FULL).config;
    for (section, key, value) in BAD {
        let Some(text) = replace(FULL, section, key, value) else {
            continue;
        };
        let l = parse(&text);
        assert_eq!(
            l.warnings.len(),
            1,
            "{section}.{key} = {value}: {:?}",
            l.warnings
        );
        assert_eq!(l.warnings[0].key, format!("{section}.{key}"));
        // Only that key went back to its default.
        let want = parse(&replace(FULL, section, key, "__drop__").unwrap()).config;
        assert_ne!(want, full, "{section}.{key}");
        assert_eq!(l.config, want, "{section}.{key} = {value}");
    }
}

/// `FULL` with `section.key`'s value replaced; `__drop__` removes the line.
fn replace(text: &str, section: &str, key: &str, value: &str) -> Option<String> {
    let mut out = String::new();
    let mut current = String::new();
    let mut found = false;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.to_string();
        }
        if current == section && line.starts_with(&format!("{key} = ")) {
            found = true;
            if value != "__drop__" {
                out.push_str(&format!("{key} = {value}\n"));
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    found.then_some(out)
}

#[test]
fn unknown_keys_and_sections() {
    let l = parse("stray = 2\n[window]\nhieght_pct = 50\nheight_pct = 50\n[colours]\nx = 1\n");
    let keys: Vec<&str> = l.warnings.iter().map(|w| w.key.as_str()).collect();
    assert_eq!(keys, ["colours", "stray", "window.hieght_pct"]);
    assert!(
        l.warnings[2]
            .message
            .contains("unknown key window.hieght_pct")
    );
    assert_eq!(l.config.window.height_pct, 50);
    let l = parse("[theme.custom]\nforegroud = \"#000000\"\n");
    assert_eq!(l.warnings[0].key, "theme.custom.foregroud");
    let l = parse("window = 3\n");
    assert_eq!(l.warnings[0].key, "window");
    assert_eq!(l.config, Config::default());
}

#[test]
fn custom_colours() {
    let l = parse("[theme.custom]\nbase = \"\"\nforeground = \"#abc\"\ncursor = \"red\"\n");
    let keys: Vec<&str> = l.warnings.iter().map(|w| w.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "theme.custom.base",
            "theme.custom.foreground",
            "theme.custom.cursor"
        ]
    );
    assert_eq!(l.config.custom, CustomColors::default());
    let twenty: Vec<String> = (0..20).map(|i| format!("\"#0000{i:02x}\"")).collect();
    let l = parse(&format!("[theme.custom]\nansi = [{}]\n", twenty.join(",")));
    assert_eq!(l.warnings.len(), 1);
    assert_eq!(l.warnings[0].key, "theme.custom.ansi");
    assert_eq!(l.config.custom.ansi[15], Some(15));
    let l = parse("[theme.custom]\nansi = [\"#000001\", \"nope\", \"#000003\"]\n");
    assert_eq!(l.warnings[0].key, "theme.custom.ansi[1]");
    assert_eq!(&l.config.custom.ansi[..4], &[Some(1), None, Some(3), None]);
    let l = parse("[theme.custom]\nansi = \"#000000\"\n");
    assert_eq!(l.warnings[0].key, "theme.custom.ansi");
}

#[test]
fn syntax_error_gives_its_line_and_column() {
    let e = Config::parse("[window]\nshape = \"island\"\nheight_pct = = 3\n").unwrap_err();
    assert_eq!(e.line, 3);
    assert!(e.column > 1, "{e}");
    assert!(!e.message.is_empty());
    let e = Config::parse("[window\n").unwrap_err();
    assert_eq!(e.line, 1);
    let e = Config::parse("é = \"ü\" x\n").unwrap_err();
    assert_eq!(e.line, 1);
    // Duplicate keys are a syntax error in TOML.
    assert!(Config::parse("[window]\nshape = \"island\"\nshape = \"sheet\"\n").is_err());
}

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
fn a_sequence_used_twice_goes_to_its_first_holder() {
    // The hotkey first.
    let l = parse("[hotkey]\ntoggle = \"Ctrl+Shift+T\"\n");
    assert_eq!(l.config.hotkey, Some(seq("Ctrl+Shift+T")));
    assert_eq!(l.config.keys.get(Action::NewTab), None);
    assert_eq!(l.warnings.len(), 1);
    assert_eq!(l.warnings[0].key, "keys.new_tab");
    assert!(l.warnings[0].message.contains("hotkey.toggle"));
    // Then the actions in order: copy is before paste.
    let l = parse("[keys]\npaste = \"Ctrl+Shift+C\"\n");
    assert_eq!(l.config.keys.get(Action::Copy), Some(seq("Ctrl+Shift+C")));
    assert_eq!(l.config.keys.get(Action::Paste), None);
    assert_eq!(l.warnings[0].key, "keys.paste");
    assert!(l.warnings[0].message.contains("keys.copy"));
    // `Ctrl+Shift++` is `Ctrl++`.
    let l = parse("[keys]\nfont_reset = \"Ctrl+Shift++\"\n");
    assert_eq!(l.config.keys.get(Action::FontReset), None);
    assert_eq!(l.warnings[0].key, "keys.font_reset");
    assert!(l.warnings[0].message.contains("keys.font_bigger"));
    // A swap is fine.
    let l = parse("[keys]\ncopy = \"Ctrl+Shift+V\"\npaste = \"Ctrl+Shift+C\"\n");
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn empty_string_unbinds() {
    let l = parse("[hotkey]\ntoggle = \"\"\n[keys]\nsearch = \"\"\n");
    assert!(l.warnings.is_empty());
    assert_eq!(l.config.hotkey, None);
    assert_eq!(l.config.keys.get(Action::Search), None);
    // Unbinding the hotkey frees F12 for an action.
    let l = parse("[hotkey]\ntoggle = \"\"\n[keys]\nsearch = \"F12\"\n");
    assert!(l.warnings.is_empty());
    let none = Mods::default();
    assert_eq!(
        l.config.keys.action_for(none, Key::F(12)),
        Some(Action::Search)
    );
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
