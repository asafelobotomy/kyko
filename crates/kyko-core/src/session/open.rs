//! Opening panes: a new tab, the folder it starts in, and splitting the focused pane.

use super::*;

impl Session {
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
                zoomed: false,
                broadcast: false,
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
        let cwd = self.new_tab_dir(NewTabDir::Current);
        self.split_with(
            axis,
            SpawnRequest {
                cwd,
                args: Vec::new(),
                hold: false,
            },
            shell,
        )
    }

    /// Splits the focused pane, the new one running `req`.
    pub fn split_with(
        &mut self,
        axis: SplitAxis,
        req: SpawnRequest,
        shell: &str,
    ) -> Result<Vec<Effect>, Full> {
        self.room()?;
        let i = self.active_index();
        let Some(&old) = self.tabs.get(i).and_then(|t| t.focus.first()) else {
            return Err(Full);
        };
        let pane = self.pane_id()?;
        self.panes.insert(pane, Pane::new(shell, req.hold));
        let tab = &mut self.tabs[i];
        tab.zoomed = false;
        if matches!(tab.root, Node::Leaf(_)) {
            tab.broadcast = false;
        }
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
            cwd: req.cwd,
            args: req.args,
        }])
    }
}
