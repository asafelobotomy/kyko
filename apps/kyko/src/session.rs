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

pub const PANE: u32 = 1;

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
    End,
}

pub struct Session {
    tx: Sender<Cmd>,
    pub frame: Arc<Mutex<Frame>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// What to run in the pane.
pub struct Launch {
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: String,
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
        let thread = std::thread::Builder::new()
            .name("kyko-link".into())
            .spawn(move || {
                let outcome = run(
                    &socket,
                    launch,
                    cols,
                    rows,
                    &colors,
                    &rx,
                    &shared,
                    &*on_change,
                );
                if let Err(e) = outcome {
                    shared.lock().unwrap().notice = Some(format!("Sessions aren't available: {e}"));
                    on_change();
                }
            })
            .expect("thread starts");
        Session {
            tx,
            frame,
            thread: Mutex::new(Some(thread)),
        }
    }

    pub fn input(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Input(bytes));
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

#[allow(clippy::too_many_arguments)]
fn run(
    socket: &Path,
    launch: Launch,
    cols: u16,
    rows: u16,
    colors: &Colors,
    rx: &Receiver<Cmd>,
    frame: &Mutex<Frame>,
    on_change: &dyn Fn(),
) -> Result<(), LinkError> {
    let mut link = Link::connect(socket, 10_000)?;
    link.spawn(SpawnSpec {
        pane: PANE,
        shell: launch.shell,
        args: launch.args,
        cwd: launch.cwd,
        env: vec![
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
        ],
        cols,
        rows,
        ring_bytes: 0,
    })?;
    link.attach(PANE)?;
    loop {
        let mut end = false;
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Input(bytes) => link.input(PANE, &bytes)?,
                Cmd::Resize(c, r) => link.resize(PANE, c, r)?,
                Cmd::End => end = true,
            }
        }
        if end {
            // Forced: the window is closing, and its shell goes with it.
            let _ = link.end_sessions(true);
            return Ok(());
        }
        let events = link.pump(Duration::from_millis(4))?;
        let mut changed = false;
        for event in events {
            match event {
                Event::Changed(_) | Event::Info { .. } | Event::Exited { .. } => changed = true,
                Event::SpawnFailed { message, .. } => {
                    frame.lock().unwrap().notice =
                        Some(format!("Could not start the shell: {message}"));
                    changed = true;
                }
                Event::Busy(_) => {}
            }
        }
        if changed {
            if let Some(p) = link.pane(PANE) {
                let modes = p.emulator().modes();
                let mut f = frame.lock().unwrap();
                f.snapshot = Some(p.emulator().snapshot(colors));
                f.app_cursor = modes.app_cursor;
                f.bracketed_paste = modes.bracketed_paste;
                f.program.clone_from(&p.program);
                f.exited = p.exited;
            }
            on_change();
        }
    }
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
