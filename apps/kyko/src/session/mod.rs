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

mod apply;
mod commands;
mod handle;
mod holder;
mod mouse;
mod notify;
mod remote;
mod run;
mod workspace;

use apply::*;
use handle::*;
pub use holder::{ensure_shared_holder, reap, start_private_holder};
use mouse::*;
use notify::*;
use remote::*;
use run::*;

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

/// Whether a paste has to be confirmed first: it has more than one line, the program did not
/// ask for bracketed paste (so a shell would run each line as it arrives), and the setting is on.
fn needs_paste_confirm(text: &str, bracketed: bool, setting: bool) -> bool {
    setting && !bracketed && text.trim_end_matches(['\n', '\r']).contains(['\n', '\r'])
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
