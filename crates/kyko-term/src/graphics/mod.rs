//! kitty's graphics protocol: `ESC _ G <keys> ; <base64 payload> ESC \`. Programs send images
//! (PNG, or raw RGB/RGBA, optionally zlib-compressed) directly, in chunks, or by naming a file,
//! and place them at the cursor, sized in cells. The scanner finds the sequences; this module
//! decodes them, keeps the images and their placements, and says what to answer.
//!
//! Supported: actions `t`, `T`, `p`, `q`, `d`; formats 24, 32, 100; media `d` (direct), `f`
//! (file) and `t` (temporary file); `o=z`; chunking with `m`; `c`/`r` sizes, source rectangles,
//! cell offsets, `z`-index, `C=1` and the quiet levels. Not supported: shared memory (`t=s`,
//! answered with an error so programs fall back to direct), animation and unicode placeholders.

use std::collections::BTreeMap;
use std::sync::Arc;

mod files;
mod place;

use files::*;

/// All images kept, in decoded bytes; the oldest go first past it.
const QUOTA: usize = 256 << 20;
/// The largest image side accepted, in pixels.
const MAX_SIDE: u32 = 10_000;
/// The most pixels one image may have (100 MiB as RGBA), however its sides are split.
const MAX_PIXELS: u64 = 25 << 20;
/// The longest payload gathered from chunks for one image (base64 text).
const MAX_PAYLOAD: usize = 64 << 20;

#[derive(Clone, Debug)]
pub struct Image {
    pub id: u32,
    pub number: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
    /// Bumped when an id is reused, so a window's cached texture is not taken for the new image.
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub image: u32,
    pub id: u32,
    /// The row the top-left cell is on, numbered as the emulator numbers rows.
    pub row: u64,
    pub col: u16,
    pub cols: u16,
    pub rows: u16,
    /// Source rectangle in the image, in pixels.
    pub src: (u32, u32, u32, u32),
    /// Offset inside the first cell, in pixels.
    pub offset: (u32, u32),
    /// The drawn size in pixels: the source's own, or scaled to the cells asked for.
    pub size: (u32, u32),
    pub z: i32,
    /// Placed while the alternate screen was showing.
    pub alt: bool,
}

/// One command's keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Command {
    pub action: u8,
    pub format: u32,
    pub medium: u8,
    pub width: u32,
    pub height: u32,
    pub size: u32,
    pub offset_bytes: u32,
    pub id: u32,
    pub number: u32,
    pub placement: u32,
    pub cols: u32,
    pub rows: u32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub cell_x: u32,
    pub cell_y: u32,
    pub no_move: bool,
    pub z: i32,
    pub more: bool,
    pub quiet: u8,
    pub zlib: bool,
    pub delete: u8,
}

impl Command {
    /// `a=T,f=100,i=3` and so on; unknown keys are ignored.
    pub fn parse(control: &str) -> Command {
        let mut c = Command {
            action: b't',
            format: 32,
            medium: b'd',
            delete: b'a',
            ..Command::default()
        };
        for kv in control.split(',') {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            let n = || v.parse::<u32>().unwrap_or(0);
            let ch = || v.bytes().next().unwrap_or(0);
            match k {
                "a" => c.action = ch(),
                "f" => c.format = n(),
                "t" => c.medium = ch(),
                "s" => c.width = n(),
                "v" => c.height = n(),
                "S" => c.size = n(),
                "O" => c.offset_bytes = n(),
                "i" => c.id = n(),
                "I" => c.number = n(),
                "p" => c.placement = n(),
                "c" => c.cols = n(),
                "r" => c.rows = n(),
                "x" => c.x = n(),
                "y" => c.y = n(),
                "w" => c.w = n(),
                "h" => c.h = n(),
                "X" => c.cell_x = n(),
                "Y" => c.cell_y = n(),
                "C" => c.no_move = v == "1",
                "z" => c.z = v.parse().unwrap_or(0),
                "m" => c.more = v == "1",
                "q" => c.quiet = v.parse().unwrap_or(0),
                "o" => c.zlib = v == "z",
                "d" => c.delete = ch(),
                _ => {}
            }
        }
        c
    }
}

/// Where the command arrived: the cursor and the cell size.
#[derive(Clone, Copy, Debug)]
pub struct At {
    pub col: u16,
    pub row: u64,
    pub cell_w: u32,
    pub cell_h: u32,
    pub alt: bool,
}

