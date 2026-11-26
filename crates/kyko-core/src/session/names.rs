//! Tab names and what runs in each pane: renaming, the program and folder `kykod` reports, and
//! which panes are busy.

use super::*;

impl Session {
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

    pub(crate) fn tab_program(&self, i: usize) -> &str {
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
}
