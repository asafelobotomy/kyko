//! The session: the tabs, each a binary split tree of panes, which tab is active and which pane
//! has focus, tab names, pane states, and the close rules of §2.3. Pure data: every change
//! returns the [`Effect`]s the window carries out (frames to `kykod`, hiding the window).

use std::collections::BTreeMap;

use kyko_proto::{MAX_PANES, PaneId};

use crate::config::NewTabDir;

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

/// Edges as coordinates, not sizes: a dividing line is computed once and given to both sides,
/// so two panes that share an edge have bitwise-equal coordinates there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

const UNIT: Bounds = Bounds {
    x0: 0.0,
    y0: 0.0,
    x1: 1.0,
    y1: 1.0,
};

impl Bounds {
    fn rect(self) -> Rect {
        Rect {
            x: self.x0,
            y: self.y0,
            w: self.x1 - self.x0,
            h: self.y1 - self.y0,
        }
    }

    fn split(self, axis: SplitAxis, ratio: f64) -> (Bounds, Bounds) {
        match axis {
            SplitAxis::Side => {
                let mid = self.x0 + (self.x1 - self.x0) * ratio;
                (Bounds { x1: mid, ..self }, Bounds { x0: mid, ..self })
            }
            SplitAxis::Stacked => {
                let mid = self.y0 + (self.y1 - self.y0) * ratio;
                (Bounds { y1: mid, ..self }, Bounds { y0: mid, ..self })
            }
        }
    }
}

