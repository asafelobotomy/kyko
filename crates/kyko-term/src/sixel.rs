//! Sixel images (`ESC P ... q DATA ESC \`), decoded to RGBA for the graphics store. Pixels no
//! sixel paints stay transparent, so the terminal's background shows through them.

/// The widest and tallest image decoded.
const MAX_SIDE: usize = 10_000;
/// The most pixels decoded: 100 MiB of RGBA.
const MAX_PIXELS: usize = 25 << 20;

/// The VT340's sixteen colours, which a sixel image starts with.
const VT340: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (51, 51, 204),
    (204, 33, 33),
    (51, 204, 51),
    (204, 51, 204),
    (51, 204, 204),
    (204, 204, 51),
    (120, 120, 120),
    (69, 69, 69),
    (87, 87, 153),
    (153, 69, 69),
    (87, 153, 87),
    (153, 87, 153),
    (87, 153, 153),
    (153, 153, 87),
    (204, 204, 204),
];

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

impl Canvas {
    /// Room for (x, y); false when that would pass the limits.
    fn reach(&mut self, x: usize, y: usize) -> bool {
        if x < self.w && y < self.h {
            return true;
        }
        if x >= MAX_SIDE || y >= MAX_SIDE {
            return false;
        }
        let w = if x < self.w {
            self.w
        } else {
            (x + 1).max(self.w * 2).min(MAX_SIDE)
        };
        let h = if y < self.h {
            self.h
        } else {
            (y + 1).max(self.h * 2).min(MAX_SIDE)
        };
        if w * h > MAX_PIXELS {
            return false;
        }
        let mut px = vec![[0u8; 4]; w * h];
        for row in 0..self.h {
            px[row * w..row * w + self.w]
                .copy_from_slice(&self.px[row * self.w..(row + 1) * self.w]);
        }
        self.px = px;
        self.w = w;
        self.h = h;
        true
    }
}

