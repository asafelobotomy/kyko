//! One connection's main loop: connect and reconcile with what `kykod` holds, then take the
//! window's commands and `kykod`'s events in rounds, publishing frames and the layout after each.

use super::*;

/// The state of [`run`] for one connection to `kykod`.
pub(super) struct Run<'a> {
    pub(super) ctx: &'a Ctx,
    pub(super) link: Link,
    pub(super) session: &'a mut Session,
    pub(super) sizes: &'a mut HashMap<PaneId, (u16, u16)>,
    pub(super) login: String,
    pub(super) use_launch: bool,
    pub(super) quit: bool,
    pub(super) last_status: i32,
    pub(super) pending: Option<(Pending, Vec<String>)>,
    /// The text a paste waiting for the user's word would send.
    pub(super) held_paste: String,
    pub(super) find: Find,
    pub(super) unicode: bool,
    pub(super) palette: bool,
    pub(super) dirty: HashSet<PaneId>,
    pub(super) layout_changed: bool,
    /// What this round's commands and events ask of `kykod`.
    pub(super) effects: Vec<Effect>,
}

pub(super) fn run(
    ctx: &Ctx,
    rx: &Receiver<Cmd>,
    sizes: &mut HashMap<PaneId, (u16, u16)>,
    slot: &mut Option<Session>,
) -> Result<(), Stop> {
    let mut r = Run::start(ctx, sizes, slot)?;
    loop {
        r.effects = Vec::new();
        r.link.set_theme(&ctx.colors.lock().unwrap());
        while let Ok(cmd) = rx.try_recv() {
            if r.command(cmd)? {
                return Err(Stop::Window);
            }
        }
        let events = r.link.pump(Duration::from_millis(4))?;
        for event in events {
            r.event(event);
        }
        r.end_round()?;
    }
}

