//! `config.toml`: the settings of §2.6, each field read on its own so a bad value costs only
//! that key. A file that does not parse at all is an error, not a file of defaults: the window
//! keeps the last config it had.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::Warning;
use crate::keys::{Key, KeySeq, Mods};
use crate::theme::{CustomColors, FALLBACK, ThemeSource, parse_rgb};

mod actions;
mod fields;
mod reader;

pub use actions::*;
use reader::*;

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
    /// With shell integration: kyko's shell defaults (completion, history, suggestions, the
    /// command-not-found hint, zoxide and fzf keys) where the user's own files set none.
    pub shell_extras: bool,
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
                shell_extras: true,
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
