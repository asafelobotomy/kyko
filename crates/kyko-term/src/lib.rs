#![forbid(unsafe_code)]
//! A terminal emulator fed by bytes. It never touches a PTY: output from a shell goes in
//! (`feed`), a grid snapshot comes out (`snapshot`), and what the terminal must answer to the
//! shell (cursor position reports and the like) is collected by `take_replies`.
mod colors;

use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};

pub use colors::{Colors, resolve};

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const UNDERLINE: u8 = 4;
pub const INVERSE: u8 = 8;
pub const WIDE: u8 = 16;
pub const WIDE_SPACER: u8 = 32;
pub const STRIKE: u8 = 64;

/// One grid cell. Colours are 0xRRGGBB, already swapped with each other for inverse cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: u32,
    pub bg: u32,
    pub flags: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `cols * rows` cells.
    pub cells: Vec<Cell>,
    /// Column and row, or `None` when the program hid the cursor.
    pub cursor: Option<(u16, u16)>,
}

#[derive(Default)]
struct Shared {
    replies: Vec<u8>,
    title: Option<String>,
}

/// Collects what the terminal says back to the shell and the window title. Clipboard and colour
/// requests are ignored: programs cannot read the clipboard (spec 2.11).
#[derive(Clone)]
struct Listener(Arc<Mutex<Shared>>);

impl Listener {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.lock().replies.extend_from_slice(text.as_bytes()),
            Event::Title(title) => self.lock().title = Some(title),
            Event::ResetTitle => self.lock().title = None,
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
struct Size {
    cols: u16,
    rows: u16,
}

impl Size {
    fn clamped(cols: u16, rows: u16) -> Size {
        Size {
            cols: cols.max(2),
            rows: rows.max(1),
        }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

pub struct Emulator {
    term: Term<Listener>,
    processor: Processor,
    shared: Listener,
    /// Copy of the listener's title, refreshed after each feed, so `title` can borrow.
    title: Option<String>,
    size: Size,
    scrollback: usize,
}

impl Emulator {
    /// A blank emulator; sizes below 2 columns by 1 row are clamped up to it.
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let size = Size::clamped(cols, rows);
        let shared = Listener(Arc::new(Mutex::new(Shared::default())));
        Emulator {
            term: Self::new_term(size, scrollback, &shared),
            processor: Processor::new(),
            shared,
            title: None,
            size,
            scrollback,
        }
    }

    fn new_term(size: Size, scrollback: usize, shared: &Listener) -> Term<Listener> {
        let config = Config {
            scrolling_history: scrollback,
            ..Config::default()
        };
        Term::new(config, &size, shared.clone())
    }

    /// Output from the shell.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
        self.sync_title();
    }

    /// Forget everything, then feed `bytes`. When `truncated` the bytes are the tail of a ring
    /// that wrapped, so they start mid-stream: skip to just after the first newline, which drops
    /// a partial escape sequence or character. With no newline there is nothing safe to show.
    /// Replies the replayed output provokes are dropped: they answer queries the shell sent in the
    /// past, and sent now they would land in the live shell as junk at its prompt.
    pub fn replay(&mut self, bytes: &[u8], truncated: bool) {
        self.term = Self::new_term(self.size, self.scrollback, &self.shared);
        self.processor = Processor::new();
        *self.shared.lock() = Shared::default();
        self.title = None;
        let bytes = if truncated {
            match bytes.iter().position(|&b| b == b'\n') {
                Some(i) => &bytes[i + 1..],
                None => &[],
            }
        } else {
            bytes
        };
        self.feed(bytes);
        self.shared.lock().replies.clear();
    }

    /// Sizes below 2 columns by 1 row are clamped up to it.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = Size::clamped(cols, rows);
        self.term.resize(self.size);
    }

    pub fn snapshot(&self, colors: &Colors) -> Snapshot {
        let term = &self.term;
        let cols = term.columns();
        let rows = term.screen_lines();
        let blank = Cell {
            ch: ' ',
            fg: colors.fg,
            bg: colors.bg,
            flags: 0,
        };
        let mut cells = vec![blank; cols * rows];
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
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
            ] {
                if f.intersects(flag) {
                    flags |= bit;
                }
            }
            let mut fg = resolve(item.cell.fg, content.colors, colors);
            let mut bg = resolve(item.cell.bg, content.colors, colors);
            if flags & INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            let ch = if f.contains(Flags::HIDDEN) || item.cell.c == '\t' {
                ' '
            } else {
                item.cell.c
            };
            cells[row as usize * cols + item.point.column.0] = Cell { ch, fg, bg, flags };
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
            cursor,
        }
    }

    /// Bytes the terminal must send back to the shell; empties the buffer.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.shared.lock().replies)
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    fn sync_title(&mut self) {
        let latest = self.shared.lock().title.clone();
        if latest != self.title {
            self.title = latest;
        }
    }

    /// Scrollback lines held.
    pub fn history_len(&self) -> usize {
        self.term.history_size()
    }

    /// A synchronised update (CSI ? 2026 h) is open: output is held back until it ends.
    pub fn pending_sync(&self) -> bool {
        self.processor.sync_timeout().sync_timeout().is_some()
    }

    /// End an open synchronised update now; the caller's timeout.
    pub fn flush_sync(&mut self) {
        self.processor.stop_sync(&mut self.term);
        self.sync_title();
    }
}
