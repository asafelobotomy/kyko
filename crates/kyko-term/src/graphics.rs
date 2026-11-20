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

/// All images kept, in decoded bytes; the oldest go first past it.
const QUOTA: usize = 256 << 20;
/// The largest image side accepted, in pixels.
const MAX_SIDE: u32 = 10_000;
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

/// A file the program named, for `t=f` and `t=t`: a regular file only, never a device or a
/// `/proc` or `/sys` entry; a temporary file must live in a temporary folder and have
/// `tty-graphics-protocol` in its name, and it is removed after reading.
fn read_named(path: &str, temporary: bool, c: &Command) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let p = std::path::Path::new(path);
    if path.starts_with("/proc/") || path.starts_with("/sys/") || path.starts_with("/dev/") {
        return Err("EINVAL:not a regular file".into());
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("EBADF:{e}"))?;
    if !meta.is_file() {
        return Err("EINVAL:not a regular file".into());
    }
    if temporary {
        let dir_ok = ["/tmp/", "/dev/shm/"].iter().any(|d| path.starts_with(d))
            || std::env::var("TMPDIR").is_ok_and(|d| !d.is_empty() && path.starts_with(&d));
        if !dir_ok || !path.contains("tty-graphics-protocol") {
            return Err("EPERM:not a temporary file for the graphics protocol".into());
        }
    }
    let mut f = std::fs::File::open(p).map_err(|e| format!("EBADF:{e}"))?;
    if c.offset_bytes > 0 {
        f.seek(SeekFrom::Start(u64::from(c.offset_bytes)))
            .map_err(|e| format!("EBADF:{e}"))?;
    }
    let limit = if c.size > 0 {
        u64::from(c.size)
    } else {
        QUOTA as u64
    };
    let mut out = Vec::new();
    f.take(limit)
        .read_to_end(&mut out)
        .map_err(|e| format!("EBADF:{e}"))?;
    if temporary {
        let _ = std::fs::remove_file(p);
    }
    Ok(out)
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

    fn place(&mut self, c: &Command, at: At) -> Outcome {
        let id = if c.id != 0 {
            c.id
        } else if c.number != 0 {
            // The newest image with that number.
            match self
                .images
                .values()
                .filter(|i| i.number == c.number)
                .max_by_key(|i| i.generation)
            {
                Some(i) => i.id,
                None => {
                    return Outcome {
                        reply: reply(c, "ENOENT:no such image"),
                        moved: None,
                    };
                }
            }
        } else {
            return Outcome::default();
        };
        let Some(img) = self.images.get(&id) else {
            return Outcome {
                reply: reply(c, "ENOENT:no such image"),
                moved: None,
            };
        };
        let sx = c.x.min(img.width);
        let sy = c.y.min(img.height);
        let sw = if c.w == 0 {
            img.width - sx
        } else {
            c.w.min(img.width - sx)
        };
        let sh = if c.h == 0 {
            img.height - sy
        } else {
            c.h.min(img.height - sy)
        };
        let (cw, ch) = (at.cell_w.max(1), at.cell_h.max(1));
        let (sw1, sh1) = (u64::from(sw.max(1)), u64::from(sh.max(1)));
        // `c` and `r` scale the image into that many cells; given one, the other keeps the
        // image's shape. Neither: the image's own size.
        let size = match (c.cols, c.rows) {
            (0, 0) => (sw, sh),
            (n, 0) => {
                let w = u64::from(n) * u64::from(cw);
                (w as u32, (w * sh1 / sw1) as u32)
            }
            (0, n) => {
                let h = u64::from(n) * u64::from(ch);
                ((h * sw1 / sh1) as u32, h as u32)
            }
            (a, b) => (a * cw, b * ch),
        };
        let cols = if c.cols > 0 {
            c.cols
        } else {
            (size.0 + c.cell_x).div_ceil(cw)
        }
        .clamp(1, 1000);
        let rows = if c.rows > 0 {
            c.rows
        } else {
            (size.1 + c.cell_y).div_ceil(ch)
        }
        .clamp(1, 1000);
        let placement = Placement {
            image: id,
            id: c.placement,
            row: at.row,
            col: at.col,
            cols: cols as u16,
            rows: rows as u16,
            src: (sx, sy, sw, sh),
            offset: (c.cell_x.min(cw - 1), c.cell_y.min(ch - 1)),
            size,
            z: c.z,
            alt: at.alt,
        };
        // The same image and placement id moves the placement instead of adding one.
        if c.placement != 0 {
            self.placements
                .retain(|p| !(p.image == id && p.id == c.placement));
        }
        self.placements.push(placement);
        if self.placements.len() > 4096 {
            self.placements.remove(0);
        }
        Outcome {
            reply: reply(c, "OK"),
            moved: (!c.no_move).then_some((cols as u16, rows as u16)),
        }
    }

    fn delete(&mut self, c: &Command, at: At) {
        let free = c.delete.is_ascii_uppercase();
        match c.delete.to_ascii_lowercase() {
            b'i' => {
                self.placements
                    .retain(|p| !(p.image == c.id && (c.placement == 0 || p.id == c.placement)));
                if free {
                    self.images.remove(&c.id);
                }
            }
            b'n' => {
                let ids: Vec<u32> = self
                    .images
                    .values()
                    .filter(|i| i.number == c.number)
                    .map(|i| i.id)
                    .collect();
                self.placements.retain(|p| !ids.contains(&p.image));
                if free {
                    for id in ids {
                        self.images.remove(&id);
                    }
                }
            }
            b'c' | b'p' => {
                // Placements over the cursor's cell (`c`) or a given cell (`p`, `x`/`y` 1-based).
                let (col, row) = if c.delete.eq_ignore_ascii_case(&b'c') {
                    (u32::from(at.col), at.row)
                } else {
                    (c.x.saturating_sub(1), at.row)
                };
                self.placements.retain(|p| {
                    !(u32::from(p.col) <= col
                        && col < u32::from(p.col) + u32::from(p.cols)
                        && p.row <= row
                        && row < p.row + u64::from(p.rows))
                });
            }
            b'z' => self.placements.retain(|p| p.z != c.z),
            // `a` and anything not handled: every placement.
            _ => self.placements.retain(|p| p.alt != at.alt),
        }
        if free {
            let used: Vec<u32> = self.placements.iter().map(|p| p.image).collect();
            if c.delete == b'A' {
                self.images.retain(|id, _| used.contains(id));
            }
            self.order.retain(|id| self.images.contains_key(id));
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
mod tests {
    use super::*;

    const AT: At = At {
        col: 2,
        row: 10,
        cell_w: 10,
        cell_h: 20,
        alt: false,
    };

    fn b64(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.len();
            let v = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= n {
                    out.push(T[((v >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn transmit_and_display_raw_rgba_moves_the_cursor_and_answers() {
        let mut s = Store::default();
        let px = vec![255u8; 25 * 30 * 4];
        let body = format!("a=T,f=32,s=25,v=30,i=7;{}", b64(&px));
        let out = s.command(body.as_bytes(), AT);
        assert_eq!(out.reply.as_deref(), Some(&b"\x1b_Gi=7;OK\x1b\\"[..]));
        assert_eq!(out.moved, Some((3, 2)), "25x30 px in 10x20 cells");
        let p = &s.placements[0];
        assert_eq!((p.col, p.row, p.cols, p.rows), (2, 10, 3, 2));
        assert_eq!(s.image(7).unwrap().width, 25);
    }

    #[test]
    fn one_cell_size_keeps_the_shape() {
        let mut s = Store::default();
        let px = b64(&[0u8; 20 * 10 * 4]);
        s.command(
            format!("a=T,f=32,s=20,v=10,i=1,c=6,q=2;{px}").as_bytes(),
            AT,
        );
        let p = &s.placements[0];
        assert_eq!((p.cols, p.rows, p.size), (6, 2, (60, 30)));
    }

    #[test]
    fn chunks_gather_then_decode() {
        let mut s = Store::default();
        let px = b64(&[1u8, 2, 3].repeat(4));
        let (a, b) = px.split_at(8);
        assert_eq!(
            s.command(format!("a=t,f=24,s=2,v=2,i=1,q=2,m=1;{a}").as_bytes(), AT),
            Outcome::default()
        );
        assert_eq!(
            s.command(format!("m=0;{b}").as_bytes(), AT),
            Outcome::default()
        );
        assert_eq!(&s.image(1).unwrap().rgba[..4], &[1, 2, 3, 255]);
        assert!(s.placements.is_empty(), "t only stores");
    }

    #[test]
    fn put_query_delete_and_errors() {
        let mut s = Store::default();
        let px = b64(&[9u8; 4]);
        s.command(format!("a=t,f=32,s=1,v=1,i=5;{px}").as_bytes(), AT);
        let out = s.command(b"a=p,i=5,p=2,c=4,r=1,C=1", AT);
        assert_eq!(out.moved, None);
        assert_eq!(out.reply.as_deref(), Some(&b"\x1b_Gi=5,p=2;OK\x1b\\"[..]));
        // Moving placement 2 replaces it.
        s.command(b"a=p,i=5,p=2,q=1", At { col: 0, ..AT });
        assert_eq!(s.placements.len(), 1);
        assert_eq!(s.placements[0].col, 0);
        let q = s.command(format!("a=q,i=31,s=1,v=1,f=32;{px}").as_bytes(), AT);
        assert_eq!(q.reply.as_deref(), Some(&b"\x1b_Gi=31;OK\x1b\\"[..]));
        assert!(s.image(31).is_none(), "a query keeps nothing");
        let miss = s.command(b"a=p,i=99", AT);
        assert!(
            String::from_utf8(miss.reply.unwrap())
                .unwrap()
                .contains("ENOENT")
        );
        let shm = s.command(format!("a=t,t=s,i=3;{}", b64(b"/kyko")).as_bytes(), AT);
        assert!(
            String::from_utf8(shm.reply.unwrap())
                .unwrap()
                .contains("EINVAL")
        );
        s.command(b"a=d,d=I,i=5", AT);
        assert!(s.placements.is_empty());
        assert!(s.image(5).is_none());
    }

    #[test]
    fn files_must_be_regular_and_temporary_ones_are_marked() {
        let mut s = Store::default();
        let dev = s.command(format!("a=t,t=f,i=1;{}", b64(b"/dev/zero")).as_bytes(), AT);
        assert!(
            String::from_utf8(dev.reply.unwrap())
                .unwrap()
                .contains("EINVAL")
        );
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("x.rgba");
        std::fs::write(&plain, [7u8; 4]).unwrap();
        let path = plain.to_str().unwrap();
        let t = s.command(
            format!("a=t,t=t,f=32,s=1,v=1,i=2;{}", b64(path.as_bytes())).as_bytes(),
            AT,
        );
        assert!(
            String::from_utf8(t.reply.unwrap())
                .unwrap()
                .contains("EPERM")
        );
        let f = s.command(
            format!("a=t,t=f,f=32,s=1,v=1,i=2;{}", b64(path.as_bytes())).as_bytes(),
            AT,
        );
        assert_eq!(f.reply.as_deref(), Some(&b"\x1b_Gi=2;OK\x1b\\"[..]));
        assert!(plain.exists(), "a named file is left alone");
    }

    #[test]
    fn png_decodes() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 1);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 0, 255, 0]).unwrap();
        }
        let c = Command::parse("f=100");
        let (w, h, rgba) = decode(&c, png_bytes).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, [255, 0, 0, 255, 0, 255, 0, 255]);
    }
}
