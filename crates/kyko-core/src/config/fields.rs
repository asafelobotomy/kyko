//! Reading each section of `config.toml` into [`Config`], key by key.

use super::*;

impl Reader {
    pub(crate) fn config(&mut self, root: &Table) -> Config {
        let d = Config::default();
        for key in root.keys() {
            if !SECTIONS.contains(&key.as_str()) {
                self.warn(key.as_str(), format!("unknown key {key}"));
            }
        }

        let s = self.section(root, "hotkey", "hotkey", &["toggle"]);
        let hotkey = match self.seq(&s, "toggle") {
            Some(Ok(seq)) => seq,
            _ => d.hotkey,
        };

        let s = self.section(
            root,
            "window",
            "window",
            &[
                "shape",
                "position",
                "tabs",
                "height_pct",
                "width_pct",
                "side_width_pct",
                "side_height_pct",
                "center_width_pct",
                "center_height_pct",
                "dim_pct",
                "opacity_pct",
                "monitor",
                "above_fullscreen",
                "hide_on_focus_loss",
                "animations",
                "opacity",
            ],
        );
        let dw = &d.window;
        let monitor = match self.string(&s, "monitor") {
            None => dw.monitor.clone(),
            Some("focused") => Monitor::Focused,
            Some("primary") => Monitor::Primary,
            Some("") => {
                self.warn("window.monitor", "empty");
                dw.monitor.clone()
            }
            Some(name) => Monitor::Output(name.to_string()),
        };
        let window = WindowConfig {
            shape: self.choice(
                &s,
                "shape",
                &[("island", Shape::Island), ("sheet", Shape::Sheet)],
                dw.shape,
            ),
            position: self.choice(
                &s,
                "position",
                &[
                    ("top", Position::Top),
                    ("bottom", Position::Bottom),
                    ("left", Position::Left),
                    ("right", Position::Right),
                    ("center", Position::Center),
                ],
                dw.position,
            ),
            tabs: self.choice(
                &s,
                "tabs",
                &[("top", TabsAt::Top), ("bottom", TabsAt::Bottom)],
                dw.tabs,
            ),
            height_pct: self.int(&s, "height_pct", 20..=100, dw.height_pct),
            width_pct: self.int(&s, "width_pct", 20..=100, dw.width_pct),
            side_width_pct: self.int(&s, "side_width_pct", 20..=100, dw.side_width_pct),
            side_height_pct: self.int(&s, "side_height_pct", 20..=100, dw.side_height_pct),
            center_width_pct: self.int(&s, "center_width_pct", 20..=100, dw.center_width_pct),
            center_height_pct: self.int(&s, "center_height_pct", 20..=100, dw.center_height_pct),
            dim_pct: self.int(&s, "dim_pct", 0..=80, dw.dim_pct),
            opacity_pct: self.int(&s, "opacity_pct", 30..=100, dw.opacity_pct),
            monitor,
            above_fullscreen: self.bool(&s, "above_fullscreen", dw.above_fullscreen),
            hide_on_focus_loss: self.bool(&s, "hide_on_focus_loss", dw.hide_on_focus_loss),
            animations: self.bool(&s, "animations", dw.animations),
            opacity: self.float(&s, "opacity", 0.5..=1.0, dw.opacity),
        };

        let s = self.section(root, "theme", "theme", &["source", "custom"]);
        let theme = match self.string(&s, "source") {
            None => ThemeSource::Auto,
            Some("uq") => ThemeSource::Uq,
            Some("custom") => ThemeSource::Custom,
            Some(v) => match v.strip_prefix("builtin:") {
                Some(key) if !key.is_empty() => ThemeSource::Builtin(key.to_string()),
                _ => {
                    self.warn(
                        "theme.source",
                        format!("{v:?} is not uq, builtin:<id>, or custom"),
                    );
                    ThemeSource::Auto
                }
            },
        };
        let custom = match s.table {
            Some(t) => self.custom(t),
            None => CustomColors::default(),
        };

        let s = self.section(
            root,
            "terminal",
            "terminal",
            &[
                "font",
                "font_size",
                "line_height",
                "padding",
                "cursor",
                "cursor_blink",
                "new_tab_dir",
                "bell",
                "notify_on_finish",
                "notify_after",
                "copy_on_select",
                "ligatures",
                "shell_integration",
                "shell_extras",
                "confirm_multiline_paste",
                "scrollback",
                "shell",
            ],
        );
        let dt = &d.terminal;
        let font = match self.string(&s, "font") {
            Some("") => {
                self.warn("terminal.font", "empty");
                None
            }
            other => other.map(str::to_string),
        };
        let shell = match self.string(&s, "shell") {
            Some(p) if p.starts_with('/') && p.len() <= 4096 => Some(p.to_string()),
            Some(p) => {
                self.warn(
                    "terminal.shell",
                    format!("{p:?} is not an absolute path of at most 4096 bytes"),
                );
                None
            }
            None => None,
        };
        let terminal = TerminalConfig {
            font,
            font_size: self.opt_int(&s, "font_size", 6..=48),
            line_height: self.float(&s, "line_height", 0.8..=2.0, dt.line_height),
            padding: self.int(&s, "padding", 0..=32, dt.padding),
            cursor: self.choice(
                &s,
                "cursor",
                &[
                    ("block", Cursor::Block),
                    ("beam", Cursor::Beam),
                    ("underline", Cursor::Underline),
                ],
                dt.cursor,
            ),
            cursor_blink: self.bool(&s, "cursor_blink", dt.cursor_blink),
            new_tab_dir: self.choice(
                &s,
                "new_tab_dir",
                &[("home", NewTabDir::Home), ("current", NewTabDir::Current)],
                dt.new_tab_dir,
            ),
            bell: self.choice(
                &s,
                "bell",
                &[
                    ("none", Bell::None),
                    ("visual", Bell::Visual),
                    ("sound", Bell::Sound),
                ],
                dt.bell,
            ),
            notify_on_finish: self.choice(
                &s,
                "notify_on_finish",
                &[
                    ("never", NotifyOnFinish::Never),
                    ("unfocused", NotifyOnFinish::Unfocused),
                    ("always", NotifyOnFinish::Always),
                ],
                dt.notify_on_finish,
            ),
            notify_after: self.int(&s, "notify_after", 0..=86_400, dt.notify_after),
            copy_on_select: self.bool(&s, "copy_on_select", dt.copy_on_select),
            ligatures: self.bool(&s, "ligatures", dt.ligatures),
            shell_integration: self.bool(&s, "shell_integration", dt.shell_integration),
            shell_extras: self.bool(&s, "shell_extras", dt.shell_extras),
            confirm_multiline_paste: self.bool(
                &s,
                "confirm_multiline_paste",
                dt.confirm_multiline_paste,
            ),
            scrollback: self.int(&s, "scrollback", 0..=100_000, dt.scrollback),
            shell,
        };

        let s = self.section(root, "sessions", "sessions", &["ring_kib"]);
        let sessions = SessionsConfig {
            ring_kib: self.int(&s, "ring_kib", 256..=16_384, d.sessions.ring_kib),
        };

        let names: Vec<&str> = Action::all().map(Action::key_name).collect();
        let s = self.section(root, "keys", "keys", &names);
        let mut keys = Keys::default();
        for action in Action::all() {
            if let Some(Ok(seq)) = self.seq(&s, action.key_name()) {
                keys.set(action, seq);
            }
        }
        // A sequence used twice: the first holder, hotkey first, keeps it.
        let mut taken: Vec<(KeySeq, String)> = hotkey
            .map(|h| (h, "hotkey.toggle".to_string()))
            .into_iter()
            .collect();
        for action in Action::all() {
            let Some(seq) = keys.get(action) else {
                continue;
            };
            let name = format!("keys.{}", action.key_name());
            if let Some((_, holder)) = taken.iter().find(|(t, _)| t.clashes(&seq)) {
                self.warn(&name, format!("{seq} is already used by {holder}"));
                keys.set(action, None);
            } else {
                taken.push((seq, name));
            }
        }

        let s = self.section(
            root,
            "advanced",
            "advanced",
            &["programs_write_clipboard", "allow_run"],
        );
        let advanced = AdvancedConfig {
            programs_write_clipboard: self.bool(
                &s,
                "programs_write_clipboard",
                d.advanced.programs_write_clipboard,
            ),
            allow_run: self.bool(&s, "allow_run", d.advanced.allow_run),
        };

        Config {
            hotkey,
            window,
            theme,
            custom,
            terminal,
            sessions,
            keys,
            advanced,
        }
    }

