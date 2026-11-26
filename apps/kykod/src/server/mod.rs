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
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kyko_proto::{
    Decoder, Frame, MAX_LAYOUT, MAX_PANES, PaneId, SpawnSpec, ToHolder, ToWindow, VERSION,
};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use nix::sys::stat::{Mode, umask};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::pane::Pane;

mod ending;
mod exits;
mod socket;
mod window;

use exits::*;
use socket::*;
pub use window::Window;

/// How long one write to a window may block. A frame that cannot be written
/// within it (about twice this at worst, see [`write_within`]) drops the
/// connection.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

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

struct Server {
    panes: Mutex<BTreeMap<PaneId, Arc<Pane>>>,
    uid: u32,
    next_window: AtomicU64,
    /// True while a connection holds the one window slot.
    window_live: AtomicBool,
    /// Connections refused because a window was live, since that window
    /// took the slot. Only the first is logged; the rest are summed up when
    /// the window leaves, so a client retrying in a loop cannot flood the log.
    refused: AtomicU64,
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
        refused: AtomicU64::new(0),
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
                problem!(WARN, "[KYKO-015] cannot accept a connection: {e}");
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
                problem!(
                    WARN,
                    "[KYKO-012] window {id}: refused: uid {} is not {}",
                    cred.uid.as_raw(),
                    self.uid
                );
                return;
            }
            Err(e) => {
                problem!(
                    WARN,
                    "[KYKO-012] window {id}: refused: cannot read its credentials: {e}"
                );
                return;
            }
        }
        // One window at a time: closed here, before any of its bytes is read.
        if self
            .window_live
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            if self.refused.fetch_add(1, Ordering::Relaxed) == 0 {
                problem!(
                    WARN,
                    "[KYKO-012] window {id}: refused: another window is connected \
                     (further refusals are counted until it leaves)"
                );
            }
            return;
        }
        self.refused.store(0, Ordering::Relaxed);
        let slot = Slot(self.clone());
        // On failure the closure, and with it the slot, is dropped.
        if let Err(e) = std::thread::Builder::new()
            .name(format!("window-{id}"))
            .spawn(move || slot.0.clone().serve(id, stream, slot))
        {
            problem!(
                WARN,
                "[KYKO-012] window {id}: refused: cannot start a thread: {e}"
            );
        }
    }

    /// One connection, from its first byte to its end. `slot` is released
    /// only after every pane has gone back to ring-only, and before the
    /// "disconnected" line is logged.
    fn serve(&self, id: u64, mut stream: UnixStream, slot: Slot) {
        let window = match Window::new(id, &stream) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                problem!(WARN, "[KYKO-012] window {id}: refused: {e}");
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
        // Taken before the slot is let go, so it counts only refusals made
        // while this window held it.
        let more = self.refused.swap(0, Ordering::Relaxed).saturating_sub(1);
        drop(slot);
        if more == 0 {
            log!("window {id} disconnected: {why}");
        } else {
            log!("window {id} disconnected: {why}; {more} more connection(s) refused meanwhile");
        }
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
        problem!(ERROR, "[KYKO-013] pane {id} not started: {message}");
        match window.send(&ToWindow::SpawnFailed { pane: id, message }) {
            Ok(()) => Flow::Go,
            Err(_) => Flow::Close("cannot write to the window".to_string()),
        }
    }
}

#[cfg(test)]
mod tests;