impl<'a> Run<'a> {
    /// Connects, reconciles the session with the panes `kykod` holds and runs a session file's
    /// requests.
    fn start(
        ctx: &'a Ctx,
        sizes: &'a mut HashMap<PaneId, (u16, u16)>,
        slot: &'a mut Option<Session>,
    ) -> Result<Run<'a>, Stop> {
        let look = || ctx.look.lock().unwrap().clone();
        let mut link = Link::connect(&ctx.socket, look().scrollback)?;
        // A program that draws an image at once needs the cell size: give the window a moment to
        // measure it (it is there already after the first time).
        let until = std::time::Instant::now() + Duration::from_millis(500);
        while ctx.shared.lock().unwrap().cell_px.is_none() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let px = ctx.shared.lock().unwrap().cell_px;
        if let Some((w, h)) = px {
            link.set_cell_pixels(u32::from(w), u32::from(h));
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let infos: Vec<PaneInfo> = link
            .pane_ids()
            .into_iter()
            .filter_map(|id| {
                link.pane(id).map(|p| PaneInfo {
                    pane: id,
                    program: p.program.clone(),
                    cwd: p.cwd.clone(),
                    cols: 80,
                    rows: 24,
                    exited: p.exited,
                })
            })
            .collect();
        let mut use_launch = infos.is_empty() && slot.is_none();
        let base = match slot.take() {
            Some(s) => kyko_core::layout::Base::Current(s),
            None => kyko_core::layout::Base::Blob(&link.layout),
        };
        let (fresh, effects, _warnings) =
            Session::reconcile(base, &infos, &home, &ctx.launch.shell);
        // Kept across a lost connection: the next link reconciles it with what `kykod` still holds.
        let session = slot.insert(fresh);
        let login = ctx.launch.login_shell.clone();
        let mut quit = false;
        let spawned: HashSet<PaneId> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Spawn { pane, .. } => Some(*pane),
                _ => None,
            })
            .collect();
        if apply(effects, &mut link, ctx, sizes, &mut use_launch)? && !ctx.launch.persistent {
            quit = true;
        }
        for id in session.pane_ids() {
            if link.pane(id).is_some() && !spawned.contains(&id) {
                link.attach(id)?;
                if let Some(&(c, r)) = sizes.get(&id) {
                    link.resize(id, c, r)?;
                }
            }
        }
        // A session file: its tabs take the place of the first one.
        let startup = std::mem::take(&mut *ctx.startup.lock().unwrap());
        if !startup.is_empty() {
            use kyko_core::remote::{Place, Request};
            let first = session.focused_pane();
            let replaces = startup.iter().any(|r| {
                matches!(
                    r,
                    Request::Launch {
                        place: Place::Tab,
                        ..
                    }
                )
            });
            let mut more = Vec::new();
            for req in startup {
                let (reply, effects) = remote_request(req, session, &mut link, &login)?;
                if let Some(e) = reply.error {
                    kyko_diag::problem(
                        "kyko",
                        kyko_diag::Level::WARN,
                        &format!("[KYKO-009] session file: {e}"),
                    );
                }
                more.extend(effects);
            }
            if replaces {
                more.extend(session.close_pane(first, &login));
            }
            apply(more, &mut link, ctx, sizes, &mut use_launch)?;
        }
        ctx.shared.lock().unwrap().notice = None;
        let last_status = 0;
        let pending: Option<(Pending, Vec<String>)> = None;
        // The text a paste waiting for the user's word would send.
        let held_paste = String::new();
        let find = Find::default();
        let unicode = false;
        let palette = false;
        let dirty: HashSet<PaneId> = session.pane_ids().into_iter().collect();
        let layout_changed = true;
        Ok(Run {
            ctx,
            link,
            session,
            sizes,
            login,
            use_launch,
            quit,
            last_status,
            pending,
            held_paste,
            find,
            unicode,
            palette,
            dirty,
            layout_changed,
            effects: Vec::new(),
        })
    }

    /// One event from `kykod`.
    fn event(&mut self, event: Event) {
        let ctx = self.ctx;
        let look = || ctx.look.lock().unwrap().clone();
        let Run {
            link,
            session,
            login,
            last_status,
            dirty,
            layout_changed,
            effects,
            ..
        } = self;
        match event {
            Event::Changed(p) => {
                dirty.insert(p);
                if let Some(pn) = link.pane_mut(p) {
                    for (title, body) in pn.emulator_mut().take_notifications() {
                        notify(title, body);
                    }
                    if pn.emulator_mut().take_bell() {
                        (ctx.on_bell)(p);
                    }
                    // OSC 52: only when the user allows programs to set the clipboard.
                    if let Some((text, primary)) = pn.emulator_mut().take_clipboard()
                        && look().programs_write_clipboard
                        && text.len() <= 1 << 20
                    {
                        (ctx.on_copy)(text, primary);
                    }
                    let finished = pn.emulator_mut().take_finished();
                    if !finished.is_empty() {
                        let l = look();
                        let seen = ctx.shared.lock().unwrap().looking_at == Some(p);
                        let title = pn.emulator().title().unwrap_or("kyko").to_string();
                        {
                            let mut sh = ctx.shared.lock().unwrap();
                            for c in finished.iter().filter_map(|f| f.command.clone()) {
                                sh.recent.retain(|r| *r != c);
                                sh.recent.insert(0, c);
                            }
                            sh.recent.truncate(RECENT);
                        }
                        for f in finished {
                            if finish_worth_telling(l.notify_on_finish, l.notify_after, f.ran, seen)
                            {
                                // The command itself says more than the tab's title.
                                let head = f.command.unwrap_or_else(|| title.clone());
                                notify(head, finished_text(f.ran, f.status));
                            }
                        }
                    }
                }
            }
            Event::Info { pane, program, cwd } => {
                ctx.shared.lock().unwrap().cwds.insert(pane, cwd.clone());
                *layout_changed |= session.apply_info(pane, &program, &cwd);
                dirty.insert(pane);
            }
            Event::Exited { pane, status } => {
                *last_status = status;
                effects.extend(session.pane_exited(pane, status, login));
                *layout_changed = true;
                dirty.insert(pane);
            }
            Event::SpawnFailed { pane, message } => {
                session.spawn_failed(pane, &message);
                *layout_changed = true;
            }
            Event::Busy(_) => {}
        }
    }

    /// After each round: carries out the effects, then publishes the layout and the frames that
    /// changed.
    fn end_round(&mut self) -> Result<(), Stop> {
        let ctx = self.ctx;
        let Run {
            link,
            session,
            sizes,
            use_launch,
            quit,
            last_status,
            pending,
            find,
            unicode,
            palette,
            dirty,
            layout_changed,
            effects,
            ..
        } = self;
        if !effects.is_empty() {
            let hide = apply(std::mem::take(effects), link, ctx, sizes, use_launch)?;
            *layout_changed = true;
            if hide {
                if ctx.launch.persistent {
                    (ctx.on_hide)();
                } else if !*quit {
                    *quit = true;
                    ctx.shared.lock().unwrap().exit = Some(*last_status);
                    (ctx.on_quit)();
                }
            }
        }
        if *layout_changed {
            *layout_changed = false;
            link.save_layout(session.to_blob())?;
            let ids: HashSet<PaneId> = session.pane_ids().into_iter().collect();
            let json = {
                let mut sh = ctx.shared.lock().unwrap();
                sh.frames.retain(|id, _| ids.contains(id));
                sh.focused = session.focused_pane();
                layout_json(
                    session,
                    sh.notice.as_deref(),
                    pending.as_ref(),
                    *find,
                    *unicode,
                    *palette,
                )
            };
            ctx.shared.lock().unwrap().layout = json;
            dirty.extend(ids);
            (ctx.on_layout)();
        }
        if !dirty.is_empty() {
            for p in dirty.drain() {
                publish(link, p, &ctx.colors, &ctx.shared);
            }
            (ctx.on_frames)();
        }
        Ok(())
    }
}
