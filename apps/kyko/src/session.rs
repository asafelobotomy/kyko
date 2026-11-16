//! The window's side of the sessions, with no Qt: one thread owns the link to `kykod` and the
//! [`kyko_core::session::Session`] (tabs, split trees, focus), takes input, resizes and actions
//! from the window over a channel, and publishes each pane's screen as a [`Frame`] and the tab
//! layout as JSON the QML draws.
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kyko_core::config::Action;
use kyko_core::keys::{Key, Mods};
use kyko_core::session::{Dir, Effect, PaneState, Session, SpawnRequest, SplitAxis};
use kyko_link::{Event, Link, LinkError};
use kyko_proto::{PaneId, PaneInfo, SpawnSpec};
use kyko_term::{Colors, SelectKind, Snapshot};

use crate::appearance::Look;

/// The colours the next snapshot is resolved with; the window swaps them on a theme change.
pub type SharedColors = Arc<Mutex<Colors>>;
/// What the window's look says about tabs and keys, read when it is needed.
pub type SharedLook = Arc<Mutex<Look>>;

/// What the window draws for one pane.
#[derive(Clone)]
pub struct Frame {
    pub snapshot: Option<Snapshot>,
    pub app_cursor: bool,
}

/// What the window reads: the panes' frames and the tab layout.
#[derive(Default)]
pub struct Shared {
    pub frames: HashMap<PaneId, Frame>,
    /// The tabs as JSON, see [`layout_json`].
    pub layout: String,
    pub focused: PaneId,
    /// The status of the last pane to end, once the window has nothing left to show.
    pub exit: Option<i32>,
    /// Set when `kykod` cannot be reached or went away; the window says so.
    pub notice: Option<String>,
}

/// What the find bar asks of the focused pane's scrollback.
#[derive(Clone, Debug)]
pub enum SearchOp {
    Find(String),
    Next,
    Prev,
    Close,
}

/// The find bar as the QML shows it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Find {
    pub open: bool,
    pub pos: usize,
    pub total: usize,
}

/// What a mouse did over a pane's grid, in cells.
#[derive(Clone, Copy, Debug)]
pub enum MouseKind {
    Press,
    Move,
    Release,
    Wheel,
}

#[derive(Clone, Copy, Debug)]
pub struct MouseEv {
    pub kind: MouseKind,
    /// 0 left, 1 middle, 2 right, 3 none; for the wheel 0 up and 1 down.
    pub button: u8,
    pub col: u16,
    pub row: u16,
    /// The pointer is in the right half of its cell.
    pub right: bool,
    /// Shift 1, Alt 2, Ctrl 4, as for keys.
    pub mods: u8,
    /// 1, 2 or 3 for a single, double or triple click.
    pub clicks: u8,
}

enum Cmd {
    Input(PaneId, Vec<u8>),
    Mouse(PaneId, MouseEv),
    Copy(PaneId),
    Paste(PaneId, String),
    Search(SearchOp),
    Resize(PaneId, u16, u16),
    Redraw,
    Act(Act),
    End,
}

/// What the window asks of the tabs.
#[derive(Clone, Debug)]
pub enum Act {
    Key(Action),
    FocusPane(PaneId),
    FocusTab(u32),
    CloseTab(u32),
    ClosePane(PaneId),
    Ratio(u32, Vec<bool>, f64),
    Rename(u32, Option<String>),
    Dismiss(PaneId),
    /// The answer to a close that asked first.
    Confirm(bool),
    /// A new tab that runs a command (held open after it ends), or a plain one.
    Run {
        command: Option<String>,
        cwd: Option<String>,
    },
}