/// HLS as sixel gives it (hue 0 is blue, lightness and saturation in percent) to RGB.
fn hls(h: u32, l: u32, s: u32) -> (u8, u8, u8) {
    let h = ((h + 240) % 360) as f32;
    let (l, s) = (l.min(100) as f32 / 100.0, s.min(100) as f32 / 100.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// Reads the numbers after `at`: digits separated by `;`. Returns them and where they end.
fn numbers(data: &[u8], mut at: usize) -> (Vec<u32>, usize) {
    let mut out = vec![0u32];
    while let Some(&b) = data.get(at) {
        match b {
            b'0'..=b'9' => {
                let n = out.last_mut().expect("never empty");
                *n = n.saturating_mul(10).saturating_add(u32::from(b - b'0'));
            }
            b';' if out.len() < 8 => out.push(0),
            b';' => {}
            _ => break,
        }
        at += 1;
    }
    (out, at)
}

/// Decodes the data that follows the `q`: width, height and RGBA pixels, or `None` when nothing
/// was painted or it is too big.
pub fn decode(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut palette = [[0u8; 4]; 256];
    for (i, &(r, g, b)) in VT340.iter().enumerate() {
        palette[i] = [r, g, b, 255];
    }
    let mut canvas = Canvas {
        w: 0,
        h: 0,
        px: Vec::new(),
    };
    let (mut x, mut y, mut color) = (0usize, 0usize, 0usize);
    let (mut right, mut bottom) = (0usize, 0usize);
    let (mut raster_w, mut raster_h) = (0usize, 0usize);
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        match b {
            b'"' => {
                let (n, end) = numbers(data, i + 1);
                raster_w = n.get(2).copied().unwrap_or(0) as usize;
                raster_h = n.get(3).copied().unwrap_or(0) as usize;
                i = end;
                continue;
            }
            b'#' => {
                let (n, end) = numbers(data, i + 1);
                color = n[0] as usize % 256;
                if n.len() >= 5 {
                    let (a, b2, c) = (n[2], n[3], n[4]);
                    let rgb = if n[1] == 1 {
                        hls(a, b2, c)
                    } else {
                        let p = |v: u32| (v.min(100) * 255 / 100) as u8;
                        (p(a), p(b2), p(c))
                    };
                    palette[color] = [rgb.0, rgb.1, rgb.2, 255];
                }
                i = end;
                continue;
            }
            b'!' => {
                let (n, end) = numbers(data, i + 1);
                let count = (n[0].max(1) as usize).min(MAX_SIDE);
                if let Some(&s) = data.get(end)
                    && (0x3f..=0x7e).contains(&s)
                {
                    paint(
                        &mut canvas,
                        &mut x,
                        y,
                        s,
                        count,
                        palette[color],
                        &mut right,
                        &mut bottom,
                    )?;
                    i = end + 1;
                    continue;
                }
                i = end;
                continue;
            }
            b'$' => x = 0,
            b'-' => {
                x = 0;
                y += 6;
                if y >= MAX_SIDE {
                    return None;
                }
            }
            0x3f..=0x7e => paint(
                &mut canvas,
                &mut x,
                y,
                b,
                1,
                palette[color],
                &mut right,
                &mut bottom,
            )?,
            _ => {}
        }
        i += 1;
    }
    // The raster size, when given, is the image's; otherwise what was painted.
    let w = if raster_w > 0 {
        raster_w.min(MAX_SIDE)
    } else {
        right
    };
    let h = if raster_h > 0 {
        raster_h.min(MAX_SIDE)
    } else {
        bottom
    };
    if w == 0 || h == 0 || w * h > MAX_PIXELS {
        return None;
    }
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h.min(canvas.h) {
        for col in 0..w.min(canvas.w) {
            let p = canvas.px[row * canvas.w + col];
            rgba[(row * w + col) * 4..(row * w + col) * 4 + 4].copy_from_slice(&p);
        }
    }
    Some((w as u32, h as u32, rgba))
}

/// Paints one sixel (six pixels, top bit first) `count` times from (x, y), moving x on.
#[allow(clippy::too_many_arguments)]
fn paint(
    canvas: &mut Canvas,
    x: &mut usize,
    y: usize,
    sixel: u8,
    count: usize,
    color: [u8; 4],
    right: &mut usize,
    bottom: &mut usize,
) -> Option<()> {
    let bits = sixel - 0x3f;
    let end = *x + count;
    if bits != 0 {
        if !canvas.reach(end - 1, y + 5) {
            return None;
        }
        for bit in 0..6 {
            if bits & (1 << bit) != 0 {
                let row = (y + bit) * canvas.w;
                canvas.px[row + *x..row + end].fill(color);
                *bottom = (*bottom).max(y + bit + 1);
            }
        }
    }
    *right = (*right).max(end);
    *x = end;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_repeats_and_bands_paint_where_they_should() {
        // A 4x12 image: band one red, the top pixel of band two green, repeated.
        let (w, h, px) = decode(b"\"1;1;4;12#1;2;100;0;0#2;2;0;100;0#1!4~-#2!4@").unwrap();
        assert_eq!((w, h), (4, 12));
        let at = |x: usize, y: usize| &px[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(at(0, 0), [255, 0, 0, 255]);
        assert_eq!(at(3, 5), [255, 0, 0, 255]);
        assert_eq!(at(2, 6), [0, 255, 0, 255]);
        // Not painted: transparent.
        assert_eq!(at(2, 7), [0, 0, 0, 0]);
    }

    #[test]
    fn without_a_raster_the_painted_part_is_the_image_and_hls_reads() {
        let (w, h, px) = decode(b"#5;1;120;50;100#5??~$!2@").unwrap();
        assert_eq!((w, h), (3, 6));
        let at = |x: usize, y: usize| &px[(y * 3 + x) * 4..(y * 3 + x) * 4 + 4];
        // Hue 120 is red in sixel's HLS.
        assert_eq!(at(2, 5), [255, 0, 0, 255]);
        assert_eq!(at(0, 0), [255, 0, 0, 255]);
        assert_eq!(at(0, 1), [0, 0, 0, 0]);
        assert!(decode(b"").is_none());
        assert!(decode(b"!99999999~").is_some());
        assert!(decode(b"-".repeat(2000).as_slice()).is_none());
    }
}
