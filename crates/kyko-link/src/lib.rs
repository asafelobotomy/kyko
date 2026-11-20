#![forbid(unsafe_code)]
//! The window's side of the `kykod` socket, with no Qt: connect, say Hello, spawn and attach
//! panes, keep one emulator per pane fed from the holder's output, and answer the terminal's own
//! replies. The window thread calls [`Link::pump`] in a loop and draws from [`Link::pane`].

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use kyko_proto::{
    Decoder, Frame, PaneId, PaneInfo, ProtoError, SpawnSpec, ToHolder, ToWindow, VERSION,
};
use kyko_term::Emulator;

/// Why a link could not be made or has ended.
#[derive(Debug)]
pub enum LinkError {
    Io(io::Error),
    Proto(ProtoError),
    /// `kykod` answered with another protocol version; the window cannot talk to it.
    VersionMismatch {
        holder: u32,
    },
    /// The connection closed: `kykod` refuses a second window, and exits when asked to.
    Closed,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "kykod socket: {e}"),
            Self::Proto(e) => write!(f, "kykod protocol: {e}"),
            Self::VersionMismatch { holder } => {
                write!(
                    f,
                    "kykod speaks protocol {holder}, this window speaks {VERSION}"
                )
            }
            Self::Closed => f.write_str("kykod closed the connection"),
        }
    }
}

impl std::error::Error for LinkError {}

impl From<io::Error> for LinkError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ProtoError> for LinkError {
    fn from(e: ProtoError) -> Self {
        Self::Proto(e)
    }
}

/// What a call to [`Link::pump`] saw.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The pane's screen may have changed: draw it again.
    Changed(PaneId),
    /// The pane's foreground program or folder changed.
    Info {
        pane: PaneId,
        program: String,
        cwd: String,
    },
    Exited {
        pane: PaneId,
        status: i32,
    },
    SpawnFailed {
        pane: PaneId,
        message: String,
    },
    /// `kykod` refused `EndSessions` because these panes run something other than a shell.
    Busy(Vec<PaneId>),
}

/// One pane as the window sees it.
pub struct Pane {
    emulator: Emulator,
    pub program: String,
    pub cwd: String,
    pub exited: Option<i32>,
    /// Output received since `Attach`, held until `Replayed` ends the replay.
    replay: Option<Vec<u8>>,
}

impl Pane {
    pub fn emulator(&self) -> &Emulator {
        &self.emulator
    }

    /// For the view's own state: scrolling and selecting. What a program printed is only ever
    /// changed by [`Link::pump`].
    pub fn emulator_mut(&mut self) -> &mut Emulator {
        &mut self.emulator
    }
}

pub struct Link {
    stream: UnixStream,
    decoder: Decoder,
    panes: HashMap<PaneId, Pane>,
    scrollback: usize,
    /// The layout blob `kykod` held when this window connected.
    pub layout: Vec<u8>,
}

impl Link {
    /// Connects to `socket` and exchanges Hellos. `scrollback` is the lines each pane's emulator
    /// keeps. Panes `kykod` already holds are known but not attached: call [`Link::attach`].
    pub fn connect(socket: &Path, scrollback: usize) -> Result<Link, LinkError> {
        let stream = UnixStream::connect(socket)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut link = Link {
            stream,
            decoder: Decoder::new(),
            panes: HashMap::new(),
            scrollback,
            layout: Vec::new(),
        };
        link.send(&ToHolder::Hello { version: VERSION })?;
        loop {
            let Some(frame) = link.read_frame(Duration::from_secs(5))? else {
                return Err(LinkError::Closed);
            };
            if let ToWindow::Hello {
                version,
                panes,
                layout,
            } = frame
            {
                if version != VERSION {
                    return Err(LinkError::VersionMismatch { holder: version });
                }
                link.layout = layout;
                for info in panes {
                    link.add_pane(&info);
                }
                return Ok(link);
            }
        }
    }

    fn add_pane(&mut self, info: &PaneInfo) {
        self.panes.insert(
            info.pane,
            Pane {
                emulator: {
                    let mut e = Emulator::new(info.cols, info.rows, self.scrollback);
                    e.set_kitty_keyboard(true);
                    e
                },
                program: info.program.clone(),
                cwd: info.cwd.clone(),
                exited: info.exited,
                replay: None,
            },
        );
    }

    fn send(&mut self, frame: &ToHolder) -> Result<(), LinkError> {
        let mut out = Vec::new();
        frame.encode(&mut out);
        self.stream.write_all(&out)?;
        Ok(())
    }

