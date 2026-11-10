//! The socket server: a private Unix socket, one window at a time, the pane
//! table and the stored layout.
//!
//! Threads: the accept loop (the caller of [`run`]), the reader thread of the
//! one live connection, and per pane a reader and an input thread (see
//! [`crate::pane`]). Every one of them blocks in `accept`, `read`, `poll` or on
//! a channel; nothing wakes on a timer while no shell is producing output.
//!
//! One window at a time: while a connection is live, any other is closed by
//! the accept loop before a byte of it is read. When the live connection ends,
//! for any reason, every pane goes back to keeping its output in its ring only.
//!
//! Locks, never nested the other way round:
//! - the pane table: taken by connection threads only, briefly, to look panes
//!   up, add or remove them; never held while waiting on a pane's lock (except
//!   when a pane is created, when nobody else can know it yet) and never held
//!   while writing to a socket;
//! - a pane's lock, which may be held while writing to the window;
//! - a window's writer, taken last, held only for one frame's write.
//!
//! Three more are taken alone or under the pane table, never under any other
//! lock and never two of them at once:
//! - the list of panes being closed;
//! - `SPAWN` in [`crate::pty`], held while a terminal is made and its shell
//!   forked, so that no other spawn forks while a descriptor is not yet
//!   close-on-exec;
//! - inside a shell's [`crate::pty::Pty`], the status lock, under which the
//!   child lock may be taken (status, then child).
//!
//! Nothing else is taken under any of these. The stored layout's lock and a
//! pane's input lock (its input queue) stand alone: never held with any other.
//!
//! No lock is held across a wait that is not bounded, nor across the waits of
//! up to 2 s when a shell is closed or the sessions end.
//!
//! Ending: `kykod` stops only through [`Server::end_sessions`], called by an
//! accepted `EndSessions` (on the connection's thread) or by `SIGTERM` or
//! `SIGINT` (on the signal thread, which blocks in `signal-hook`'s iterator).
//! Under the pane table it decides, in one pass, whether to end and which
//! panes there are, and marks the server as ending, so it runs at most once
//! and no pane can be spawned after the list is taken. With the table released
//! it removes the socket file, hangs every shell up at once, waits on their
//! pidfds (and those of panes still being closed) in one `poll` until all have
//! ended or [`END_GRACE`] has passed, kills the process group of any closed
//! pane whose shell is still running, reaps every shell that has ended, and
//! exits the process with status 0. Exiting from there ends every other thread
//! wherever it is blocked. The lock file is left in place: see [`end_sessions`].
//!
//! [`end_sessions`]: Server::end_sessions
//!
//! A window that stops reading cannot hold a shell up: every write has
//! [`WRITE_TIMEOUT`], after which the connection is dropped and every later
//! send to it fails at once. A replay on `Attach` shares one such deadline
//! across all its frames.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kyko_proto::{Decoder, Frame, PaneId, SpawnSpec, ToHolder, ToWindow, VERSION};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use nix::sys::stat::{Mode, umask};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::pane::Pane;

/// The most panes `kykod` holds at once, exited ones included.
pub const MAX_PANES: usize = 256;

/// How long one write to a window may block. A frame that cannot be written
/// within it (about twice this at worst, see [`write_within`]) drops the
/// connection.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

/// The largest layout blob `SaveLayout` may store; a larger one closes the
/// connection.
pub const MAX_LAYOUT: usize = 256 * 1024;

/// How long ending the sessions waits, in all, for the hung-up shells to end
/// before `kykod` exits anyway.
pub const END_GRACE: Duration = Duration::from_secs(2);

/// How long ending the sessions waits, after killing the shells of closed
/// panes at the end of [`END_GRACE`], for them to end so they can be reaped.
const KILL_REAP: Duration = Duration::from_millis(100);

/// Only the user `kykod` runs as may connect.
pub fn peer_allowed(peer_uid: u32, own_uid: u32) -> bool {
    peer_uid == own_uid
}

/// The writing side of one connection, shared by the connection's thread and
/// every pane that is live for it.
pub struct Window {
    id: u64,
    out: Mutex<UnixStream>,
    /// Set once the connection is over or a write failed; every send then
    /// fails at once, without touching the socket.
    dead: AtomicBool,
}

