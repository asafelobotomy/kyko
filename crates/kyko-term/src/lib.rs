#![forbid(unsafe_code)]
//! A terminal emulator fed by bytes. It never touches a PTY: output from a shell goes in
//! (`feed`), a grid snapshot comes out (`snapshot`), and what the terminal must answer to the
//! shell (cursor position reports and the like) is collected by `take_replies`.
mod colors;
pub mod keys;
pub mod marks;

use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::event::{Event, EventListener};
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
    kitty_keyboard: bool,
    /// Where the last search found its query: line, first column, last column.
    matches: Vec<(i32, usize, usize)>,
    current: usize,
    scanner: marks::Scanner,
    /// The shell's marks, by row number.
    marks: marks::Table,
    /// Lines that scrolled off the top of the history and are gone: row numbers count from them.
    gone: u64,
}

impl Emulator {
    /// A blank emulator; sizes below 2 columns by 1 row are clamped up to it.
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let size = Size::clamped(cols, rows);
        let shared = Listener(Arc::new(Mutex::new(Shared::default())));
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
    ///
    /// Not safe to turn on for untrusted output yet: `alacritty_terminal` 0.26.0 panics on a
    /// kitty stack 4096 deep when no window title has been pushed (`push_keyboard_mode` removes
    /// from the title stack). Tests only, until that is fixed.
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

    /// Output from the shell.
    pub fn feed(&mut self, bytes: &[u8]) {
        let events = self.scanner.scan(bytes);
        let mut at = 0;
        for (end, event) in events {
            // The terminal takes the output up to the mark first, so the cursor is where the
            // shell had it when it sent the mark.
            self.advance(&bytes[at..end]);
            self.mark(event);
            at = end;
        }
        self.advance(&bytes[at..]);
        self.sync_title();
    }

    /// Output to the terminal, a piece at a time, trimming the history to its limit after each.
    fn advance(&mut self, bytes: &[u8]) {
        for piece in bytes.chunks(PIECE) {
            let before = self.history_len();
            self.processor.advance(&mut self.term, piece);
            if self.term.mode().contains(TermMode::ALT_SCREEN) {
                continue;
            }
            let after = self.history_len();
            if after < before {
                // The program cleared the history (`CSI 3 J`).
                self.gone += (before - after) as u64;
            }
            if after > self.scrollback {
                // Setting the limit lower drops the oldest lines at once; then it goes back up.
                self.term.set_options(Config {
                    scrolling_history: self.scrollback,
                    ..Self::config(self.scrollback, self.kitty_keyboard)
                });
                self.term
                    .set_options(Self::config(self.scrollback, self.kitty_keyboard));
                self.gone += (after - self.history_len()) as u64;
            }
        }
    }

    /// Forget everything, then feed `bytes`, a pane's replay from `kykod`. When the pane's ring
    /// has wrapped, `kykod` has already cut the replay at a clean place and put the terminal
    /// modes in force there in front of it (protocol version 2), so it is fed whole.
    /// Replies the replayed output provokes are dropped: they answer queries the shell sent in the
    /// past, and sent now they would land in the live shell as junk at its prompt. Replies from
    /// before the replay that were not yet taken with [`Emulator::take_replies`] are dropped too.
    pub fn replay(&mut self, bytes: &[u8]) {
        self.term = Self::new_term(
            self.size,
            self.scrollback,
            self.kitty_keyboard,
            &self.shared,
        );
        self.processor = Processor::new();
        self.scanner = marks::Scanner::default();
        self.marks.clear();
        self.gone = 0;
        *self.shared.lock() = Shared::default();
        self.title = None;
        self.feed(bytes);
        self.shared.lock().replies.clear();
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
        // A new width re-wraps lines, and then row numbers no longer mean what they did. A shell
        // that has only printed its first prompt has nothing to re-wrap, and the cursor's row
        // number says so: it is unchanged.
        if width_changed && cursor_row(self) != before {
            self.marks.clear();
        }
    }

    pub fn snapshot(&self, colors: &Colors) -> Snapshot {
        let term = &self.term;
        let cols = term.columns();
        let rows = term.screen_lines();
        let blank = Cell::plain(' ', colors.fg, colors.bg, 0);
        let mut combining = Vec::new();
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

    /// Moves the view `lines` up into the history (down when negative).
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// Back to the live screen.
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// How many lines the view is above the live screen.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn point(&self, col: u16, row: u16) -> Point {
        let col = usize::from(col).min(self.term.columns().saturating_sub(1));
        let row = i32::from(row).min(self.term.screen_lines() as i32 - 1);
        Point::new(Line(row - self.display_offset() as i32), Column(col))
    }

