#![forbid(unsafe_code)]
//! The kykod wire protocol: length-prefixed frames, no I/O.
//!
//! A frame is a `u32` little-endian length, then one type byte, then the body;
//! the length counts the type byte and the body. Frames larger than
//! [`MAX_FRAME`] are refused. [`Frame::encode`] does not enforce the limit:
//! senders must keep each frame (for example each `Output` chunk) within it.

mod codec;

use codec::{Reader, framed, put_bool, put_bytes, put_i32, put_str, put_u16, put_u32};

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 1 << 20;

pub type PaneId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub pane: PaneId,
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub ring_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    pub pane: PaneId,
    pub program: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub exited: Option<i32>,
}

/// Window to holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToHolder {
    /// A Hello of any other version is decoded leniently: only `version` is
    /// read and the rest of the body is ignored, so a peer can recognise a
    /// version mismatch (and answer with its own Hello) even though it cannot
    /// parse a newer layout.
    Hello {
        version: u32,
    }, // 1
    Spawn(SpawnSpec), // 2
    Input {
        pane: PaneId,
        bytes: Vec<u8>,
    }, // 3
    Resize {
        pane: PaneId,
        cols: u16,
        rows: u16,
    }, // 4
    Close {
        pane: PaneId,
    }, // 5
    Attach {
        pane: PaneId,
    }, // 6
    SaveLayout {
        blob: Vec<u8>,
    }, // 7
    EndSessions {
        force: bool,
    }, // 8
}

/// Holder to window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToWindow {
    /// A Hello of any other version is decoded leniently: only `version` is
    /// read, `panes` and `layout` come back empty, and the rest of the body is
    /// ignored, so a window can recognise a version mismatch even though it
    /// cannot parse a newer layout.
    Hello {
        version: u32,
        panes: Vec<PaneInfo>,
        layout: Vec<u8>,
    }, // 65
    Output {
        pane: PaneId,
        bytes: Vec<u8>,
    }, // 66
    Info {
        pane: PaneId,
        program: String,
        cwd: String,
    }, // 67
    Exited {
        pane: PaneId,
        status: i32,
    }, // 68
    Busy {
        panes: Vec<PaneId>,
    }, // 69
    SpawnFailed {
        pane: PaneId,
        message: String,
    }, // 70
    Replayed {
        pane: PaneId,
        truncated: bool,
    }, // 71
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    TooLong(usize),
    UnknownType(u8),
    Malformed(&'static str),
}

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong(n) => write!(f, "frame of {n} bytes exceeds the {MAX_FRAME} byte limit"),
            Self::UnknownType(t) => write!(f, "unknown frame type {t}"),
            Self::Malformed(why) => write!(f, "malformed frame: {why}"),
        }
    }
}

impl std::error::Error for ProtoError {}

pub trait Frame: Sized {
    /// Append one whole frame, length prefix included, to `out`.
    fn encode(&self, out: &mut Vec<u8>);
    /// Decode the body of a frame of type `kind`; `body` excludes the type byte.
    fn decode(kind: u8, body: &[u8]) -> Result<Self, ProtoError>;
}

fn put_spec(out: &mut Vec<u8>, s: &SpawnSpec) {
    put_u32(out, s.pane);
    put_str(out, &s.shell);
    put_u32(out, s.args.len() as u32);
    for a in &s.args {
        put_str(out, a);
    }
    put_str(out, &s.cwd);
    put_u32(out, s.env.len() as u32);
    for (k, v) in &s.env {
        put_str(out, k);
        put_str(out, v);
    }
    put_u16(out, s.cols);
    put_u16(out, s.rows);
    put_u32(out, s.ring_bytes);
}

fn get_spec(r: &mut Reader<'_>) -> Result<SpawnSpec, ProtoError> {
    Ok(SpawnSpec {
        pane: r.u32()?,
        shell: r.string()?,
        args: r.list(4, |r| r.string())?,
        cwd: r.string()?,
        env: r.list(8, |r| Ok((r.string()?, r.string()?)))?,
        cols: r.u16()?,
        rows: r.u16()?,
        ring_bytes: r.u32()?,
    })
}

fn put_pane_info(out: &mut Vec<u8>, p: &PaneInfo) {
    put_u32(out, p.pane);
    put_str(out, &p.program);
    put_str(out, &p.cwd);
    put_u16(out, p.cols);
    put_u16(out, p.rows);
    match p.exited {
        None => {
            out.push(0);
            put_i32(out, 0);
        }
        Some(status) => {
            out.push(1);
            put_i32(out, status);
        }
    }
}

fn get_pane_info(r: &mut Reader<'_>) -> Result<PaneInfo, ProtoError> {
    let pane = r.u32()?;
    let program = r.string()?;
    let cwd = r.string()?;
    let cols = r.u16()?;
    let rows = r.u16()?;
    let flag = r.bool()?;
    let status = r.i32()?;
    Ok(PaneInfo {
        pane,
        program,
        cwd,
        cols,
        rows,
        exited: flag.then_some(status),
    })
}

