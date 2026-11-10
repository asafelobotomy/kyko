//! A pane: one shell on a PTY, the ring of its recent output, and the threads
//! that read its output and write its input.
//!
//! Each pane has one reader thread, which blocks in `read` on the PTY, and one
//! input thread, which blocks on its queue's channel when idle and in `write`
//! while the program is slow to read. Neither touches the server's pane table,
//! and the connection thread only ever queues input, so a program that never
//! reads cannot hold up the connection. Everything that changes with the pane's
//! output (the ring, whether a window gets it live, the exit status) sits under
//! the pane's own lock, so whoever holds that lock sees the ring and the live
//! stream agree. `Attach` relies on that: it sends the ring and flips the pane
//! live under one hold of the lock, so every byte reaches the window exactly
//! once, either in the replay or live.
//!
//! The foreground program and folder are checked once right after `Spawn` and
//! `Attach`, and by the reader thread once the output of a live pane has been
//! quiet for [`QUIET`]. The reader waits for that with a `poll` timeout that is
//! armed only by output and disarmed once the check has run, so an idle pane's
//! threads never wake.

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kyko_proto::{PaneId, PaneInfo, SpawnSpec, ToWindow};

use crate::pty::Pty;
use crate::ring::Ring;
use crate::server::{WRITE_TIMEOUT, Window};

/// The most output bytes one `Output` frame carries.
pub const OUTPUT_CHUNK: usize = 64 * 1024;

/// The ring size used when `Spawn` asks for 0.
pub const DEFAULT_RING: usize = 2 * 1024 * 1024;

/// The most input bytes a pane holds waiting for its program to read them;
/// `Input` beyond that is dropped.
pub const INPUT_LIMIT: usize = 4 * 1024 * 1024;

/// How long `Close` gives a shell after `SIGHUP` before killing its group.
pub const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// How long a live pane's output must have been quiet before its program and
/// folder are checked.
pub const QUIET: Duration = Duration::from_millis(100);

pub struct Pane {
    id: PaneId,
    pty: Pty,
    state: Mutex<State>,
    input: Mutex<Input>,
    /// Bytes queued for the input thread and not yet written.
    pending: AtomicUsize,
    /// Set by `Close`: the input thread discards whatever is still queued.
    discard_input: AtomicBool,
}

struct Input {
    /// The input thread's queue; `None` once the shell has ended or the pane
    /// was closed, which also ends the input thread.
    queue: Option<Sender<Vec<u8>>>,
    /// True from the first dropped `Input` until one fits again, so an
    /// overflow is logged once, not once per frame.
    overflowing: bool,
}

struct State {
    ring: Ring,
    /// The window this pane's output is sent to as it arrives, if any. Set and
    /// cleared only under this lock, so "is this pane live for that window" has
    /// one answer at a time.
    live: Option<Arc<Window>>,
    cols: u16,
    rows: u16,
    program: String,
    cwd: String,
    /// The program and folder last sent to the live window in `Info`; `None`
    /// until one is sent, and again on every `Attach`.
    info_sent: Option<(String, String)>,
    exited: Option<i32>,
    /// Set by `Close`: the pane is out of the table and its output is dropped
    /// until the shell has gone.
    closed: bool,
}

