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

use kyko_core::config::{Action, NotifyOnFinish};
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
    /// The kitty keyboard protocol flags the program turned on (0: legacy keys).
    pub kitty: u8,
    /// The program asked to hear focus in and out (`CSI ? 1004 h`).
    pub focus_reporting: bool,
    /// The program takes mouse clicks itself: a link then opens only with Ctrl+Shift.
    pub mouse_tracking: bool,
    /// Lines the view is above the live screen, and lines of history there are.
    pub scroll: (u32, u32),
    /// The command to pin at the top while the view is inside its output.
    pub header: Option<String>,
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
    /// Link labels over a pane, while `open_link` waits for a label to be typed.
    pub hints: Option<(PaneId, kyko_term::hints::Hints)>,
    /// The pane that has the keyboard in the active window, if any.
    pub looking_at: Option<PaneId>,
    /// The size of a cell in pixels, as the window last measured it (images are sized by it).
    pub cell_px: Option<(u16, u16)>,
    /// Each pane's folder, as kykod last reported it (relative paths on screen are opened from it).
    pub cwds: HashMap<PaneId, String>,
    /// Command lines the shells ran, newest first, each once (the command palette lists them).
    pub recent: Vec<String>,
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
    Typed(PaneId, Vec<u8>),
    Mouse(PaneId, MouseEv),
    Copy(PaneId),
    /// Scroll to the previous (-1) or next (1) prompt.
    PromptJump(PaneId, i32),
    /// Scroll to the previous (< 0) or next failed command.
    FailedJump(PaneId, i32),
    CopyLastCommand(PaneId),
    /// Select the last command's output and copy it.
    CopyLastOutput(PaneId),
    ShowScrollback(PaneId),
    /// A `kyko @` request, and where its answer goes.
    Remote(kyko_core::remote::Request, Sender<kyko_core::remote::Reply>),
    /// Put the view this many lines above the live screen (the scrollbar).
    ScrollTo(PaneId, u32),
    Paste(PaneId, String),
    Search(SearchOp),
    Resize(PaneId, u16, u16),
    /// A pane's cell size in pixels.
    CellPx(u16, u16),
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
    /// The Unicode input box closed: with the text to type, or without (Escape).
    Unicode(Option<String>),
    /// The command palette closed.
    PaletteClosed,
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
    /// `--session FILE`: `kyko @` commands to run once the window is up.
    pub startup: Vec<kyko_core::remote::Request>,
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
    on_copy: Box<dyn Fn(String, bool) + Send>,
    on_bell: Box<dyn Fn(PaneId) + Send>,
    /// The control socket for `kyko @`, named to the panes in `$KYKO_LISTEN_ON`.
    rc_path: Option<PathBuf>,
    /// The session file's commands, until they have run.
    startup: Mutex<Vec<kyko_core::remote::Request>>,
}

impl Workspace {
    /// Starts the thread. The callbacks run on it: `on_frames` when a pane's screen changed,
    /// `on_layout` when the tabs did, `on_hide` when the drop-down should hide (its last tab
    /// closed), `on_quit` when a windowed kyko has nothing left to show, `on_bell` when a
    /// pane's program rang the bell.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        socket: PathBuf,
        mut launch: Launch,
        look: SharedLook,
        on_frames: Box<dyn Fn() + Send>,
        on_layout: Box<dyn Fn() + Send>,
        on_hide: Box<dyn Fn() + Send>,
        on_quit: Box<dyn Fn() + Send>,
        on_copy: Box<dyn Fn(String, bool) + Send>,
        on_bell: Box<dyn Fn(PaneId) + Send>,
    ) -> Workspace {
        let (tx, rx) = channel();
        let rc_tx = tx.clone();
        let rc_path = crate::remote::serve(&crate::remote::socket_path(), move |req| {
            let (reply_tx, reply_rx) = channel();
            if rc_tx.send(Cmd::Remote(req, reply_tx)).is_err() {
                return kyko_core::remote::Reply::err("the window is closing");
            }
            reply_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| kyko_core::remote::Reply::err("the window did not answer"))
        });
        let shared = Arc::new(Mutex::new(Shared::default()));
        let colors: SharedColors = Arc::new(Mutex::new(look.lock().unwrap().colors.clone()));
        let startup = Mutex::new(std::mem::take(&mut launch.startup));
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
            on_bell,
            rc_path,
            startup,
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

    /// Keys typed in `pane`: they reach every pane of its tab while the tab broadcasts.
    pub fn typed(&self, pane: PaneId, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Typed(pane, bytes));
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

    /// Scrolls the pane to the previous (`dir < 0`) or next prompt.
    pub fn jump_prompt(&self, pane: PaneId, dir: i32) {
        let _ = self.tx.send(Cmd::PromptJump(pane, dir));
    }

    /// Selects what the last command printed and puts it on the clipboard.
    pub fn scroll_to(&self, pane: PaneId, offset: u32) {
        let _ = self.tx.send(Cmd::ScrollTo(pane, offset));
    }

    pub fn show_scrollback(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::ShowScrollback(pane));
    }

    pub fn copy_last_output(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::CopyLastOutput(pane));
    }

    /// Scrolls the pane to the previous (`dir < 0`) or next command that failed.
    pub fn jump_failed(&self, pane: PaneId, dir: i32) {
        let _ = self.tx.send(Cmd::FailedJump(pane, dir));
    }

    pub fn copy_last_command(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::CopyLastCommand(pane));
    }

    pub fn paste(&self, pane: PaneId, text: String) {
        let _ = self.tx.send(Cmd::Paste(pane, text));
    }

    pub fn resize(&self, pane: PaneId, cols: u16, rows: u16) {
        let _ = self.tx.send(Cmd::Resize(pane, cols, rows));
    }

    /// The cell size in pixels. One font serves the window, so it holds for every pane.
    pub fn cell_px(&self, w: u16, h: u16) {
        self.shared.lock().unwrap().cell_px = Some((w, h));
        let _ = self.tx.send(Cmd::CellPx(w, h));
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
    kyko_core::keys::key_from_qt(key, text)
}

