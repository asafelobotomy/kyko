//! Moving focus: to a neighbouring pane, to a given pane or tab, and between tabs.

use super::*;

impl Session {
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
}
