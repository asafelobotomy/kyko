#![forbid(unsafe_code)]
//! A terminal emulator fed by bytes. It never touches a PTY: output from a shell goes in
//! (`feed`), a grid snapshot comes out (`snapshot`), and what the terminal must answer to the
//! shell (cursor position reports and the like) is collected by `take_replies`.
mod colors;
mod feed;
pub mod graphics;
pub mod hints;
pub mod keys;
pub mod links;
pub mod marks;
mod prompts;
pub mod sixel;
mod snapshot;
mod text;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};

pub use colors::{Colors, resolve};

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const UNDERLINE: u8 = 4;
pub const INVERSE: u8 = 8;
pub const WIDE: u8 = 16;
pub const WIDE_SPACER: u8 = 32;
pub const STRIKE: u8 = 64;
/// The cell is dim (SGR 2): drawn with its colour faded towards the background.
pub const DIM: u8 = 128;

/// Underline styles (SGR 4, 4:2 to 4:5, 21), in [`Cell::underline`].
pub const UL_NONE: u8 = 0;
pub const UL_SINGLE: u8 = 1;
pub const UL_DOUBLE: u8 = 2;
pub const UL_CURLY: u8 = 3;
pub const UL_DOTTED: u8 = 4;
pub const UL_DASHED: u8 = 5;

/// One grid cell. Colours are 0xRRGGBB, already swapped with each other for inverse cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: u32,
    pub bg: u32,
    pub flags: u8,
    /// One of the `UL_*` styles.
    pub underline: u8,
    /// The underline's own colour (SGR 58) as 0xRRGGBB, or `None` for the text's colour.
    pub underline_color: Option<u32>,
}

/// Cells a link covers: `(row, start, end)` with `end` exclusive, one per row it spans.
pub type LinkSpans = Vec<(u16, u16, u16)>;

impl Cell {
    /// A cell with no underline.
    pub const fn plain(ch: char, fg: u32, bg: u32, flags: u8) -> Self {
        Cell {
            ch,
            fg,
            bg,
            flags,
            underline: UL_NONE,
            underline_color: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `cols * rows` cells.
    pub cells: Vec<Cell>,
    /// One entry per row: [`GUTTER_NONE`], [`GUTTER_RUNNING`], [`GUTTER_OK`] or [`GUTTER_FAILED`]:
    /// what the bar at the row's left edge shows about the command the row belongs to.
    pub gutter: Vec<u8>,
    /// Column and row, or `None` when the program hid the cursor.
    pub cursor: Option<(u16, u16)>,
    /// The combining marks and joiners that follow a cell's character (a grapheme cluster such as
    /// `e` + U+0301 or a ZWJ emoji sequence), by the cell's index in `cells`, in order.
    pub combining: Vec<(u32, String)>,
    /// OSC 8 hyperlinks: each linked cell's index in `cells` and its index in `link_uris`.
    pub links: Vec<(u32, u32)>,
    pub link_uris: Vec<String>,
    /// Images on screen (kitty's graphics protocol), in the order to draw them.
    pub images: Vec<ImageView>,
}

/// An image placement in view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageView {
    /// The image's id; with `generation`, what a cached texture is keyed by.
    pub image: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
    /// The part of the image shown, in pixels.
    pub src: (u32, u32, u32, u32),
    /// The top-left cell in view; the row may be above the top when the image is cut off.
    pub col: i32,
    pub row: i32,
    pub cols: u16,
    pub rows: u16,
    /// From the cell's corner, in pixels.
    pub offset: (u32, u32),
    /// The drawn size, in pixels.
    pub size: (u32, u32),
    /// Below 0 the text is drawn over the image.
    pub z: i32,
}

/// More than any piece of output `feed` hands the terminal at once can scroll.
const HISTORY_ROOM: usize = 4096;
/// The longest run of output handed to the terminal in one go.
const PIECE: usize = 2048;

/// The row is not part of a command the shell marked.
pub const GUTTER_NONE: u8 = 0;
/// The command is running.
pub const GUTTER_RUNNING: u8 = 1;
/// The command exited with status 0.
pub const GUTTER_OK: u8 = 2;
/// The command exited with another status.
pub const GUTTER_FAILED: u8 = 3;

/// What a mouse press starts selecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectKind {
    /// Cells between the press and the drag.
    Simple,
    /// A word: a double click.
    Word,
    /// Whole lines: a triple click.
    Line,
    /// A rectangle of cells: Ctrl+Alt+drag.
    Block,
}

/// A cursor shape, as a program set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Block,
    Underline,
    Beam,
    /// Any other shape alacritty knows (hollow block, hidden).
    Other,
}