impl Node {
    fn leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(p) => out.push(*p),
            Node::Split { a, b, .. } => {
                a.leaves(out);
                b.leaves(out);
            }
        }
    }

    pub(crate) fn pane_ids(&self) -> Vec<PaneId> {
        let mut v = Vec::new();
        self.leaves(&mut v);
        v
    }

    fn contains(&self, pane: PaneId) -> bool {
        match self {
            Node::Leaf(p) => *p == pane,
            Node::Split { a, b, .. } => a.contains(pane) || b.contains(pane),
        }
    }

    pub(crate) fn bounds(&self, at: Bounds, out: &mut Vec<(PaneId, Bounds)>) {
        match self {
            Node::Leaf(p) => out.push((*p, at)),
            Node::Split { axis, ratio, a, b } => {
                let (ba, bb) = at.split(*axis, *ratio);
                a.bounds(ba, out);
                b.bounds(bb, out);
            }
        }
    }

    fn dividers(&self, at: Bounds, path: &mut Vec<bool>, out: &mut Vec<DividerView>) {
        if let Node::Split { axis, ratio, a, b } = self {
            out.push(DividerView {
                path: path.clone(),
                axis: *axis,
                rect: at.rect(),
                ratio: *ratio,
            });
            let (ba, bb) = at.split(*axis, *ratio);
            path.push(false);
            a.dividers(ba, path, out);
            path.pop();
            path.push(true);
            b.dividers(bb, path, out);
            path.pop();
        }
    }

    /// The tree without `pane`: its sibling takes the parent's place. `None` when the tree was
    /// that one leaf.
    pub(crate) fn without(self, pane: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(p) if p == pane => None,
            leaf @ Node::Leaf(_) => Some(leaf),
            Node::Split { axis, ratio, a, b } => match (a.without(pane), b.without(pane)) {
                (Some(a), Some(b)) => Some(Node::Split {
                    axis,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    /// Replaces the leaf `pane` with `with`. False when `pane` is not here.
    fn replace_leaf(&mut self, pane: PaneId, with: Node) -> bool {
        match self {
            Node::Leaf(p) if *p == pane => {
                *self = with;
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => {
                if a.contains(pane) {
                    a.replace_leaf(pane, with)
                } else {
                    b.replace_leaf(pane, with)
                }
            }
        }
    }

    fn at_path(&mut self, path: &[bool]) -> Option<&mut Node> {
        match (path.split_first(), self) {
            (None, node) => Some(node),
            (Some((side, rest)), Node::Split { a, b, .. }) => {
                if *side { b } else { a }.at_path(rest)
            }
            (Some(_), Node::Leaf(_)) => None,
        }
    }

    fn ratios_ok(&self) -> bool {
        match self {
            Node::Leaf(_) => true,
            Node::Split { ratio, a, b, .. } => {
                ratio.is_finite()
                    && (MIN_RATIO..=MAX_RATIO).contains(ratio)
                    && a.ratios_ok()
                    && b.ratios_ok()
            }
        }
    }
}

/// The overlap of two intervals; 0 or less when they do not overlap.
fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    a1.min(b1) - a0.max(b0)
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
            let panes = bounds
                .into_iter()
                .filter_map(|(id, b)| {
                    let p = self.panes.get(&id)?;
                    Some(PaneView {
                        id,
                        rect: b.rect(),
                        state: &p.state,
                        program: &p.program,
                        cwd: &p.cwd,
                        busy: p.state == PaneState::Running && p.busy(),
                        focused: Some(id) == focused,
                    })
                })
                .collect();
            let mut dividers = Vec::new();
            t.root.dividers(UNIT, &mut Vec::new(), &mut dividers);
            TabView {
                id: t.id,
                name: self.tab_name(t),
                program: focused
                    .and_then(|id| self.panes.get(&id))
                    .map_or("", |p| p.program.as_str()),
                active: t.id == self.active,
                panes,
                dividers,
            }
        })
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

    pub fn new_tab(&mut self, req: SpawnRequest, shell: &str) -> Result<Vec<Effect>, Full> {
        self.room()?;
        let (pane, tab) = self.ids()?;
        self.panes.insert(pane, Pane::new(shell, req.hold));
        let at = if self.tabs.is_empty() {
            0
        } else {
            self.active_index() + 1
        };
        self.tabs.insert(
            at,
            Tab {
                id: tab,
                rename: None,
                root: Node::Leaf(pane),
                focus: vec![pane],
            },
        );
        self.active = tab;
        Ok(vec![Effect::Spawn {
            pane,
            cwd: req.cwd,
            args: req.args,
        }])
    }

    /// §2.6 `new_tab_dir`: `Current` is the focused pane's folder once it has had an `Info`.
    pub fn new_tab_dir(&self, dir: NewTabDir) -> Option<String> {
        match dir {
            NewTabDir::Home => None,
            NewTabDir::Current => self
                .panes
                .get(&self.focused_pane())
                .filter(|p| p.had_info && !p.cwd.is_empty())
                .map(|p| p.cwd.clone()),
        }
    }

    /// Splits the focused pane: the new pane goes right of or below it, at 0.5, in its folder,
    /// and takes the focus.
    pub fn split(&mut self, axis: SplitAxis, shell: &str) -> Result<Vec<Effect>, Full> {
        self.room()?;
        let i = self.active_index();
        let Some(&old) = self.tabs.get(i).and_then(|t| t.focus.first()) else {
            return Err(Full);
        };
        let pane = self.pane_id()?;
        let cwd = self.new_tab_dir(NewTabDir::Current);
        self.panes.insert(pane, Pane::new(shell, false));
        let tab = &mut self.tabs[i];
        tab.root.replace_leaf(
            old,
            Node::Split {
                axis,
                ratio: 0.5,
                a: Box::new(Node::Leaf(old)),
                b: Box::new(Node::Leaf(pane)),
            },
        );
        tab.focus.insert(0, pane);
        Ok(vec![Effect::Spawn {
            pane,
            cwd,
            args: Vec::new(),
        }])
    }

    /// Takes `pane` out of the session; `Close` when it is still running in `kykod`.
    fn drop_pane(&mut self, pane: PaneId, effects: &mut Vec<Effect>) {
        if let Some(p) = self.panes.remove(&pane)
            && p.state == PaneState::Running
        {
            effects.push(Effect::Close { pane });
        }
    }

    /// Takes `pane` out without effects, as reconcile does: its split collapses as in
    /// [`Session::close_pane`], and a tab left empty goes, the tab to its right (else left)
    /// becoming active in its place. The session may be left with no tab.
    pub(crate) fn remove_quietly(&mut self, pane: PaneId) {
        self.panes.remove(&pane);
        let Some(i) = self.tab_of(pane) else {
            return;
        };
        let tab = &mut self.tabs[i];
        let root = std::mem::replace(&mut tab.root, Node::Leaf(0));
        tab.focus.retain(|p| *p != pane);
        match root.without(pane) {
            Some(r) => tab.root = r,
            None => {
                let gone = self.tabs.remove(i).id;
                if gone == self.active && !self.tabs.is_empty() {
                    self.active = self.tabs[i.min(self.tabs.len() - 1)].id;
                }
            }
        }
    }

    /// Closes a pane: its sibling takes its parent's place, the focus goes to the most recently
    /// focused pane left in the tab, and the last pane closes the tab.
    pub fn close_pane(&mut self, pane: PaneId, shell: &str) -> Vec<Effect> {
        let Some(i) = self.tab_of(pane) else {
            return Vec::new();
        };
        if self.tabs[i].focus.len() == 1 {
            let id = self.tabs[i].id;
            return self.close_tab(id, shell);
        }
        let mut effects = Vec::new();
        self.drop_pane(pane, &mut effects);
        let tab = &mut self.tabs[i];
        let root = std::mem::replace(&mut tab.root, Node::Leaf(0));
        tab.root = root.without(pane).unwrap_or(Node::Leaf(0));
        tab.focus.retain(|p| *p != pane);
        effects
    }

    /// Closes a tab and every pane in it. The tab to its right becomes active, else the one to
    /// its left. Closing the last tab opens a fresh one in the home folder and hides kyko.
    pub fn close_tab(&mut self, tab: TabId, shell: &str) -> Vec<Effect> {
        let Some(i) = self.tab_index(tab) else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        if self.tabs.len() == 1 {
            // The replacement needs ids; with none left (after 4 billion panes) the tab stays.
            if self.next_pane == u32::MAX || self.next_tab == u32::MAX {
                return vec![Effect::Hide];
            }
        }
        let removed = self.tabs.remove(i);
        for pane in removed.root.pane_ids() {
            self.drop_pane(pane, &mut effects);
        }
        if self.tabs.is_empty() {
            effects.extend(self.push_fresh_tab(shell).unwrap_or_default());
            effects.push(Effect::Hide);
        } else if removed.id == self.active {
            self.active = self.tabs[i.min(self.tabs.len() - 1)].id;
        }
        effects
    }

    /// A pane's shell exited: a held pane shows its status until dismissed, any other closes.
    /// Either way `kykod` gets `Close`, which frees its slot.
    pub fn pane_exited(&mut self, pane: PaneId, status: i32, shell: &str) -> Vec<Effect> {
        let Some(p) = self.panes.get_mut(&pane) else {
            return Vec::new();
        };
        if p.state != PaneState::Running {
            return Vec::new();
        }
        if p.hold {
            p.state = PaneState::Exited(status);
            return vec![Effect::Close { pane }];
        }
        self.close_pane(pane, shell)
    }

    /// `SpawnFailed`: `kykod` never added the pane, so no `Close` is sent for it.
    pub fn spawn_failed(&mut self, pane: PaneId, message: &str) {
        if let Some(p) = self.panes.get_mut(&pane)
            && p.state == PaneState::Running
        {
            p.state = PaneState::Failed(message.to_string());
        }
    }

    /// A key on a pane that has exited, failed or ended closes it.
    pub fn dismiss(&mut self, pane: PaneId, shell: &str) -> Vec<Effect> {
        match self.panes.get(&pane).map(|p| &p.state) {
            Some(PaneState::Running) | None => Vec::new(),
            Some(_) => self.close_pane(pane, shell),
        }
    }

    /// `kykod` went away: every running pane has ended. Panes that had already exited or
    /// failed keep their own message.
    pub fn holder_lost(&mut self) {
        for p in self.panes.values_mut() {
            if p.state == PaneState::Running {
                p.state = PaneState::Ended;
            }
        }
    }

    /// Moves the focus to the pane on that side: the one whose edge touches the focused pane's
    /// and overlaps it most on the other axis, ties to the most recently focused. False when
    /// nothing is there.
    pub fn focus_dir(&mut self, dir: Dir) -> bool {
        let i = self.active_index();
        let Some(tab) = self.tabs.get(i) else {
            return false;
        };
        let Some(&cur) = tab.focus.first() else {
            return false;
        };
        let mut all = Vec::new();
        tab.root.bounds(UNIT, &mut all);
        let Some(&(_, f)) = all.iter().find(|(p, _)| *p == cur) else {
            return false;
        };
        let rank = |p: PaneId| tab.focus.iter().position(|q| *q == p).unwrap_or(usize::MAX);
        let best = all
            .iter()
            .filter(|(p, _)| *p != cur)
            .filter_map(|&(p, b)| {
                let (touches, ov) = match dir {
                    Dir::Left => (b.x1 == f.x0, overlap(b.y0, b.y1, f.y0, f.y1)),
                    Dir::Right => (b.x0 == f.x1, overlap(b.y0, b.y1, f.y0, f.y1)),
                    Dir::Up => (b.y1 == f.y0, overlap(b.x0, b.x1, f.x0, f.x1)),
                    Dir::Down => (b.y0 == f.y1, overlap(b.x0, b.x1, f.x0, f.x1)),
                };
                (touches && ov > 0.0).then_some((p, ov))
            })
            .max_by(|(p, a), (q, b)| a.total_cmp(b).then(rank(*q).cmp(&rank(*p))));
        match best {
            Some((p, _)) => self.focus_pane(p),
            None => false,
        }
    }

    /// A click on a pane; also switches to its tab.
    pub fn focus_pane(&mut self, pane: PaneId) -> bool {
        let Some(i) = self.tab_of(pane) else {
            return false;
        };
        let tab = &mut self.tabs[i];
        tab.focus.retain(|p| *p != pane);
        tab.focus.insert(0, pane);
        self.active = tab.id;
        true
    }

    pub fn focus_tab(&mut self, tab: TabId) -> bool {
        let found = self.tab_index(tab).is_some();
        if found {
            self.active = tab;
        }
        found
    }

    /// Alt+1…9; 0-based here.
    pub fn focus_tab_index(&mut self, n: usize) -> bool {
        match self.tabs.get(n) {
            Some(t) => {
                self.active = t.id;
                true
            }
            None => false,
        }
    }

    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            let i = (self.active_index() + 1) % self.tabs.len();
            self.active = self.tabs[i].id;
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            let n = self.tabs.len();
            let i = (self.active_index() + n - 1) % n;
            self.active = self.tabs[i].id;
        }
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

    /// Renames a tab, without control characters and cut to 256 bytes; `None` or an empty
    /// result clears the rename.
    pub fn rename(&mut self, tab: TabId, name: Option<&str>) -> bool {
        let Some(i) = self.tab_index(tab) else {
            return false;
        };
        self.tabs[i].rename = name.and_then(clean_rename);
        true
    }

    /// True when a tab's name or program changed (the window then emits `TabsChanged`).
    pub fn apply_info(&mut self, pane: PaneId, program: &str, cwd: &str) -> bool {
        let Some(i) = self.tab_of(pane) else {
            return false;
        };
        let before = (
            self.tab_name(&self.tabs[i]),
            self.tab_program(i).to_string(),
        );
        if let Some(p) = self.panes.get_mut(&pane) {
            p.program = program.to_string();
            p.cwd = cwd.to_string();
            p.had_info = true;
        }
        before
            != (
                self.tab_name(&self.tabs[i]),
                self.tab_program(i).to_string(),
            )
    }

    fn tab_program(&self, i: usize) -> &str {
        self.tabs[i]
            .focus
            .first()
            .and_then(|p| self.panes.get(p))
            .map_or("", |p| p.program.as_str())
    }

    /// The panes of a tab whose foreground program is not the shell, for the close
    /// confirmation.
    pub fn busy_panes(&self, tab: TabId) -> Vec<PaneId> {
        let Some(i) = self.tab_index(tab) else {
            return Vec::new();
        };
        self.tabs[i]
            .root
            .pane_ids()
            .into_iter()
            .filter(|p| {
                self.panes
                    .get(p)
                    .is_some_and(|p| p.state == PaneState::Running && p.busy())
            })
            .collect()
    }

    #[doc(hidden)]
    pub fn check_invariants(&self) -> Result<(), String> {
        if self.tabs.is_empty() {
            return Err("no tab".into());
        }
        if self.tab_index(self.active).is_none() {
            return Err(format!("active tab {} does not exist", self.active));
        }
        if self.panes.len() > MAX_PANES {
            return Err(format!("{} panes", self.panes.len()));
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in &self.tabs {
            if t.id >= self.next_tab {
                return Err(format!("tab {} is not below next {}", t.id, self.next_tab));
            }
            let leaves = t.root.pane_ids();
            for p in &leaves {
                if !seen.insert(*p) {
                    return Err(format!("pane {p} in two leaves"));
                }
                if !self.panes.contains_key(p) {
                    return Err(format!("pane {p} has a leaf and no state"));
                }
                if *p >= self.next_pane {
                    return Err(format!("pane {p} is not below next {}", self.next_pane));
                }
            }
            let mut a = leaves.clone();
            let mut b = t.focus.clone();
            a.sort_unstable();
            b.sort_unstable();
            if a != b {
                return Err(format!(
                    "tab {}: focus {:?}, leaves {:?}",
                    t.id, t.focus, leaves
                ));
            }
            if !t.root.ratios_ok() {
                return Err(format!("tab {}: a ratio out of bounds", t.id));
            }
            let mut bounds = Vec::new();
            t.root.bounds(UNIT, &mut bounds);
            let area: f64 = bounds
                .iter()
                .map(|(_, b)| (b.x1 - b.x0) * (b.y1 - b.y0))
                .sum();
            if (area - 1.0).abs() > 1e-9 {
                return Err(format!("tab {}: panes cover {area}", t.id));
            }
            for (i, (p, x)) in bounds.iter().enumerate() {
                for (q, y) in &bounds[i + 1..] {
                    if overlap(x.x0, x.x1, y.x0, y.x1) > 0.0
                        && overlap(x.y0, x.y1, y.y0, y.y1) > 0.0
                    {
                        return Err(format!("tab {}: panes {p} and {q} overlap", t.id));
                    }
                }
            }
            if let Some(r) = &t.rename
                && (r.is_empty() || r.len() > MAX_RENAME || r.chars().any(char::is_control))
            {
                return Err(format!("tab {}: rename {r:?}", t.id));
            }
        }
        if seen.len() != self.panes.len() {
            return Err(format!(
                "{} panes have state, {} have leaves",
                self.panes.len(),
                seen.len()
            ));
        }
        Ok(())
    }
}

pub(crate) fn clean_rename(name: &str) -> Option<String> {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    let clean = cut(&clean, MAX_RENAME);
    (!clean.is_empty()).then(|| clean.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SH: &str = "/bin/bash";

    fn fresh() -> Session {
        Session::fresh("/home/u", SH).0
    }

    fn tab(s: &Session) -> TabView<'_> {
        s.tabs().find(|t| t.active).unwrap()
    }

    fn rect_of(s: &Session, pane: PaneId) -> Rect {
        s.tabs()
            .flat_map(|t| t.panes)
            .find(|p| p.id == pane)
            .unwrap()
            .rect
    }

    #[test]
    fn fresh_spawns_one_pane() {
        let (s, e) = Session::fresh("/home/u", SH);
        assert_eq!(
            e,
            [Effect::Spawn {
                pane: 1,
                cwd: None,
                args: vec![]
            }]
        );
        assert_eq!(s.focused_pane(), 1);
        assert_eq!(tab(&s).name, "bash");
        s.check_invariants().unwrap();
    }

    #[test]
    fn focus_in_an_l_shape() {
        // [1 | [2 / 3]]
        let mut s = fresh();
        s.split(SplitAxis::Side, SH).unwrap();
        s.split(SplitAxis::Stacked, SH).unwrap();
        assert_eq!(s.focused_pane(), 3);
        assert!(s.focus_dir(Dir::Up));
        assert_eq!(s.focused_pane(), 2);
        assert!(s.focus_dir(Dir::Left));
        assert_eq!(s.focused_pane(), 1);
        // From 1, right touches both 2 and 3 equally: the most recently focused, 2.
        assert!(s.focus_dir(Dir::Right));
        assert_eq!(s.focused_pane(), 2);
        assert!(s.focus_dir(Dir::Down));
        assert!(s.focus_dir(Dir::Left));
        assert!(s.focus_dir(Dir::Right));
        assert_eq!(s.focused_pane(), 3, "3 was more recent than 2");
        assert!(!s.focus_dir(Dir::Right));
        assert!(!s.focus_dir(Dir::Down));
        s.check_invariants().unwrap();
    }

    #[test]
    fn focus_in_a_stack_of_three_and_a_grid_of_four() {
        let mut s = fresh();
        s.split(SplitAxis::Stacked, SH).unwrap();
        s.split(SplitAxis::Stacked, SH).unwrap();
        assert!(s.focus_dir(Dir::Up));
        assert!(s.focus_dir(Dir::Up));
        assert_eq!(s.focused_pane(), 1);
        assert!(!s.focus_dir(Dir::Up));
        assert!(!s.focus_dir(Dir::Left));
        // Grid: [[1 / 3] | [2 / 4]]
        let mut g = fresh();
        g.split(SplitAxis::Side, SH).unwrap(); // 2 right of 1
        g.focus_pane(1);
        g.split(SplitAxis::Stacked, SH).unwrap(); // 3 below 1
        g.focus_pane(2);
        g.split(SplitAxis::Stacked, SH).unwrap(); // 4 below 2
        assert_eq!(g.focused_pane(), 4);
        assert!(g.focus_dir(Dir::Left));
        assert_eq!(g.focused_pane(), 3);
        assert!(g.focus_dir(Dir::Up));
        assert_eq!(g.focused_pane(), 1);
        assert!(g.focus_dir(Dir::Right));
        assert_eq!(g.focused_pane(), 2);
        g.check_invariants().unwrap();
    }

    #[test]
    fn focus_through_255_nested_splits() {
        // Each split is of pane 1, which shrinks towards the origin, where f64 keeps its
        // precision at every depth.
        let mut s = fresh();
        for i in 0..255 {
            let axis = if i % 2 == 0 {
                SplitAxis::Side
            } else {
                SplitAxis::Stacked
            };
            s.split(axis, SH).unwrap();
            s.focus_pane(1);
        }
        assert_eq!(s.panes.len(), 256);
        s.check_invariants().unwrap();
        assert!(rect_of(&s, 1).w > 0.0 && rect_of(&s, 1).h > 0.0);
        // The deepest pane walks out to the first split's other half.
        let mut moves = 0;
        while s.focus_dir(Dir::Right) || s.focus_dir(Dir::Down) {
            moves += 1;
            assert!(moves < 1000);
        }
        assert_eq!(s.focused_pane(), 2);
        assert!(moves >= 1);
        // And back in: left from 2 lands in the chain.
        assert!(s.focus_dir(Dir::Left));
        assert_ne!(s.focused_pane(), 2);
        assert_eq!(s.split(SplitAxis::Side, SH), Err(Full));
        assert_eq!(s.new_tab(SpawnRequest::default(), SH), Err(Full));
        s.check_invariants().unwrap();
    }

    #[test]
    fn closing_the_middle_of_three() {
        let mut s = fresh();
        s.split(SplitAxis::Side, SH).unwrap();
        s.split(SplitAxis::Side, SH).unwrap();
        s.focus_pane(1);
        let e = s.close_pane(2, SH);
        assert_eq!(e, [Effect::Close { pane: 2 }]);
        s.check_invariants().unwrap();
        assert_eq!(s.focused_pane(), 1);
        // 3 keeps its own half; 1 takes its half.
        assert_eq!(rect_of(&s, 3).x, 0.5);
        assert_eq!(rect_of(&s, 1).w, 0.5);
        // Closing the focused one: focus goes to the most recent left, 3.
        s.focus_pane(3);
        s.focus_pane(1);
        let e = s.close_pane(1, SH);
        assert_eq!(e, [Effect::Close { pane: 1 }]);
        assert_eq!(s.focused_pane(), 3);
        assert_eq!(
            rect_of(&s, 3),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0
            }
        );
        s.check_invariants().unwrap();
    }

    #[test]
    fn closing_the_last_pane_and_the_last_tab() {
        let mut s = fresh();
        s.new_tab(SpawnRequest::default(), SH).unwrap();
        let second = s.active_tab();
        assert_eq!(s.close_pane(2, SH), [Effect::Close { pane: 2 }]);
        assert!(s.tabs().all(|t| t.id != second));
        s.check_invariants().unwrap();
        let e = s.close_pane(1, SH);
        assert_eq!(
            e,
            [
                Effect::Close { pane: 1 },
                Effect::Spawn {
                    pane: 3,
                    cwd: None,
                    args: vec![]
                },
                Effect::Hide
            ]
        );
        assert_eq!(s.tabs().count(), 1);
        s.check_invariants().unwrap();
    }

    #[test]
    fn closing_a_tab_picks_the_right_then_the_left() {
        let mut s = fresh(); // tab 1
        for _ in 0..3 {
            s.new_tab(SpawnRequest::default(), SH).unwrap(); // tabs 2, 3, 4 in order
        }
        let ids: Vec<TabId> = s.tabs().map(|t| t.id).collect();
        assert_eq!(ids, [1, 2, 3, 4]);
        s.focus_tab(2);
        s.close_tab(2, SH);
        assert_eq!(s.active_tab(), 3);
        s.focus_tab(4);
        s.close_tab(4, SH);
        assert_eq!(s.active_tab(), 3);
        // A non-active tab: the active one stays.
        s.close_tab(1, SH);
        assert_eq!(s.active_tab(), 3);
        // A new tab goes right after the active one.
        s.focus_tab(3);
        s.new_tab(SpawnRequest::default(), SH).unwrap();
        s.check_invariants().unwrap();
        assert!(s.close_tab(99, SH).is_empty());
    }

    #[test]
    fn new_tab_goes_after_the_active_one() {
        let mut s = fresh();
        s.new_tab(SpawnRequest::default(), SH).unwrap();
        s.focus_tab(1);
        s.new_tab(SpawnRequest::default(), SH).unwrap();
        let ids: Vec<TabId> = s.tabs().map(|t| t.id).collect();
        assert_eq!(ids, [1, 3, 2]);
        assert_eq!(s.active_tab(), 3);
    }

    #[test]
    fn held_failed_and_ended_panes() {
        let mut s = fresh();
        let e = s
            .new_tab(
                SpawnRequest {
                    cwd: Some("/tmp".into()),
                    args: vec!["-c".into(), "make".into()],
                    hold: true,
                },
                SH,
            )
            .unwrap();
        assert_eq!(
            e,
            [Effect::Spawn {
                pane: 2,
                cwd: Some("/tmp".into()),
                args: vec!["-c".into(), "make".into()]
            }]
        );
        // Held: Exited, Close at once, stays.
        assert_eq!(s.pane_exited(2, 2, SH), [Effect::Close { pane: 2 }]);
        assert_eq!(s.pane_state(2), Some(&PaneState::Exited(2)));
        assert!(s.pane_exited(2, 2, SH).is_empty(), "exits once");
        // Dismiss: closes it, no second Close.
        assert!(s.dismiss(2, SH).is_empty());
        assert_eq!(s.pane_state(2), None);
        // Not held: closes with Close.
        s.split(SplitAxis::Side, SH).unwrap();
        assert_eq!(s.pane_exited(3, 0, SH), [Effect::Close { pane: 3 }]);
        assert_eq!(s.pane_state(3), None);
        // Failed: no Close, dismiss closes it.
        s.split(SplitAxis::Side, SH).unwrap();
        s.spawn_failed(4, "no such file");
        assert_eq!(
            s.pane_state(4),
            Some(&PaneState::Failed("no such file".into()))
        );
        assert!(s.dismiss(4, SH).is_empty());
        assert!(
            s.dismiss(1, SH).is_empty(),
            "a running pane is not dismissed"
        );
        assert_eq!(s.pane_state(1), Some(&PaneState::Running));
        // Holder lost: Ended, no Close on dismiss or close.
        s.split(SplitAxis::Side, SH).unwrap();
        s.holder_lost();
        assert_eq!(s.pane_state(1), Some(&PaneState::Ended));
        assert!(s.dismiss(5, SH).is_empty());
        let e = s.close_tab(s.active_tab(), SH);
        assert_eq!(
            e,
            [
                Effect::Spawn {
                    pane: 6,
                    cwd: None,
                    args: vec![]
                },
                Effect::Hide
            ]
        );
        s.check_invariants().unwrap();
    }

    #[test]
    fn full_at_256_panes() {
        let mut s = fresh();
        for _ in 0..255 {
            s.new_tab(SpawnRequest::default(), SH).unwrap();
        }
        let before = s.clone();
        assert_eq!(s.new_tab(SpawnRequest::default(), SH), Err(Full));
        assert_eq!(s.split(SplitAxis::Stacked, SH), Err(Full));
        assert_eq!(s, before);
        // An exited held pane still counts.
        s.close_tab(s.active_tab(), SH);
        s.new_tab(
            SpawnRequest {
                hold: true,
                ..Default::default()
            },
            SH,
        )
        .unwrap();
        s.pane_exited(s.focused_pane(), 0, SH);
        assert_eq!(s.split(SplitAxis::Side, SH), Err(Full));
    }

    #[test]
    fn no_ids_left() {
        let mut s = fresh();
        s.next_pane = u32::MAX;
        assert_eq!(s.split(SplitAxis::Side, SH), Err(Full));
        assert_eq!(s.new_tab(SpawnRequest::default(), SH), Err(Full));
        // The last tab cannot be replaced: it stays, and kyko hides.
        assert_eq!(s.close_tab(1, SH), [Effect::Hide]);
        s.check_invariants().unwrap();
    }

    #[test]
    fn renames() {
        let mut s = fresh();
        assert!(s.rename(1, Some("build")));
        assert_eq!(tab(&s).name, "build");
        assert!(s.rename(1, None));
        assert_eq!(tab(&s).name, "bash");
        s.rename(1, Some("a\u{1b}[31mb\nc\u{7}"));
        assert_eq!(tab(&s).name, "a[31mbc");
        s.rename(1, Some("\n\t"));
        assert_eq!(tab(&s).name, "bash", "an empty result clears");
        let long = "€".repeat(300);
        s.rename(1, Some(&long));
        let name = tab(&s).name;
        assert!(name.len() <= MAX_RENAME && name.len() > MAX_RENAME - 3);
        assert!(name.chars().all(|c| c == '€'));
        assert!(!s.rename(42, Some("x")));
        s.check_invariants().unwrap();
    }

    #[test]
    fn names_from_info() {
        let mut s = fresh();
        assert_eq!(tab(&s).name, "bash");
        assert!(s.apply_info(1, "bash", "/home/u"));
        assert_eq!(tab(&s).name, "~");
        assert!(s.apply_info(1, "bash", "/"));
        assert_eq!(tab(&s).name, "/");
        assert!(s.apply_info(1, "bash", "/home/u/src/kyko"));
        assert_eq!(tab(&s).name, "kyko");
        assert!(
            !s.apply_info(1, "bash", "/home/u/src/kyko"),
            "nothing changed"
        );
        assert!(s.apply_info(1, "vim", "/home/u/src/kyko"));
        assert_eq!(tab(&s).name, "vim");
        assert_eq!(tab(&s).program, "vim");
        assert_eq!(s.busy_panes(1), [1]);
        // An empty program is idle.
        assert!(s.apply_info(1, "", "/home/u/src/kyko"));
        assert_eq!(tab(&s).name, "kyko");
        assert!(s.busy_panes(1).is_empty());
        // A pane that is not focused changes neither the name nor the program.
        s.split(SplitAxis::Side, SH).unwrap();
        s.focus_pane(1);
        assert!(!s.apply_info(2, "", "/home/u"));
        assert!(!s.apply_info(99, "x", "/"));
    }

    #[test]
    fn a_15_byte_comm_is_the_shell() {
        let shell = "/usr/local/bin/a-very-long-shell-name";
        let (mut s, _) = Session::fresh("/home/u", shell);
        assert_eq!(tab(&s).name, "a-very-long-she");
        // kykod's first Info may carry the uncut name, later ones the cut one: both idle.
        s.apply_info(1, "a-very-long-shell-name", "/home/u");
        assert_eq!(tab(&s).name, "~");
        s.apply_info(1, "a-very-long-she", "/home/u");
        assert_eq!(tab(&s).name, "~");
        assert!(s.busy_panes(1).is_empty());
        s.apply_info(1, "a-very-long-sh", "/home/u");
        assert_eq!(s.busy_panes(1), [1]);
    }

    #[test]
    fn ratios() {
        let mut s = fresh();
        s.split(SplitAxis::Side, SH).unwrap();
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(!s.set_ratio(1, &[], bad));
        }
        assert!(s.set_ratio(1, &[], 0.0));
        assert_eq!(tab(&s).dividers[0].ratio, MIN_RATIO);
        assert!(s.set_ratio(1, &[], 7.0));
        assert_eq!(tab(&s).dividers[0].ratio, MAX_RATIO);
        assert!(s.set_ratio(1, &[], 0.3));
        assert_eq!(rect_of(&s, 1).w, 0.3);
        assert!(!s.set_ratio(1, &[false], 0.3), "a leaf has no ratio");
        assert!(!s.set_ratio(9, &[], 0.3));
        s.check_invariants().unwrap();
    }

    #[test]
    fn split_spawns_in_the_focused_folder() {
        let mut s = fresh();
        let e = s.split(SplitAxis::Side, SH).unwrap();
        assert_eq!(
            e,
            [Effect::Spawn {
                pane: 2,
                cwd: None,
                args: vec![]
            }]
        );
        s.apply_info(2, "bash", "/srv");
        let e = s.split(SplitAxis::Stacked, SH).unwrap();
        assert_eq!(
            e,
            [Effect::Spawn {
                pane: 3,
                cwd: Some("/srv".into()),
                args: vec![]
            }]
        );
        assert_eq!(s.new_tab_dir(NewTabDir::Home), None);
        s.apply_info(3, "bash", "/var");
        assert_eq!(s.new_tab_dir(NewTabDir::Current), Some("/var".into()));
    }

    #[test]
    fn tab_switching_wraps() {
        let mut s = fresh();
        s.new_tab(SpawnRequest::default(), SH).unwrap();
        s.new_tab(SpawnRequest::default(), SH).unwrap();
        s.next_tab();
        assert_eq!(s.active_tab(), 1);
        s.prev_tab();
        assert_eq!(s.active_tab(), 3);
        assert!(s.focus_tab_index(1));
        assert_eq!(s.active_tab(), 2);
        assert!(!s.focus_tab_index(3));
        assert!(!s.focus_tab(7));
        // A click on a pane in another tab switches to it.
        assert!(s.focus_pane(1));
        assert_eq!(s.active_tab(), 1);
    }
}