pub struct Workspace {
    tx: Sender<Cmd>,
    colors: SharedColors,
    pub shared: Arc<Mutex<Shared>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// What to run in the first pane.
pub struct Launch {
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// The shell for the panes the user opens later (`$SHELL`).
    pub login_shell: String,
    /// The holder outlives this window (the drop-down's): the tabs are restored from it, a lost
    /// connection is retried, and closing the window ends nothing.
    pub persistent: bool,
}

/// The window's links to the thread that do not need `Look` locked on the thread.
struct Ctx {
    socket: PathBuf,
    launch: Launch,
    colors: SharedColors,
    look: SharedLook,
    shared: Arc<Mutex<Shared>>,
    on_frames: Box<dyn Fn() + Send>,
    on_layout: Box<dyn Fn() + Send>,
    on_hide: Box<dyn Fn() + Send>,
    on_quit: Box<dyn Fn() + Send>,
    on_copy: Box<dyn Fn(String) + Send>,
}

impl Workspace {
    /// Starts the thread. The callbacks run on it: `on_frames` when a pane's screen changed,
    /// `on_layout` when the tabs did, `on_hide` when the drop-down should hide (its last tab
    /// closed), `on_quit` when a windowed kyko has nothing left to show.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        socket: PathBuf,
        launch: Launch,
        look: SharedLook,
        on_frames: Box<dyn Fn() + Send>,
        on_layout: Box<dyn Fn() + Send>,
        on_hide: Box<dyn Fn() + Send>,
        on_quit: Box<dyn Fn() + Send>,
        on_copy: Box<dyn Fn(String) + Send>,
    ) -> Workspace {
        let (tx, rx) = channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let colors: SharedColors = Arc::new(Mutex::new(look.lock().unwrap().colors.clone()));
        let ctx = Ctx {
            socket,
            launch,
            colors: colors.clone(),
            look,
            shared: shared.clone(),
            on_frames,
            on_layout,
            on_hide,
            on_quit,
            on_copy,
        };
        let thread = std::thread::Builder::new()
            .name("kyko-link".into())
            .spawn(move || thread_main(ctx, rx))
            .expect("thread starts");
        Workspace {
            tx,
            colors,
            shared,
            thread: Mutex::new(Some(thread)),
        }
    }

