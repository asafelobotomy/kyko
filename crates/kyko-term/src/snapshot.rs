//! The screen as the window draws it: [`Emulator::snapshot`] and the links found in one.

use super::*;

impl Snapshot {
    /// The link at a cell: an OSC 8 hyperlink, else a URL in the text, as the URL and the
    /// cells it covers on that row (`row, start..end`). A URL that runs to the end of the row
    /// goes on into the next one, as a wrapped line does.
    pub fn link_at(&self, col: u16, row: u16) -> Option<(String, LinkSpans)> {
        let cols = usize::from(self.cols);
        let (col, row) = (usize::from(col), usize::from(row));
        if col >= cols || row >= usize::from(self.rows) {
            return None;
        }
        let index = (row * cols + col) as u32;
        if let Some(&(_, n)) = self.links.iter().find(|(i, _)| *i == index) {
            let uri = self.link_uris.get(n as usize)?.clone();
            // The run of cells with the same link, on this row.
            let same = |c: usize| {
                self.links
                    .iter()
                    .any(|&(i, m)| i == (row * cols + c) as u32 && m == n)
            };
            let mut a = col;
            while a > 0 && same(a - 1) {
                a -= 1;
            }
            let mut b = col + 1;
            while b < cols && same(b) {
                b += 1;
            }
            return Some((uri, vec![(row as u16, a as u16, b as u16)]));
        }
        // Text: this row, with the rows before and after joined on when the URL reaches the
        // edge, so a wrapped URL is found whole.
        let text_row = |r: usize| -> Vec<char> {
            self.cells[r * cols..(r + 1) * cols]
                .iter()
                .map(|c| c.ch)
                .collect()
        };
        let mut first = row;
        while first > 0
            && self.cells[first * cols - 1].ch != ' '
            && self.cells[first * cols].ch != ' '
        {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < usize::from(self.rows) && self.cells[(last + 1) * cols - 1].ch != ' ' {
            last += 1;
        }
        let mut line = Vec::new();
        for r in first..=last {
            line.extend(text_row(r));
        }
        let at = (row - first) * cols + col;
        let (a, b) = links::url_at(&line, at)?;
        let url: String = line[a..b].iter().collect();
        let mut spans = Vec::new();
        let mut i = a;
        while i < b {
            let r = i / cols;
            let end = ((r + 1) * cols).min(b);
            spans.push((
                (first + r) as u16,
                (i % cols) as u16,
                (end - r * cols) as u16,
            ));
            i = end;
        }
        Some((url, spans))
    }

    /// The file named at a cell (see [`links::path_at`]), and the cells it covers on that row.
    /// Whether the file exists is for the caller, which knows the shell's folder.
    pub fn path_at(&self, col: u16, row: u16) -> Option<(links::FileTarget, LinkSpans)> {
        let cols = usize::from(self.cols);
        let (col, row) = (usize::from(col), usize::from(row));
        if col >= cols || row >= usize::from(self.rows) {
            return None;
        }
        let line: Vec<char> = self.cells[row * cols..(row + 1) * cols]
            .iter()
            .map(|c| c.ch)
            .collect();
        let (a, b, t) = links::path_at(&line, col)?;
        Some((t, vec![(row as u16, a as u16, b as u16)]))
    }

    /// Every openable link on screen, top to bottom, each once.
    pub fn all_links(&self) -> Vec<(String, LinkSpans)> {
        let mut found: Vec<(String, LinkSpans)> = Vec::new();
        let mut covered = vec![false; self.cells.len()];
        let cols = usize::from(self.cols);
        for row in 0..self.rows {
            for col in 0..self.cols {
                let index = usize::from(row) * cols + usize::from(col);
                if covered[index] || self.cells[index].ch == ' ' {
                    continue;
                }
                let Some((url, spans)) = self.link_at(col, row) else {
                    continue;
                };
                for &(r, a, b) in &spans {
                    for c in a..b {
                        covered[usize::from(r) * cols + usize::from(c)] = true;
                    }
                }
                if links::openable(&url) {
                    found.push((url, spans));
                }
            }
        }
        found
    }
}

impl Emulator {
    pub fn snapshot(&self, colors: &Colors) -> Snapshot {
        self.set_theme(colors);
        let term = &self.term;
        let cols = term.columns();
        let rows = term.screen_lines();
        let blank = Cell::plain(' ', colors.fg, colors.bg, 0);
        let mut combining = Vec::new();
        let mut links: Vec<(u32, u32)> = Vec::new();
        let mut link_uris: Vec<String> = Vec::new();
        let mut cells = vec![blank; cols * rows];
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let selection = content.selection;
        for item in content.display_iter {
            let row = item.point.line.0 + offset;
            if row < 0 || row as usize >= rows || item.point.column.0 >= cols {
                continue;
            }
            let f = item.cell.flags;
            let mut flags = 0u8;
            for (bit, flag) in [
                (BOLD, Flags::BOLD),
                (ITALIC, Flags::ITALIC),
                (UNDERLINE, Flags::ALL_UNDERLINES),
                (INVERSE, Flags::INVERSE),
                (WIDE, Flags::WIDE_CHAR),
                (WIDE_SPACER, Flags::WIDE_CHAR_SPACER),
                (STRIKE, Flags::STRIKEOUT),
                (DIM, Flags::DIM),
            ] {
                if f.intersects(flag) {
                    flags |= bit;
                }
            }
            let mut fg = resolve(item.cell.fg, content.colors, colors);
            let mut bg = resolve(item.cell.bg, content.colors, colors);
            // A selected cell is drawn the other way round, an inverse cell back again.
            let selected = selection.is_some_and(|r| r.contains(item.point));
            if (flags & INVERSE != 0) != selected {
                std::mem::swap(&mut fg, &mut bg);
            }
            let ch = if f.contains(Flags::HIDDEN) || item.cell.c == '\t' {
                ' '
            } else {
                item.cell.c
            };
            let underline = if f.contains(Flags::UNDERCURL) {
                UL_CURLY
            } else if f.contains(Flags::DOUBLE_UNDERLINE) {
                UL_DOUBLE
            } else if f.contains(Flags::DOTTED_UNDERLINE) {
                UL_DOTTED
            } else if f.contains(Flags::DASHED_UNDERLINE) {
                UL_DASHED
            } else if f.contains(Flags::UNDERLINE) {
                UL_SINGLE
            } else {
                UL_NONE
            };
            let underline_color = item
                .cell
                .underline_color()
                .map(|c| resolve(c, content.colors, colors));
            let index = row as usize * cols + item.point.column.0;
            if ch != ' '
                && let Some(zw) = item.cell.zerowidth()
            {
                combining.push((index as u32, zw.iter().collect()));
            }
            if let Some(h) = item.cell.hyperlink() {
                let uri = h.uri();
                let n = match link_uris.iter().rposition(|u| u == uri) {
                    Some(n) => n,
                    None => {
                        link_uris.push(uri.to_string());
                        link_uris.len() - 1
                    }
                };
                links.push((index as u32, n as u32));
            }
            cells[index] = Cell {
                ch,
                fg,
                bg,
                flags,
                underline,
                underline_color,
            };
        }
        let cursor = (content.cursor.shape != CursorShape::Hidden)
            .then(|| {
                (
                    content.cursor.point.column.0 as u16,
                    (content.cursor.point.line.0 + offset) as u16,
                )
            })
            .filter(|&(x, y)| usize::from(x) < cols && usize::from(y) < rows);
        Snapshot {
            cols: cols as u16,
            rows: rows as u16,
            cells,
            gutter: self.gutter(),
            cursor,
            combining,
            links,
            link_uris,
            images: self.image_views(offset, rows),
        }
    }

    /// The placements in view, below-text ones first.
    pub(crate) fn image_views(&self, offset: i32, rows: usize) -> Vec<ImageView> {
        let alt = self.in_alt_screen();
        let mut out: Vec<ImageView> = self
            .graphics
            .placements
            .iter()
            .filter(|p| p.alt == alt)
            .filter_map(|p| {
                let line = if alt {
                    p.row as i32
                } else {
                    self.line_of(p.row)
                };
                let row = line + offset;
                if row + i32::from(p.rows) <= 0 || row >= rows as i32 {
                    return None;
                }
                let img = self.graphics.image(p.image)?;
                Some(ImageView {
                    image: img.id,
                    generation: img.generation,
                    width: img.width,
                    height: img.height,
                    rgba: img.rgba.clone(),
                    src: p.src,
                    col: i32::from(p.col),
                    row,
                    cols: p.cols,
                    rows: p.rows,
                    offset: p.offset,
                    size: p.size,
                    z: p.z,
                })
            })
            .collect();
        out.sort_by_key(|v| v.z);
        out
    }
}
