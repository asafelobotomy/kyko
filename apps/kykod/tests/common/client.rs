//! One connection to a `kykod`, speaking the protocol as the window does.

use super::*;

/// One connection to a `kykod`, speaking the protocol as the window does.
pub struct Client {
    stream: UnixStream,
    path: PathBuf,
    decoder: Decoder,
    eof: bool,
    /// Frames received on this connection so far.
    frames: usize,
}

impl Client {
    /// A raw connection: nothing is sent. While another window is live, kykod
    /// closes it at once; [`Client::hello`] retries such a refusal.
    pub fn connect(h: &Holder) -> Client {
        Client::connect_path(h.socket())
    }

    pub fn connect_path(path: &Path) -> Client {
        let stream = UnixStream::connect(path).unwrap();
        // A kykod that stops reading fails the test instead of hanging it.
        stream.set_write_timeout(Some(LIMIT)).unwrap();
        Client {
            stream,
            path: path.to_path_buf(),
            decoder: Decoder::new(),
            eof: false,
            frames: 0,
        }
    }

    /// True once kykod has ended this connection.
    pub fn ended(&self) -> bool {
        self.eof
    }

    /// Frames received on this connection so far.
    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn send(&mut self, frame: ToHolder) {
        let mut out = Vec::new();
        frame.encode(&mut out);
        self.send_raw(&out);
    }

    pub fn send_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).unwrap();
    }

    /// The next frame, or `None` on timeout or end of stream.
    pub fn recv(&mut self, timeout: Duration) -> Option<ToWindow> {
        let end = Instant::now() + timeout;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            if let Some(f) = self
                .decoder
                .next::<ToWindow>()
                .expect("kykod sent a bad frame")
            {
                self.frames += 1;
                return Some(f);
            }
            if self.eof {
                return None;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            self.stream.set_read_timeout(Some(left)).unwrap();
            match self.stream.read(&mut buf) {
                Ok(0) => self.eof = true,
                Ok(n) => self.decoder.push(&buf[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return None;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                // A reset is how an abruptly closed connection can end.
                Err(_) => self.eof = true,
            }
        }
    }

    /// The first frame that satisfies `pred`, skipping the others.
    pub fn wait_for(
        &mut self,
        pred: impl Fn(&ToWindow) -> bool,
        timeout: Duration,
    ) -> Option<ToWindow> {
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let f = self.recv(left)?;
            if pred(&f) {
                return Some(f);
            }
        }
    }

    /// Sends `Hello` of this protocol version and returns the reply. kykod
    /// takes one window at a time and closes any other connection at once,
    /// without a frame; the previous window's end may not have reached it yet.
    /// So a connection that ends with no frame at all is opened again, until
    /// [`LIMIT`]. Anything else (no reply on an open connection, or an end
    /// after some frame) fails the test.
    pub fn hello(&mut self) -> (u32, Vec<PaneInfo>, Vec<u8>) {
        let end = Instant::now() + LIMIT;
        loop {
            if let Some(reply) = self.hello_once() {
                return reply;
            }
            assert!(
                self.eof && self.frames == 0,
                "no Hello came back on an open connection"
            );
            assert!(
                Instant::now() < end,
                "kykod refused every connection for {LIMIT:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
            *self = Client::connect_path(&self.path.clone());
        }
    }

    /// Sends `Hello` once on this connection; `None` if no `Hello` came back.
    pub fn hello_once(&mut self) -> Option<(u32, Vec<PaneInfo>, Vec<u8>)> {
        let mut out = Vec::new();
        ToHolder::Hello { version: VERSION }.encode(&mut out);
        // A refused connection may already be closed: the reply decides.
        let _ = self.stream.write_all(&out);
        match self.wait_for(|f| matches!(f, ToWindow::Hello { .. }), LIMIT) {
            Some(ToWindow::Hello {
                version,
                panes,
                layout,
            }) => Some((version, panes, layout)),
            _ => None,
        }
    }

    /// Sends `Attach` and collects the replay: every `Output` for `pane` up to
    /// its `Replayed`. Returns the replayed bytes and the `truncated` flag.
    /// Fails the test if another frame for `pane` comes first.
    pub fn attach(&mut self, pane: PaneId) -> (Vec<u8>, bool) {
        self.send(ToHolder::Attach { pane });
        let end = Instant::now() + LIMIT;
        let mut got = Vec::new();
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.recv(left) {
                Some(ToWindow::Output { pane: p, bytes }) if p == pane => {
                    assert!(bytes.len() <= 64 * 1024, "Output of {} bytes", bytes.len());
                    got.extend(bytes);
                }
                Some(ToWindow::Replayed { pane: p, truncated }) if p == pane => {
                    return (got, truncated);
                }
                Some(other) => assert!(
                    !matches!(
                        other,
                        ToWindow::Info { pane: p, .. } | ToWindow::Exited { pane: p, .. } if p == pane
                    ),
                    "{other:?} before Replayed"
                ),
                None => panic!("no Replayed for pane {pane}; got {} bytes", got.len()),
            }
        }
    }

    /// Collects `pane`'s output until it contains `needle`; panics on timeout.
    pub fn output_until(&mut self, pane: PaneId, needle: &[u8], timeout: Duration) -> Vec<u8> {
        self.output_until_n(pane, needle, 1, timeout)
    }

    /// Collects `pane`'s output until `needle` has appeared `times` times.
    pub fn output_until_n(
        &mut self,
        pane: PaneId,
        needle: &[u8],
        times: usize,
        timeout: Duration,
    ) -> Vec<u8> {
        let end = Instant::now() + timeout;
        let mut got = Vec::new();
        let mut seen = 0;
        while seen < times {
            let left = end.saturating_duration_since(Instant::now());
            match self.recv(left) {
                Some(ToWindow::Output { pane: p, bytes }) if p == pane => {
                    assert!(bytes.len() <= 64 * 1024, "Output of {} bytes", bytes.len());
                    // Count only matches that end in the new bytes.
                    let from = got.len().saturating_sub(needle.len() - 1);
                    got.extend(bytes);
                    seen += count(&got[from..], needle);
                }
                Some(_) => {}
                None => panic!(
                    "waiting for {:?} from pane {pane}: got {:?}",
                    String::from_utf8_lossy(needle),
                    String::from_utf8_lossy(&got)
                ),
            }
        }
        got
    }

    /// True if the connection reaches its end within `timeout`, whatever arrives first.
    pub fn closed_within(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        while !self.eof {
            if Instant::now() >= end {
                return false;
            }
            let left = end.saturating_duration_since(Instant::now());
            self.recv(left);
        }
        true
    }
}