/// What the emulator does after a command: answer the program, and move the cursor past a
/// placed image (columns right, rows down).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub reply: Option<Vec<u8>>,
    pub moved: Option<(u16, u16)>,
}

#[derive(Default)]
pub struct Store {
    images: BTreeMap<u32, Image>,
    pub placements: Vec<Placement>,
    /// The first chunk's command while later chunks arrive, and the payload so far.
    loading: Option<(Command, Vec<u8>)>,
    next_id: u32,
    generation: u64,
    /// Ids in the order they were added, oldest first, for the quota.
    order: Vec<u32>,
}

fn reply(c: &Command, message: &str) -> Option<Vec<u8>> {
    let ok = message == "OK";
    if (c.id == 0 && c.number == 0) || (ok && c.quiet >= 1) || c.quiet >= 2 {
        return None;
    }
    let mut keys = Vec::new();
    if c.id != 0 {
        keys.push(format!("i={}", c.id));
    }
    if c.number != 0 {
        keys.push(format!("I={}", c.number));
    }
    if c.placement != 0 {
        keys.push(format!("p={}", c.placement));
    }
    Some(format!("\x1b_G{};{message}\x1b\\", keys.join(",")).into_bytes())
}

/// The pixels of an image as RGBA, from the bytes the program sent.
pub fn decode(c: &Command, data: Vec<u8>) -> Result<(u32, u32, Vec<u8>), String> {
    let data = if c.zlib {
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&data, QUOTA)
            .map_err(|_| "EINVAL:bad zlib data".to_string())?
    } else {
        data
    };
    match c.format {
        100 => {
            let mut dec = png::Decoder::new(std::io::Cursor::new(&data));
            dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
            let mut reader = dec.read_info().map_err(|e| format!("EINVAL:{e}"))?;
            let (w, h) = (reader.info().width, reader.info().height);
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                return Err("EINVAL:image too large".into());
            }
            // Checked before the buffer is made: a few bytes of PNG can claim 10000x10000.
            if u64::from(w) * u64::from(h) > MAX_PIXELS {
                return Err("EINVAL:image too large".into());
            }
            let mut buf = vec![0; reader.output_buffer_size()];
            let frame = reader
                .next_frame(&mut buf)
                .map_err(|e| format!("EINVAL:{e}"))?;
            buf.truncate(frame.buffer_size());
            let rgba = match frame.color_type {
                png::ColorType::Rgba => buf,
                png::ColorType::Rgb => buf
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|&[r, g, b]| [r, g, b, 255])
                    .collect(),
                png::ColorType::GrayscaleAlpha => buf
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|&[g, a]| [g, g, g, a])
                    .collect(),
                png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
                png::ColorType::Indexed => return Err("EINVAL:indexed PNG".into()),
            };
            Ok((w, h, rgba))
        }
        24 | 32 => {
            let (w, h) = (c.width, c.height);
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                return Err("EINVAL:bad size".into());
            }
            if u64::from(w) * u64::from(h) > MAX_PIXELS {
                return Err("EINVAL:image too large".into());
            }
            let px = if c.format == 24 { 3 } else { 4 };
            let need = (w as usize) * (h as usize) * px;
            if data.len() < need {
                return Err("ENODATA:too little data for the size".into());
            }
            let rgba = if px == 4 {
                data[..need].to_vec()
            } else {
                data[..need]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|&[r, g, b]| [r, g, b, 255])
                    .collect()
            };
            Ok((w, h, rgba))
        }
        _ => Err("EINVAL:unknown format".into()),
    }
}

impl Store {
    /// The image with this id, if any.
    pub fn image(&self, id: u32) -> Option<&Image> {
        self.images.get(&id)
    }

    /// Every image's decoded bytes.
    pub fn bytes(&self) -> usize {
        self.images.values().map(|i| i.rgba.len()).sum()
    }

