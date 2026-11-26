//! What the window's actions do to the session: keys, tab and pane buttons, and confirmations.

use super::*;

pub(crate) fn handle(act: Act, s: &mut Session, look: &Look, login: &str) -> Vec<Effect> {
    match act {
        Act::Key(Action::NewTab) => {
            let cwd = s.new_tab_dir(look.new_tab_dir);
            s.new_tab(
                SpawnRequest {
                    cwd,
                    ..SpawnRequest::default()
                },
                login,
            )
            .unwrap_or_default()
        }
        Act::Key(Action::ClosePane) => {
            let pane = s.focused_pane();
            s.close_pane(pane, login)
        }
        Act::Key(Action::PrevTab) => {
            s.prev_tab();
            Vec::new()
        }
        Act::Key(Action::NextTab) => {
            s.next_tab();
            Vec::new()
        }
        Act::Key(Action::Tab(n)) => {
            s.focus_tab_index(usize::from(n).saturating_sub(1));
            Vec::new()
        }
        Act::Key(Action::SplitSide) => s.split(SplitAxis::Side, login).unwrap_or_default(),
        Act::Key(Action::SplitStacked) => s.split(SplitAxis::Stacked, login).unwrap_or_default(),
        Act::Key(Action::FocusLeft) => focus(s, Dir::Left),
        Act::Key(Action::FocusRight) => focus(s, Dir::Right),
        Act::Key(Action::FocusUp) => focus(s, Dir::Up),
        Act::Key(Action::FocusDown) => focus(s, Dir::Down),
        Act::Key(Action::Broadcast) => {
            s.toggle_broadcast();
            Vec::new()
        }
        Act::Key(Action::ZoomPane) => {
            s.toggle_zoom();
            Vec::new()
        }
        // Copy, paste, search and the font size are the pane's own; they come with the
        // selection and search slices.
        Act::Key(_) => Vec::new(),
        Act::FocusPane(p) => {
            s.focus_pane(p);
            Vec::new()
        }
        Act::FocusTab(t) => {
            s.focus_tab(t);
            Vec::new()
        }
        Act::CloseTab(t) => s.close_tab(t, login),
        Act::ClosePane(p) => s.close_pane(p, login),
        Act::Ratio(t, path, r) => {
            s.set_ratio(t, &path, r);
            Vec::new()
        }
        Act::Rename(t, name) => {
            s.rename(t, name.as_deref());
            Vec::new()
        }
        Act::Dismiss(p) => s.dismiss(p, login),
        Act::Confirm(_) | Act::Unicode(_) | Act::PaletteClosed => Vec::new(),
        Act::Run { command, cwd } => {
            let (args, hold) = match command {
                Some(c) => (kyko_core::runq::shell_args(&c), true),
                None => (Vec::new(), false),
            };
            s.new_tab(SpawnRequest { cwd, args, hold }, login)
                .unwrap_or_default()
        }
    }
}