    /// Starts a selection at a cell of the view; `right` is the cell's right half.
    pub fn select_start(&mut self, col: u16, row: u16, kind: SelectKind, right: bool) {
        let ty = match kind {
            SelectKind::Simple => SelectionType::Simple,
            SelectKind::Word => SelectionType::Semantic,
            SelectKind::Line => SelectionType::Lines,
        };
        let side = if right { Side::Right } else { Side::Left };
        self.term.selection = Some(Selection::new(ty, self.point(col, row), side));
    }

    /// Extends the selection to a cell of the view.
    pub fn select_update(&mut self, col: u16, row: u16, right: bool) {
        let side = if right { Side::Right } else { Side::Left };
        let point = self.point(col, row);
        if let Some(sel) = self.term.selection.as_mut() {
            sel.update(point, side);
        }
    }

    pub fn select_clear(&mut self) {
        self.term.selection = None;
    }

    /// The selected text, `None` when nothing (or only an empty range) is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|t| !t.is_empty())
    }

    /// Finds `query` in the history and the screen, one line at a time (a match does not cross a
    /// wrapped line). Smart case: a query with a capital letter is case-sensitive. Selects the
    /// match nearest the live screen and brings it into view; returns how many there are.
    pub fn search_find(&mut self, query: &str) -> usize {
        self.matches.clear();
        self.current = 0;
        if query.is_empty() {
            self.select_clear();
            return 0;
        }
        let sensitive = query.chars().any(char::is_uppercase);
        let fold = |c: char| {
            if sensitive {
                c
            } else {
                c.to_lowercase().next().unwrap_or(c)
            }
        };
        let needle: Vec<char> = query.chars().map(fold).collect();
        let grid = self.term.grid();
        let (cols, top, bottom) = (
            self.term.columns(),
            grid.topmost_line(),
            grid.bottommost_line(),
        );
        let mut line = top;
        while line <= bottom {
            let row = &grid[line];
            let mut text = Vec::new();
            let mut at = Vec::new();
            for c in 0..cols {
                let cell = &row[Column(c)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                text.push(fold(if cell.c == '\0' { ' ' } else { cell.c }));
                at.push(c);
            }
            let mut i = 0;
            while i + needle.len() <= text.len() {
                if text[i..i + needle.len()] == needle[..] {
                    self.matches.push((line.0, at[i], at[i + needle.len() - 1]));
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
            line += 1;
        }
        self.current = self.matches.len().saturating_sub(1);
        self.show_match();
        self.matches.len()
    }

    /// Moves to the next (or previous) match, wrapping round; `(position, total)`, from 0.
    pub fn search_step(&mut self, forward: bool) -> Option<(usize, usize)> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
        self.show_match();
        Some((self.current, n))
    }

    /// Where the search stands: `(position, total)`, from 0.
    pub fn search_state(&self) -> Option<(usize, usize)> {
        (!self.matches.is_empty()).then_some((self.current, self.matches.len()))
    }

    /// Ends the search; the match is no longer selected.
    pub fn search_clear(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.select_clear();
    }

    /// Selects the current match and scrolls so that it is in view.
    fn show_match(&mut self) {
        let Some(&(line, from, to)) = self.matches.get(self.current) else {
            return;
        };
        let rows = self.term.screen_lines() as i32;
        let offset = self.display_offset() as i32;
        // The lines in view are `-offset .. rows - offset`.
        if line < -offset || line >= rows - offset {
            let want = (rows / 2 - line).clamp(0, self.history_len() as i32);
            self.term.scroll_display(Scroll::Delta(want - offset));
        }
        let mut sel = Selection::new(
            SelectionType::Simple,
            Point::new(Line(line), Column(from)),
            Side::Left,
        );
        sel.update(Point::new(Line(line), Column(to)), Side::Right);
        self.term.selection = Some(sel);
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

/// Prompt marks (see [`marks`]): what the shell's OSC 133 sequences let the window do.
impl Emulator {
    /// The number of the row at screen line `line` (negative lines are the history): it stays the
    /// same line's number as the screen scrolls, and counts up from the oldest line ever lost.
    fn row_id(&self, line: i32) -> u64 {
        (self.gone as i64 + self.history_len() as i64 + i64::from(line)).max(0) as u64
    }

    /// The screen line of row number `id`.
    fn line_of(&self, id: u64) -> i32 {
        (id as i64 - self.gone as i64 - self.history_len() as i64) as i32
    }

    fn in_alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Records a mark the shell sent, on the row the cursor is on.
    fn mark(&mut self, event: marks::Event) {
        if self.in_alt_screen() {
            // A full-screen program has the screen; the shell's marks are for the main one.
            return;
        }
        let cursor = self.term.grid().cursor.point;
        let id = self.row_id(cursor.line.0);
        match event {
            marks::Event::Prompt => self.marks.entry(id).or_default().prompt = true,
            marks::Event::Input => {}
            marks::Event::Output => self.marks.entry(id).or_default().output = true,
            marks::Event::Done(code) => {
                self.marks.entry(id).or_default().end = Some(cursor.column.0 > 0);
                // The status belongs to the prompt the command began at: the nearest above.
                let prompt = self
                    .marks
                    .range(..=id)
                    .rev()
                    .find(|(_, m)| m.prompt)
                    .map(|(&p, _)| p);
                if let Some(p) = prompt {
                    let row = self.marks.entry(p).or_default();
                    row.status.get_or_insert(code.unwrap_or(0));
                }
            }
        }
        self.forget_lost_marks();
    }

    /// Marks on lines that have scrolled out of the history are of no use.
    fn forget_lost_marks(&mut self) {
        let keep = self.gone;
        self.marks = self.marks.split_off(&keep);
    }

    /// The nearest prompt row number before `id`.
    fn prompt_before(&self, id: u64) -> Option<u64> {
        self.marks
            .range(..id)
            .rev()
            .find(|(_, m)| m.prompt)
            .map(|(&p, _)| p)
    }

    /// The nearest prompt row number after `id`.
    fn prompt_after(&self, id: u64) -> Option<u64> {
        self.marks
            .range(id + 1..)
            .find(|(_, m)| m.prompt)
            .map(|(&p, _)| p)
    }

    /// What the gutter shows for the command whose prompt is on row `prompt`.
    fn gutter_of(&self, prompt: u64) -> u8 {
        let row = self.marks.get(&prompt);
        match row.and_then(|r| r.status) {
            Some(0) => GUTTER_OK,
            Some(_) => GUTTER_FAILED,
            // No status yet: running if its output began and no later prompt came.
            None if self.prompt_after(prompt).is_none()
                && self.marks.range(prompt + 1..).any(|(_, m)| m.output) =>
            {
                GUTTER_RUNNING
            }
            None => GUTTER_NONE,
        }
    }

    /// The gutter of every row in view.
    fn gutter(&self) -> Vec<u8> {
        let rows = self.term.screen_lines();
        let mut out = vec![GUTTER_NONE; rows];
        if self.marks.is_empty() || self.in_alt_screen() {
            return out;
        }
        let first = -(self.display_offset() as i32);
        let mut code = self
            .prompt_before(self.row_id(first))
            .map_or(GUTTER_NONE, |p| self.gutter_of(p));
        for (i, slot) in out.iter_mut().enumerate() {
            let id = self.row_id(first + i as i32);
            if self.marks.get(&id).is_some_and(|m| m.prompt) {
                code = self.gutter_of(id);
            }
            *slot = code;
        }
        out
    }

    /// Scrolls to the previous (`dir < 0`) or next (`dir > 0`) prompt: the one before or after the
    /// line at the top of the view. False when there is none that way.
    pub fn jump_prompt(&mut self, dir: i32) -> bool {
        if self.in_alt_screen() {
            return false;
        }
        let top = self.row_id(-(self.display_offset() as i32));
        let target = if dir < 0 {
            self.prompt_before(top)
        } else {
            // The prompt at the top of the view is where the view already is.
            self.prompt_after(top)
        };
        let Some(id) = target else { return false };
        let want = (-self.line_of(id)).clamp(0, self.history_len() as i32);
        let now = self.display_offset() as i32;
        if want == now {
            // Already as far as the view goes: the prompt is on the live screen.
            return false;
        }
        self.term.scroll_display(Scroll::Delta(want - now));
        true
    }

    /// Selects the output of the last command that finished, from the line its output began on to
    /// the line it ended on. False when there is none, or it printed nothing.
    pub fn select_last_output(&mut self) -> bool {
        if self.in_alt_screen() {
            return false;
        }
        let Some((&end, inclusive)) = self
            .marks
            .iter()
            .rev()
            .find_map(|(id, m)| m.end.map(|inc| (id, inc)))
        else {
            return false;
        };
        let previous_end = self
            .marks
            .range(..end)
            .rev()
            .find(|(_, m)| m.end.is_some())
            .map(|(&p, _)| p);
        // Its output began at the last output mark before the end, and after the previous end. If
        // that mark has scrolled out of the history, what is left starts at the oldest line.
        let begin = match self.marks.range(..=end).rev().find(|(_, m)| m.output) {
            Some((&b, _)) if previous_end.is_none_or(|p| p < b) => b,
            None if previous_end.is_none() => self.gone,
            _ => return false,
        };
        let last = if inclusive {
            end
        } else {
            end.saturating_sub(1)
        };
        if last < begin {
            return false;
        }
        let cols = self.term.columns();
        let mut sel = Selection::new(
            SelectionType::Lines,
            Point::new(Line(self.line_of(begin)), Column(0)),
            Side::Left,
        );
        sel.update(
            Point::new(Line(self.line_of(last)), Column(cols.saturating_sub(1))),
            Side::Right,
        );
        self.term.selection = Some(sel);
        true
    }
}
