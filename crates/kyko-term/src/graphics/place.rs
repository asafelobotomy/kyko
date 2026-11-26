//! Placing images on the screen (`a=p`, `a=T`) and deleting them (`a=d`).

use super::*;

impl Store {
    pub(crate) fn place(&mut self, c: &Command, at: At) -> Outcome {
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
            (a, b) => (a.saturating_mul(cw), b.saturating_mul(ch)),
        };
        let cols = if c.cols > 0 {
            c.cols
        } else {
            size.0.saturating_add(c.cell_x).div_ceil(cw)
        }
        .clamp(1, 1000);
        let rows = if c.rows > 0 {
            c.rows
        } else {
            size.1.saturating_add(c.cell_y).div_ceil(ch)
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

    pub(crate) fn delete(&mut self, c: &Command, at: At) {
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
}