    pub fn input(&self, pane: PaneId, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Input(pane, bytes));
    }

    pub fn search(&self, op: SearchOp) {
        let _ = self.tx.send(Cmd::Search(op));
    }

    pub fn mouse(&self, pane: PaneId, ev: MouseEv) {
        let _ = self.tx.send(Cmd::Mouse(pane, ev));
    }

    /// Puts the pane's selection on the clipboard.
    pub fn copy(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::Copy(pane));
    }

    pub fn paste(&self, pane: PaneId, text: String) {
        let _ = self.tx.send(Cmd::Paste(pane, text));
    }

    pub fn resize(&self, pane: PaneId, cols: u16, rows: u16) {
        let _ = self.tx.send(Cmd::Resize(pane, cols, rows));
    }

    pub fn act(&self, act: Act) {
        let _ = self.tx.send(Cmd::Act(act));
    }

    /// Draws with these colours from the next change on, and asks for one.
    pub fn set_colors(&self, colors: Colors) {
        *self.colors.lock().unwrap() = colors;
        let _ = self.tx.send(Cmd::Redraw);
    }

    /// Tells the thread to end the sessions (a windowed kyko) and stop.
    pub fn request_end(&self) {
        let _ = self.tx.send(Cmd::End);
    }

    /// Waits for the thread to finish, after [`Workspace::request_end`].
    pub fn wait(&self) {
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

/// Why [`run`] stopped.
enum Stop {
    /// The window is closing, or has nothing left to show.
    Window,
    Link(LinkError),
}

impl From<LinkError> for Stop {
    fn from(e: LinkError) -> Stop {
        Stop::Link(e)
    }
}

fn thread_main(ctx: Ctx, rx: Receiver<Cmd>) {
    let mut sizes: HashMap<PaneId, (u16, u16)> = HashMap::new();
    let mut session: Option<Session> = None;
    // Only a lost link brings another try; anything else ends the thread.
    while let Err(Stop::Link(e)) = run(&ctx, &rx, &mut sizes, &mut session) {
        ctx.shared.lock().unwrap().notice = Some(format!("Sessions aren't available: {e}"));
        (ctx.on_layout)();
        // The shared holder may come back (it restarts on failure): try again.
        if !ctx.launch.persistent || wait_or_end(&rx, &mut sizes, Duration::from_secs(2)) {
            break;
        }
    }
}

/// Waits `d` for commands while there is no link, keeping the latest sizes; true when the window
/// asked to end.
fn wait_or_end(rx: &Receiver<Cmd>, sizes: &mut HashMap<PaneId, (u16, u16)>, d: Duration) -> bool {
    let end = Instant::now() + d;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(Cmd::End) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return true,
            Ok(Cmd::Resize(p, c, r)) => {
                sizes.insert(p, (c, r));
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    false
}

fn qt_key(key: u32, text: &str) -> Option<Key> {
    Some(match key {
        0x0100_0012 => Key::Left,
        0x0100_0013 => Key::Up,
        0x0100_0014 => Key::Right,
        0x0100_0015 => Key::Down,
        0x0100_0016 => Key::PgUp,
        0x0100_0017 => Key::PgDn,
        0x0100_0010 => Key::Home,
        0x0100_0011 => Key::End,
        0x0100_0006 => Key::Insert,
        0x0100_0007 => Key::Delete,
        0x0100_0001 => Key::Tab,
        0x0100_0004 | 0x0100_0005 => Key::Enter,
        0x0100_0000 => Key::Esc,
        0x0100_0003 => Key::Backspace,
        0x20 => Key::Space,
        0x0100_0030..=0x0100_0047 => Key::F((key - 0x0100_0030 + 1) as u8),
        0x21..=0x7e => Key::Char(char::from_u32(key)?),
        _ => Key::Char(text.chars().next()?),
    })
}

/// The action a key press is bound to, when it is one. `mods` as `kyko_term::keys` has them:
/// Shift 1, Alt 2, Ctrl 4.
pub fn action_for(look: &Look, key: u32, mods: u8, text: &str) -> Option<Action> {
    let key = qt_key(key, text)?;
    let mods = Mods {
        shift: mods & 1 != 0,
        alt: mods & 2 != 0,
        ctrl: mods & 4 != 0,
        super_: false,
    };
    look.keys.action_for(mods, key)
}

/// What a close is waiting for the user's word on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Pane(PaneId),
    Tab(u32),
}

/// The programs running in what `what` would close, other than the shell.
fn busy_programs(s: &Session, what: Pending) -> Vec<String> {
    let mut out = Vec::new();
    for tab in s.tabs() {
        for pane in &tab.panes {
            let hit = match what {
                Pending::Pane(p) => pane.id == p,
                Pending::Tab(t) => tab.id == t,
            };
            if hit && pane.busy && !out.iter().any(|n| n == pane.program) {
                out.push(pane.program.to_string());
            }
        }
    }
    out
}

/// Bytes for a pasted text: bracketed when the program asked, with the escape characters taken
/// out so the text cannot end the bracket itself; otherwise lines end in a carriage return.
fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let clean: String = text.chars().filter(|c| *c != '\x1b').collect();
    if bracketed {
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        clean.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// A mouse event on a pane: sent to the program when it asked for mouse reports, else it selects
/// text or scrolls. True when the pane's picture changed.
fn mouse(
    link: &mut Link,
    pane: PaneId,
    ev: MouseEv,
    copy_on_select: bool,
    on_copy: &dyn Fn(String),
) -> Result<bool, LinkError> {
    let Some(p) = link.pane(pane) else {
        return Ok(false);
    };
    let modes = p.emulator().modes();
    let shift = ev.mods & 1 != 0;
    let tracking = modes.mouse_click || modes.mouse_drag || modes.mouse_motion;
    if tracking && !shift {
        let button = match ev.kind {
            MouseKind::Wheel => 64 + ev.button,
            _ => ev.button.min(3),
        };
        let motion = matches!(ev.kind, MouseKind::Move);
        if motion && !(modes.mouse_motion || (modes.mouse_drag && ev.button < 3)) {
            return Ok(false);
        }
        let mut cb = u32::from(button)
            + if ev.mods & 2 != 0 { 8 } else { 0 }
            + if ev.mods & 4 != 0 { 16 } else { 0 }
            + if motion { 32 } else { 0 };
        let (x, y) = (u32::from(ev.col) + 1, u32::from(ev.row) + 1);
        let release = matches!(ev.kind, MouseKind::Release);
        let bytes = if modes.sgr_mouse {
            format!("\x1b[<{cb};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes()
        } else {
            if release {
                cb = (cb & !3) | 3;
            }
            let mut out = b"\x1b[M".to_vec();
            for v in [cb + 32, x + 32, y + 32] {
                if modes.utf8_mouse {
                    out.extend(char::from_u32(v).unwrap_or(' ').to_string().into_bytes());
                } else {
                    out.push(u8::try_from(v).unwrap_or(255));
                }
            }
            out
        };
        link.input(pane, &bytes)?;
        return Ok(false);
    }
    let Some(p) = link.pane_mut(pane) else {
        return Ok(false);
    };
    match ev.kind {
        MouseKind::Wheel => {
            let lines = if ev.button == 0 { 3 } else { -3 };
            if modes.alt_screen && modes.alternate_scroll {
                // A full-screen program without mouse reports: the wheel is the arrow keys.
                let key = match (ev.button == 0, modes.app_cursor) {
                    (true, true) => "\x1bOA",
                    (true, false) => "\x1b[A",
                    (false, true) => "\x1bOB",
                    (false, false) => "\x1b[B",
                };
                link.input(pane, key.repeat(3).as_bytes())?;
                return Ok(false);
            }
            p.emulator_mut().scroll(lines);
            Ok(true)
        }
        MouseKind::Press if ev.button == 0 => {
            let kind = match ev.clicks {
                2 => SelectKind::Word,
                3.. => SelectKind::Line,
                _ => SelectKind::Simple,
            };
            p.emulator_mut()
                .select_start(ev.col, ev.row, kind, ev.right);
            Ok(true)
        }
        MouseKind::Move if ev.button == 0 => {
            p.emulator_mut().select_update(ev.col, ev.row, ev.right);
            Ok(true)
        }
        MouseKind::Release if ev.button == 0 => {
            if copy_on_select && let Some(t) = p.emulator().selection_text() {
                on_copy(t);
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle(act: Act, s: &mut Session, look: &Look, login: &str) -> Vec<Effect> {
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
        Act::Confirm(_) => Vec::new(),
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

fn focus(s: &mut Session, dir: Dir) -> Vec<Effect> {
    s.focus_dir(dir);
    Vec::new()
}

/// Carries out what the session asked for. True when it asked to hide the window.
fn apply(
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
                    (
                        ctx.launch.login_shell.clone(),
                        args,
                        cwd.unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into())),
                    )
                };
                link.spawn(SpawnSpec {
                    pane,
                    shell,
                    args,
                    cwd,
                    env: vec![
                        ("TERM".into(), "xterm-256color".into()),
                        ("COLORTERM".into(), "truecolor".into()),
                    ],
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

fn publish(link: &Link, pane: PaneId, colors: &Mutex<Colors>, shared: &Mutex<Shared>) {
    if let Some(p) = link.pane(pane) {
        let modes = p.emulator().modes();
        let snapshot = p.emulator().snapshot(&colors.lock().unwrap());
        let frame = Frame {
            snapshot: Some(snapshot),
            app_cursor: modes.app_cursor,
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
                           "focused": p.focused, "busy": p.busy, "state": state,
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
            json!({"id": t.id, "name": t.name, "active": t.active, "panes": panes,
                   "dividers": dividers})
        })
        .collect();
    let mut out = String::new();
    let _ = write!(
        out,
        "{}",
        json!({"active": s.active_tab(), "focused": s.focused_pane(), "title": title,
               "notice": notice.unwrap_or(""), "tabs": tabs,
               "find": {"open": find.open, "pos": find.pos + 1, "total": find.total},
               "confirm": confirm.map(|(what, programs)| json!({
                   "what": match what { Pending::Pane(_) => "pane", Pending::Tab(_) => "tab" },
                   "programs": programs.join(", ")}))})
    );
    out
}

fn run(
    ctx: &Ctx,
    rx: &Receiver<Cmd>,
    sizes: &mut HashMap<PaneId, (u16, u16)>,
    slot: &mut Option<Session>,
) -> Result<(), Stop> {
    let look = || ctx.look.lock().unwrap().clone();
    let mut link = Link::connect(&ctx.socket, look().scrollback)?;
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
    let (fresh, effects, _warnings) = Session::reconcile(base, &infos, &home, &ctx.launch.shell);
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
    ctx.shared.lock().unwrap().notice = None;
    let mut last_status = 0;
    let mut pending: Option<(Pending, Vec<String>)> = None;
    let mut find = Find::default();
    let mut dirty: HashSet<PaneId> = session.pane_ids().into_iter().collect();
    let mut layout_changed = true;
    'main: loop {
        let mut effects = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Input(p, bytes) => {
                    // Typing goes to the live screen and takes the selection away.
                    if let Some(pn) = link.pane_mut(p) {
                        let e = pn.emulator_mut();
                        if e.display_offset() > 0 || e.selection_text().is_some() {
                            e.scroll_to_bottom();
                            e.select_clear();
                            dirty.insert(p);
                        }
                    }
                    link.input(p, &bytes)?;
                }
                Cmd::Mouse(p, ev) => {
                    let l = look();
                    if mouse(&mut link, p, ev, l.copy_on_select, &*ctx.on_copy)? {
                        dirty.insert(p);
                    }
                }
                Cmd::Copy(p) => {
                    if let Some(t) = link.pane(p).and_then(|pn| pn.emulator().selection_text()) {
                        (ctx.on_copy)(t);
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
                                find = Find::default();
                            }
                        }
                        dirty.insert(pane);
                    }
                    layout_changed = true;
                }
                Cmd::Paste(p, text) => {
                    let bracketed = link
                        .pane(p)
                        .is_some_and(|pn| pn.emulator().modes().bracketed_paste);
                    link.input(p, &paste_bytes(&text, bracketed))?;
                }
                Cmd::Resize(p, c, r) => {
                    sizes.insert(p, (c, r));
                    if link.pane(p).is_some() {
                        link.resize(p, c, r)?;
                        dirty.insert(p);
                    }
                }
                Cmd::Redraw => dirty.extend(session.pane_ids()),
                Cmd::Act(act) => {
                    layout_changed = true;
                    if matches!(act, Act::Key(Action::Search)) {
                        // The key opens the find bar; the bar's own Escape closes it.
                        find.open = true;
                        continue;
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
                            pending = Some((t, programs));
                            continue;
                        }
                    }
                    if let Act::Confirm(yes) = act {
                        if let (Some((t, _)), true) = (pending.take(), yes) {
                            let act = match t {
                                Pending::Pane(p) => Act::ClosePane(p),
                                Pending::Tab(t) => Act::CloseTab(t),
                            };
                            effects.extend(handle(act, session, &look(), &login));
                        }
                        continue;
                    }
                    effects.extend(handle(act, session, &look(), &login));
                }
                Cmd::End => {
                    if !ctx.launch.persistent {
                        // The window is closing and its shells go with it.
                        let _ = link.end_sessions(true);
                    }
                    break 'main;
                }
            }
        }
        let events = link.pump(Duration::from_millis(4))?;
        for event in events {
            match event {
                Event::Changed(p) => {
                    dirty.insert(p);
                }
                Event::Info { pane, program, cwd } => {
                    layout_changed |= session.apply_info(pane, &program, &cwd);
                    dirty.insert(pane);
                }
                Event::Exited { pane, status } => {
                    last_status = status;
                    effects.extend(session.pane_exited(pane, status, &login));
                    layout_changed = true;
                    dirty.insert(pane);
                }
                Event::SpawnFailed { pane, message } => {
                    session.spawn_failed(pane, &message);
                    layout_changed = true;
                }
                Event::Busy(_) => {}
            }
        }
        if !effects.is_empty() {
            let hide = apply(effects, &mut link, ctx, sizes, &mut use_launch)?;
            layout_changed = true;
            if hide {
                if ctx.launch.persistent {
                    (ctx.on_hide)();
                } else if !quit {
                    quit = true;
                    ctx.shared.lock().unwrap().exit = Some(last_status);
                    (ctx.on_quit)();
                }
            }
        }
        if layout_changed {
            layout_changed = false;
            link.save_layout(session.to_blob())?;
            let ids: HashSet<PaneId> = session.pane_ids().into_iter().collect();
            let json = {
                let mut sh = ctx.shared.lock().unwrap();
                sh.frames.retain(|id, _| ids.contains(id));
                sh.focused = session.focused_pane();
                layout_json(session, sh.notice.as_deref(), pending.as_ref(), find)
            };
            ctx.shared.lock().unwrap().layout = json;
            dirty.extend(ids);
            (ctx.on_layout)();
        }
        if !dirty.is_empty() {
            for p in dirty.drain() {
                publish(&link, p, &ctx.colors, &ctx.shared);
            }
            (ctx.on_frames)();
        }
    }
    Err(Stop::Window)
}

/// Finds `kykod`: next to this executable, else on `PATH`.
fn kykod_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("kykod")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| "kykod".into())
}

/// A `kykod` serving `socket` for this window alone: started here, ended with the window. The
/// drop-down will use the shared one the systemd unit starts; a window opened with `kyko -e`
/// must not take that one's single connection.
pub fn start_private_holder(socket: &Path) -> std::io::Result<Child> {
    let mut child = Command::new(kykod_path())
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Ok(child);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "kykod did not start",
    ))
}

/// Waits briefly for the private `kykod` to end after `EndSessions`, so it is not left a zombie;
/// kills it if it does not.
pub fn reap(mut holder: Child) {
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        if matches!(holder.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = holder.kill();
    let _ = holder.wait();
}

/// Makes sure a `kykod` serves `socket`, starting one of its own when none does: the drop-down's
/// shared holder. With the systemd unit installed `kykod` is already there and this does nothing.
pub fn ensure_shared_holder(socket: &Path) -> std::io::Result<()> {
    if std::os::unix::net::UnixStream::connect(socket).is_ok() {
        return Ok(());
    }
    use std::os::unix::process::CommandExt;
    // Its own process group: the holder must outlive this window and its terminal.
    let _child = Command::new(kykod_path())
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "kykod did not start",
    ))
}