    /// One `ESC _ G ... ESC \` body (after the `G`).
    pub fn command(&mut self, body: &[u8], at: At) -> Outcome {
        let text = String::from_utf8_lossy(body);
        let (control, payload) = text.split_once(';').unwrap_or((&text, ""));
        let c = Command::parse(control);
        // A later chunk carries only `m` (and maybe `q`): it belongs to the image being loaded.
        if let Some((first, mut data)) = self.loading.take() {
            if data.len() + payload.len() > MAX_PAYLOAD {
                return Outcome {
                    reply: reply(&first, "EFBIG:image too large"),
                    moved: None,
                };
            }
            data.extend_from_slice(payload.trim().as_bytes());
            if c.more {
                self.loading = Some((first, data));
                return Outcome::default();
            }
            return self.finish(first, data, at);
        }
        match c.action {
            b't' | b'T' | b'q' => {
                if c.more && c.medium == b'd' {
                    self.loading = Some((c, payload.trim().as_bytes().to_vec()));
                    return Outcome::default();
                }
                self.finish(c, payload.trim().as_bytes().to_vec(), at)
            }
            b'p' => self.place(&c, at),
            b'd' => {
                self.delete(&c, at);
                Outcome::default()
            }
            _ => Outcome {
                reply: reply(&c, "EINVAL:unsupported action"),
                moved: None,
            },
        }
    }

    /// The whole payload is here: decode it, keep it (not for a query), place it for `T`.
    fn finish(&mut self, mut c: Command, b64: Vec<u8>, at: At) -> Outcome {
        let raw = match crate::marks::base64_bytes(&b64) {
            Some(r) => r,
            None => {
                return Outcome {
                    reply: reply(&c, "EINVAL:bad base64"),
                    moved: None,
                };
            }
        };
        let data = match c.medium {
            b'd' => Ok(raw),
            b'f' | b't' => match String::from_utf8(raw) {
                Ok(path) => read_named(&path, c.medium == b't', &c),
                Err(_) => Err("EINVAL:bad file name".into()),
            },
            b's' => Err("EINVAL:shared memory is not supported".into()),
            _ => Err("EINVAL:unknown medium".into()),
        };
        let decoded = data.and_then(|d| decode(&c, d));
        let (w, h, rgba) = match decoded {
            Ok(x) => x,
            Err(e) => {
                return Outcome {
                    reply: reply(&c, &e),
                    moved: None,
                };
            }
        };
        if c.action == b'q' {
            return Outcome {
                reply: reply(&c, "OK"),
                moved: None,
            };
        }
        if c.id == 0 {
            // `I=` alone, or nothing: kyko picks an id from the top of the range.
            self.next_id = self.next_id.wrapping_add(1);
            c.id = 0x8000_0000 | (self.next_id & 0x7fff_ffff);
        }
        self.generation += 1;
        self.images.insert(
            c.id,
            Image {
                id: c.id,
                number: c.number,
                width: w,
                height: h,
                rgba: Arc::new(rgba),
                generation: self.generation,
            },
        );
        self.order.retain(|i| *i != c.id);
        self.order.push(c.id);
        // A new image under an old id replaces it, and its placements go with the old pixels.
        self.placements.retain(|p| p.image != c.id);
        self.evict();
        let shown_id = c.id;
        let mut out = if c.action == b'T' {
            self.place(&c, at)
        } else {
            Outcome::default()
        };
        if out.reply.is_none() {
            // Only answer with the id the program gave.
            let mut answer = c.clone();
            if shown_id & 0x8000_0000 != 0 && c.number == 0 {
                answer.id = 0;
            }
            out.reply = reply(&answer, "OK");
        }
        out
    }

    /// Adds decoded pixels (a sixel image) and places them at their own size at `at`; the
    /// cells it covers, columns and rows.
    pub fn show(&mut self, w: u32, h: u32, rgba: Vec<u8>, at: At) -> Option<(u16, u16)> {
        self.next_id = self.next_id.wrapping_add(1);
        let id = 0x8000_0000 | (self.next_id & 0x7fff_ffff);
        self.generation += 1;
        self.images.insert(
            id,
            Image {
                id,
                number: 0,
                width: w,
                height: h,
                rgba: Arc::new(rgba),
                generation: self.generation,
            },
        );
        self.order.push(id);
        self.evict();
        let c = Command {
            action: b'p',
            id,
            quiet: 2,
            ..Command::parse("")
        };
        self.place(&c, at).moved
    }

    fn evict(&mut self) {
        while self.bytes() > QUOTA && self.order.len() > 1 {
            let old = self.order.remove(0);
            self.images.remove(&old);
            self.placements.retain(|p| p.image != old);
        }
    }

    /// The alternate screen went away: its images go with it.
    pub fn leave_alt(&mut self) {
        self.placements.retain(|p| !p.alt);
    }

    /// Forget everything (a reset).
    pub fn clear(&mut self) {
        *self = Store::default();
    }
}

#[cfg(test)]
mod tests;
