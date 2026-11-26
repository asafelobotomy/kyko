//! Feeding output to the emulator: the scanners that run ahead of alacritty's parser (marks,
//! graphics, sixel), synchronized updates, and the answers to colour queries.

use super::*;

impl Emulator {
    /// Output from the shell.
    pub fn feed(&mut self, bytes: &[u8]) {
        let events = self.scanner.scan(bytes);
        let mut at = 0;
        for (end, event) in events {
            // The terminal takes the output up to the mark first, so the cursor is where the
            // shell had it when it sent the mark.
            self.advance(&bytes[at..end]);
            match event {
                marks::Event::Notify(title, body) => {
                    if self.notifications.len() < 16 {
                        self.notifications.push((title, body));
                    }
                }
                marks::Event::Graphics(body) => self.graphics_command(&body),
                marks::Event::Sixel(data) => self.sixel(&data),
                marks::Event::CellSizeRequest => {
                    let (w, h) = self.cell_px;
                    let reply = format!("\x1b[6;{h};{w}t");
                    self.shared
                        .lock()
                        .replies
                        .extend_from_slice(reply.as_bytes());
                }
                marks::Event::Termcap(names) => {
                    let reply = termcap_reply(&names);
                    self.shared
                        .lock()
                        .replies
                        .extend_from_slice(reply.as_bytes());
                }
                marks::Event::VersionRequest => {
                    let reply = format!("\x1bP>|kyko {}\x1b\\", env!("CARGO_PKG_VERSION"));
                    self.shared
                        .lock()
                        .replies
                        .extend_from_slice(reply.as_bytes());
                }
                other => self.mark(other),
            }
            at = end;
        }
        self.advance(&bytes[at..]);
        self.sync_title();
    }

    /// Where the cursor is, for the graphics store.
    pub(crate) fn graphics_at(&self) -> graphics::At {
        let point = self.term.grid().cursor.point;
        let alt = self.in_alt_screen();
        graphics::At {
            col: point.column.0 as u16,
            row: if alt {
                point.line.0.max(0) as u64
            } else {
                self.row_id(point.line.0)
            },
            cell_w: self.cell_px.0,
            cell_h: self.cell_px.1,
            alt,
        }
    }

    /// A sixel image, at the cursor; the cursor ends on the line below it, in the same column.
    pub(crate) fn sixel(&mut self, data: &[u8]) {
        let Some((w, h, rgba)) = sixel::decode(data) else {
            return;
        };
        let at = self.graphics_at();
        if let Some((_, rows)) = self.graphics.show(w, h, rgba, at) {
            let moves = vec![b'\n'; usize::from(rows)];
            self.advance(&moves);
        }
    }

    /// A kitty graphics command, at the cursor.
    pub(crate) fn graphics_command(&mut self, body: &[u8]) {
        let at = self.graphics_at();
        let out = self.graphics.command(body, at);
        if let Some(r) = out.reply {
            self.shared.lock().replies.extend_from_slice(&r);
        }
        if let Some((cols, rows)) = out.moved {
            // Past the image: down to its last row, then right of it.
            let mut moves = vec![b'\n'; usize::from(rows.saturating_sub(1))];
            moves.extend_from_slice(format!("\x1b[{cols}C").as_bytes());
            self.advance(&moves);
        }
    }

    /// Output to the terminal, a piece at a time, trimming the history to its limit after each.
    pub(crate) fn advance(&mut self, bytes: &[u8]) {
        for piece in bytes.chunks(PIECE) {
            let before = self.history_len();
            let was_alt = self.in_alt_screen();
            self.processor.advance(&mut self.term, piece);
            self.answer_colors();
            if was_alt != self.in_alt_screen() {
                // Images placed on the alternate screen go when it is left (or entered anew).
                self.graphics.leave_alt();
            }
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
        self.graphics.clear();
        *self.shared.lock() = Shared {
            window: (
                self.size.cols,
                self.size.rows,
                self.cell_px.0,
                self.cell_px.1,
            ),
            ..Shared::default()
        };
        self.title = None;
        self.feed(bytes);
        let mut s = self.shared.lock();
        s.replies.clear();
        s.clipboard = None;
    }

    /// Answers the colour queries the last output made: the program's own colours (OSC 4, 10,
    /// 11) first, then the theme's.
    pub(crate) fn answer_colors(&mut self) {
        let queries = std::mem::take(&mut self.shared.lock().color_queries);
        if queries.is_empty() {
            return;
        }
        let theme = self.theme.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(theme) = theme else { return };
        let mut out = String::new();
        for (index, format) in queries {
            let color = if index < 256 {
                alacritty_terminal::vte::ansi::Color::Indexed(index as u8)
            } else {
                match index {
                    257 => alacritty_terminal::vte::ansi::Color::Named(
                        alacritty_terminal::vte::ansi::NamedColor::Background,
                    ),
                    258 => alacritty_terminal::vte::ansi::Color::Named(
                        alacritty_terminal::vte::ansi::NamedColor::Cursor,
                    ),
                    _ => alacritty_terminal::vte::ansi::Color::Named(
                        alacritty_terminal::vte::ansi::NamedColor::Foreground,
                    ),
                }
            };
            let v = resolve(color, self.term.colors(), &theme);
            out.push_str(&format(alacritty_terminal::vte::ansi::Rgb {
                r: (v >> 16) as u8,
                g: (v >> 8) as u8,
                b: v as u8,
            }));
        }
        self.shared.lock().replies.extend_from_slice(out.as_bytes());
    }
}

/// The answer to an XTGETTCAP query: `DCS 1 + r NAME=VALUE ST` (hex-encoded) for each capability
/// kyko knows, `DCS 0 + r NAME ST` for the rest. fish asks for `indn` to keep the screen in the
/// scrollback on Ctrl+L, and `query-os-name` (kitty's) for its key bindings.
fn termcap_reply(names: &[String]) -> String {
    fn hex(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02X}")).collect()
    }
    let mut out = String::new();
    for name in names {
        let value = match name.as_str() {
            "TN" | "name" => Some("xterm-256color"),
            "Co" | "colors" => Some("256"),
            "RGB" => Some("8/8/8"),
            "indn" => Some("\x1b[%p1%dS"),
            "query-os-name" => Some(std::env::consts::OS),
            _ => None,
        };
        match value {
            Some(v) => out.push_str(&format!("\x1bP1+r{}={}\x1b\\", hex(name), hex(v))),
            None => out.push_str(&format!("\x1bP0+r{}\x1b\\", hex(name))),
        }
    }
    out
}
