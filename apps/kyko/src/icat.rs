//! `kyko icat`: shows images in the terminal with kitty's graphics protocol. PNGs go as they
//! are; other formats are turned into PNG by ImageMagick (`magick`) when it is installed. An
//! image wider than the terminal is shrunk to fit its width.
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::{Duration, Instant};

/// The base64 carried by each escape; the protocol's limit.
const CHUNK: usize = 4096;

/// Shows each file in turn; the status is 1 when any could not be shown.
pub fn run(files: &[std::path::PathBuf]) -> std::process::ExitCode {
    let window = Window::ask();
    let mut out = std::io::stdout().lock();
    let mut failed = false;
    for file in files {
        match png_of(file) {
            Ok(png) => {
                let size = png_size(&png);
                let fit = match (size, window) {
                    (Some((w, _)), Some(win)) if win.width > 0 && w > win.width => Some(win.cols),
                    _ => None,
                };
                let _ = out.write_all(&escapes(&png, fit));
                let _ = out.write_all(b"\n");
            }
            Err(e) => {
                eprintln!("kyko icat: {}: {e}", file.display());
                failed = true;
            }
        }
    }
    let _ = out.flush();
    std::process::ExitCode::from(u8::from(failed))
}

/// The file as PNG bytes.
fn png_of(file: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(bytes);
    }
    // The first frame only, of an animation.
    let mut arg = file.as_os_str().to_owned();
    arg.push("[0]");
    let out = std::process::Command::new("magick")
        .arg(arg)
        .arg("png:-")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|_| {
            "not a PNG, and ImageMagick (magick) is not installed to convert it".to_string()
        })?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err("not an image ImageMagick can read".into());
    }
    Ok(out.stdout)
}

/// Width and height from the PNG's header.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let w = png.get(16..20)?;
    let h = png.get(20..24)?;
    Some((
        u32::from_be_bytes(w.try_into().ok()?),
        u32::from_be_bytes(h.try_into().ok()?),
    ))
}

/// The escapes that send and place `png`, `cols` wide when given (the height keeps the shape).
fn escapes(png: &[u8], cols: Option<u16>) -> Vec<u8> {
    let data = base64(png);
    let mut out = Vec::with_capacity(data.len() + data.len() / CHUNK * 16 + 64);
    let pieces: Vec<&[u8]> = data.chunks(CHUNK).collect();
    for (i, piece) in pieces.iter().enumerate() {
        let more = u8::from(i + 1 < pieces.len());
        out.extend_from_slice(b"\x1b_G");
        if i == 0 {
            out.extend_from_slice(b"a=T,f=100,q=2,");
            if let Some(c) = cols {
                out.extend_from_slice(format!("c={c},").as_bytes());
            }
        }
        out.extend_from_slice(format!("m={more};").as_bytes());
        out.extend_from_slice(piece);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

fn base64(data: &[u8]) -> Vec<u8> {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(ABC[(n >> 18) as usize & 63]);
        out.push(ABC[(n >> 12) as usize & 63]);
        out.push(if c.len() > 1 {
            ABC[(n >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if c.len() > 2 {
            ABC[n as usize & 63]
        } else {
            b'='
        });
    }
    out
}

/// The terminal's text area: columns, and width in pixels (0 when it would not say).
#[derive(Clone, Copy, Debug)]
struct Window {
    cols: u16,
    width: u32,
}

impl Window {
    /// Asks the terminal on `/dev/tty`: the columns from the kernel, the pixels with `CSI 14 t`.
    fn ask() -> Option<Window> {
        let mut tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()?;
        let fd = tty.as_raw_fd();
        // SAFETY: plain ioctl/termios calls on a descriptor this function owns.
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } != 0 || ws.ws_col == 0 {
            return None;
        }
        let mut width = u32::from(ws.ws_xpixel);
        if width == 0 {
            width = ask_pixels(&mut tty).unwrap_or(0);
        }
        Some(Window {
            cols: ws.ws_col,
            width,
        })
    }
}

/// The text area's width in pixels, from the reply to `CSI 14 t` (`CSI 4 ; height ; width t`).
fn ask_pixels(tty: &mut std::fs::File) -> Option<u32> {
    let fd = tty.as_raw_fd();
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: as above; the old settings are put back before returning.
    if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
        return None;
    }
    let mut raw = saved;
    raw.c_lflag &= !(libc::ICANON | libc::ECHO);
    raw.c_cc[libc::VMIN] = 0;
    raw.c_cc[libc::VTIME] = 1;
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) };
    let mut reply = Vec::new();
    if tty.write_all(b"\x1b[14t").is_ok() {
        let until = Instant::now() + Duration::from_millis(500);
        let mut buf = [0u8; 64];
        while Instant::now() < until && !reply.ends_with(b"t") {
            match tty.read(&mut buf) {
                Ok(n) => reply.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
    }
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };
    parse_pixels(&reply)
}

fn parse_pixels(reply: &[u8]) -> Option<u32> {
    let s = std::str::from_utf8(reply).ok()?;
    let body = s.rsplit("\x1b[4;").next()?.strip_suffix('t')?;
    let (_, w) = body.split_once(';')?;
    w.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_goes_in_chunks_with_its_size_asked_only_when_it_must_fit() {
        let png: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
        let out = escapes(&png, None);
        let text = String::from_utf8(out).unwrap();
        let pieces: Vec<&str> = text.split("\x1b\\").filter(|p| !p.is_empty()).collect();
        assert_eq!(pieces.len(), 4); // 13336 base64 bytes
        assert!(pieces[0].starts_with("\x1b_Ga=T,f=100,q=2,m=1;"));
        assert!(pieces[1].starts_with("\x1b_Gm=1;"));
        assert!(pieces[3].starts_with("\x1b_Gm=0;"));
        let fit = String::from_utf8(escapes(b"x", Some(80))).unwrap();
        assert_eq!(fit, "\x1b_Ga=T,f=100,q=2,c=80,m=0;eA==\x1b\\");
    }

    #[test]
    fn base64_and_the_header_and_the_reply_read_right() {
        assert_eq!(base64(b"Man"), b"TWFu");
        assert_eq!(base64(b"Ma"), b"TWE=");
        assert_eq!(base64(b"M"), b"TQ==");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&240u32.to_be_bytes());
        png.extend_from_slice(&160u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((240, 160)));
        assert_eq!(parse_pixels(b"\x1b[4;660;1000t"), Some(1000));
        assert_eq!(parse_pixels(b"junk"), None);
    }
}
