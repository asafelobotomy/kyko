//! `config.toml`: the settings of §2.6, each field read on its own so a bad value costs only
//! that key. A file that does not parse at all is an error, not a file of defaults: the window
//! keeps the last config it had.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::Warning;
use crate::keys::{Key, KeySeq, Mods};
use crate::theme::{CustomColors, FALLBACK, ThemeSource, parse_rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Island,
    Sheet,
}

/// Where the drop-down comes from: the edge it slides in from, or the middle of the screen,
/// where it fades in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

impl Position {
    pub const NAMES: [&'static str; 5] = ["top", "bottom", "left", "right", "center"];
    const ALL: [Position; 5] = [
        Position::Top,
        Position::Bottom,
        Position::Left,
        Position::Right,
        Position::Center,
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES[Self::ALL.iter().position(|p| *p == self).unwrap_or(0)]
    }

    /// The `window.*` keys that hold this position's size, width then height: top and bottom
    /// share one pair, the sides another, and the middle its own, so each keeps the size it was
    /// last dragged to.
    pub fn size_keys(self) -> (&'static str, &'static str) {
        match self {
            Position::Top | Position::Bottom => ("width_pct", "height_pct"),
            Position::Left | Position::Right => ("side_width_pct", "side_height_pct"),
            Position::Center => ("center_width_pct", "center_height_pct"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsAt {
    Top,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Monitor {
    Focused,
    Primary,
    Output(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewTabDir {
    Home,
    Current,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bell {
    None,
    Visual,
    Sound,
}

/// When a long command's end shows a desktop notification (needs shell integration).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyOnFinish {
    Never,
    /// Only while the pane is not what the user is looking at.
    Unfocused,
    Always,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowConfig {
    pub shape: Shape,
    pub position: Position,
    pub tabs: TabsAt,
    /// The size at the top and bottom, in percent of the screen's free area.
    pub height_pct: u32,
    pub width_pct: u32,
    /// The size at the left and right edges.
    pub side_width_pct: u32,
    pub side_height_pct: u32,
    /// The size in the middle of the screen.
    pub center_width_pct: u32,
    pub center_height_pct: u32,
    /// How much the drop-down darkens the screen behind it while shown, in percent; 0 is off.
    pub dim_pct: u32,
    /// How opaque the terminal's background is, in percent: below 100 the desktop (and its blur)
    /// shows through. Cells a program coloured stay opaque.
    pub opacity_pct: u32,
    pub monitor: Monitor,
    pub above_fullscreen: bool,
    pub hide_on_focus_loss: bool,
    pub animations: bool,
    pub opacity: f64,
}

impl WindowConfig {
    /// The drop-down's size where it is now, width and height in percent.
    pub fn size_pct(&self) -> (u32, u32) {
        match self.position {
            Position::Top | Position::Bottom => (self.width_pct, self.height_pct),
            Position::Left | Position::Right => (self.side_width_pct, self.side_height_pct),
            Position::Center => (self.center_width_pct, self.center_height_pct),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalConfig {
    /// Unset: the theme's `fonts.mono`.
    pub font: Option<String>,
    /// Unset: the theme's `fonts.size`.
    pub font_size: Option<u32>,
    pub line_height: f64,
    pub padding: u32,
    pub cursor: Cursor,
    pub cursor_blink: bool,
    pub new_tab_dir: NewTabDir,
    pub bell: Bell,
    pub notify_on_finish: NotifyOnFinish,
    /// Seconds a command must run before its end is worth a notification.
    pub notify_after: u32,
    pub copy_on_select: bool,
    /// Join character sequences the font draws as one (`=>`, `!=`); never under the cursor.
    pub ligatures: bool,
    /// Marks, folder reports and kyko's prompt in bash, zsh and fish.
    pub shell_integration: bool,
    pub confirm_multiline_paste: bool,
    pub scrollback: u32,
    /// Unset: `$SHELL`. An absolute path.
    pub shell: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionsConfig {
    pub ring_kib: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvancedConfig {
    pub programs_write_clipboard: bool,
    pub allow_run: bool,
}

/// What a key in the kyko window does (§2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    NewTab,
    ClosePane,
    PrevTab,
    NextTab,
    /// Tab by number, 1 to 9.
    Tab(u8),
    SplitSide,
    SplitStacked,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    Copy,
    Paste,
    Search,
    FontBigger,
    FontSmaller,
    FontReset,
    /// Scrolls to the previous prompt.
    PrevPrompt,
    /// Scrolls to the next prompt.
    NextPrompt,
    /// Selects and copies the output of the last command.
    CopyLastOutput,
    /// The whole scrollback in a pager (`less`), in a new tab.
    ShowScrollback,
    /// Labels every link on screen; typing a label opens that link.
    OpenLink,
    /// The focused pane fills its tab, or goes back to its place.
    ZoomPane,
    /// Typing in the tab goes to all its panes, or back to one.
    Broadcast,
    /// Picks a character to type by its code point or name.
    UnicodeInput,
}

const ACTIONS: [Action; 33] = [
    Action::NewTab,
    Action::ClosePane,
    Action::PrevTab,
    Action::NextTab,
    Action::Tab(1),
    Action::Tab(2),
    Action::Tab(3),
    Action::Tab(4),
    Action::Tab(5),
    Action::Tab(6),
    Action::Tab(7),
    Action::Tab(8),
    Action::Tab(9),
    Action::SplitSide,
    Action::SplitStacked,
    Action::FocusLeft,
    Action::FocusRight,
    Action::FocusUp,
    Action::FocusDown,
    Action::Copy,
    Action::Paste,
    Action::Search,
    Action::FontBigger,
    Action::FontSmaller,
    Action::FontReset,
    Action::PrevPrompt,
    Action::NextPrompt,
    Action::CopyLastOutput,
    Action::ShowScrollback,
    Action::OpenLink,
    Action::ZoomPane,
    Action::Broadcast,
    Action::UnicodeInput,
];

const fn m(ctrl: bool, alt: bool, shift: bool) -> Mods {
    Mods {
        ctrl,
        alt,
        shift,
        super_: false,
    }
}
const CTRL: Mods = m(true, false, false);
const ALT: Mods = m(false, true, false);
const CTRL_SHIFT: Mods = m(true, false, true);

impl Action {
    /// Every action, in the order of §2.3: the order duplicate sequences are settled in.
    pub fn all() -> impl Iterator<Item = Action> {
        ACTIONS.into_iter()
    }

    fn index(self) -> usize {
        ACTIONS.iter().position(|a| *a == self).unwrap_or(0)
    }

    /// The key under `[keys]`.
    pub fn key_name(self) -> &'static str {
        const TABS: [&str; 9] = [
            "tab_1", "tab_2", "tab_3", "tab_4", "tab_5", "tab_6", "tab_7", "tab_8", "tab_9",
        ];
        match self {
            Action::NewTab => "new_tab",
            Action::ClosePane => "close_pane",
            Action::PrevTab => "prev_tab",
            Action::NextTab => "next_tab",
            Action::Tab(n) => TABS[usize::from(n.clamp(1, 9)) - 1],
            Action::SplitSide => "split_side",
            Action::SplitStacked => "split_stacked",
            Action::FocusLeft => "focus_left",
            Action::FocusRight => "focus_right",
            Action::FocusUp => "focus_up",
            Action::FocusDown => "focus_down",
            Action::Copy => "copy",
            Action::Paste => "paste",
            Action::Search => "search",
            Action::FontBigger => "font_bigger",
            Action::FontSmaller => "font_smaller",
            Action::FontReset => "font_reset",
            Action::PrevPrompt => "prev_prompt",
            Action::NextPrompt => "next_prompt",
            Action::CopyLastOutput => "copy_last_output",
            Action::ShowScrollback => "show_scrollback",
            Action::OpenLink => "open_link",
            Action::ZoomPane => "zoom_pane",
            Action::Broadcast => "broadcast",
            Action::UnicodeInput => "unicode_input",
        }
    }

    pub fn default_seq(self) -> KeySeq {
        let (mods, key) = match self {
            Action::NewTab => (CTRL_SHIFT, Key::Char('T')),
            Action::ClosePane => (CTRL_SHIFT, Key::Char('W')),
            Action::PrevTab => (CTRL, Key::PgUp),
            Action::NextTab => (CTRL, Key::PgDn),
            Action::Tab(n) => (ALT, Key::Char(char::from(b'0' + n.clamp(1, 9)))),
            Action::SplitSide => (CTRL_SHIFT, Key::Char('D')),
            Action::SplitStacked => (CTRL_SHIFT, Key::Char('E')),
            Action::FocusLeft => (ALT, Key::Left),
            Action::FocusRight => (ALT, Key::Right),
            Action::FocusUp => (ALT, Key::Up),
            Action::FocusDown => (ALT, Key::Down),
            Action::Copy => (CTRL_SHIFT, Key::Char('C')),
            Action::Paste => (CTRL_SHIFT, Key::Char('V')),
            Action::Search => (CTRL_SHIFT, Key::Char('F')),
            Action::FontBigger => (CTRL, Key::Char('+')),
            Action::FontSmaller => (CTRL, Key::Char('-')),
            Action::FontReset => (CTRL, Key::Char('0')),
            Action::PrevPrompt => (CTRL_SHIFT, Key::Up),
            Action::NextPrompt => (CTRL_SHIFT, Key::Down),
            Action::CopyLastOutput => (CTRL_SHIFT, Key::Char('O')),
            Action::ShowScrollback => (CTRL_SHIFT, Key::Char('H')),
            Action::OpenLink => (CTRL_SHIFT, Key::Char('L')),
            Action::ZoomPane => (CTRL_SHIFT, Key::Char('Z')),
            Action::Broadcast => (CTRL_SHIFT, Key::Char('B')),
            Action::UnicodeInput => (CTRL_SHIFT, Key::Char('U')),
        };
        KeySeq::new(mods, key)
    }
}

/// The sequence bound to each action; `None` is unbound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keys([Option<KeySeq>; 33]);

impl Default for Keys {
    fn default() -> Self {
        Keys(ACTIONS.map(|a| Some(a.default_seq())))
    }
}

impl Keys {
    pub fn get(&self, action: Action) -> Option<KeySeq> {
        self.0[action.index()]
    }

    pub fn set(&mut self, action: Action, seq: Option<KeySeq>) {
        self.0[action.index()] = seq;
    }

    /// The action a key press triggers, if any.
    pub fn action_for(&self, mods: Mods, key: Key) -> Option<Action> {
        Action::all().find(|a| self.get(*a).is_some_and(|s| s.matches(mods, key)))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// `hotkey.toggle`; `None` when set to `""`.
    pub hotkey: Option<KeySeq>,
    pub window: WindowConfig,
    pub theme: ThemeSource,
    pub custom: CustomColors,
    pub terminal: TerminalConfig,
    pub sessions: SessionsConfig,
    pub keys: Keys,
    pub advanced: AdvancedConfig,
}

impl Default for Config {
    /// The §2.6 table.
    fn default() -> Self {
        Config {
            hotkey: Some(KeySeq::new(Mods::default(), Key::F(12))),
            window: WindowConfig {
                shape: Shape::Island,
                position: Position::Top,
                tabs: TabsAt::Top,
                height_pct: 45,
                width_pct: 88,
                side_width_pct: 40,
                side_height_pct: 100,
                center_width_pct: 60,
                center_height_pct: 60,
                dim_pct: 40,
                opacity_pct: 100,
                monitor: Monitor::Focused,
                above_fullscreen: true,
                hide_on_focus_loss: true,
                animations: true,
                opacity: 1.0,
            },
            theme: ThemeSource::Auto,
            custom: CustomColors::default(),
            terminal: TerminalConfig {
                font: None,
                font_size: None,
                line_height: 1.0,
                padding: 8,
                cursor: Cursor::Block,
                cursor_blink: false,
                new_tab_dir: NewTabDir::Current,
                bell: Bell::Visual,
                notify_on_finish: NotifyOnFinish::Never,
                notify_after: 10,
                copy_on_select: false,
                ligatures: true,
                shell_integration: true,
                confirm_multiline_paste: true,
                scrollback: 10_000,
                shell: None,
            },
            sessions: SessionsConfig { ring_kib: 2048 },
            keys: Keys::default(),
            advanced: AdvancedConfig {
                programs_write_clipboard: false,
                allow_run: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<Warning>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    /// 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
    pub message: String,
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Syntax(SyntaxError),
    /// Permission, not UTF-8, a directory.
    Unreadable {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Syntax(e) => e.fmt(f),
            LoadError::Unreadable { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
        }
    }
}

/// Groups of settings that changed, so the window re-applies only those.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes(u8);

impl Changes {
    pub const HOTKEY: Changes = Changes(1);
    pub const WINDOW: Changes = Changes(1 << 1);
    /// `theme.*`.
    pub const THEME: Changes = Changes(1 << 2);
    /// `terminal.font`, `font_size` and `line_height`: the glyph atlas is rebuilt.
    pub const FONT: Changes = Changes(1 << 3);
    /// The other `terminal.*` keys.
    pub const TERMINAL: Changes = Changes(1 << 4);
    pub const KEYS: Changes = Changes(1 << 5);
    pub const SESSIONS: Changes = Changes(1 << 6);
    pub const ADVANCED: Changes = Changes(1 << 7);

    pub fn contains(self, other: Changes) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Changes {
    type Output = Changes;
    fn bitor(self, rhs: Changes) -> Changes {
        Changes(self.0 | rhs.0)
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Loaded, SyntaxError> {
        let root: Table = text.parse().map_err(|e: toml::de::Error| {
            let (line, column) = e.span().map(|s| line_col(text, s.start)).unwrap_or((1, 1));
            SyntaxError {
                line,
                column,
                message: e.message().to_string(),
            }
        })?;
        let mut r = Reader::default();
        let config = r.config(&root);
        Ok(Loaded {
            config,
            warnings: r.warnings,
        })
    }

    /// A missing file is the defaults with no warning; that is not an error.
    pub fn load(path: &Path) -> Result<Loaded, LoadError> {
        let unreadable = |message: String| LoadError::Unreadable {
            path: path.to_path_buf(),
            message,
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Loaded {
                    config: Config::default(),
                    warnings: Vec::new(),
                });
            }
            Err(e) => return Err(unreadable(e.to_string())),
        };
        let text = String::from_utf8(bytes).map_err(|_| unreadable("not UTF-8".into()))?;
        Config::parse(&text).map_err(LoadError::Syntax)
    }

    /// What differs from `old`.
    pub fn changes(&self, old: &Config) -> Changes {
        let mut c = Changes::default();
        let mut mark = |differs: bool, group: Changes| {
            if differs {
                c = c | group;
            }
        };
        mark(self.hotkey != old.hotkey, Changes::HOTKEY);
        mark(self.window != old.window, Changes::WINDOW);
        mark(
            self.theme != old.theme || self.custom != old.custom,
            Changes::THEME,
        );
        let (t, o) = (&self.terminal, &old.terminal);
        let font = (&t.font, t.font_size, t.line_height) != (&o.font, o.font_size, o.line_height);
        mark(font, Changes::FONT);
        let rest = |t: &TerminalConfig| TerminalConfig {
            font: None,
            font_size: None,
            line_height: 1.0,
            ..t.clone()
        };
        mark(rest(t) != rest(o), Changes::TERMINAL);
        mark(self.keys != old.keys, Changes::KEYS);
        mark(self.sessions != old.sessions, Changes::SESSIONS);
        mark(self.advanced != old.advanced, Changes::ADVANCED);
        c
    }
}

/// 1-based line and character column of byte `at`.
fn line_col(text: &str, at: usize) -> (usize, usize) {
    let at = at.min(text.len());
    let at = (0..=at)
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

const SECTIONS: &[&str] = &[
    "hotkey", "window", "theme", "terminal", "sessions", "keys", "advanced",
];

#[derive(Default)]
struct Reader {
    warnings: Vec<Warning>,
}

/// One section: its table, its dotted name, and the keys read from it so far.
struct Section<'a> {
    name: &'a str,
    table: Option<&'a Table>,
}

impl<'a> Section<'a> {
    fn get(&self, key: &str) -> Option<&'a Value> {
        self.table.and_then(|t| t.get(key))
    }
}

impl Reader {
    fn warn(&mut self, key: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(Warning::new(key, message));
    }

    fn section<'a>(
        &mut self,
        parent: &'a Table,
        name: &'a str,
        dotted: &'a str,
        known: &[&str],
    ) -> Section<'a> {
        let table = match parent.get(name) {
            None => None,
            Some(Value::Table(t)) => {
                for key in t.keys() {
                    if !known.contains(&key.as_str()) {
                        self.warn(
                            format!("{dotted}.{key}"),
                            format!("unknown key {dotted}.{key}"),
                        );
                    }
                }
                Some(t)
            }
            Some(_) => {
                self.warn(dotted, "not a table");
                None
            }
        };
        Section {
            name: dotted,
            table,
        }
    }

    fn bool(&mut self, s: &Section, key: &str, default: bool) -> bool {
        match s.get(key) {
            None => default,
            Some(Value::Boolean(b)) => *b,
            Some(_) => {
                self.warn(format!("{}.{key}", s.name), "not true or false");
                default
            }
        }
    }

    fn int(&mut self, s: &Section, key: &str, range: RangeInclusive<i64>, default: u32) -> u32 {
        self.opt_int(s, key, range).unwrap_or(default)
    }

    fn opt_int(&mut self, s: &Section, key: &str, range: RangeInclusive<i64>) -> Option<u32> {
        match s.get(key)? {
            Value::Integer(n) if range.contains(n) => u32::try_from(*n).ok(),
            Value::Integer(n) => {
                self.warn(
                    format!("{}.{key}", s.name),
                    format!("{n} is outside {}–{}", range.start(), range.end()),
                );
                None
            }
            _ => {
                self.warn(format!("{}.{key}", s.name), "not an integer");
                None
            }
        }
    }

    fn float(&mut self, s: &Section, key: &str, range: RangeInclusive<f64>, default: f64) -> f64 {
        let v = match s.get(key) {
            None => return default,
            Some(Value::Float(f)) => *f,
            Some(Value::Integer(n)) => *n as f64,
            Some(_) => {
                self.warn(format!("{}.{key}", s.name), "not a number");
                return default;
            }
        };
        if v.is_finite() && range.contains(&v) {
            v
        } else {
            self.warn(
                format!("{}.{key}", s.name),
                format!("{v} is outside {}–{}", range.start(), range.end()),
            );
            default
        }
    }

    fn string<'a>(&mut self, s: &Section<'a>, key: &str) -> Option<&'a str> {
        match s.get(key)? {
            Value::String(v) => Some(v),
            _ => {
                self.warn(format!("{}.{key}", s.name), "not a string");
                None
            }
        }
    }

    fn choice<T: Copy>(&mut self, s: &Section, key: &str, options: &[(&str, T)], default: T) -> T {
        let Some(v) = self.string(s, key) else {
            return default;
        };
        match options.iter().find(|(n, _)| *n == v) {
            Some((_, t)) => *t,
            None => {
                let names: Vec<&str> = options.iter().map(|(n, _)| *n).collect();
                self.warn(
                    format!("{}.{key}", s.name),
                    format!("{v:?} is not one of {}", names.join(", ")),
                );
                default
            }
        }
    }

    /// A key sequence, `""` for none. `Err(())` when the value is bad (already warned).
    fn seq(&mut self, s: &Section, key: &str) -> Option<Result<Option<KeySeq>, ()>> {
        let v = match s.get(key)? {
            Value::String(v) => v,
            _ => {
                self.warn(format!("{}.{key}", s.name), "not a string");
                return Some(Err(()));
            }
        };
        if v.is_empty() {
            return Some(Ok(None));
        }
        Some(match v.parse::<KeySeq>() {
            Ok(seq) => Ok(Some(seq)),
            Err(e) => {
                self.warn(format!("{}.{key}", s.name), e.0);
                Err(())
            }
        })
    }

    fn colour(&mut self, s: &Section, key: &str) -> Option<u32> {
        let v = self.string(s, key)?;
        let c = parse_rgb(v);
        if c.is_none() {
            self.warn(format!("{}.{key}", s.name), format!("{v:?} is not #rrggbb"));
        }
        c
    }

    fn config(&mut self, root: &Table) -> Config {
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

    fn custom(&mut self, theme: &Table) -> CustomColors {
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