    /// Reads until one whole frame is decoded; `None` on timeout.
    fn read_frame(&mut self, timeout: Duration) -> Result<Option<ToWindow>, LinkError> {
        loop {
            if let Some(frame) = self.decoder.next::<ToWindow>()? {
                return Ok(Some(frame));
            }
            self.stream
                .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
            let mut buf = [0u8; 64 * 1024];
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(LinkError::Closed),
                Ok(n) => self.decoder.push(&buf[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&id)
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids: Vec<_> = self.panes.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Asks `kykod` to start a shell. The pane exists here at once and shows `Exited` or
    /// `SpawnFailed` later if it did not start.
    pub fn spawn(&mut self, spec: SpawnSpec) -> Result<(), LinkError> {
        self.add_pane(&PaneInfo {
            pane: spec.pane,
            program: spec.shell.clone(),
            cwd: spec.cwd.clone(),
            cols: spec.cols,
            rows: spec.rows,
            exited: None,
        });
        self.send(&ToHolder::Spawn(spec))
    }

    /// Asks for the pane's recent output; the emulator is rebuilt from it when `kykod` has sent
    /// it all, and a `Changed` follows.
    pub fn attach(&mut self, pane: PaneId) -> Result<(), LinkError> {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.replay = Some(Vec::new());
        }
        self.send(&ToHolder::Attach { pane })
    }

    pub fn input(&mut self, pane: PaneId, bytes: &[u8]) -> Result<(), LinkError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.send(&ToHolder::Input {
            pane,
            bytes: bytes.to_vec(),
        })
    }

    pub fn resize(&mut self, pane: PaneId, cols: u16, rows: u16) -> Result<(), LinkError> {
        if let Some(p) = self.panes.get_mut(&pane) {
            p.emulator.resize(cols, rows);
        }
        self.send(&ToHolder::Resize { pane, cols, rows })
    }

    pub fn close(&mut self, pane: PaneId) -> Result<(), LinkError> {
        self.panes.remove(&pane);
        self.send(&ToHolder::Close { pane })
    }

    pub fn save_layout(&mut self, blob: Vec<u8>) -> Result<(), LinkError> {
        self.send(&ToHolder::SaveLayout { blob })
    }

    pub fn end_sessions(&mut self, force: bool) -> Result<(), LinkError> {
        self.send(&ToHolder::EndSessions { force })
    }

    /// Waits up to `timeout` for the holder's frames and applies them: output is fed to the
    /// pane's emulator, and what the terminal answers to the program's queries goes back to the
    /// shell. Returns what changed; empty on a quiet timeout.
    pub fn pump(&mut self, timeout: Duration) -> Result<Vec<Event>, LinkError> {
        let mut events = Vec::new();
        let mut wait = timeout;
        while let Some(frame) = self.read_frame(wait)? {
            wait = Duration::from_millis(0);
            if let Some(event) = self.apply(frame)?
                && !(matches!(event, Event::Changed(_)) && events.contains(&event))
            {
                events.push(event);
            }
        }
        Ok(events)
    }

    fn apply(&mut self, frame: ToWindow) -> Result<Option<Event>, LinkError> {
        Ok(match frame {
            ToWindow::Output { pane, bytes } => {
                let Some(p) = self.panes.get_mut(&pane) else {
                    return Ok(None);
                };
                if let Some(held) = &mut p.replay {
                    held.extend_from_slice(&bytes);
                    return Ok(None);
                }
                p.emulator.feed(&bytes);
                let replies = p.emulator.take_replies();
                self.input(pane, &replies)?;
                Some(Event::Changed(pane))
            }
            ToWindow::Replayed { pane, .. } => {
                let Some(p) = self.panes.get_mut(&pane) else {
                    return Ok(None);
                };
                let held = p.replay.take().unwrap_or_default();
                p.emulator.replay(&held);
                Some(Event::Changed(pane))
            }
            ToWindow::Info { pane, program, cwd } => {
                if let Some(p) = self.panes.get_mut(&pane) {
                    p.program.clone_from(&program);
                    p.cwd.clone_from(&cwd);
                }
                Some(Event::Info { pane, program, cwd })
            }
            ToWindow::Exited { pane, status } => {
                if let Some(p) = self.panes.get_mut(&pane) {
                    p.exited = Some(status);
                }
                Some(Event::Exited { pane, status })
            }
            ToWindow::SpawnFailed { pane, message } => {
                self.panes.remove(&pane);
                Some(Event::SpawnFailed { pane, message })
            }
            ToWindow::Busy { panes } => Some(Event::Busy(panes)),
            ToWindow::Hello { .. } => None,
        })
    }
}
