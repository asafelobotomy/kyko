//! Ending panes and tabs by the close rules of §2.3: closing, a shell that exits or fails to start,
//! and losing `kykod`.

use super::*;

impl Session {
    /// Takes `pane` out of the session; `Close` when it is still running in `kykod`.
    pub(crate) fn drop_pane(&mut self, pane: PaneId, effects: &mut Vec<Effect>) {
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
}
