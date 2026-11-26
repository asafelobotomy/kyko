//! The terminal modes a pane's program has set, read from its output without emulating the
//! terminal.
//!
//! The ring feeds [`Modes`] every byte it drops, so it always describes the terminal at the
//! ring's first kept byte. When a window attaches to a ring that has wrapped, [`Modes::find_start`]
//! picks a clean place to start the replay and [`Modes::preamble`] gives the bytes that put a
//! fresh emulator into the same modes (spec §2.8).
//!
//! The reference is the window's emulator, `alacritty_terminal` 0.26.0 on `vte` 0.15.0, not
//! xterm: every rule here is what they do, and `tests/modes_differential.rs` checks it against
//! them. Line numbers below are of those releases. In short:
//!
//! - The scanner is `vte`'s state machine (`vte` `src/lib.rs`), minus everything that does not
//!   change a tracked mode. ESC ends any sequence or string and starts a new escape; BEL ends an
//!   OSC only; 0x9C ends a DCS only; CAN and SUB abort. A CSI with more than 32 parameters
//!   (subparameters included) or more than 2 intermediates is dropped whole (`ansi.rs` ~1545).
//! - Only `CSI ? 1049` switches screens: `vte` maps neither 47 nor 1047 (`ansi.rs` ~898).
//! - Setting a mouse mode clears the other two; unsetting one clears only its own bit
//!   (`term/mod.rs` ~1953, ~2017). Mouse encodings likewise (~1970).
//! - `CSI ? 12` and `CSI Ps SP q` write the same cursor style (~1988, ~2204).
//! - Kitty keyboard: a stack per screen; `CSI = f ; m u` changes only the active flags; a push,
//!   a pop and a screen switch reload the active flags from the current stack's top (~1303,
//!   ~1314, ~726).
//! - `ESC c` resets every tracked mode (~1835). There is no soft reset (`CSI ! p`).
//!
//! Not tracked, and left to the redraw nudge after attach: the scroll region, which alacritty
//! resets on every resize, and the character sets, which are saved and restored with the cursor.
//! Nor SGR, the title or anything on the grid.

use std::collections::VecDeque;

mod scan;
mod tracked;

use scan::*;
use tracked::*;

const ESC: u8 = 0x1B;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1A;

/// `vte`'s limit on parameters and subparameters in one sequence.
const MAX_PARAMS: usize = 32;
/// `vte`'s limit on intermediates in one sequence.
const MAX_INTERMEDIATES: usize = 2;
/// The kitty keyboard stack's depth per screen: alacritty's `KEYBOARD_MODE_STACK_MAX_DEPTH`,
/// beyond which the oldest entry is dropped. (Alacritty 0.26.0 means to do this but removes from
/// the wrong stack; see `kyko_term::Emulator::set_kitty_keyboard`.)
pub const KITTY_STACK: usize = 4096;
/// How far [`Modes::find_start`] looks for a line break to start after, before settling for the
/// first clean place. A full-screen program may write no line break for megabytes, and the
/// replay should not lose all of them for want of one.
pub const LINE_SEARCH: usize = 64 * 1024;
/// The kitty protocol's flag bits.
const KITTY_BITS: u8 = 0x1F;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mouse {
    #[default]
    Off,
    Click,
    Drag,
    Motion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Encoding {
    #[default]
    Default,
    Utf8,
    Sgr,
}

/// A cursor style: the shape (1 block, 3 underline, 5 beam, as `DECSCUSR` numbers them) and
/// whether it blinks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cursor {
    shape: u8,
    blinking: bool,
}

/// The style alacritty starts from when `CSI ? 12` finds none: its default, a steady block.
const DEFAULT_CURSOR: Cursor = Cursor {
    shape: 1,
    blinking: false,
};

/// The on/off modes, each a bit, with its `CSI` number, whether it is a DEC private mode, and
/// its default.
#[derive(Clone, Copy)]
struct Flag {
    bit: u16,
    number: u16,
    private: bool,
    on_by_default: bool,
}

const APP_CURSOR: u16 = 1 << 0;
const ORIGIN: u16 = 1 << 1;
const LINE_WRAP: u16 = 1 << 2;
const SHOW_CURSOR: u16 = 1 << 3;
const FOCUS: u16 = 1 << 4;
const ALTERNATE_SCROLL: u16 = 1 << 5;
const URGENCY: u16 = 1 << 6;
const BRACKETED_PASTE: u16 = 1 << 7;
const INSERT: u16 = 1 << 8;
const LINE_FEED_NEW_LINE: u16 = 1 << 9;
const APP_KEYPAD: u16 = 1 << 10;

#[rustfmt::skip]
const FLAGS: [Flag; 10] = [
    Flag { bit: APP_CURSOR, number: 1, private: true, on_by_default: false },
    Flag { bit: ORIGIN, number: 6, private: true, on_by_default: false },
    Flag { bit: LINE_WRAP, number: 7, private: true, on_by_default: true },
    Flag { bit: SHOW_CURSOR, number: 25, private: true, on_by_default: true },
    Flag { bit: FOCUS, number: 1004, private: true, on_by_default: false },
    Flag { bit: ALTERNATE_SCROLL, number: 1007, private: true, on_by_default: true },
    Flag { bit: URGENCY, number: 1042, private: true, on_by_default: true },
    Flag { bit: BRACKETED_PASTE, number: 2004, private: true, on_by_default: false },
    Flag { bit: INSERT, number: 4, private: false, on_by_default: false },
    Flag { bit: LINE_FEED_NEW_LINE, number: 20, private: false, on_by_default: false },
];