impl Window {
    /// This connection's number, unique for the life of the process.
    pub fn id(&self) -> u64 {
        self.id
    }

    fn new(id: u64, stream: &UnixStream) -> io::Result<Window> {
        let out = stream.try_clone()?;
        out.set_write_timeout(Some(WRITE_TIMEOUT))?;
        Ok(Window {
            id,
            out: Mutex::new(out),
            dead: AtomicBool::new(false),
        })
    }

    /// Writes one whole frame within [`WRITE_TIMEOUT`]. On failure the
    /// connection is shut down, so its reader thread ends it, and the window
    /// is dead from then on.
    pub fn send(&self, frame: &ToWindow) -> io::Result<()> {
        self.send_by(frame, Instant::now() + WRITE_TIMEOUT)
    }

    /// [`Window::send`] with a deadline shared by several frames: a frame
    /// that has not been written by `end` fails like a stalled write. The
    /// socket's own write timeout bounds the last single write, so this
    /// returns at most [`WRITE_TIMEOUT`] after `end`.
    pub fn send_by(&self, frame: &ToWindow, end: Instant) -> io::Result<()> {
        let mut bytes = Vec::new();
        frame.encode(&mut bytes);
        let mut out = self.out.lock().unwrap_or_else(|p| p.into_inner());
        if self.dead.load(Ordering::Acquire) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        write_within(&mut out, &bytes, end).inspect_err(|e| {
            self.dead.store(true, Ordering::Release);
            let _ = out.shutdown(Shutdown::Both);
            log!("window {}: dropping the connection: {e}", self.id);
        })
    }
}

/// `write_all` that gives up once `end` has passed. The socket's own write
/// timeout bounds each single write, so it returns at most [`WRITE_TIMEOUT`]
/// after `end`.
fn write_within(out: &mut UnixStream, mut bytes: &[u8], end: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= end {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the window stopped reading",
            ));
        }
        match out.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the window stopped reading",
                ));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

struct Server {
    panes: Mutex<BTreeMap<PaneId, Arc<Pane>>>,
    uid: u32,
    next_window: AtomicU64,
    /// True while a connection holds the one window slot.
    window_live: AtomicBool,
    /// The last blob `SaveLayout` stored, returned by `Hello`.
    layout: Mutex<Vec<u8>>,
    /// The socket file, removed when the sessions end.
    path: PathBuf,
    /// Set once the sessions are being ended. Read and set only under the
    /// pane table's lock, so it and the table's contents agree.
    ending: AtomicBool,
    /// Panes taken out of the table by `Close` whose close thread has not yet
    /// finished ending the shell. A pane moves from the table to here under
    /// the table's lock, so ending the sessions sees every pane in one or the
    /// other. Shared with the close threads, which remove their pane when done.
    closing: Arc<Mutex<Vec<Arc<Pane>>>>,
}

/// Why [`Server::end_sessions`] returned instead of exiting.
enum NotEnded {
    /// These panes, in ascending order, are running a program.
    Busy(Vec<PaneId>),
    /// Another caller is already ending the sessions.
    AlreadyEnding,
}

/// The one window slot, held by the live connection; dropping it lets the
/// next window in.
struct Slot(Arc<Server>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.window_live.store(false, Ordering::Release);
    }
}

/// What the connection does after a frame.
enum Flow {
    Go,
    Close(String),
}

