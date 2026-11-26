//! Parsing: the defaults, every key set, bad values, unknown keys, colours and syntax errors.

use super::*;

#[test]
fn default_is_the_table_in_the_spec() {
    let d = Config::default();
    assert_eq!(d.hotkey, Some(seq("F12")));
    let w = &d.window;
    assert_eq!(w.shape, Shape::Island);
    assert_eq!(w.tabs, TabsAt::Top);
    assert_eq!(w.height_pct, 45);
    assert_eq!(w.width_pct, 88);
    assert_eq!(w.position, Position::Top);
    assert_eq!(w.size_pct(), (88, 45));
    assert_eq!((w.side_width_pct, w.side_height_pct), (40, 100));
    assert_eq!((w.center_width_pct, w.center_height_pct), (60, 60));
    assert_eq!(w.dim_pct, 40);
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
    assert!(t.shell_extras);
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
        ("prev_failed", "Ctrl+Alt+Shift+Up"),
        ("next_failed", "Ctrl+Alt+Shift+Down"),
        ("copy_last_command", "Ctrl+Shift+K"),
        ("command_palette", "Ctrl+Shift+P"),
    ];
    for (name, s) in want {
        let action = Action::all().find(|a| a.key_name() == name).unwrap();
        assert_eq!(d.keys.get(action), Some(seq(s)), "{name}");
        assert_eq!(action.default_seq(), seq(s), "{name}");
    }
    assert_eq!(Action::all().count(), 37);
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
fn every_key_set() {
    let l = parse(FULL);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    let c = &l.config;
    assert_eq!(c.hotkey, Some(seq("Ctrl+Alt+T")));
    assert_eq!(
        c.window,
        WindowConfig {
            shape: Shape::Sheet,
            position: Position::Left,
            tabs: TabsAt::Bottom,
            height_pct: 60,
            width_pct: 70,
            side_width_pct: 35,
            side_height_pct: 90,
            center_width_pct: 50,
            center_height_pct: 55,
            dim_pct: 0,
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
            shell_extras: false,
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