/// The terminal modes programs set, read from the emulator. Mouse tracking and mouse encoding
/// each hold at most one of their flags: setting one clears the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermModes {
    pub alt_screen: bool,
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub origin: bool,
    pub line_wrap: bool,
    pub show_cursor: bool,
    pub focus_reporting: bool,
    pub alternate_scroll: bool,
    pub urgency_hints: bool,
    pub bracketed_paste: bool,
    pub insert: bool,
    pub line_feed_new_line: bool,
    pub mouse_click: bool,
    pub mouse_drag: bool,
    pub mouse_motion: bool,
    pub utf8_mouse: bool,
    pub sgr_mouse: bool,
    /// The active kitty keyboard flags (the protocol's bits 1, 2, 4, 8 and 16).
    pub kitty_flags: u8,
    pub cursor_shape: Shape,
    pub cursor_blinking: bool,
}

/// How a colour query (OSC 4, 10, 11, 12 with `?`) wants its answer written.
type ColorFormat = Arc<dyn Fn(alacritty_terminal::vte::ansi::Rgb) -> String + Sync + Send>;

#[derive(Default)]
struct Shared {
    replies: Vec<u8>,
    title: Option<String>,
    bell: bool,
    /// Columns, rows and the cell size in pixels, for `CSI 14 t`.
    window: (u16, u16, u32, u32),
    /// Colour queries, answered after the output that asked is taken in.
    color_queries: Vec<(usize, ColorFormat)>,
    /// Text a program put on the clipboard (OSC 52), and whether it was the primary selection.
    clipboard: Option<(String, bool)>,
}

/// Collects what the terminal says back to the shell and the window title. Programs cannot read
/// the clipboard (spec 2.11); what they write is kept for the window to allow or not.
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
            Event::Bell => self.lock().bell = true,
            Event::ColorRequest(index, format) => {
                let mut s = self.lock();
                if s.color_queries.len() < 64 {
                    s.color_queries.push((index, format));
                }
            }
            Event::ClipboardStore(kind, text) => {
                let primary = kind == alacritty_terminal::term::ClipboardType::Selection;
                self.lock().clipboard = Some((text, primary));
            }
            Event::TextAreaSizeRequest(format) => {
                let mut s = self.lock();
                let (cols, rows, w, h) = s.window;
                let text = format(WindowSize {
                    num_lines: rows,
                    num_cols: cols,
                    cell_width: w.min(u32::from(u16::MAX)) as u16,
                    cell_height: h.min(u32::from(u16::MAX)) as u16,
                });
                s.replies.extend_from_slice(text.as_bytes());
            }
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
    kitty_keyboard: bool,
    /// Where the last search found its query: line, first column, last column.
    matches: Vec<(i32, usize, usize)>,
    current: usize,
    scanner: marks::Scanner,
    /// Desktop notifications the program asked for, not yet shown.
    notifications: Vec<(String, String)>,
    /// When the running command's output began (OSC 133;C).
    started: Option<std::time::Instant>,
    /// Commands that ended since the last look.
    finished: Vec<Finished>,
    /// The command line of the command now running, when the shell sent it (OSC 133;C).
    running: Option<String>,
    /// Command lines by the row number of their prompt.
    commands: BTreeMap<u64, String>,
    /// Where the shell said its command line starts (OSC 133;B): row number and column. Cleared
    /// when the command runs (133;C) or a new prompt begins.
    input: Option<(u64, usize)>,
    /// Images and where they are placed.
    graphics: graphics::Store,
    /// A cell's size in pixels, as the window draws it.
    cell_px: (u32, u32),
    /// The theme the window last drew with, for colour queries.
    theme: Mutex<Option<Colors>>,
    /// The shell's marks, by row number.
    marks: marks::Table,
    /// Lines that scrolled off the top of the history and are gone: row numbers count from them.
    gone: u64,
}

