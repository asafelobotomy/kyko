//! What the grid item draws: each pane's snapshot, its images, the sticky header and its text.

use super::*;

pub(crate) fn kyko_images(pane: u32, out: &mut Vec<ffi::FfiImage>) {
    out.clear();
    let Some(w) = WORKSPACE.get() else { return };
    let shared = w.shared.lock().unwrap();
    let Some(snap) = shared.frames.get(&pane).and_then(|f| f.snapshot.as_ref()) else {
        return;
    };
    out.extend(snap.images.iter().map(|v| ffi::FfiImage {
        image: v.image,
        generation: v.generation,
        width: v.width,
        height: v.height,
        sx: v.src.0,
        sy: v.src.1,
        sw: v.src.2,
        sh: v.src.3,
        col: v.col,
        row: v.row,
        ox: v.offset.0,
        oy: v.offset.1,
        dw: v.size.0,
        dh: v.size.1,
        z: v.z,
    }));
}

/// An image's RGBA pixels, once, for the window's texture cache.
pub(crate) fn kyko_image_pixels(pane: u32, image: u32, generation: u64, out: &mut Vec<u8>) -> bool {
    out.clear();
    let Some(w) = WORKSPACE.get() else {
        return false;
    };
    let rgba = {
        let shared = w.shared.lock().unwrap();
        let Some(snap) = shared.frames.get(&pane).and_then(|f| f.snapshot.as_ref()) else {
            return false;
        };
        match snap
            .images
            .iter()
            .find(|v| v.image == image && v.generation == generation)
        {
            Some(v) => v.rgba.clone(),
            None => return false,
        }
    };
    out.extend_from_slice(&rgba);
    true
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn kyko_snapshot(
    pane: u32,
    out: &mut Vec<ffi::FfiCell>,
    extras: &mut Vec<ffi::FfiExtra>,
    gutter: &mut Vec<u8>,
    cols: &mut u16,
    rows: &mut u16,
    cur_x: &mut i32,
    cur_y: &mut i32,
) -> u64 {
    out.clear();
    extras.clear();
    gutter.clear();
    *cols = 0;
    *rows = 0;
    let Some(w) = WORKSPACE.get() else { return 0 };
    let shared = w.shared.lock().unwrap();
    let Some(snap) = shared.frames.get(&pane).and_then(|f| f.snapshot.as_ref()) else {
        return 0;
    };
    *cols = snap.cols;
    *rows = snap.rows;
    (*cur_x, *cur_y) = snap
        .cursor
        .map_or((-1, -1), |(x, y)| (i32::from(x), i32::from(y)));
    gutter.extend_from_slice(&snap.gutter);
    out.extend(snap.cells.iter().map(|c| ffi::FfiCell {
        ch: c.ch as u32,
        fg: c.fg,
        bg: c.bg,
        flags: c.flags,
        underline: c.underline,
        underline_color: c.underline_color.map_or(0, |c| 0x0100_0000 | c),
    }));
    extras.extend(snap.combining.iter().map(|(index, text)| ffi::FfiExtra {
        index: *index,
        text: text.clone(),
    }));
    if let Some((_, hints)) = shared.hints.as_ref().filter(|(p, _)| *p == pane) {
        let l = look();
        let cols = usize::from(snap.cols);
        let shown = hints.shown();
        // Underline the links still in play, then write each one's label over its start.
        for h in &hints.items {
            if !h.label.starts_with(&hints.typed) {
                continue;
            }
            for &(r, a, b) in &h.spans {
                for c in a..b {
                    if let Some(cell) = out.get_mut(usize::from(r) * cols + usize::from(c)) {
                        cell.underline = kyko_term::UL_SINGLE;
                        cell.underline_color = 0x0100_0000 | l.accent;
                    }
                }
            }
        }
        for (r, c, label) in shown {
            for (k, ch) in label.chars().enumerate() {
                if let Some(cell) = out.get_mut(usize::from(r) * cols + usize::from(c) + k) {
                    cell.ch = ch as u32;
                    cell.fg = l.colors.bg;
                    cell.bg = l.accent;
                    cell.flags = kyko_term::BOLD;
                    cell.underline = kyko_term::UL_NONE;
                }
                extras.retain(|e| e.index as usize != usize::from(r) * cols + usize::from(c) + k);
            }
        }
    }
    1
}

/// The command pinned at the top of `pane`, or "" for none.
pub(crate) fn kyko_sticky_header(pane: u32) -> String {
    WORKSPACE
        .get()
        .and_then(|w| {
            w.shared
                .lock()
                .unwrap()
                .frames
                .get(&pane)
                .and_then(|f| f.header.clone())
        })
        .unwrap_or_default()
}

pub(crate) fn kyko_text(pane: u32, text: &str) {
    if let Some(w) = WORKSPACE.get() {
        w.typed(pane, text.as_bytes().to_vec());
    }
}
