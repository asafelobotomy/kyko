//! Windows attaching to and detaching from a pane: the replay, the modes preamble, and the
//! `Info` frames that keep a window's tab names current.

use super::*;

impl Pane {
    /// Sends the ring to `window` as `Output` frames of at most
    /// [`OUTPUT_CHUNK`] bytes, then `Replayed`, and from then on the pane's
    /// output as it arrives; the window's first `Resize` from then on may
    /// nudge the program to redraw (see [`Pane::resize`]). After `Replayed`
    /// comes `Exited` if the shell has ended (it may have ended after `Hello`
    /// listed it as running), or else an `Info`. All of it happens under the pane's lock, so the reader thread
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
        let (replay, truncated) = st.ring.replay();
        for chunk in replay.chunks(OUTPUT_CHUNK) {
            let frame = ToWindow::Output {
                pane: self.id,
                bytes: chunk.to_vec(),
            };
            window.send_by(&frame, end)?;
        }
        let done = ToWindow::Replayed {
            pane: self.id,
            truncated,
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
        st.nudge = Some(window.id());
        // A same-size nudge makes some programs (htop) repaint only what changed, so a row is
        // taken off here and the window's `Resize` gives it back: a real size change both ways.
        if st.exited.is_none() && st.rows > 1 {
            self.pty.resize(st.cols, st.rows - 1);
            st.shrunk = Some(Instant::now());
            st.sized += 1;
        }
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
    pub(super) fn send_info(&self, st: &mut State, end: Instant) {
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
    pub(super) fn refresh(&self, st: &mut State) {
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
