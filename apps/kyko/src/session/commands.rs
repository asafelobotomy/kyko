//! The window's commands to the session thread, one at a time.

use super::*;

impl Run<'_> {
    /// One command from the window; `true` when the window is closing.
    pub(super) fn command(&mut self, cmd: Cmd) -> Result<bool, Stop> {
        let ctx = self.ctx;
        let look = || ctx.look.lock().unwrap().clone();
        let Run {
            link,
            session,
            sizes,
            login,
            pending,
            held_paste,
            find,
            unicode,
            palette,
            dirty,
            layout_changed,
            effects,
            ..
        } = self;
        match cmd {
            Cmd::Input(p, bytes) => link.input(p, &bytes)?,
            Cmd::Typed(p, bytes) => {
                for t in session.typing_targets(p) {
                    // Typing goes to the live screen and takes the selection away.
                    if let Some(pn) = link.pane_mut(t) {
                        let e = pn.emulator_mut();
                        if e.display_offset() > 0 || e.selection_text().is_some() {
                            e.scroll_to_bottom();
                            e.select_clear();
                            dirty.insert(t);
                        }
                    }
                    link.input(t, &bytes)?;
                }
            }
            Cmd::Mouse(p, ev) => {
                let l = look();
                if mouse(link, p, ev, l.copy_on_select, &*ctx.on_copy)? {
                    dirty.insert(p);
                }
            }
            Cmd::Copy(p) => {
                if let Some(t) = link.pane(p).and_then(|pn| pn.emulator().selection_text()) {
                    (ctx.on_copy)(t, false);
                }
            }
            Cmd::PromptJump(p, dir) => {
                if let Some(pn) = link.pane_mut(p)
                    && pn.emulator_mut().jump_prompt(dir)
                {
                    dirty.insert(p);
                }
            }
            Cmd::FailedJump(p, dir) => {
                if let Some(pn) = link.pane_mut(p)
                    && pn.emulator_mut().jump_failed(dir)
                {
                    dirty.insert(p);
                }
            }
            Cmd::CopyLastCommand(p) => {
                let last = ctx.shared.lock().unwrap().recent.first().cloned();
                let here = link
                    .pane(p)
                    .and_then(|pn| pn.emulator().last_command().map(str::to_string));
                if let Some(c) = here.or(last) {
                    (ctx.on_copy)(c, false);
                }
            }
            Cmd::ScrollTo(p, offset) => {
                if let Some(pn) = link.pane_mut(p) {
                    let e = pn.emulator_mut();
                    let target = (offset as usize).min(e.history_len());
                    let by = target as i64 - e.display_offset() as i64;
                    if by != 0 {
                        e.scroll(by.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
                        dirty.insert(p);
                    }
                }
            }
            Cmd::ShowScrollback(p) => {
                let Some(text) = link.pane(p).map(|pn| pn.emulator().all_text()) else {
                    return Ok(false);
                };
                match scrollback_file(&text) {
                    Ok(path) => {
                        let q = kyko_core::runq::quote(&path.to_string_lossy());
                        // `#!/bin/sh` runs it in sh whatever the login shell is.
                        let command = format!("#!/bin/sh\nless -R +G -- {q}; rm -f -- {q}");
                        let req = SpawnRequest {
                            cwd: None,
                            args: kyko_core::runq::shell_args(&command),
                            hold: false,
                        };
                        effects.extend(session.new_tab(req, login).unwrap_or_default());
                        *layout_changed = true;
                    }
                    Err(e) => kyko_diag::problem(
                        "kyko",
                        kyko_diag::Level::WARN,
                        &format!("[KYKO-010] cannot write the scrollback: {e}"),
                    ),
                }
            }
            Cmd::CopyLastOutput(p) => {
                if let Some(pn) = link.pane_mut(p) {
                    let e = pn.emulator_mut();
                    if e.select_last_output() {
                        if let Some(t) = e.selection_text() {
                            (ctx.on_copy)(t, false);
                        }
                        dirty.insert(p);
                    }
                }
            }
            Cmd::Search(op) => {
                let pane = session.focused_pane();
                if let Some(pn) = link.pane_mut(pane) {
                    let e = pn.emulator_mut();
                    match op {
                        SearchOp::Find(q) => {
                            find.total = e.search_find(&q);
                            find.pos = e.search_state().map_or(0, |(p, _)| p);
                        }
                        SearchOp::Next | SearchOp::Prev => {
                            if let Some((p, t)) = e.search_step(matches!(op, SearchOp::Next)) {
                                (find.pos, find.total) = (p, t);
                            }
                        }
                        SearchOp::Close => {
                            e.search_clear();
                            *find = Find::default();
                        }
                    }
                    dirty.insert(pane);
                }
                *layout_changed = true;
            }
            Cmd::Paste(p, text) => {
                let bracketed = link
                    .pane(p)
                    .is_some_and(|pn| pn.emulator().modes().bracketed_paste);
                if needs_paste_confirm(&text, bracketed, look().confirm_multiline_paste) {
                    let lines = text.trim_end_matches('\n').lines().count();
                    *pending = Some((Pending::Paste(p), vec![lines.to_string()]));
                    *held_paste = text;
                    *layout_changed = true;
                    return Ok(false);
                }
                for t in session.typing_targets(p) {
                    link.input(t, &paste_bytes(&text, bracketed))?;
                }
            }
            Cmd::CellPx(w, h) => link.set_cell_pixels(u32::from(w), u32::from(h)),
            Cmd::Resize(p, c, r) => {
                sizes.insert(p, (c, r));
                if link.pane(p).is_some() {
                    link.resize(p, c, r)?;
                    dirty.insert(p);
                }
            }
            Cmd::Redraw => dirty.extend(session.pane_ids()),
            Cmd::Act(act) => {
                *layout_changed = true;
                if matches!(act, Act::Key(Action::Search)) {
                    // The key opens the find bar; the bar's own Escape closes it.
                    find.open = true;
                    return Ok(false);
                }
                if matches!(act, Act::Key(Action::UnicodeInput)) {
                    *unicode = true;
                    return Ok(false);
                }
                if matches!(act, Act::Key(Action::CommandPalette)) {
                    *palette = true;
                    return Ok(false);
                }
                if matches!(act, Act::PaletteClosed) {
                    *palette = false;
                    return Ok(false);
                }
                if let Act::Unicode(text) = act {
                    *unicode = false;
                    if let Some(t) = text {
                        for p in session.typing_targets(session.focused_pane()) {
                            link.input(p, t.as_bytes())?;
                        }
                    }
                    return Ok(false);
                }
                // Closing something that runs a program asks first.
                let target = match &act {
                    Act::Key(Action::ClosePane) => Some(Pending::Pane(session.focused_pane())),
                    Act::ClosePane(p) => Some(Pending::Pane(*p)),
                    Act::CloseTab(t) => Some(Pending::Tab(*t)),
                    _ => None,
                };
                if let Some(t) = target {
                    let programs = busy_programs(session, t);
                    if !programs.is_empty() {
                        *pending = Some((t, programs));
                        return Ok(false);
                    }
                }
                if let Act::Confirm(yes) = act {
                    if let (Some((t, _)), true) = (pending.take(), yes) {
                        let act = match t {
                            Pending::Pane(p) => Act::ClosePane(p),
                            Pending::Tab(t) => Act::CloseTab(t),
                            Pending::Paste(p) => {
                                link.input(p, &paste_bytes(held_paste, false))?;
                                held_paste.clear();
                                return Ok(false);
                            }
                        };
                        effects.extend(handle(act, session, &look(), login));
                    }
                    held_paste.clear();
                    return Ok(false);
                }
                effects.extend(handle(act, session, &look(), login));
            }
            Cmd::Remote(req, reply) => {
                *layout_changed = true;
                let (answer, more) = remote_request(req, session, link, login)?;
                for p in session.pane_ids() {
                    dirty.insert(p);
                }
                effects.extend(more);
                let _ = reply.send(answer);
            }
            Cmd::End => {
                if let Some(p) = &ctx.rc_path {
                    let _ = std::fs::remove_file(p);
                }
                if !ctx.launch.persistent {
                    // The window is closing and its shells go with it.
                    let _ = link.end_sessions(true);
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
}