impl Emulator {
    /// A blank emulator; sizes below 2 columns by 1 row are clamped up to it.
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let size = Size::clamped(cols, rows);
        let shared = Listener(Arc::new(Mutex::new(Shared {
            window: (size.cols, size.rows, 8, 16),
            ..Shared::default()
        })));
        Emulator {
            term: Self::new_term(size, scrollback, false, &shared),
            processor: Processor::new(),
            shared,
            title: None,
            size,
            scrollback,
            kitty_keyboard: false,
            matches: Vec::new(),
            current: 0,
            scanner: marks::Scanner::default(),
            notifications: Vec::new(),
            started: None,
            finished: Vec::new(),
            running: None,
            commands: BTreeMap::new(),
            input: None,
            graphics: graphics::Store::default(),
            cell_px: (8, 16),
            theme: Mutex::new(None),
            marks: marks::Table::new(),
            gone: 0,
        }
    }

    fn config(scrollback: usize, kitty_keyboard: bool) -> Config {
        Config {
            // The terminal itself is given room beyond the limit, so that it never drops a line
            // unseen: `feed` trims the history to the limit itself, in pieces small enough not to
            // overshoot the room, and so knows how many lines are gone (see `row_id`).
            scrolling_history: scrollback + HISTORY_ROOM,
            kitty_keyboard,
            ..Config::default()
        }
    }

    fn new_term(
        size: Size,
        scrollback: usize,
        kitty_keyboard: bool,
        shared: &Listener,
    ) -> Term<Listener> {
        Term::new(
            Self::config(scrollback, kitty_keyboard),
            &size,
            shared.clone(),
        )
    }

    /// Turns the kitty keyboard protocol on or off; it is off until this is called. While it
    /// is on, the emulator answers a program's `CSI ? u` query, so programs switch to kitty key
    /// encoding: the caller must then encode keys that way.
    /// The window turns it on for every pane; it is safe for untrusted output since alacritty's
    /// fix for the full-stack panic (alacritty#9073), which the pinned commit has.
    pub fn set_kitty_keyboard(&mut self, on: bool) {
        self.kitty_keyboard = on;
        self.term.set_options(Self::config(self.scrollback, on));
    }

    /// The terminal modes now in force.
    pub fn modes(&self) -> TermModes {
        let m = *self.term.mode();
        let style = self.term.cursor_style();
        TermModes {
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            app_cursor: m.contains(TermMode::APP_CURSOR),
            app_keypad: m.contains(TermMode::APP_KEYPAD),
            origin: m.contains(TermMode::ORIGIN),
            line_wrap: m.contains(TermMode::LINE_WRAP),
            show_cursor: m.contains(TermMode::SHOW_CURSOR),
            focus_reporting: m.contains(TermMode::FOCUS_IN_OUT),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            urgency_hints: m.contains(TermMode::URGENCY_HINTS),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            insert: m.contains(TermMode::INSERT),
            line_feed_new_line: m.contains(TermMode::LINE_FEED_NEW_LINE),
            mouse_click: m.contains(TermMode::MOUSE_REPORT_CLICK),
            mouse_drag: m.contains(TermMode::MOUSE_DRAG),
            mouse_motion: m.contains(TermMode::MOUSE_MOTION),
            utf8_mouse: m.contains(TermMode::UTF8_MOUSE),
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            kitty_flags: [
                TermMode::DISAMBIGUATE_ESC_CODES,
                TermMode::REPORT_EVENT_TYPES,
                TermMode::REPORT_ALTERNATE_KEYS,
                TermMode::REPORT_ALL_KEYS_AS_ESC,
                TermMode::REPORT_ASSOCIATED_TEXT,
            ]
            .iter()
            .enumerate()
            .filter(|(_, f)| m.contains(**f))
            .fold(0, |acc, (i, _)| acc | 1 << i),
            cursor_shape: match style.shape {
                CursorShape::Block => Shape::Block,
                CursorShape::Underline => Shape::Underline,
                CursorShape::Beam => Shape::Beam,
                _ => Shape::Other,
            },
            cursor_blinking: style.blinking,
        }
    }

    /// The window's cell size in pixels: images are sized in cells from it.
    pub fn set_cell_pixels(&mut self, w: u32, h: u32) {
        self.cell_px = (w.max(1), h.max(1));
        self.shared.lock().window.2 = self.cell_px.0;
        self.shared.lock().window.3 = self.cell_px.1;
    }

    /// Sizes below 2 columns by 1 row are clamped up to it.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let new = Size::clamped(cols, rows);
        let width_changed = new.cols != self.size.cols;
        // Where the cursor's line stands, counted from the top of the history.
        let cursor_row = |e: &Emulator| e.row_id(e.term.grid().cursor.point.line.0);
        let before = cursor_row(self);
        self.size = new;
        self.term.resize(self.size);
        {
            let mut s = self.shared.lock();
            s.window.0 = new.cols;
            s.window.1 = new.rows;
        }
        // A new width re-wraps lines, and then row numbers no longer mean what they did. A shell
        // that has only printed its first prompt has nothing to re-wrap, and the cursor's row
        // number says so: it is unchanged.
        if width_changed && cursor_row(self) != before {
            self.marks.clear();
        }
    }

    /// Notifications programs asked for (OSC 9, 777, 99), as title and body; empties the list.
    pub fn take_notifications(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.notifications)
    }

    /// Commands the shell said ended (OSC 133;D after 133;C) since the last call.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }

    /// The theme colours, for answering colour queries; `snapshot` keeps it up to date too.
    pub fn set_theme(&self, colors: &Colors) {
        let mut t = self.theme.lock().unwrap_or_else(|e| e.into_inner());
        if t.as_ref() != Some(colors) {
            *t = Some(colors.clone());
        }
    }

    /// What a program put on the clipboard (OSC 52) since the last call, and whether it was the
    /// primary selection.
    pub fn take_clipboard(&mut self) -> Option<(String, bool)> {
        self.shared.lock().clipboard.take()
    }

    /// Whether the program rang the bell (BEL) since the last call.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.shared.lock().bell)
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

/// A command the shell said ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    /// From its output's start (OSC 133;C) to its end (OSC 133;D).
    pub ran: std::time::Duration,
    pub status: u8,
    /// Its command line, when the shell sent it.
    pub command: Option<String>,
}

/// One command and its output, as the shell marked them, in view rows (negative: above the view).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The row its prompt is on.
    pub prompt: i32,
    /// The first and last rows of its output, when it printed any and it is not lost.
    pub output: Option<(i32, i32)>,
    /// `None` while it runs, or when the shell never said.
    pub status: Option<u8>,
    /// Its command line, when the shell sent it.
    pub command: Option<String>,
}
