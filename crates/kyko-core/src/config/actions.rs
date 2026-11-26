//! The actions a key can trigger in the kyko window (§2.3), their default keys, and [`Keys`],
//! the sequence bound to each.

use super::*;

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
    /// Scrolls to the previous command that failed.
    PrevFailed,
    /// Scrolls to the next command that failed.
    NextFailed,
    /// Copies the last command line the shell ran.
    CopyLastCommand,
    /// A list of every action, tab and recent command to pick from by typing.
    CommandPalette,
}

pub(crate) const ACTIONS: [Action; 37] = [
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
    Action::PrevFailed,
    Action::NextFailed,
    Action::CopyLastCommand,
    Action::CommandPalette,
];

pub(crate) const fn m(ctrl: bool, alt: bool, shift: bool) -> Mods {
    Mods {
        ctrl,
        alt,
        shift,
        super_: false,
    }
}
pub(crate) const CTRL: Mods = m(true, false, false);
pub(crate) const ALT: Mods = m(false, true, false);
pub(crate) const CTRL_SHIFT: Mods = m(true, false, true);
pub(crate) const CTRL_ALT_SHIFT: Mods = m(true, true, true);

impl Action {
    /// Every action, in the order of §2.3: the order duplicate sequences are settled in.
    pub fn all() -> impl Iterator<Item = Action> {
        ACTIONS.into_iter()
    }

    pub(crate) fn index(self) -> usize {
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
            Action::PrevFailed => "prev_failed",
            Action::NextFailed => "next_failed",
            Action::CopyLastCommand => "copy_last_command",
            Action::CommandPalette => "command_palette",
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
            Action::PrevFailed => (CTRL_ALT_SHIFT, Key::Up),
            Action::NextFailed => (CTRL_ALT_SHIFT, Key::Down),
            Action::CopyLastCommand => (CTRL_SHIFT, Key::Char('K')),
            Action::CommandPalette => (CTRL_SHIFT, Key::Char('P')),
        };
        KeySeq::new(mods, key)
    }
}

/// The sequence bound to each action; `None` is unbound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keys([Option<KeySeq>; 37]);

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