/// The action a key press is bound to, when it is one. `mods` as `kyko_term::keys` has them:
/// Shift 1, Alt 2, Ctrl 4, Super 8.
pub fn action_for(look: &Look, key: u32, mods: u8, text: &str) -> Option<Action> {
    let key = qt_key(key, text)?;
    let mods = Mods {
        shift: mods & 1 != 0,
        alt: mods & 2 != 0,
        ctrl: mods & 4 != 0,
        super_: mods & 8 != 0,
    };
    look.keys.action_for(mods, key)
}

/// What a close is waiting for the user's word on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Pane(PaneId),
    Tab(u32),
    /// A paste of several lines into a program that did not turn on bracketed paste, where
    /// each line would run as it arrives.
    Paste(PaneId),
}

/// The programs running in what `what` would close, other than the shell.
fn busy_programs(s: &Session, what: Pending) -> Vec<String> {
    let mut out = Vec::new();
    for tab in s.tabs() {
        for pane in &tab.panes {
            let hit = match what {
                Pending::Pane(p) => pane.id == p,
                Pending::Tab(t) => tab.id == t,
                Pending::Paste(_) => false,
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

/// Writes a pane's scrollback to a private file for the pager to read, in kyko's runtime folder.
fn scrollback_file(text: &str) -> std::io::Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("kyko");
    std::fs::create_dir_all(&dir)?;
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = dir.join(format!("scrollback-{}-{n}.txt", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    f.write_all(text.as_bytes())?;
    Ok(path)
}

/// Shows a notification a program asked for (OSC 9, 777, 99) on the desktop's notification
/// service, off the session thread so a slow service never holds the terminal up.
/// Whether a command that ran for `ran` earns a notification, the pane being `seen` or not.
fn finish_worth_telling(when: NotifyOnFinish, after: u32, ran: Duration, seen: bool) -> bool {
    ran.as_secs() >= u64::from(after)
        && match when {
            NotifyOnFinish::Never => false,
            NotifyOnFinish::Unfocused => !seen,
            NotifyOnFinish::Always => true,
        }
}

/// "Finished after 1m 05s", or "Failed (exit 2) after 12s".
fn finished_text(ran: Duration, status: u8) -> String {
    let s = ran.as_secs();
    let took = match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        _ => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    };
    if status == 0 {
        format!("Finished after {took}")
    } else {
        format!("Failed (exit {status}) after {took}")
    }
}

/// How many recent command lines the command palette keeps.
const RECENT: usize = 50;

/// At most this many program notifications in [`NOTIFY_WINDOW`]; a program that prints them in
/// a loop gets the first few shown and the rest dropped, instead of a thread and a bus
/// connection each.
const NOTIFY_BURST: usize = 5;
const NOTIFY_WINDOW: Duration = Duration::from_secs(10);

/// Whether one more notification may go out now, given when the recent ones went (kept here).
fn notify_allowed(sent: &mut std::collections::VecDeque<Instant>, now: Instant) -> bool {
    while sent
        .front()
        .is_some_and(|t| now.duration_since(*t) >= NOTIFY_WINDOW)
    {
        sent.pop_front();
    }
    if sent.len() >= NOTIFY_BURST {
        return false;
    }
    sent.push_back(now);
    true
}

fn notify(title: String, body: String) {
    static SENT: std::sync::Mutex<std::collections::VecDeque<Instant>> =
        std::sync::Mutex::new(std::collections::VecDeque::new());
    if !notify_allowed(&mut SENT.lock().unwrap(), Instant::now()) {
        return;
    }
    std::thread::spawn(move || {
        let summary = if title.is_empty() {
            "kyko".to_string()
        } else {
            title
        };
        let result = zbus::block_on(async {
            let bus = zbus::Connection::session().await?;
            let hints: std::collections::HashMap<&str, zbus::Variant<'_>> =
                [("desktop-entry", zbus::Variant::from("org.kyko.Kyko"))].into();
            bus.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    "kyko",
                    0u32,
                    "org.kyko.Kyko",
                    summary.as_str(),
                    body.as_str(),
                    Vec::<&str>::new(),
                    hints,
                    -1i32,
                ),
            )
            .await
            .map(|_| ())
        });
        if let Err(e) = result {
            kyko_diag::problem(
                "kyko",
                kyko_diag::Level::WARN,
                &format!("[KYKO-007] notification not shown: {e}"),
            );
        }
    });
}

/// Whether a paste has to be confirmed first: it has more than one line, the program did not
/// ask for bracketed paste (so a shell would run each line as it arrives), and the setting is on.
fn needs_paste_confirm(text: &str, bracketed: bool, setting: bool) -> bool {
    setting && !bracketed && text.trim_end_matches(['\n', '\r']).contains(['\n', '\r'])
}

/// A mouse event on a pane: sent to the program when it asked for mouse reports, else it selects
/// text or scrolls. True when the pane's picture changed.
fn mouse(
    link: &mut Link,
    pane: PaneId,
    ev: MouseEv,
    copy_on_select: bool,
    on_copy: &dyn Fn(String, bool),
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
                // Ctrl+Alt+drag selects a rectangle, as in kitty.
                _ if ev.mods & 6 == 6 => SelectKind::Block,
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
            // What was selected becomes the primary selection (middle-click pastes it), and
            // the clipboard as well when the setting asks.
            if let Some(t) = p.emulator().selection_text() {
                if copy_on_select {
                    on_copy(t.clone(), false);
                }
                on_copy(t, true);
                return Ok(false);
            }
            // A plain click (no drag) on the command line being typed moves the shell's cursor
            // there, as kitty and fish do.
            if ev.clicks == 1
                && ev.mods == 0
                && let Some(bytes) = p.emulator().click_to_cursor(ev.col, ev.row)
            {
                if !bytes.is_empty() {
                    link.input(pane, &bytes)?;
                }
                return Ok(false);
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

/// The panes a `kyko @` command names; `Here` is the focused pane (the client turns it into
/// its own pane when it runs inside kyko).
fn remote_targets(s: &Session, m: &kyko_core::remote::Match) -> Result<Vec<PaneId>, String> {
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
fn remote_request(
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

fn focus(s: &mut Session, dir: Dir) -> Vec<Effect> {
    s.focus_dir(dir);
    Vec::new()
}

/// Where the shell integration scripts live, written on first use; `None` (logged once) when
/// they cannot be written, and the shell then starts without them.
fn integration_dir() -> Option<PathBuf> {
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

fn publish(link: &Link, pane: PaneId, colors: &Mutex<Colors>, shared: &Mutex<Shared>) {
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

fn run(
    ctx: &Ctx,
    rx: &Receiver<Cmd>,
    sizes: &mut HashMap<PaneId, (u16, u16)>,
    slot: &mut Option<Session>,
) -> Result<(), Stop> {
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
    let mut last_status = 0;
    let mut pending: Option<(Pending, Vec<String>)> = None;
    // The text a paste waiting for the user's word would send.
    let mut held_paste = String::new();
    let mut find = Find::default();
    let mut unicode = false;
    let mut palette = false;
    let mut dirty: HashSet<PaneId> = session.pane_ids().into_iter().collect();
    let mut layout_changed = true;
    'main: loop {
        let mut effects = Vec::new();
        link.set_theme(&ctx.colors.lock().unwrap());
        while let Ok(cmd) = rx.try_recv() {
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
                    if mouse(&mut link, p, ev, l.copy_on_select, &*ctx.on_copy)? {
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
                        continue;
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
                            effects.extend(session.new_tab(req, &login).unwrap_or_default());
                            layout_changed = true;
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
                    if needs_paste_confirm(&text, bracketed, look().confirm_multiline_paste) {
                        let lines = text.trim_end_matches('\n').lines().count();
                        pending = Some((Pending::Paste(p), vec![lines.to_string()]));
                        held_paste = text;
                        layout_changed = true;
                        continue;
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
                    layout_changed = true;
                    if matches!(act, Act::Key(Action::Search)) {
                        // The key opens the find bar; the bar's own Escape closes it.
                        find.open = true;
                        continue;
                    }
                    if matches!(act, Act::Key(Action::UnicodeInput)) {
                        unicode = true;
                        continue;
                    }
                    if matches!(act, Act::Key(Action::CommandPalette)) {
                        palette = true;
                        continue;
                    }
                    if matches!(act, Act::PaletteClosed) {
                        palette = false;
                        continue;
                    }
                    if let Act::Unicode(text) = act {
                        unicode = false;
                        if let Some(t) = text {
                            for p in session.typing_targets(session.focused_pane()) {
                                link.input(p, t.as_bytes())?;
                            }
                        }
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
                                Pending::Paste(p) => {
                                    link.input(p, &paste_bytes(&held_paste, false))?;
                                    held_paste.clear();
                                    continue;
                                }
                            };
                            effects.extend(handle(act, session, &look(), &login));
                        }
                        held_paste.clear();
                        continue;
                    }
                    effects.extend(handle(act, session, &look(), &login));
                }
                Cmd::Remote(req, reply) => {
                    layout_changed = true;
                    let (answer, more) = remote_request(req, session, &mut link, &login)?;
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
                    break 'main;
                }
            }
        }
        let events = link.pump(Duration::from_millis(4))?;
        for event in events {
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
                                if finish_worth_telling(
                                    l.notify_on_finish,
                                    l.notify_after,
                                    f.ran,
                                    seen,
                                ) {
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
                layout_json(
                    session,
                    sh.notice.as_deref(),
                    pending.as_ref(),
                    find,
                    unicode,
                    palette,
                )
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

#[cfg(test)]
mod paste_tests {
    use super::needs_paste_confirm;

    #[test]
    fn only_unbracketed_multi_line_pastes_ask() {
        assert!(needs_paste_confirm("rm -rf x\nls\n", false, true));
        assert!(
            !needs_paste_confirm("ls\n", false, true),
            "one line with its newline"
        );
        assert!(
            !needs_paste_confirm("a\nb", true, true),
            "bracketed paste is safe"
        );
        assert!(!needs_paste_confirm("a\nb", false, false), "setting off");
        assert!(needs_paste_confirm("a\r\nb", false, true));
    }
}

#[cfg(test)]
mod notify_tests {
    use super::{Duration, Instant, NOTIFY_BURST, NOTIFY_WINDOW, notify_allowed};

    #[test]
    fn a_flood_of_notifications_is_cut_to_a_few() {
        let mut sent = std::collections::VecDeque::new();
        let t0 = Instant::now();
        let shown = (0..1000)
            .filter(|i| notify_allowed(&mut sent, t0 + Duration::from_micros(*i)))
            .count();
        assert_eq!(shown, NOTIFY_BURST);
        // Once the window has passed, they are shown again.
        assert!(notify_allowed(
            &mut sent,
            t0 + NOTIFY_WINDOW + Duration::from_secs(1)
        ));
    }
}

#[cfg(test)]
mod finish_tests {
    use super::{Duration, NotifyOnFinish, finish_worth_telling, finished_text};

    #[test]
    fn long_commands_notify_when_asked() {
        let long = Duration::from_secs(12);
        let short = Duration::from_secs(3);
        assert!(!finish_worth_telling(
            NotifyOnFinish::Never,
            10,
            long,
            false
        ));
        assert!(finish_worth_telling(
            NotifyOnFinish::Unfocused,
            10,
            long,
            false
        ));
        assert!(!finish_worth_telling(
            NotifyOnFinish::Unfocused,
            10,
            long,
            true
        ));
        assert!(!finish_worth_telling(
            NotifyOnFinish::Always,
            10,
            short,
            false
        ));
        assert!(finish_worth_telling(NotifyOnFinish::Always, 10, long, true));
    }

    #[test]
    fn the_text_says_how_it_ended_and_how_long_it_took() {
        assert_eq!(
            finished_text(Duration::from_secs(12), 0),
            "Finished after 12s"
        );
        assert_eq!(
            finished_text(Duration::from_secs(65), 2),
            "Failed (exit 2) after 1m 05s"
        );
        assert_eq!(
            finished_text(Duration::from_secs(7322), 0),
            "Finished after 2h 02m"
        );
    }
}