/// Serves `path` until the sessions are ended, which exits the process.
/// Returns only on a setup error, as a message fit for the log.
pub fn run(path: &Path) -> Result<Infallible, String> {
    // The lock is held for as long as this process lives.
    let (listener, _lock) = bind(path)?;
    let server = Arc::new(Server {
        panes: Mutex::new(BTreeMap::new()),
        uid: rustix::process::geteuid().as_raw(),
        next_window: AtomicU64::new(1),
        window_live: AtomicBool::new(false),
        layout: Mutex::new(Vec::new()),
        path: path.to_path_buf(),
        ending: AtomicBool::new(false),
        closing: Arc::new(Mutex::new(Vec::new())),
    });
    // Only now that the socket is ours: ending removes it.
    handle_signals(&server)?;
    log!("listening on {}", path.display());
    loop {
        match listener.accept() {
            Ok((stream, _)) => server.accept(stream),
            Err(e) => {
                log!("cannot accept a connection: {e}");
                // Out of descriptors or memory: accept would fail again at
                // once, so give the system a moment. Only ever on this error.
                let exhausted = [Errno::EMFILE, Errno::ENFILE, Errno::ENOBUFS, Errno::ENOMEM];
                if exhausted
                    .iter()
                    .any(|n| e.raw_os_error() == Some(*n as i32))
                {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

/// Starts the thread that ends the sessions on `SIGTERM` or `SIGINT`. It
/// blocks in `signal-hook`'s iterator until a signal comes.
fn handle_signals(server: &Arc<Server>) -> Result<(), String> {
    let mut signals =
        Signals::new([SIGTERM, SIGINT]).map_err(|e| format!("cannot handle signals: {e}"))?;
    let server = server.clone();
    std::thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            for signal in signals.forever() {
                let name = if signal == SIGINT {
                    "SIGINT"
                } else {
                    "SIGTERM"
                };
                // Returns only if the sessions are already being ended.
                server.end_sessions(true, name);
            }
        })
        .map(drop)
        .map_err(|e| format!("cannot start the signal thread: {e}"))
}

/// The lock file beside `socket`: `<socket path>.lock`.
fn lock_path(socket: &Path) -> PathBuf {
    let mut p = socket.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

/// Makes the socket: its folder 0700 if it has to be created, then an
/// exclusive lock on `<socket path>.lock` (0600), then a stale socket file
/// replaced, then the socket itself, 0600. Only the holder of the lock touches
/// the socket file, so two kykods starting together cannot both decide the
/// socket is stale and take it from each other. The caller keeps the returned
/// lock for the life of the process.
fn bind(path: &Path) -> Result<(UnixListener, Flock<File>), String> {
    let shown = path.display();
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && fs::symlink_metadata(dir).is_err()
    {
        DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .and_then(|()| fs::set_permissions(dir, Permissions::from_mode(0o700)))
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let lock_file = lock_path(path);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        // A link here is not ours to follow: refused (ELOOP), not locked.
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&lock_file)
        .map_err(|e| format!("cannot open {}: {e}", lock_file.display()))?;
    let lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| {
        if e == Errno::EWOULDBLOCK {
            format!("another kykod is serving {shown}")
        } else {
            format!("cannot lock {}: {}", lock_file.display(), e.desc())
        }
    })?;
    match fs::symlink_metadata(path) {
        Ok(_) => match UnixStream::connect(path) {
            Ok(_) => return Err(format!("{shown} is already served by a running kykod")),
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                fs::remove_file(path).map_err(|e| format!("cannot replace {shown}: {e}"))?;
                log!("replaced a stale socket file at {shown}");
            }
            Err(e) => return Err(format!("cannot use {shown}: {e}")),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot use {shown}: {e}")),
    }
    // The socket is created with mode 0600 from the start: no moment in which
    // another user could connect. This runs before any thread is started, so
    // changing the process-wide umask here is safe.
    let old = umask(Mode::from_bits_truncate(0o177));
    let bound = UnixListener::bind(path);
    umask(old);
    let listener = bound.map_err(|e| format!("cannot listen on {shown}: {e}"))?;
    fs::set_permissions(path, Permissions::from_mode(0o600))
        .map_err(|e| format!("cannot set the mode of {shown}: {e}"))?;
    Ok((listener, lock))
}

fn kind(frame: &ToHolder) -> &'static str {
    match frame {
        ToHolder::Hello { .. } => "Hello",
        ToHolder::Spawn(_) => "Spawn",
        ToHolder::Input { .. } => "Input",
        ToHolder::Resize { .. } => "Resize",
        ToHolder::Close { .. } => "Close",
        ToHolder::Attach { .. } => "Attach",
        ToHolder::SaveLayout { .. } => "SaveLayout",
        ToHolder::EndSessions { .. } => "EndSessions",
    }
}

impl Server {
    fn table(&self) -> MutexGuard<'_, BTreeMap<PaneId, Arc<Pane>>> {
        self.panes.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Every pane, in id order, with the table lock already released.
    fn all_panes(&self) -> Vec<Arc<Pane>> {
        self.table().values().cloned().collect()
    }

    fn pane(&self, id: PaneId) -> Option<Arc<Pane>> {
        self.table().get(&id).cloned()
    }

    fn accept(self: &Arc<Self>, stream: UnixStream) {
        let id = self.next_window.fetch_add(1, Ordering::Relaxed);
        match rustix::net::sockopt::socket_peercred(&stream) {
            Ok(cred) if peer_allowed(cred.uid.as_raw(), self.uid) => {}
            Ok(cred) => {
                log!(
                    "window {id}: refused: uid {} is not {}",
                    cred.uid.as_raw(),
                    self.uid
                );
                return;
            }
            Err(e) => {
                log!("window {id}: refused: cannot read its credentials: {e}");
                return;
            }
        }
        // One window at a time: closed here, before any of its bytes is read.
        if self
            .window_live
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            log!("window {id}: refused: another window is connected");
            return;
        }
        let slot = Slot(self.clone());
        // On failure the closure, and with it the slot, is dropped.
        if let Err(e) = std::thread::Builder::new()
            .name(format!("window-{id}"))
            .spawn(move || slot.0.clone().serve(id, stream, slot))
        {
            log!("window {id}: refused: cannot start a thread: {e}");
        }
    }

    /// One connection, from its first byte to its end. `slot` is released
    /// only after every pane has gone back to ring-only, and before the
    /// "disconnected" line is logged.
    fn serve(&self, id: u64, mut stream: UnixStream, slot: Slot) {
        let window = match Window::new(id, &stream) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log!("window {id}: refused: {e}");
                return;
            }
        };
        log!("window {id} connected");
        let why = self.converse(&window, &mut stream);
        window.dead.store(true, Ordering::Release);
        let _ = stream.shutdown(Shutdown::Both);
        for pane in self.all_panes() {
            pane.detach(&window);
        }
        drop(slot);
        log!("window {id} disconnected: {why}");
    }

    /// Reads and handles frames until the connection ends; returns why it ended.
    fn converse(&self, window: &Arc<Window>, stream: &mut UnixStream) -> String {
        let mut decoder = Decoder::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut greeted = false;
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) => return "closed".to_string(),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return e.to_string(),
            };
            decoder.push(&buf[..n]);
            // Every whole frame is taken before the next read, so the decoder
            // never holds more than one incomplete frame.
            loop {
                match decoder.next::<ToHolder>() {
                    Ok(None) => break,
                    Ok(Some(frame)) => {
                        if let Flow::Close(why) = self.handle(window, &mut greeted, frame) {
                            return why;
                        }
                    }
                    Err(e) => return format!("protocol error: {e}"),
                }
            }
        }
    }

    fn handle(&self, window: &Arc<Window>, greeted: &mut bool, frame: ToHolder) -> Flow {
        match frame {
            ToHolder::Hello { version } if version != VERSION => {
                // The window cannot attach, but it can read our version and tell
                // the user why.
                let _ = window.send(&ToWindow::Hello {
                    version: VERSION,
                    panes: vec![],
                    layout: vec![],
                });
                Flow::Close(format!("protocol version {version}, not {VERSION}"))
            }
            ToHolder::Hello { .. } => {
                *greeted = true;
                self.hello(window)
            }
            other if !*greeted => Flow::Close(format!("{} before Hello", kind(&other))),
            ToHolder::Spawn(spec) => self.spawn(window, spec),
            ToHolder::Input { pane, bytes } => {
                if let Some(p) = self.pane(pane) {
                    p.input(bytes);
                }
                Flow::Go
            }
            ToHolder::Resize { pane, cols, rows } => {
                if let Some(p) = self.pane(pane) {
                    p.resize(window, cols, rows);
                }
                Flow::Go
            }
            ToHolder::Close { pane } => {
                let removed = {
                    let mut table = self.table();
                    let removed = table.remove(&pane);
                    if let Some(p) = &removed {
                        lock_list(&self.closing).push(p.clone());
                    }
                    removed
                };
                if let Some(p) = removed {
                    let closing = self.closing.clone();
                    let done = p.clone();
                    p.close(move || lock_list(&closing).retain(|q| !Arc::ptr_eq(q, &done)));
                    log!("pane {pane} closed");
                }
                Flow::Go
            }
            ToHolder::Attach { pane } => match self.pane(pane) {
                Some(p) if p.attach(window).is_err() => {
                    Flow::Close("cannot write to the window".to_string())
                }
                _ => Flow::Go,
            },
            ToHolder::SaveLayout { blob } if blob.len() > MAX_LAYOUT => Flow::Close(format!(
                "layout of {} bytes exceeds the {MAX_LAYOUT} byte limit",
                blob.len()
            )),
            ToHolder::SaveLayout { blob } => {
                *self.layout.lock().unwrap_or_else(|p| p.into_inner()) = blob;
                Flow::Go
            }
            ToHolder::EndSessions { force } => {
                let why = if force {
                    "EndSessions, forced"
                } else {
                    "EndSessions"
                };
                match self.end_sessions(force, why) {
                    NotEnded::Busy(panes) => {
                        log!("EndSessions refused: busy panes {panes:?}");
                        match window.send(&ToWindow::Busy { panes }) {
                            Ok(()) => Flow::Go,
                            Err(_) => Flow::Close("cannot write to the window".to_string()),
                        }
                    }
                    NotEnded::AlreadyEnding => Flow::Go,
                }
            }
        }
    }

    /// Ends every session and exits the process with status 0, unless `force`
    /// is false and some pane is busy, or the sessions are already being
    /// ended; then it returns why and changes nothing.
    ///
    /// The decision is one pass under the pane table's lock: every pane is
    /// checked, then either nothing happens or the server is marked as ending
    /// and the list of panes taken. Nothing is hung up until all are checked.
    /// Input comes only from the connection's thread, which is the one making
    /// this call for `EndSessions`, so no new command can be typed between the
    /// check and the hang-up; input already queued, or a background job
    /// taking the terminal, in the moment between them is not seen.
    ///
    /// Then, with no lock held: the socket file is removed, so no window can
    /// reach a holder that is going away; every shell's process group gets
    /// `SIGHUP` at once; one `poll` on their pidfds, and on those of panes
    /// still being closed (already hung up by `Close`), waits until all have
    /// ended or [`END_GRACE`] has passed. At that point any pane still being
    /// closed whose shell is running gets `SIGKILL` to its process group, as
    /// `Close` promised, since exiting ends its close thread, and up to
    /// [`KILL_REAP`] more is given for those to end. The process then exits 0,
    /// after reaping every shell that has ended by then (a non-blocking wait on
    /// each). A shell of a pane that was not closed and is still running is
    /// not killed: it outlives `kykod`, and the kernel hangs its terminal up
    /// when `kykod`'s side closes.
    ///
    /// The lock file is left behind on purpose. Removing it would let a
    /// `kykod` that had opened it just before lock a file no longer in the
    /// folder while a third one creates and locks a new one, and both would
    /// then serve the same socket. The lock itself is released by the kernel
    /// when this process exits.
    fn end_sessions(&self, force: bool, why: &str) -> NotEnded {
        let (panes, closing): (Vec<Arc<Pane>>, Vec<Arc<Pane>>) = {
            let table = self.table();
            if self.ending.load(Ordering::Acquire) {
                return NotEnded::AlreadyEnding;
            }
            if !force {
                // The table is ordered by id, so the list is ascending.
                let busy: Vec<PaneId> = table
                    .values()
                    .filter(|p| p.is_busy())
                    .map(|p| p.id())
                    .collect();
                if !busy.is_empty() {
                    return NotEnded::Busy(busy);
                }
            }
            self.ending.store(true, Ordering::Release);
            let closing = lock_list(&self.closing).clone();
            (table.values().cloned().collect(), closing)
        };
        log!(
            "ending every session ({why}): hanging up {} shell(s), {} more being closed",
            panes.len(),
            closing.len()
        );
        if let Err(e) = fs::remove_file(&self.path) {
            log!("cannot remove {}: {e}", self.path.display());
        }
        for pane in &panes {
            pane.hangup();
        }
        let end = Instant::now() + END_GRACE;
        let fds: Vec<OwnedFd> = panes
            .iter()
            .chain(&closing)
            .filter_map(|p| match p.exit_fd() {
                Ok(fd) => fd,
                Err(e) => {
                    log!("pane {}: cannot wait for its shell: {e}", p.id());
                    None
                }
            })
            .collect();
        let left = wait_for_exits(fds, end);
        if left > 0 {
            log!(
                "{left} shell(s) still running after {}s; exiting anyway",
                END_GRACE.as_secs()
            );
        }
        // Closed panes: the list as it is now, which also holds any pane closed
        // since the decision. A close thread that has finished took its pane
        // out, and one that has not is ended with the process.
        let still_closing = lock_list(&self.closing).clone();
        let killed: Vec<OwnedFd> = still_closing
            .iter()
            .filter(|p| p.kill_if_running())
            .filter_map(|p| p.exit_fd().ok().flatten())
            .collect();
        // A killed shell ends at once unless it is stuck in the kernel; it
        // gets a moment, so that it can be reaped below.
        wait_for_exits(killed, Instant::now() + KILL_REAP);
        // Reap every shell that has ended, without waiting for any: once
        // `kykod` has gone, an unreaped one would be left to PID 1, and not
        // every PID 1 reaps (a container's often does not).
        for pane in panes.iter().chain(&closing).chain(&still_closing) {
            pane.reap();
        }
        log!("exiting");
        std::process::exit(0);
    }

    /// Answers `Hello` with every pane and the stored layout. No pane's output
    /// is sent until the window attaches it.
    fn hello(&self, window: &Arc<Window>) -> Flow {
        let panes = self.all_panes();
        let layout = self
            .layout
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let reply = ToWindow::Hello {
            version: VERSION,
            panes: panes.iter().map(|p| p.info()).collect(),
            layout,
        };
        if window.send(&reply).is_err() {
            return Flow::Close("cannot write to the window".to_string());
        }
        Flow::Go
    }

    fn spawn(&self, window: &Arc<Window>, spec: SpawnSpec) -> Flow {
        let id = spec.pane;
        let result = {
            let mut table = self.table();
            if self.ending.load(Ordering::Acquire) {
                Err("kykod is ending every session".to_string())
            } else if table.contains_key(&id) {
                Err(format!("pane {id} already exists"))
            } else if table.len() >= MAX_PANES {
                Err("too many panes".to_string())
            } else {
                // Live for this window from its first byte.
                Pane::spawn(&spec, Some(window.clone())).inspect(|pane| {
                    log!("pane {id} started: {} (pid {})", spec.shell, pane.pid());
                    table.insert(id, pane.clone());
                })
            }
        };
        let message = match result {
            // The one check right after Spawn, with the table lock released.
            Ok(pane) => {
                pane.check_info();
                return Flow::Go;
            }
            Err(message) => message,
        };
        log!("pane {id} not started: {message}");
        match window.send(&ToWindow::SpawnFailed { pane: id, message }) {
            Ok(()) => Flow::Go,
            Err(_) => Flow::Close("cannot write to the window".to_string()),
        }
    }
}

