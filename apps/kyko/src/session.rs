//! One pane's life, with no Qt: a thread owns the link to `kykod`, takes input and resizes
//! from the window over a channel, and publishes the screen as a [`Frame`] the window draws.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kyko_link::{Event, Link, LinkError};
use kyko_proto::SpawnSpec;
use kyko_term::{Colors, Snapshot};

/// The colours the next snapshot is resolved with; the window swaps them on a theme change.
pub type SharedColors = Arc<Mutex<Colors>>;

/// What the window draws.
#[derive(Clone)]
pub struct Frame {
    pub snapshot: Option<Snapshot>,
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    /// The program in the foreground, for the window title.
    pub program: String,
    pub exited: Option<i32>,
    /// Set when `kykod` cannot be reached or went away; the window says so.
    pub notice: Option<String>,
}

impl Frame {
    fn new() -> Frame {
        Frame {
            snapshot: None,
            app_cursor: false,
            bracketed_paste: false,
            program: String::new(),
            exited: None,
            notice: None,
        }
    }
}

enum Cmd {
    Input(Vec<u8>),
    Resize(u16, u16),
    Redraw,
    End,
}

pub struct Session {
    tx: Sender<Cmd>,
    colors: SharedColors,
    pub frame: Arc<Mutex<Frame>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// What to run in the pane.
pub struct Launch {
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// The holder outlives this window (the drop-down's): the pane is attached again when it is
    /// still there, a lost connection is retried, and closing the window ends nothing.
    pub persistent: bool,
}

impl Session {
    /// Starts the pane's thread. `on_change` is called from it whenever the frame changed.
    pub fn start(
        socket: PathBuf,
        launch: Launch,
        cols: u16,
        rows: u16,
        colors: Colors,
        on_change: Box<dyn Fn() + Send>,
    ) -> Session {
        let (tx, rx) = channel();
        let frame = Arc::new(Mutex::new(Frame::new()));
        let shared = frame.clone();
        let colors = Arc::new(Mutex::new(colors));
        let thread_colors = colors.clone();
        let thread = std::thread::Builder::new()
            .name("kyko-link".into())
            .spawn(move || {
                let persistent = launch.persistent;
                let mut size = (cols, rows);
                loop {
                    let outcome = run(
                        &socket,
                        &launch,
                        &mut size,
                        &thread_colors,
                        &rx,
                        &shared,
                        &*on_change,
                    );
                    match outcome {
                        Err(Stop::Link(e)) => {
                            shared.lock().unwrap().notice =
                                Some(format!("Sessions aren't available: {e}"));
                            on_change();
                            // The shared holder may come back (it restarts on failure): try again.
                            if !persistent || wait_or_end(&rx, &mut size, Duration::from_secs(2)) {
                                break;
                            }
                        }
                        Err(Stop::Window) | Ok(()) => break,
                    }
                }
            })
            .expect("thread starts");
        Session {
            tx,
            colors,
            frame,
            thread: Mutex::new(Some(thread)),
        }
    }

    pub fn input(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Input(bytes));
    }

    /// Draws with these colours from the next change on, and asks for one.
    pub fn set_colors(&self, colors: Colors) {
        *self.colors.lock().unwrap() = colors;
        let _ = self.tx.send(Cmd::Redraw);
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.tx.send(Cmd::Resize(cols, rows));
    }

    /// Tells the thread to end the pane's shell and `kykod` with it.
    pub fn request_end(&self) {
        let _ = self.tx.send(Cmd::End);
    }

    /// Waits for the thread to finish, after [`Session::request_end`].
    pub fn wait(&self) {
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

/// Why [`run`] stopped.
enum Stop {
    /// The window is closing.
    Window,
    Link(LinkError),
}

impl From<LinkError> for Stop {
    fn from(e: LinkError) -> Stop {
        Stop::Link(e)
    }
}

/// Waits `d` for commands while there is no link, keeping the latest size; true when the window
/// asked to end.
fn wait_or_end(rx: &Receiver<Cmd>, size: &mut (u16, u16), d: Duration) -> bool {
    let end = Instant::now() + d;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(Cmd::End) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return true,
            Ok(Cmd::Resize(c, r)) => *size = (c, r),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    false
}

fn publish(link: &Link, pane: u32, colors: &Mutex<Colors>, frame: &Mutex<Frame>) {
    if let Some(p) = link.pane(pane) {
        let modes = p.emulator().modes();
        let snapshot = p.emulator().snapshot(&colors.lock().unwrap());
        let mut f = frame.lock().unwrap();
        f.snapshot = Some(snapshot);
        f.app_cursor = modes.app_cursor;
        f.bracketed_paste = modes.bracketed_paste;
        f.program.clone_from(&p.program);
        f.exited = p.exited;
        f.notice = None;
    }
}

fn run(
    socket: &Path,
    launch: &Launch,
    size: &mut (u16, u16),
    colors: &Mutex<Colors>,
    rx: &Receiver<Cmd>,
    frame: &Mutex<Frame>,
    on_change: &dyn Fn(),
) -> Result<(), Stop> {
    let mut link = Link::connect(socket, 10_000)?;
    // A shared holder may already hold a pane from an earlier window: attach to it.
    let existing = link
        .pane_ids()
        .into_iter()
        .find(|id| link.pane(*id).is_some_and(|p| p.exited.is_none()));
    let mut pane = existing.unwrap_or_else(|| link.pane_ids().last().map_or(1, |id| id + 1));
    if existing.is_none() {
        start_shell(&mut link, pane, launch, *size)?;
    }
    link.attach(pane)?;
    if existing.is_some() {
        link.resize(pane, size.0, size.1)?;
    }
    loop {
        let mut redraw = false;
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Input(bytes) => link.input(pane, &bytes)?,
                Cmd::Resize(c, r) => {
                    *size = (c, r);
                    link.resize(pane, c, r)?;
                }
                Cmd::Redraw => redraw = true,
                Cmd::End => {
                    if !launch.persistent {
                        // The window is closing and its shell goes with it.
                        let _ = link.end_sessions(true);
                    }
                    return Err(Stop::Window);
                }
            }
        }
        let events = link.pump(Duration::from_millis(4))?;
        let mut changed = redraw;
        let mut respawn = false;
        for event in events {
            match event {
                Event::Changed(_) | Event::Info { .. } => changed = true,
                // The drop-down always has a shell: a new one takes the place of the one that ended.
                Event::Exited { .. } if launch.persistent => respawn = true,
                Event::Exited { .. } => changed = true,
                Event::SpawnFailed { message, .. } => {
                    frame.lock().unwrap().notice =
                        Some(format!("Could not start the shell: {message}"));
                    changed = true;
                }
                Event::Busy(_) => {}
            }
        }
        if respawn {
            link.close(pane)?;
            pane = link.pane_ids().last().map_or(1, |id| id + 1).max(pane + 1);
            start_shell(&mut link, pane, launch, *size)?;
            link.attach(pane)?;
            changed = true;
        }
        if changed {
            publish(&link, pane, colors, frame);
            on_change();
        }
    }
}

fn start_shell(
    link: &mut Link,
    pane: u32,
    launch: &Launch,
    size: (u16, u16),
) -> Result<(), LinkError> {
    link.spawn(SpawnSpec {
        pane,
        shell: launch.shell.clone(),
        args: launch.args.clone(),
        cwd: launch.cwd.clone(),
        env: vec![
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
        ],
        cols: size.0,
        rows: size.1,
        ring_bytes: 0,
    })
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