    pub(crate) fn custom(&mut self, theme: &Table) -> CustomColors {
        let s = self.section(
            theme,
            "custom",
            "theme.custom",
            &[
                "base",
                "foreground",
                "background",
                "cursor",
                "selection",
                "selection_text",
                "ansi",
            ],
        );
        let mut c = CustomColors::default();
        match self.string(&s, "base") {
            Some("") => self.warn("theme.custom.base", "empty"),
            Some(b) => c.base = b.to_string(),
            None => c.base = FALLBACK.to_string(),
        }
        c.foreground = self.colour(&s, "foreground");
        c.background = self.colour(&s, "background");
        c.cursor = self.colour(&s, "cursor");
        c.selection = self.colour(&s, "selection");
        c.selection_text = self.colour(&s, "selection_text");
        match s.get("ansi") {
            None => {}
            Some(Value::Array(list)) => {
                if list.len() > 16 {
                    self.warn(
                        "theme.custom.ansi",
                        format!("{} colours; the ones after 16 are ignored", list.len()),
                    );
                }
                for (i, v) in list.iter().take(16).enumerate() {
                    match v.as_str().and_then(parse_rgb) {
                        Some(rgb) => c.ansi[i] = Some(rgb),
                        None => self.warn(format!("theme.custom.ansi[{i}]"), "not #rrggbb"),
                    }
                }
            }
            Some(_) => self.warn("theme.custom.ansi", "not an array"),
        }
        c
    }
}
