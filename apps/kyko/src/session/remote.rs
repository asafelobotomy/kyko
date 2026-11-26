//! Requests from other programs (`kyko --remote`, a session file): which panes they mean, and
//! carrying them out.

use super::*;

/// The panes a `kyko @` command names; `Here` is the focused pane (the client turns it into
/// its own pane when it runs inside kyko).
pub(crate) fn remote_targets(
    s: &Session,
    m: &kyko_core::remote::Match,
) -> Result<Vec<PaneId>, String> {
    use kyko_core::remote::Match;
    let all: Vec<(u32, PaneId)> = s
        .tabs()
        .flat_map(|t| t.panes.iter().map(|p| (t.id, p.id)).collect::<Vec<_>>())
        .collect();
    let found: Vec<PaneId> = match m {
        Match::Here => vec![s.focused_pane()],
        Match::Pane(p) => all
            .iter()
            .filter(|(_, q)| q == p)
            .map(|(_, q)| *q)
            .collect(),
        Match::Tab(t) => all
            .iter()
            .filter(|(x, _)| x == t)
            .map(|(_, q)| *q)
            .collect(),
        Match::All => all.iter().map(|(_, q)| *q).collect(),
    };
    if found.is_empty() {
        return Err(match m {
            Match::Pane(p) => format!("no pane {p}"),
            Match::Tab(t) => format!("no tab {t}"),
            _ => "no panes".to_string(),
        });
    }
    Ok(found)
}

/// Carries out a `kyko @` request.
pub(crate) fn remote_request(
    req: kyko_core::remote::Request,
    s: &mut Session,
    link: &mut Link,
    login: &str,
) -> Result<(kyko_core::remote::Reply, Vec<Effect>), LinkError> {
    use kyko_core::remote::{Place, Reply, Request};
    use serde_json::json;
    let fail = |e: String| Ok((Reply::err(e), Vec::new()));
    let done = |data: serde_json::Value| Ok((Reply::ok(data), Vec::new()));
    match req {
        Request::Ls => {
            let tabs: Vec<_> = s
                .tabs()
                .map(|t| {
                    let panes: Vec<_> = t
                        .panes
                        .iter()
                        .map(|p| {
                            let title = link
                                .pane(p.id)
                                .and_then(|x| x.emulator().title().map(str::to_string));
                            let state = match p.state {
                                PaneState::Running => "running",
                                PaneState::Exited(_) => "exited",
                                PaneState::Failed(_) => "failed",
                                PaneState::Ended => "ended",
                            };
                            json!({"id": p.id, "focused": p.focused, "program": p.program,
                                   "cwd": p.cwd, "title": title, "state": state})
                        })
                        .collect();
                    json!({"id": t.id, "name": t.name, "active": t.active, "zoomed": t.zoomed,
                           "broadcast": t.broadcast, "panes": panes})
                })
                .collect();
            done(json!({"tabs": tabs}))
        }
        Request::SendText { target, text } => {
            let panes = match remote_targets(s, &target) {
                Ok(p) => p,
                Err(e) => return fail(e),
            };
            for p in &panes {
                link.input(*p, text.as_bytes())?;
            }
            done(json!({"panes": panes}))
        }
        Request::GetText { target, all } => {
            let panes = match remote_targets(s, &target) {
                Ok(p) => p,
                Err(e) => return fail(e),
            };
            let Some(pn) = link.pane(panes[0]) else {
                return fail(format!("pane {} has no screen", panes[0]));
            };
            let e = pn.emulator();
            done(json!({"text": if all { e.all_text() } else { e.screen_text() }}))
        }
        Request::Launch {
            place,
            cwd,
            args,
            hold,
        } => {
            // A pane's args are its shell's: the command goes to it as `-c 'word' 'word'`.
            let args = if args.is_empty() {
                args
            } else {
                let words: Vec<String> = args.iter().map(|a| kyko_core::runq::quote(a)).collect();
                kyko_core::runq::shell_args(&words.join(" "))
            };
            let req = SpawnRequest { cwd, args, hold };
            let made = match place {
                Place::Tab => s.new_tab(req, login),
                Place::SplitSide => s.split_with(SplitAxis::Side, req, login),
                Place::SplitStacked => s.split_with(SplitAxis::Stacked, req, login),
            };
            match made {
                Ok(effects) => {
                    let pane = s.focused_pane();
                    Ok((Reply::ok(json!({"pane": pane})), effects))
                }
                Err(_) => fail("no room for another pane".into()),
            }
        }
        Request::FocusPane { pane } => {
            if s.focus_pane(pane) {
                done(serde_json::Value::Null)
            } else {
                fail(format!("no pane {pane}"))
            }
        }
        Request::FocusTab { tab } => {
            if s.focus_tab(tab) {
                done(serde_json::Value::Null)
            } else {
                fail(format!("no tab {tab}"))
            }
        }
        Request::ClosePane { target } => {
            let panes = match remote_targets(s, &target) {
                Ok(p) => p,
                Err(e) => return fail(e),
            };
            let mut effects = Vec::new();
            for p in panes {
                effects.extend(s.close_pane(p, login));
            }
            Ok((Reply::ok(serde_json::Value::Null), effects))
        }
        Request::SetTabTitle { tab, title } => {
            let tab = tab.unwrap_or_else(|| s.active_tab());
            let name = (!title.is_empty()).then_some(title.as_str());
            if s.rename(tab, name) {
                done(serde_json::Value::Null)
            } else {
                fail(format!("no tab {tab}"))
            }
        }
        Request::ToggleZoom => {
            s.toggle_zoom();
            done(json!({"zoomed": s.tabs().find(|t| t.active).is_some_and(|t| t.zoomed)}))
        }
        Request::ToggleBroadcast => {
            s.toggle_broadcast();
            done(json!({"broadcast": s.tabs().find(|t| t.active).is_some_and(|t| t.broadcast)}))
        }
    }
}
