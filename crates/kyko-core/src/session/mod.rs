//! The session: the tabs, each a binary split tree of panes, which tab is active and which pane
//! has focus, tab names, pane states, and the close rules of §2.3. Pure data: every change
//! returns the [`Effect`]s the window carries out (frames to `kykod`, hiding the window).

use std::collections::BTreeMap;

use kyko_proto::{MAX_PANES, PaneId};

use crate::config::NewTabDir;

mod check;
mod close;
mod focus;
mod names;
mod open;
mod tree;

use tree::*;

/// Stable for the window's life and across a restart (the layout blob).
pub type TabId = u32;

/// The longest tab rename kept, in bytes.
pub const MAX_RENAME: usize = 256;
/// The bounds a split's ratio is kept within.
pub const MIN_RATIO: f64 = 0.05;
pub const MAX_RATIO: f64 = 0.95;
/// `/proc/<pid>/comm` keeps 15 bytes of a program's name.
pub const COMM_LEN: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    /// Side by side: the first child is on the left.
    Side,
    /// Stacked: the first child is on top.
    Stacked,
}

/// A rectangle in the tab's unit square.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneState {
    Running,
    /// Shown with its status until dismissed; only for held panes.
    Exited(i32),
    /// `SpawnFailed`: the error, shown with the command (§4).
    Failed(String),
    /// `kykod` went away: "Session ended" (§2.7).
    Ended,
}

pub struct PaneView<'a> {
    pub id: PaneId,
    pub rect: Rect,
    pub state: &'a PaneState,
    pub program: &'a str,
    pub cwd: &'a str,
    pub busy: bool,
    pub focused: bool,
    /// Behind a zoomed pane: kept at its size, not shown.
    pub hidden: bool,
}

pub struct DividerView {
    /// From the tab's root: `false` the first child, `true` the second.
    pub path: Vec<bool>,
    pub axis: SplitAxis,
    /// The area the split divides.
    pub rect: Rect,
    pub ratio: f64,
}

pub struct TabView<'a> {
    pub id: TabId,
    pub name: String,
    /// The focused pane's program.
    pub program: &'a str,
    pub active: bool,
    pub panes: Vec<PaneView<'a>>,
    pub dividers: Vec<DividerView>,
    /// A pane fills the tab (Ctrl+Shift+Z).
    pub zoomed: bool,
    /// Typing goes to all the tab's panes (Ctrl+Shift+B).
    pub broadcast: bool,
}

/// A new tab's pane. `cwd` `None`: home. `args` empty: an interactive shell; the window builds
/// the `Spawn`'s shell and args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnRequest {
    pub cwd: Option<String>,
    pub args: Vec<String>,
    /// Keep the pane open, with its status, after its shell exits.
    pub hold: bool,
}

/// What a change asks the window to do beyond redrawing the tab bar and the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Send `Spawn`.
    Spawn {
        pane: PaneId,
        cwd: Option<String>,
        args: Vec<String>,
    },
    /// Send `Close`.
    Close { pane: PaneId },
    /// The last tab was replaced: hide kyko.
    Hide,
}