fn lock_list(list: &Mutex<Vec<Arc<Pane>>>) -> MutexGuard<'_, Vec<Arc<Pane>>> {
    list.lock().unwrap_or_else(|p| p.into_inner())
}

/// Waits until every pidfd in `fds` is readable (its process has ended) or
/// `end` has passed, sleeping in one `poll` on all of them at a time. Returns
/// how many processes had not ended.
fn wait_for_exits(mut fds: Vec<OwnedFd>, end: Instant) -> usize {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    while !fds.is_empty() {
        let left = end.saturating_duration_since(Instant::now());
        let Ok(timeout) = Timespec::try_from(left) else {
            break;
        };
        if left.is_zero() {
            break;
        }
        let mut polls: Vec<PollFd<'_>> = fds
            .iter()
            .map(|fd| PollFd::new(fd, PollFlags::IN))
            .collect();
        match poll(&mut polls, Some(&timeout)) {
            Ok(0) => break,
            Ok(_) => {
                let ended: Vec<bool> = polls.iter().map(|p| !p.revents().is_empty()).collect();
                let mut i = 0;
                fds.retain(|_| {
                    i += 1;
                    !ended[i - 1]
                });
            }
            // A second signal, say: wait again for what is left of the grace.
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => {
                log!("cannot wait for the shells: {e}");
                break;
            }
        }
    }
    fds.len()
}

#[cfg(test)]
mod tests {
    use super::peer_allowed;

    #[test]
    fn only_the_same_user_may_connect() {
        assert!(peer_allowed(1000, 1000));
        assert!(!peer_allowed(1001, 1000));
        assert!(!peer_allowed(0, 1000));
    }
}
