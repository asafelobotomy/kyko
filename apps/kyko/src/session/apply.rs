//! Carrying out the session's effects on the link to `kykod`, and publishing what the window
//! draws: each pane's frame and the tab layout as JSON.

use super::*;

pub(crate) fn focus(s: &mut Session, dir: Dir) -> Vec<Effect> {
    s.focus_dir(dir);
    Vec::new()
}

/// Where the shell integration scripts live, written on first use; `None` (logged once) when
/// they cannot be written, and the shell then starts without them.
pub(crate) fn integration_dir() -> Option<PathBuf> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join("kyko").join("shell");
        match kyko_core::integration::install(&dir) {
            Ok(()) => Some(dir),
            Err(e) => {
                kyko_diag::problem(
                    "kyko",
                    kyko_diag::Level::WARN,
                    &format!(
                        "[KYKO-008] shell integration is off: {}: {e}",
                        dir.display()
                    ),
                );
                None
            }
        }
    })
    .clone()
}

/// Carries out what the session asked for. True when it asked to hide the window.
pub(crate) fn apply(
    effects: Vec<Effect>,
    link: &mut Link,
    ctx: &Ctx,
    sizes: &HashMap<PaneId, (u16, u16)>,
    use_launch: &mut bool,
) -> Result<bool, LinkError> {
    let mut hide = false;
    for effect in effects {
        match effect {
            Effect::Spawn { pane, cwd, args } => {
                let size = sizes.get(&pane).copied().unwrap_or((80, 24));
                let (shell, args, cwd) = if std::mem::take(use_launch) {
                    (
                        ctx.launch.shell.clone(),
                        ctx.launch.args.clone(),
                        cwd.unwrap_or_else(|| ctx.launch.cwd.clone()),
                    )
                } else {
                    let (shell, args) = kyko_core::runq::resolve(&ctx.launch.login_shell, args);
                    (
                        shell,
                        args,
                        cwd.unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into())),
                    )
                };
                let mut env = vec![
                    ("TERM".to_string(), "xterm-256color".to_string()),
                    ("COLORTERM".to_string(), "truecolor".to_string()),
                    ("KYKO_PANE_ID".to_string(), pane.to_string()),
                ];
                if let Some(p) = &ctx.rc_path {
                    env.push((
                        "KYKO_LISTEN_ON".to_string(),
                        format!("unix:{}", p.display()),
                    ));
                }
                let mut args = args;
                let (integrate, extras) = {
                    let l = ctx.look.lock().unwrap();
                    (l.shell_integration, l.shell_extras)
                };
                if integrate && let Some(dir) = integration_dir() {
                    let mut plan = kyko_core::integration::plan(
                        &shell,
                        &args,
                        &dir,
                        &kyko_core::integration::Host::current(),
                    );
                    if !extras {
                        for (k, v) in &mut plan.env {
                            if k == "KYKO_EXTRAS" {
                                *v = "0".into();
                            }
                        }
                    }
                    // The flags go first: `bash --rcfile f`, `fish --init-command c`.
                    args.splice(0..0, plan.args);
                    env.extend(plan.env);
                }
                link.spawn(SpawnSpec {
                    pane,
                    shell,
                    args,
                    cwd,
                    env,
                    cols: size.0,
                    rows: size.1,
                    ring_bytes: 0,
                })?;
                link.attach(pane)?;
            }
            Effect::Close { pane } => link.close(pane)?,
            Effect::Hide => hide = true,
        }
    }
    Ok(hide)
}

pub(crate) fn publish(link: &Link, pane: PaneId, colors: &Mutex<Colors>, shared: &Mutex<Shared>) {
    if let Some(p) = link.pane(pane) {
        let modes = p.emulator().modes();
        let snapshot = p.emulator().snapshot(&colors.lock().unwrap());
        let frame = Frame {
            snapshot: Some(snapshot),
            app_cursor: modes.app_cursor,
            kitty: modes.kitty_flags,
            focus_reporting: modes.focus_reporting,
            mouse_tracking: modes.mouse_click || modes.mouse_drag || modes.mouse_motion,
            scroll: (
                u32::try_from(p.emulator().display_offset()).unwrap_or(u32::MAX),
                u32::try_from(p.emulator().history_len()).unwrap_or(u32::MAX),
            ),
            header: p.emulator().sticky_header(),
        };
        shared.lock().unwrap().frames.insert(pane, frame);
    }
}

/// The tabs for the QML: `{active, focused, title, notice, tabs: [{id, name, active, panes:
/// [{id, x, y, w, h, focused, state, status, message}], dividers: [{tab, path, axis, x, y, w, h,
/// ratio}]}]}`, rectangles in each tab's unit square.
pub fn layout_json(
    s: &Session,
    notice: Option<&str>,
    confirm: Option<&(Pending, Vec<String>)>,
    find: Find,
    unicode: bool,
    palette: bool,
) -> String {
    use serde_json::json;
    let mut title = String::new();
    let tabs: Vec<_> = s
        .tabs()
        .map(|t| {
            if t.active {
                title = t.program.to_string();
            }
            let panes: Vec<_> = t
                .panes
                .iter()
                .map(|p| {
                    let (state, status, message) = match p.state {
                        PaneState::Running => ("running", 0, String::new()),
                        PaneState::Exited(n) => ("exited", *n, String::new()),
                        PaneState::Failed(m) => ("failed", 0, m.clone()),
                        PaneState::Ended => ("ended", 0, String::new()),
                    };
                    json!({"id": p.id, "x": p.rect.x, "y": p.rect.y, "w": p.rect.w, "h": p.rect.h,
                           "focused": p.focused, "hidden": p.hidden, "busy": p.busy, "state": state,
                           "status": status, "message": message})
                })
                .collect();
            let dividers: Vec<_> = t
                .dividers
                .iter()
                .map(|d| {
                    let path: String = d.path.iter().map(|b| if *b { '1' } else { '0' }).collect();
                    json!({"tab": t.id, "path": path,
                           "axis": if d.axis == SplitAxis::Side { "side" } else { "stacked" },
                           "x": d.rect.x, "y": d.rect.y, "w": d.rect.w, "h": d.rect.h,
                           "ratio": d.ratio})
                })
                .collect();
            json!({"id": t.id, "name": t.name, "program": t.program, "active": t.active, "panes": panes,
                   "dividers": dividers, "zoomed": t.zoomed, "broadcast": t.broadcast})
        })
        .collect();
    let mut out = String::new();
    let _ = write!(
        out,
        "{}",
        json!({"active": s.active_tab(), "focused": s.focused_pane(), "title": title,
               "notice": notice.unwrap_or(""), "tabs": tabs,
               "find": {"open": find.open, "pos": find.pos + 1, "total": find.total},
               "unicode": unicode,
               "palette": palette,
               "confirm": confirm.map(|(what, programs)| json!({
                   "what": match what {
                       Pending::Pane(_) => "pane",
                       Pending::Tab(_) => "tab",
                       Pending::Paste(_) => "paste",
                   },
                   "programs": programs.join(", ")}))})
    );
    out
}
