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

mod panes;
mod tabs;
