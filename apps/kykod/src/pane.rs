//! A pane: one shell on a PTY, the ring of its recent output, and the thread
//! that reads it.
//!
//! Each pane has one reader thread, which blocks in `read` on the PTY and never
//! touches the server's pane table. Everything that changes with the pane's
//! output (the ring, whether a window gets it live, the exit status) sits under
//! the pane's own lock, so whoever holds that lock sees the ring and the live
//! stream agree.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use kyko_proto::{PaneId, PaneInfo, SpawnSpec, ToWindow};

use crate::pty::Pty;
use crate::ring::Ring;
use crate::server::Window;

/// The most output bytes one `Output` frame carries.
pub const OUTPUT_CHUNK: usize = 64 * 1024;

/// The ring size used when `Spawn` asks for 0.
pub const DEFAULT_RING: usize = 2 * 1024 * 1024;

pub struct Pane {
    id: PaneId,
    pty: Pty,
    state: Mutex<State>,
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
    exited: Option<i32>,
    /// Set by `Close`: the pane is out of the table and its output is dropped
    /// until the shell has gone.
    closed: bool,
}

impl Pane {
    /// Starts the shell and its reader thread. `live` is the window that gets
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
        let pane = Arc::new(Pane {
            id: spec.pane,
            pty,
            state: Mutex::new(State {
                ring: Ring::new(ring),
                live,
                cols: spec.cols.max(2),
                rows: spec.rows.max(1),
                program,
                cwd,
                exited: None,
                closed: false,
            }),
        });
        let reading = pane.clone();
        std::thread::Builder::new()
            .name(format!("pane-{}", spec.pane))
            .spawn(move || reading.pump(reader))
            .map_err(|e| format!("cannot start a thread: {e}"))?;
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

    /// The reader thread: every byte goes to the ring and, if a window is live,
    /// to it. Ends once the shell is gone and reaped.
    fn pump(&self, mut reader: File) {
        let mut buf = vec![0u8; OUTPUT_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.output(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // EIO: nothing has the terminal's other side open any more.
                Err(_) => break,
            }
        }
        drop(reader);
        let status = self.pty.wait().unwrap_or(-1);
        self.ended(status);
    }

    fn output(&self, bytes: &[u8]) {
        let mut st = self.lock();
        if st.closed {
            return;
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

    /// Types `bytes` into the shell. Errors (the shell has gone) are ignored.
    pub fn input(&self, bytes: &[u8]) {
        let _ = self.pty.write_all(bytes);
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let mut st = self.lock();
        st.cols = cols.max(2);
        st.rows = rows.max(1);
        self.pty.resize(cols, rows);
    }

    /// Hangs the shell up and stops sending or keeping its output; the reader
    /// thread runs on until the shell has gone, then reaps it.
    pub fn close(&self) {
        {
            let mut st = self.lock();
            st.closed = true;
            st.live = None;
            st.ring = Ring::new(0);
        }
        self.pty.hangup();
    }

    /// What `Hello` reports for this pane: the foreground program and folder
    /// while the shell runs, the last ones seen once it has ended.
    pub fn info(&self) -> PaneInfo {
        let mut st = self.lock();
        if st.exited.is_none() {
            if let Some((_, program)) = self.pty.foreground() {
                st.program = program;
            }
            if let Some(cwd) = self.pty.cwd() {
                st.cwd = cwd;
            }
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

    /// From now on, this pane's output goes to `window` as it arrives.
    pub fn go_live(&self, window: &Arc<Window>) {
        let mut st = self.lock();
        if !st.closed {
            st.live = Some(window.clone());
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
