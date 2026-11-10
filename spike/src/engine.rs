//! One shell on a PTY, emulated by alacritty_terminal, exposed as grid snapshots. No Qt here.
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

use crate::theme::Palette;

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const UNDERLINE: u8 = 4;
pub const INVERSE: u8 = 8;
pub const WIDE: u8 = 16;
pub const WIDE_SPACER: u8 = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    /// 0xRRGGBB, resolved through the palette; already swapped with `bg` for inverse cells.
    pub fg: u32,
    pub bg: u32,
    pub flags: u8,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub cursor: Option<(u16, u16)>,
    pub seq: u64,
}

struct Shared {
    dirty: AtomicBool,
    seq: AtomicU64,
    pending_status: Mutex<Option<i32>>,
    exit: Mutex<Option<i32>>,
    on_dirty: Mutex<Box<dyn Fn() + Send>>,
    tx: OnceLock<EventLoopSender>,
}

#[derive(Clone)]
struct Listener(Arc<Shared>);

impl Listener {
    fn mark_dirty(&self) {
        self.0.seq.fetch_add(1, Ordering::SeqCst);
        if !self.0.dirty.swap(true, Ordering::SeqCst) {
            (self.0.on_dirty.lock().unwrap())();
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => self.mark_dirty(),
            Event::ChildExit(status) => {
                use std::os::unix::process::ExitStatusExt;
                let code = status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(-2);
                *self.0.pending_status.lock().unwrap() = Some(code);
            },
            // Sent after the last output was drained: only now is the exit visible.
            Event::Exit => {
                let code = self.0.pending_status.lock().unwrap().unwrap_or(-2);
                *self.0.exit.lock().unwrap() = Some(code);
                self.mark_dirty();
            },
            Event::PtyWrite(text) => {
                if let Some(tx) = self.0.tx.get() {
                    let _ = tx.send(Msg::Input(text.into_bytes().into()));
                }
            },
            _ => {},
        }
    }
}

#[derive(Clone, Copy)]
struct Size {
    cols: u16,
    rows: u16,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

fn window_size(s: Size) -> WindowSize {
    WindowSize { num_lines: s.rows, num_cols: s.cols, cell_width: 1, cell_height: 1 }
}

fn clamp(cols: u16, rows: u16) -> Size {
    Size { cols: cols.max(2), rows: rows.max(1) }
}

pub struct Engine {
    term: Arc<FairMutex<Term<Listener>>>,
    shared: Arc<Shared>,
    palette: Palette,
}

impl Engine {
    pub fn spawn(
        shell: &str,
        cols: u16,
        rows: u16,
        palette: Palette,
        on_dirty: Box<dyn Fn() + Send>,
    ) -> io::Result<Engine> {
        Engine::spawn_args(shell, &[], cols, rows, palette, on_dirty)
    }

    pub fn spawn_args(
        shell: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        palette: Palette,
        on_dirty: Box<dyn Fn() + Send>,
    ) -> io::Result<Engine> {
        let size = clamp(cols, rows);
        let shared = Arc::new(Shared {
            dirty: AtomicBool::new(false),
            seq: AtomicU64::new(0),
            pending_status: Mutex::new(None),
            exit: Mutex::new(None),
            on_dirty: Mutex::new(on_dirty),
            tx: OnceLock::new(),
        });
        let listener = Listener(shared.clone());
        let config = Config { scrolling_history: 10_000, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));

        let mut env = HashMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());
        let options = tty::Options {
            shell: Some(tty::Shell::new(shell.to_string(), args.iter().map(|a| a.to_string()).collect())),
            working_directory: None,
            drain_on_exit: true,
            env,
        };
        let pty = tty::new(&options, window_size(size), 0)?;
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let _ = shared.tx.set(event_loop.channel());
        event_loop.spawn();
        Ok(Engine { term, shared, palette })
    }

    fn resolve(&self, color: Color, colors: &alacritty_terminal::term::color::Colors) -> u32 {
        let idx = match color {
            Color::Spec(rgb) => return (rgb.r as u32) << 16 | (rgb.g as u32) << 8 | rgb.b as u32,
            Color::Named(n) => n as usize,
            Color::Indexed(i) => i as usize,
        };
        if let Some(rgb) = colors[idx] {
            return (rgb.r as u32) << 16 | (rgb.g as u32) << 8 | rgb.b as u32;
        }
        match idx {
            0..=15 => self.palette.ansi[idx],
            16..=231 => {
                let i = idx - 16;
                let level = |v: usize| if v == 0 { 0 } else { 55 + 40 * v as u32 };
                level(i / 36) << 16 | level((i / 6) % 6) << 8 | level(i % 6)
            },
            232..=255 => {
                let g = 8 + 10 * (idx as u32 - 232);
                g << 16 | g << 8 | g
            },
            _ => {
                const BG: usize = NamedColor::Background as usize;
                const CUR: usize = NamedColor::Cursor as usize;
                const DIM0: usize = NamedColor::DimBlack as usize;
                const DIM7: usize = NamedColor::DimWhite as usize;
                match idx {
                    BG => self.palette.bg,
                    CUR => self.palette.cursor,
                    DIM0..=DIM7 => self.palette.ansi[idx - DIM0],
                    _ => self.palette.fg,
                }
            },
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let term = self.term.lock();
        self.shared.dirty.store(false, Ordering::SeqCst);
        let cols = term.columns();
        let rows = term.screen_lines();
        let blank = Cell { ch: ' ', fg: self.palette.fg, bg: self.palette.bg, flags: 0 };
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
            ] {
                if f.intersects(flag) {
                    flags |= bit;
                }
            }
            let mut fg = self.resolve(item.cell.fg, content.colors);
            let mut bg = self.resolve(item.cell.bg, content.colors);
            if flags & INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            let ch = if f.contains(Flags::HIDDEN) || item.cell.c == '\t' { ' ' } else { item.cell.c };
            cells[row as usize * cols + item.point.column.0] = Cell { ch, fg, bg, flags };
        }
        let cursor = (content.cursor.shape != CursorShape::Hidden)
            .then(|| (content.cursor.point.column.0 as u16, (content.cursor.point.line.0 + offset) as u16))
            .filter(|&(x, y)| (x as usize) < cols && (y as usize) < rows);
        Snapshot { cols: cols as u16, rows: rows as u16, cells, cursor, seq: self.shared.seq.load(Ordering::SeqCst) }
    }

    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if let Some(tx) = self.shared.tx.get() {
            let _ = tx.send(Msg::Input(bytes.to_vec().into()));
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let size = clamp(cols, rows);
        self.term.lock().resize(size);
        if let Some(tx) = self.shared.tx.get() {
            let _ = tx.send(Msg::Resize(window_size(size)));
        }
    }

    pub fn exited(&self) -> Option<i32> {
        *self.shared.exit.lock().unwrap()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(tx) = self.shared.tx.get() {
            let _ = tx.send(Msg::Shutdown);
        }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