/// The default flags: what `TermMode::default` holds (`term/mod.rs` ~113).
const DEFAULT_FLAGS: u16 = LINE_WRAP | SHOW_CURSOR | ALTERNATE_SCROLL | URGENCY;

/// The terminal modes after the bytes fed so far, and where the parser is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    scan: Scan,
    tracked: Tracked,
}

fn is_continuation(byte: u8) -> bool {
    (0x80..=0xBF).contains(&byte)
}

impl Modes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads `bytes`, which may end anywhere, even mid-sequence.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            if self.scan.state == State::Ground {
                // Nothing in ground changes a tracked mode, and only ESC leaves it.
                match memchr::memchr(ESC, &bytes[i..]) {
                    Some(e) => {
                        self.enter_escape();
                        i += e + 1;
                    }
                    None => return,
                }
            } else {
                self.step(bytes[i]);
                i += 1;
            }
        }
    }

    /// Reads `bytes` up to the first place a replay may start, and returns its index:
    /// `bytes.len()` if there is none. A place qualifies when the parser is in ground state and
    /// the byte there is not a UTF-8 continuation byte, since `vte` resynchronises at any other
    /// byte. The first such place just after a line break within [`LINE_SEARCH`] bytes is
    /// preferred, so that line-oriented output starts on a whole line; otherwise the first such
    /// place at all.
    pub fn find_start(&mut self, bytes: &[u8]) -> usize {
        let mut first: Option<(usize, Modes)> = None;
        let mut i = 0;
        while i < bytes.len() {
            if self.scan.state != State::Ground {
                self.step(bytes[i]);
                i += 1;
                continue;
            }
            let end = memchr::memchr(ESC, &bytes[i..]).map_or(bytes.len(), |e| i + e);
            // Ground from `i` to `end`, and still at `end` itself, which is ESC or the end.
            let last = end.min(bytes.len() - 1);
            if first.is_none()
                && let Some(p) = (i..=last).find(|&p| !is_continuation(bytes[p]))
            {
                first = Some((p, self.clone()));
            }
            let search_end = end.min(LINE_SEARCH);
            let mut from = i;
            while from < search_end {
                let Some(lf) = memchr::memchr(b'\n', &bytes[from..search_end]) else {
                    break;
                };
                let p = from + lf + 1;
                if p < bytes.len() && !is_continuation(bytes[p]) {
                    return p;
                }
                from = p;
            }
            if end == bytes.len() {
                break;
            }
            self.enter_escape();
            i = end + 1;
        }
        match first {
            Some((p, modes)) => {
                *self = modes;
                p
            }
            None => bytes.len(),
        }
    }

    /// The bytes that bring a fresh emulator to these modes. Empty when all are at their
    /// defaults.
    pub fn preamble(&self) -> Vec<u8> {
        let t = &self.tracked;
        let mut out = Vec::new();
        let mut csi = |text: &str| {
            out.extend_from_slice(b"\x1b[");
            out.extend_from_slice(text.as_bytes());
        };
        for f in &t.kitty[0] {
            csi(&format!(">{f}u"));
        }
        // The alternate screen's stack outlives a return to the main screen and comes back on
        // the next switch, so it is rebuilt even when the main screen is current.
        if t.alt_screen || !t.kitty[1].is_empty() {
            csi("?1049h");
            for f in &t.kitty[1] {
                csi(&format!(">{f}u"));
            }
            if !t.alt_screen {
                csi("?1049l");
            }
        }
        let top = t.kitty[usize::from(t.alt_screen)]
            .back()
            .copied()
            .unwrap_or(0);
        if t.kitty_active != top {
            csi(&format!("={};1u", t.kitty_active));
        }
        match t.mouse {
            Mouse::Off => {}
            Mouse::Click => csi("?1000h"),
            Mouse::Drag => csi("?1002h"),
            Mouse::Motion => csi("?1003h"),
        }
        match t.encoding {
            Encoding::Default => {}
            Encoding::Utf8 => csi("?1005h"),
            Encoding::Sgr => csi("?1006h"),
        }
        for f in &FLAGS {
            let on = t.flags & f.bit != 0;
            if on != f.on_by_default {
                let mark = if f.private { "?" } else { "" };
                let set = if on { 'h' } else { 'l' };
                csi(&format!("{mark}{}{set}", f.number));
            }
        }
        if let Some(c) = t.cursor {
            csi(&format!("{} q", c.shape + u8::from(!c.blinking)));
        }
        if t.flags & APP_KEYPAD != 0 {
            out.extend_from_slice(b"\x1b=");
        }
        out
    }

    fn enter_escape(&mut self) {
        self.scan.reset();
        self.scan.state = State::Escape;
    }

    /// `vte`'s "anywhere" transitions: CAN and SUB abort to ground, ESC starts an escape.
    fn anywhere(&mut self, byte: u8) {
        match byte {
            CAN | SUB => self.scan.state = State::Ground,
            ESC => self.enter_escape(),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, final_byte: u8) {
        if self.scan.full() {
            self.scan.ignoring = true;
        } else {
            self.scan.push();
        }
        self.scan.state = State::Ground;
        if !self.scan.ignoring {
            let s = &self.scan;
            self.tracked.csi(final_byte, s.intermediates(), s.params());
        }
    }

    fn dcs_hook(&mut self) {
        self.scan.state = State::DcsPassthrough;
    }

    /// One byte outside ground, as `vte`'s `change_state`.
    fn step(&mut self, byte: u8) {
        use State::*;
        let s = &mut self.scan;
        match s.state {
            Ground => {
                if byte == ESC {
                    self.enter_escape();
                }
            }
            Escape => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F => {}
                0x20..=0x2F => {
                    s.collect(byte);
                    s.state = EscapeIntermediate;
                }
                b'P' => {
                    s.reset();
                    s.state = DcsEntry;
                }
                b'X' | b'^' | b'_' => s.state = SosPmApc,
                b'[' => {
                    s.reset();
                    s.state = CsiEntry;
                }
                b']' => s.state = Osc,
                0x30..=0x7E => {
                    s.state = Ground;
                    self.tracked.esc(byte);
                }
                CAN | SUB => s.state = Ground,
                _ => {}
            },
            EscapeIntermediate => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F | 0x7F => {}
                0x20..=0x2F => s.collect(byte),
                // An escape with intermediates sets no tracked mode.
                0x30..=0x7E => s.state = Ground,
                _ => self.anywhere(byte),
            },
            CsiEntry => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F => {}
                0x20..=0x2F => {
                    s.collect(byte);
                    s.state = CsiIntermediate;
                }
                0x30..=0x39 => {
                    s.param_next(byte);
                    s.state = CsiParam;
                }
                b':' => {
                    s.subparam();
                    s.state = CsiParam;
                }
                b';' => {
                    s.param_end();
                    s.state = CsiParam;
                }
                0x3C..=0x3F => {
                    s.collect(byte);
                    s.state = CsiParam;
                }
                0x40..=0x7E => self.csi_dispatch(byte),
                _ => self.anywhere(byte),
            },
            CsiParam => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F | 0x7F => {}
                0x20..=0x2F => {
                    s.collect(byte);
                    s.state = CsiIntermediate;
                }
                0x30..=0x39 => s.param_next(byte),
                b':' => s.subparam(),
                b';' => s.param_end(),
                0x3C..=0x3F => s.state = CsiIgnore,
                0x40..=0x7E => self.csi_dispatch(byte),
                _ => self.anywhere(byte),
            },
            CsiIntermediate => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F => {}
                0x20..=0x2F => s.collect(byte),
                0x30..=0x3F => s.state = CsiIgnore,
                0x40..=0x7E => self.csi_dispatch(byte),
                _ => self.anywhere(byte),
            },
            CsiIgnore => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x3F | 0x7F => {}
                0x40..=0x7E => s.state = Ground,
                _ => self.anywhere(byte),
            },
            DcsEntry => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F | 0x7F => {}
                0x20..=0x2F => {
                    s.collect(byte);
                    s.state = DcsIntermediate;
                }
                0x30..=0x39 => {
                    s.param_next(byte);
                    s.state = DcsParam;
                }
                b':' => {
                    s.subparam();
                    s.state = DcsParam;
                }
                b';' => {
                    s.param_end();
                    s.state = DcsParam;
                }
                0x3C..=0x3F => {
                    s.collect(byte);
                    s.state = DcsParam;
                }
                0x40..=0x7E => self.dcs_hook(),
                _ => self.anywhere(byte),
            },
            DcsParam => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F | 0x7F => {}
                0x20..=0x2F => {
                    s.collect(byte);
                    s.state = DcsIntermediate;
                }
                0x30..=0x39 => s.param_next(byte),
                b':' => s.subparam(),
                b';' => s.param_end(),
                0x3C..=0x3F => s.state = DcsIgnore,
                0x40..=0x7E => self.dcs_hook(),
                _ => self.anywhere(byte),
            },
            DcsIntermediate => match byte {
                0x00..=0x17 | 0x19 | 0x1C..=0x1F | 0x7F => {}
                0x20..=0x2F => s.collect(byte),
                0x30..=0x3F => s.state = DcsIgnore,
                0x40..=0x7E => self.dcs_hook(),
                _ => self.anywhere(byte),
            },
            DcsIgnore | SosPmApc => self.anywhere(byte),
            DcsPassthrough => match byte {
                CAN | SUB | 0x9C => s.state = Ground,
                ESC => self.enter_escape(),
                _ => {}
            },
            Osc => match byte {
                0x07 | CAN | SUB => s.state = Ground,
                ESC => self.enter_escape(),
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests;