impl Frame for ToHolder {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Hello { version } => framed(out, 1, |o| put_u32(o, *version)),
            Self::Spawn(s) => framed(out, 2, |o| put_spec(o, s)),
            Self::Input { pane, bytes } => framed(out, 3, |o| {
                put_u32(o, *pane);
                put_bytes(o, bytes);
            }),
            Self::Resize { pane, cols, rows } => framed(out, 4, |o| {
                put_u32(o, *pane);
                put_u16(o, *cols);
                put_u16(o, *rows);
            }),
            Self::Close { pane } => framed(out, 5, |o| put_u32(o, *pane)),
            Self::Attach { pane } => framed(out, 6, |o| put_u32(o, *pane)),
            Self::SaveLayout { blob } => framed(out, 7, |o| put_bytes(o, blob)),
            Self::EndSessions { force } => framed(out, 8, |o| put_bool(o, *force)),
        }
    }

    fn decode(kind: u8, body: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(body);
        let f = match kind {
            1 => {
                let version = r.u32()?;
                if version != VERSION {
                    return Ok(Self::Hello { version });
                }
                Self::Hello { version }
            }
            2 => Self::Spawn(get_spec(&mut r)?),
            3 => Self::Input {
                pane: r.u32()?,
                bytes: r.bytes()?.to_vec(),
            },
            4 => Self::Resize {
                pane: r.u32()?,
                cols: r.u16()?,
                rows: r.u16()?,
            },
            5 => Self::Close { pane: r.u32()? },
            6 => Self::Attach { pane: r.u32()? },
            7 => Self::SaveLayout {
                blob: r.bytes()?.to_vec(),
            },
            8 => Self::EndSessions { force: r.bool()? },
            other => return Err(ProtoError::UnknownType(other)),
        };
        r.finish()?;
        Ok(f)
    }
}

impl Frame for ToWindow {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Hello {
                version,
                panes,
                layout,
            } => framed(out, 65, |o| {
                put_u32(o, *version);
                put_u32(o, panes.len() as u32);
                for p in panes {
                    put_pane_info(o, p);
                }
                put_bytes(o, layout);
            }),
            Self::Output { pane, bytes } => framed(out, 66, |o| {
                put_u32(o, *pane);
                put_bytes(o, bytes);
            }),
            Self::Info { pane, program, cwd } => framed(out, 67, |o| {
                put_u32(o, *pane);
                put_str(o, program);
                put_str(o, cwd);
            }),
            Self::Exited { pane, status } => framed(out, 68, |o| {
                put_u32(o, *pane);
                put_i32(o, *status);
            }),
            Self::Busy { panes } => framed(out, 69, |o| {
                put_u32(o, panes.len() as u32);
                for p in panes {
                    put_u32(o, *p);
                }
            }),
            Self::SpawnFailed { pane, message } => framed(out, 70, |o| {
                put_u32(o, *pane);
                put_str(o, message);
            }),
            Self::Replayed { pane, truncated } => framed(out, 71, |o| {
                put_u32(o, *pane);
                put_bool(o, *truncated);
            }),
        }
    }

    fn decode(kind: u8, body: &[u8]) -> Result<Self, ProtoError> {
        let mut r = Reader::new(body);
        let f = match kind {
            65 => {
                let version = r.u32()?;
                if version != VERSION {
                    return Ok(Self::Hello {
                        version,
                        panes: vec![],
                        layout: vec![],
                    });
                }
                Self::Hello {
                    version,
                    // pane: 4 + 4 + 4 + 2 + 2 + 1 + 4
                    panes: r.list(21, get_pane_info)?,
                    layout: r.bytes()?.to_vec(),
                }
            }
            66 => Self::Output {
                pane: r.u32()?,
                bytes: r.bytes()?.to_vec(),
            },
            67 => Self::Info {
                pane: r.u32()?,
                program: r.string()?,
                cwd: r.string()?,
            },
            68 => Self::Exited {
                pane: r.u32()?,
                status: r.i32()?,
            },
            69 => Self::Busy {
                panes: r.list(4, |r| r.u32())?,
            },
            70 => Self::SpawnFailed {
                pane: r.u32()?,
                message: r.string()?,
            },
            71 => Self::Replayed {
                pane: r.u32()?,
                truncated: r.bool()?,
            },
            other => return Err(ProtoError::UnknownType(other)),
        };
        r.finish()?;
        Ok(f)
    }
}

/// Reassembles frames from a byte stream that arrives in arbitrary pieces.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    start: usize,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > 0 && self.start >= self.buf.len() / 2 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole frame, `Ok(None)` if more bytes are needed. An oversized
    /// length is refused as soon as its four bytes are in, before the body is
    /// buffered, and is not consumed (the stream cannot be resynchronised). A
    /// frame that is unknown or malformed is consumed, so the next call moves
    /// on to the frame after it.
    #[allow(clippy::should_implement_trait)] // generic over the frame type, not an Iterator
    pub fn next<T: Frame>(&mut self) -> Result<Option<T>, ProtoError> {
        let avail = &self.buf[self.start..];
        let Some(head) = avail.first_chunk::<4>() else {
            return Ok(None);
        };
        let len = u32::from_le_bytes(*head) as usize;
        if len > MAX_FRAME {
            return Err(ProtoError::TooLong(len));
        }
        if len == 0 {
            self.start += 4;
            return Err(ProtoError::Malformed("empty frame"));
        }
        let Some(frame) = avail.get(4..4 + len) else {
            return Ok(None);
        };
        let result = T::decode(frame[0], &frame[1..]);
        self.start += 4 + len;
        if self.start == self.buf.len() {
            self.buf.clear();
            self.start = 0;
        }
        result.map(Some)
    }
}