impl Pane {
    /// Starts the shell, its reader thread and its input thread. `live` is the window that gets
    /// the output from the first byte.
    pub fn spawn(spec: &SpawnSpec, live: Option<Arc<Window>>) -> Result<Arc<Pane>, String> {
        let pty = Pty::spawn(
            &spec.shell,
            &spec.args,
            &spec.cwd,
            &spec.env,
            spec.cols,
            spec.rows,
        )
        .map_err(|e| e.0)?;
        // On any failure from here on, dropping the pane ends and reaps the shell.
        let reader = pty
            .reader()
            .map_err(|e| format!("cannot read the terminal: {e}"))?;
        let ring = match spec.ring_bytes {
            0 => DEFAULT_RING,
            n => n as usize,
        };
        let program = Path::new(&spec.shell)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| spec.shell.clone());
        let cwd = pty.cwd().unwrap_or_else(|| spec.cwd.clone());
        let (queue, queued) = channel();
        let pane = Arc::new(Pane {
            id: spec.pane,
            pty,
            input: Mutex::new(Input {
                queue: Some(queue),
                overflowing: false,
            }),
            pending: AtomicUsize::new(0),
            discard_input: AtomicBool::new(false),
            state: Mutex::new(State {
                ring: Ring::new(ring),
                live,
                cols: spec.cols.max(2),
                rows: spec.rows.max(1),
                program,
                cwd,
                info_sent: None,
                exited: None,
                closed: false,
            }),
        });
        let writing = pane.clone();
        std::thread::Builder::new()
            .name(format!("input-{}", spec.pane))
            .spawn(move || writing.write_input(queued))
            .map_err(|e| format!("cannot start a thread: {e}"))?;
        let reading = pane.clone();
        if let Err(e) = std::thread::Builder::new()
            .name(format!("pane-{}", spec.pane))
            .spawn(move || reading.pump(reader))
        {
            // Ends the input thread, which lets go of the pane.
            pane.stop_input();
            return Err(format!("cannot start a thread: {e}"));
        }
        Ok(pane)
    }

    pub fn id(&self) -> PaneId {
        self.id
    }

    pub fn pid(&self) -> i32 {
        self.pty.pid()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn lock_input(&self) -> MutexGuard<'_, Input> {
        self.input.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The input thread: writes queued input to the program in order, and
    /// ends once the queue is closed.
    fn write_input(&self, queued: Receiver<Vec<u8>>) {
        for bytes in queued {
            if !self.discard_input.load(Ordering::Acquire) {
                // Errors mean the shell has gone; the queue closes soon after.
                let _ = self.pty.write_all(&bytes);
            }
            self.pending.fetch_sub(bytes.len(), Ordering::AcqRel);
        }
    }

    /// Closes the input queue; the input thread ends once it has drained it.
    fn stop_input(&self) {
        self.lock_input().queue = None;
    }

    /// The reader thread: every byte goes to the ring and, if a window is live,
    /// to it. What is already waiting after a read is read too, up to
    /// [`OUTPUT_CHUNK`], and goes out as one frame: a program that writes a
    /// line at a time would otherwise cost a frame per line or so. Nothing
    /// waits for more to come. After output to a live window, the next read
    /// waits at most [`QUIET`]; if nothing comes, the program and folder are
    /// checked and the read blocks again with no timeout. Ends once the shell
    /// is gone and reaped.
    fn pump(&self, mut reader: File) {
        let mut buf = vec![0u8; OUTPUT_CHUNK];
        let mut check_when_quiet = false;
        loop {
            if check_when_quiet && !readable_within(&reader, QUIET) {
                check_when_quiet = false;
                self.check_info();
                continue;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(mut n) => {
                    while n < buf.len() && readable_within(&reader, Duration::ZERO) {
                        match reader.read(&mut buf[n..]) {
                            Ok(m) if m > 0 => n += m,
                            // The next read in the outer loop sees the end
                            // or the error again and handles it.
                            _ => break,
                        }
                    }
                    check_when_quiet = self.output(&buf[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // EIO: nothing has the terminal's other side open any more.
                Err(_) => break,
            }
        }
        drop(reader);
        let status = self.pty.wait().unwrap_or(-1);
        self.stop_input();
        self.ended(status);
    }

    /// Keeps `bytes` and sends them to the live window, if any. Returns true
    /// if a window is still live for this pane.
    fn output(&self, bytes: &[u8]) -> bool {
        let mut st = self.lock();
        if st.closed {
            return false;
        }
        st.ring.push(bytes);
        // The send happens under the pane lock (never the table lock), so the
        // ring and the live stream cannot disagree. Window::send gives up on a
        // stalled window within its write timeout, so the shell is never held
        // up for longer than that.
        if let Some(w) = &st.live {
            let frame = ToWindow::Output {
                pane: self.id,
                bytes: bytes.to_vec(),
            };
            if w.send(&frame).is_err() {
                st.live = None;
            }
        }
        st.live.is_some()
    }

    fn ended(&self, status: i32) {
        let mut st = self.lock();
        st.exited = Some(status);
        log!("pane {} ended with status {status}", self.id);
        if let Some(w) = &st.live {
            let frame = ToWindow::Exited {
                pane: self.id,
                status,
            };
            if w.send(&frame).is_err() {
                st.live = None;
            }
        }
    }

    /// Queues `bytes` to be typed into the shell; never blocks. Input that
    /// would take the queue past [`INPUT_LIMIT`] is dropped, and logged once per
    /// overflow. Input to a pane whose shell has ended is ignored.
    pub fn input(&self, bytes: Vec<u8>) {
        let mut input = self.lock_input();
        if input.queue.is_none() {
            return;
        }
        let len = bytes.len();
        if self.pending.load(Ordering::Acquire) + len > INPUT_LIMIT {
            if !input.overflowing {
                input.overflowing = true;
                log!(
                    "pane {}: input queue full ({INPUT_LIMIT} bytes waiting); dropping input",
                    self.id
                );
            }
            return;
        }
        input.overflowing = false;
        self.pending.fetch_add(len, Ordering::AcqRel);
        if input.queue.as_ref().is_some_and(|q| q.send(bytes).is_err()) {
            self.pending.fetch_sub(len, Ordering::AcqRel);
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let mut st = self.lock();
        st.cols = cols.max(2);
        st.rows = rows.max(1);
        self.pty.resize(cols, rows);
    }

    /// Stops sending or keeping the pane's output, discards its queued input,
    /// and ends the shell: `SIGHUP` now, `SIGKILL` to its process group if it
    /// is still there after [`CLOSE_GRACE`]. The wait happens on a short-lived
    /// thread, so the caller does not block; that thread calls `done` once the
    /// shell has ended or been killed. If the thread cannot start, the shell is
    /// only hung up and `done` is never called. The reader thread then sees
    /// the terminal close, reaps the shell and ends.
    pub fn close(self: &Arc<Self>, done: impl FnOnce() + Send + 'static) {
        {
            let mut st = self.lock();
            st.closed = true;
            st.live = None;
            st.ring = Ring::new(0);
        }
        self.discard_input.store(true, Ordering::Release);
        self.stop_input();
        let pane = self.clone();
        if let Err(e) = std::thread::Builder::new()
            .name(format!("close-{}", self.id))
            .spawn(move || {
                pane.pty.end(CLOSE_GRACE);
                done();
            })
        {
            log!(
                "pane {}: cannot start a thread to end it ({e}); hung up only",
                self.id
            );
            self.pty.hangup();
        }
    }

    /// True while the shell is running and something other than the shell
    /// holds its terminal. A shell that has ended is never busy. Takes no
    /// pane lock.
    pub fn is_busy(&self) -> bool {
        self.pty.try_wait().is_none() && self.pty.is_busy()
    }

    /// Sends `SIGHUP` to the shell's process group (nothing if it is reaped).
    pub fn hangup(&self) {
        self.pty.hangup();
    }

    /// See [`Pty::kill_if_running`].
    pub fn kill_if_running(&self) -> bool {
        self.pty.kill_if_running()
    }

    /// Reaps the shell if it has ended; never blocks. See [`Pty::try_wait`].
    pub fn reap(&self) {
        let _ = self.pty.try_wait();
    }

    /// See [`Pty::exit_fd`].
    pub fn exit_fd(&self) -> io::Result<Option<OwnedFd>> {
        self.pty.exit_fd()
    }

    /// What `Hello` reports for this pane: the foreground program and folder
    /// while the shell runs, the last ones seen once it has ended.
    pub fn info(&self) -> PaneInfo {
        let mut st = self.lock();
        if st.exited.is_none() {
            self.refresh(&mut st);
        }
        PaneInfo {
            pane: self.id,
            program: st.program.clone(),
            cwd: st.cwd.clone(),
            cols: st.cols,
            rows: st.rows,
            exited: st.exited,
        }
    }

    /// Sends the ring to `window` as `Output` frames of at most
    /// [`OUTPUT_CHUNK`] bytes, then `Replayed`, and from then on the pane's
    /// output as it arrives. After `Replayed` comes `Exited` if the shell has
    /// ended (it may have ended after `Hello` listed it as running), or else an
    /// `Info`. All of it happens under the pane's lock, so the reader thread
    /// cannot add a byte between the replay and the switch to live: no byte is
    /// lost or sent twice. Every frame shares one [`WRITE_TIMEOUT`] deadline,
    /// so a window that cannot take them holds the shell up no longer than one
    /// stalled frame would. A pane that is already live is replayed again: the
    /// window asked for it. An error means the window is gone.
    pub fn attach(&self, window: &Arc<Window>) -> io::Result<()> {
        let mut st = self.lock();
        if st.closed {
            return Ok(());
        }
        st.live = None;
        let end = Instant::now() + WRITE_TIMEOUT;
        let replay = st.ring.contents();
        for chunk in replay.chunks(OUTPUT_CHUNK) {
            let frame = ToWindow::Output {
                pane: self.id,
                bytes: chunk.to_vec(),
            };
            window.send_by(&frame, end)?;
        }
        let done = ToWindow::Replayed {
            pane: self.id,
            truncated: st.ring.truncated(),
        };
        window.send_by(&done, end)?;
        if let Some(status) = st.exited {
            let frame = ToWindow::Exited {
                pane: self.id,
                status,
            };
            window.send_by(&frame, end)?;
        }
        st.live = Some(window.clone());
        st.info_sent = None;
        self.send_info(&mut st, end);
        if st.live.is_some() {
            Ok(())
        } else {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    /// Sends `Info` to the live window if the foreground program or folder
    /// differs from what it was last sent.
    pub fn check_info(&self) {
        let mut st = self.lock();
        self.send_info(&mut st, Instant::now() + WRITE_TIMEOUT);
    }

    /// [`Pane::check_info`] with the lock held, writing by `end`.
    fn send_info(&self, st: &mut State, end: Instant) {
        if st.closed || st.exited.is_some() {
            return;
        }
        let Some(window) = st.live.clone() else {
            return;
        };
        self.refresh(st);
        let now = (st.program.clone(), st.cwd.clone());
        if st.info_sent.as_ref() == Some(&now) {
            return;
        }
        let frame = ToWindow::Info {
            pane: self.id,
            program: now.0.clone(),
            cwd: now.1.clone(),
        };
        if window.send_by(&frame, end).is_ok() {
            st.info_sent = Some(now);
        } else {
            st.live = None;
        }
    }

    /// Reads the foreground program and folder from the system into `st`.
    fn refresh(&self, st: &mut State) {
        if let Some((_, program)) = self.pty.foreground() {
            st.program = program;
        }
        if let Some(cwd) = self.pty.cwd() {
            st.cwd = cwd;
        }
    }

    /// Stops sending to `window`, if this pane was live for it.
    pub fn detach(&self, window: &Arc<Window>) {
        let mut st = self.lock();
        if st.live.as_ref().is_some_and(|w| Arc::ptr_eq(w, window)) {
            st.live = None;
        }
    }
}

/// True if `reader` has something to read (or has hung up) within `limit`;
/// false once `limit` has passed with nothing. An error counts as readable, so
/// the read that follows reports it.
fn readable_within(reader: &File, limit: Duration) -> bool {
    let Ok(timeout) = rustix::event::Timespec::try_from(limit) else {
        return true;
    };
    let mut fds = [rustix::event::PollFd::new(
        reader,
        rustix::event::PollFlags::IN,
    )];
    loop {
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(n) => return n > 0,
            // A signal is rare here; waiting the full limit again is harmless.
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return true,
        }
    }
}
