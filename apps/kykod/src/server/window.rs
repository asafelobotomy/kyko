//! A connected window: the frames sent to it, each write bounded by [`WRITE_TIMEOUT`].

use super::*;

/// The writing side of one connection, shared by the connection's thread and
/// every pane that is live for it.
pub struct Window {
    pub(crate) id: u64,
    pub(crate) out: Mutex<UnixStream>,
    /// Set once the connection is over or a write failed; every send then
    /// fails at once, without touching the socket.
    pub(crate) dead: AtomicBool,
}

impl Window {
    /// This connection's number, unique for the life of the process.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn new(id: u64, stream: &UnixStream) -> io::Result<Window> {
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
pub(crate) fn write_within(out: &mut UnixStream, mut bytes: &[u8], end: Instant) -> io::Result<()> {
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