/// `MAX_PANES` reached, or no pane id left: nothing changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        ratio: f64,
        a: Box<Node>,
        b: Box<Node>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tab {
    pub(crate) id: TabId,
    pub(crate) rename: Option<String>,
    pub(crate) root: Node,
    /// Most recently focused first; the first is the tab's focused pane.
    pub(crate) focus: Vec<PaneId>,
    /// The focused pane fills the tab; the others are kept at that size, out of sight. Not
    /// saved with the layout.
    pub(crate) zoomed: bool,
    /// Typing in one pane goes to every pane of the tab.
    pub(crate) broadcast: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pane {
    pub(crate) state: PaneState,
    pub(crate) program: String,
    pub(crate) cwd: String,
    /// Whether an `Info` has come: before it, the tab is named after the shell.
    pub(crate) had_info: bool,
    /// The shell's name as `comm` gives it.
    pub(crate) shell: String,
    pub(crate) hold: bool,
}

impl Pane {
    pub(crate) fn new(shell: &str, hold: bool) -> Pane {
        Pane {
            state: PaneState::Running,
            program: String::new(),
            cwd: String::new(),
            had_info: false,
            shell: comm(shell),
            hold,
        }
    }

    /// The foreground program is not the shell. An empty `program` is idle.
    fn busy(&self) -> bool {
        !self.program.is_empty() && cut(&self.program, COMM_LEN) != cut(&self.shell, COMM_LEN)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: TabId,
    pub(crate) panes: BTreeMap<PaneId, Pane>,
    pub(crate) next_pane: PaneId,
    pub(crate) next_tab: TabId,
    pub(crate) home: String,
}

/// `s` cut to at most `max` bytes, at a character boundary.
pub(crate) fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let end = (0..=max)
        .rev()
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(0);
    &s[..end]
}

/// A shell path's name as `/proc/<pid>/comm` gives it: the base name, cut to 15 bytes.
pub(crate) fn comm(shell: &str) -> String {
    let base = shell.rsplit('/').next().unwrap_or(shell);
    cut(base, COMM_LEN).to_string()
}

impl Session {
    /// An empty session; the caller adds a tab before it is used.
    pub(crate) fn empty(home: &str) -> Session {
        Session {
            tabs: Vec::new(),
            active: 0,
            panes: BTreeMap::new(),
            next_pane: 1,
            next_tab: 1,
            home: home.to_string(),
        }
    }

    /// One tab with one pane, to spawn.
    pub fn fresh(home: &str, shell: &str) -> (Session, Vec<Effect>) {
        let mut s = Session::empty(home);
        let effects = s.push_fresh_tab(shell).unwrap_or_default();
        (s, effects)
    }

    /// A tab in the home folder at the end, made active. `Full` only when no id is left.
    pub(crate) fn push_fresh_tab(&mut self, shell: &str) -> Result<Vec<Effect>, Full> {
        let (pane, tab) = self.ids()?;
        self.panes.insert(pane, Pane::new(shell, false));
        self.tabs.push(Tab {
            id: tab,
            rename: None,
            root: Node::Leaf(pane),
            focus: vec![pane],
            zoomed: false,
            broadcast: false,
        });
        self.active = tab;
        Ok(vec![Effect::Spawn {
            pane,
            cwd: None,
            args: Vec::new(),
        }])
    }

    /// A new pane id and tab id, both taken only if both are free.
    fn ids(&mut self) -> Result<(PaneId, TabId), Full> {
        if self.next_pane == u32::MAX || self.next_tab == u32::MAX {
            return Err(Full);
        }
        let ids = (self.next_pane, self.next_tab);
        self.next_pane += 1;
        self.next_tab += 1;
        Ok(ids)
    }

    fn pane_id(&mut self) -> Result<PaneId, Full> {
        if self.next_pane == u32::MAX {
            return Err(Full);
        }
        self.next_pane += 1;
        Ok(self.next_pane - 1)
    }

    fn room(&self) -> Result<(), Full> {
        if self.panes.len() >= MAX_PANES {
            Err(Full)
        } else {
            Ok(())
        }
    }

    fn tab_index(&self, tab: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == tab)
    }

    fn active_index(&self) -> usize {
        self.tab_index(self.active).unwrap_or(0)
    }

    /// The tab holding `pane`.
    pub(crate) fn tab_of(&self, pane: PaneId) -> Option<usize> {
        self.tabs.iter().position(|t| t.root.contains(pane))
    }

    fn tab_name(&self, tab: &Tab) -> String {
        if let Some(r) = &tab.rename {
            return r.clone();
        }
        let Some(p) = tab.focus.first().and_then(|id| self.panes.get(id)) else {
            return String::new();
        };
        if !p.had_info {
            return p.shell.clone();
        }
        if p.busy() {
            return p.program.clone();
        }
        if p.cwd == self.home {
            return "~".into();
        }
        if p.cwd == "/" {
            return "/".into();
        }
        match p.cwd.trim_end_matches('/').rsplit('/').next() {
            Some(last) if !last.is_empty() => last.to_string(),
            _ => p.shell.clone(),
        }
    }

    pub fn tabs(&self) -> impl Iterator<Item = TabView<'_>> {
        self.tabs.iter().map(move |t| {
            let focused = t.focus.first().copied();
            let mut bounds = Vec::new();
            t.root.bounds(UNIT, &mut bounds);
            let zoomed = t.zoomed && bounds.len() > 1;
            let panes = bounds
                .into_iter()
                .filter_map(|(id, b)| {
                    let p = self.panes.get(&id)?;
                    Some(PaneView {
                        id,
                        rect: if zoomed { UNIT.rect() } else { b.rect() },
                        hidden: zoomed && Some(id) != focused,
                        state: &p.state,
                        program: &p.program,
                        cwd: &p.cwd,
                        busy: p.state == PaneState::Running && p.busy(),
                        focused: Some(id) == focused,
                    })
                })
                .collect();
            let mut dividers = Vec::new();
            if !zoomed {
                t.root.dividers(UNIT, &mut Vec::new(), &mut dividers);
            }
            TabView {
                id: t.id,
                name: self.tab_name(t),
                program: focused
                    .and_then(|id| self.panes.get(&id))
                    .map_or("", |p| p.program.as_str()),
                active: t.id == self.active,
                panes,
                dividers,
                zoomed,
                broadcast: t.broadcast && !matches!(t.root, Node::Leaf(_)),
            }
        })
    }

    /// Ctrl+Shift+B: typing in the active tab goes to all its panes, or back to one. False
    /// when the tab has one pane.
    pub fn toggle_broadcast(&mut self) -> bool {
        let i = self.active_index();
        let Some(tab) = self.tabs.get_mut(i) else {
            return false;
        };
        if matches!(tab.root, Node::Leaf(_)) {
            tab.broadcast = false;
            return false;
        }
        tab.broadcast = !tab.broadcast;
        true
    }

    /// The panes typing in `pane` reaches: its whole tab's running panes while the tab
    /// broadcasts, else `pane` alone.
    pub fn typing_targets(&self, pane: PaneId) -> Vec<PaneId> {
        let Some(tab) = self.tab_of(pane).map(|i| &self.tabs[i]) else {
            return vec![pane];
        };
        if !tab.broadcast || matches!(tab.root, Node::Leaf(_)) {
            return vec![pane];
        }
        let mut all = Vec::new();
        tab.root.leaves(&mut all);
        all.retain(|p| {
            self.panes
                .get(p)
                .is_some_and(|x| x.state == PaneState::Running)
        });
        all
    }

    /// Ctrl+Shift+Z: the focused pane fills its tab, or goes back to its place. False when the
    /// tab has one pane.
    pub fn toggle_zoom(&mut self) -> bool {
        let i = self.active_index();
        let Some(tab) = self.tabs.get_mut(i) else {
            return false;
        };
        if matches!(tab.root, Node::Leaf(_)) {
            tab.zoomed = false;
            return false;
        }
        tab.zoomed = !tab.zoomed;
        true
    }

    pub fn active_tab(&self) -> TabId {
        self.active
    }

    pub fn focused_pane(&self) -> PaneId {
        self.tabs
            .get(self.active_index())
            .and_then(|t| t.focus.first().copied())
            .unwrap_or(0)
    }

    /// Every pane id, in id order; cheaper than [`Session::tabs`].
    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.panes.keys().copied().collect()
    }

    /// Every tab id, in tab-bar order; cheaper than [`Session::tabs`].
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.tabs.iter().map(|t| t.id).collect()
    }

    /// A pane's state, if the session has it.
    pub fn pane_state(&self, pane: PaneId) -> Option<&PaneState> {
        self.panes.get(&pane).map(|p| &p.state)
    }

    /// Sets a split's ratio, kept within 0.05–0.95. NaN and infinities are refused.
    pub fn set_ratio(&mut self, tab: TabId, path: &[bool], ratio: f64) -> bool {
        if !ratio.is_finite() {
            return false;
        }
        let Some(i) = self.tab_index(tab) else {
            return false;
        };
        match self.tabs[i].root.at_path(path) {
            Some(Node::Split { ratio: r, .. }) => {
                *r = ratio.clamp(MIN_RATIO, MAX_RATIO);
                true
            }
            _ => false,
        }
    }
}

pub(crate) fn clean_rename(name: &str) -> Option<String> {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    let clean = cut(&clean, MAX_RENAME);
    (!clean.is_empty()).then(|| clean.to_string())
}

#[cfg(test)]
mod tests;
