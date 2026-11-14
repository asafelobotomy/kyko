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
enum State {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsIgnore,
    DcsPassthrough,
    Osc,
    SosPmApc,
}

/// The parser: `vte`'s states and what a CSI collects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Scan {
    state: State,
    intermediates: [u8; MAX_INTERMEDIATES],
    n_intermediates: usize,
    ignoring: bool,
    /// Values pushed so far, parameters and subparameters together (`vte`'s `Params::len`).
    n_values: usize,
    /// The first value of each parameter, which is all alacritty reads for these sequences.
    firsts: [u16; MAX_PARAMS],
    n_params: usize,
    /// The current parameter already has subparameters, so its first value is recorded.
    in_subparams: bool,
    param: u16,
}

impl Scan {
    fn reset(&mut self) {
        self.n_intermediates = 0;
        self.ignoring = false;
        self.n_values = 0;
        self.n_params = 0;
        self.in_subparams = false;
        self.param = 0;
    }

    fn intermediates(&self) -> &[u8] {
        &self.intermediates[..self.n_intermediates]
    }

    fn collect(&mut self, byte: u8) {
        if self.n_intermediates == MAX_INTERMEDIATES {
            self.ignoring = true;
        } else {
            self.intermediates[self.n_intermediates] = byte;
            self.n_intermediates += 1;
        }
    }

    fn full(&self) -> bool {
        self.n_values == MAX_PARAMS
    }

    /// `vte`'s `Params::push`: ends the current parameter with `value`.
    fn push(&mut self) {
        if !self.in_subparams {
            self.firsts[self.n_params] = self.param;
            self.n_params += 1;
        }
        self.in_subparams = false;
        self.n_values += 1;
    }

    /// `vte`'s `Params::extend`: `value` is followed by a subparameter.
    fn extend(&mut self) {
        if !self.in_subparams {
            self.firsts[self.n_params] = self.param;
            self.n_params += 1;
        }
        self.in_subparams = true;
        self.n_values += 1;
    }

    fn param_next(&mut self, byte: u8) {
        if self.full() {
            self.ignoring = true;
        } else {
            self.param = self
                .param
                .saturating_mul(10)
                .saturating_add(u16::from(byte - b'0'));
        }
    }

    fn param_end(&mut self) {
        if self.full() {
            self.ignoring = true;
        } else {
            self.push();
            self.param = 0;
        }
    }

    fn subparam(&mut self) {
        if self.full() {
            self.ignoring = true;
        } else {
            self.extend();
            self.param = 0;
        }
    }

    /// The parameters' first values, as alacritty's `next_param_or` reads them.
    fn params(&self) -> &[u16] {
        &self.firsts[..self.n_params]
    }
}

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

/// The tracked modes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Tracked {
    flags: u16,
    alt_screen: bool,
    mouse: Mouse,
    encoding: Encoding,
    cursor: Option<Cursor>,
    /// Kitty stacks: `[main, alternate]`, oldest first.
    kitty: [VecDeque<u8>; 2],
    kitty_active: u8,
}

impl Default for Tracked {
    fn default() -> Self {
        Tracked {
            flags: DEFAULT_FLAGS,
            alt_screen: false,
            mouse: Mouse::Off,
            encoding: Encoding::Default,
            cursor: None,
            kitty: Default::default(),
            kitty_active: 0,
        }
    }
}

impl Tracked {
    fn stack(&mut self) -> &mut VecDeque<u8> {
        &mut self.kitty[usize::from(self.alt_screen)]
    }

    fn reload_kitty(&mut self) {
        self.kitty_active = self.stack().back().copied().unwrap_or(0);
    }

    fn set_flag(&mut self, bit: u16, on: bool) {
        if on {
            self.flags |= bit;
        } else {
            self.flags &= !bit;
        }
    }

    fn private_mode(&mut self, number: u16, on: bool) {
        if let Some(f) = FLAGS.iter().find(|f| f.private && f.number == number) {
            self.set_flag(f.bit, on);
            return;
        }
        match (number, on) {
            (1049, _) if on != self.alt_screen => {
                self.alt_screen = on;
                self.reload_kitty();
            }
            (1000, true) => self.mouse = Mouse::Click,
            (1002, true) => self.mouse = Mouse::Drag,
            (1003, true) => self.mouse = Mouse::Motion,
            (1000, false) if self.mouse == Mouse::Click => self.mouse = Mouse::Off,
            (1002, false) if self.mouse == Mouse::Drag => self.mouse = Mouse::Off,
            (1003, false) if self.mouse == Mouse::Motion => self.mouse = Mouse::Off,
            (1005, true) => self.encoding = Encoding::Utf8,
            (1006, true) => self.encoding = Encoding::Sgr,
            (1005, false) if self.encoding == Encoding::Utf8 => self.encoding = Encoding::Default,
            (1006, false) if self.encoding == Encoding::Sgr => self.encoding = Encoding::Default,
            (12, _) => self.cursor.get_or_insert(DEFAULT_CURSOR).blinking = on,
            _ => {}
        }
    }

    fn ansi_mode(&mut self, number: u16, on: bool) {
        if let Some(f) = FLAGS.iter().find(|f| !f.private && f.number == number) {
            self.set_flag(f.bit, on);
        }
    }

    fn csi(&mut self, final_byte: u8, intermediates: &[u8], params: &[u16]) {
        // alacritty's `next_param_or`: a missing or zero parameter takes the default.
        let param = |i: usize, default: u16| match params.get(i) {
            Some(&p) if p != 0 => p,
            _ => default,
        };
        match (final_byte, intermediates) {
            (b'h' | b'l', [b'?']) => {
                for &p in params {
                    self.private_mode(p, final_byte == b'h');
                }
            }
            (b'h' | b'l', []) => {
                for &p in params {
                    self.ansi_mode(p, final_byte == b'h');
                }
            }
            (b'q', [b' ']) => match param(0, 0) {
                0 => self.cursor = None,
                n @ 1..=6 => {
                    self.cursor = Some(Cursor {
                        shape: ((n - 1) / 2 * 2 + 1) as u8,
                        blinking: n % 2 == 1,
                    })
                }
                _ => {}
            },
            (b'u', [b'>']) => {
                let flags = (param(0, 0) as u8) & KITTY_BITS;
                let stack = self.stack();
                if stack.len() >= KITTY_STACK {
                    stack.pop_front();
                }
                stack.push_back(flags);
                self.kitty_active = flags;
            }
            (b'u', [b'<']) => {
                let n = usize::from(param(0, 1));
                let stack = self.stack();
                stack.truncate(stack.len().saturating_sub(n));
                self.reload_kitty();
            }
            (b'u', [b'=']) => {
                let flags = (param(0, 0) as u8) & KITTY_BITS;
                self.kitty_active = match param(1, 1) {
                    3 => self.kitty_active & !flags,
                    2 => self.kitty_active | flags,
                    _ => flags,
                };
            }
            _ => {}
        }
    }

    fn esc(&mut self, final_byte: u8) {
        match final_byte {
            b'=' => self.flags |= APP_KEYPAD,
            b'>' => self.flags &= !APP_KEYPAD,
            b'c' => *self = Tracked::default(),
            _ => {}
        }
    }
}

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
mod tests {
    use super::*;

    fn modes(bytes: &[u8]) -> Modes {
        let mut m = Modes::new();
        m.feed(bytes);
        m
    }

    fn preamble(bytes: &[u8]) -> String {
        String::from_utf8(modes(bytes).preamble()).unwrap()
    }

    #[test]
    fn defaults_need_no_preamble() {
        assert_eq!(preamble(b"plain text\r\n\x1b[1;31mred\x1b[0m"), "");
    }

    #[test]
    fn each_private_mode_round_trips() {
        assert_eq!(preamble(b"\x1b[?1h"), "\x1b[?1h");
        assert_eq!(preamble(b"\x1b[?7l"), "\x1b[?7l");
        assert_eq!(preamble(b"\x1b[?25l"), "\x1b[?25l");
        assert_eq!(preamble(b"\x1b[?1007l"), "\x1b[?1007l");
        assert_eq!(preamble(b"\x1b[?2004h"), "\x1b[?2004h");
        assert_eq!(preamble(b"\x1b[?1004h"), "\x1b[?1004h");
        assert_eq!(preamble(b"\x1b[4h\x1b[20h"), "\x1b[4h\x1b[20h");
        assert_eq!(preamble(b"\x1b="), "\x1b=");
        assert_eq!(preamble(b"\x1b=\x1b>"), "");
    }

    #[test]
    fn only_1049_switches_screens() {
        assert_eq!(preamble(b"\x1b[?47h\x1b[?1047h"), "");
        assert_eq!(preamble(b"\x1b[?1049h"), "\x1b[?1049h");
        assert_eq!(preamble(b"\x1b[?1049h\x1b[?1049l"), "");
    }

    #[test]
    fn mouse_modes_exclude_each_other() {
        assert_eq!(preamble(b"\x1b[?1000h\x1b[?1003h"), "\x1b[?1003h");
        // Unsetting another mode leaves the active one.
        assert_eq!(preamble(b"\x1b[?1003h\x1b[?1000l"), "\x1b[?1003h");
        assert_eq!(preamble(b"\x1b[?1003h\x1b[?1003l"), "");
        assert_eq!(
            preamble(b"\x1b[?1006h\x1b[?1005h\x1b[?1006l"),
            "\x1b[?1005h"
        );
        assert_eq!(preamble(b"\x1b[?1015h"), "");
    }

    #[test]
    fn cursor_style_merges_shape_and_blink() {
        assert_eq!(preamble(b"\x1b[5 q"), "\x1b[5 q");
        assert_eq!(preamble(b"\x1b[6 q"), "\x1b[6 q");
        assert_eq!(preamble(b"\x1b[5 q\x1b[?12l"), "\x1b[6 q");
        // With no style yet, 12 starts from a steady block.
        assert_eq!(preamble(b"\x1b[?12h"), "\x1b[1 q");
        assert_eq!(preamble(b"\x1b[?12l"), "\x1b[2 q");
        assert_eq!(preamble(b"\x1b[3 q\x1b[0 q"), "");
        assert_eq!(preamble(b"\x1b[3 q\x1b[7 q"), "\x1b[3 q");
    }

    #[test]
    fn ris_resets_everything() {
        assert_eq!(
            preamble(b"\x1b[?1049h\x1b[>3u\x1b[?1h\x1b=\x1b[5 q\x1bc"),
            ""
        );
    }

    #[test]
    fn soft_reset_is_not_a_reset() {
        assert_eq!(preamble(b"\x1b[?1h\x1b[!p"), "\x1b[?1h");
    }

    #[test]
    fn kitty_stacks_are_per_screen() {
        assert_eq!(
            preamble(b"\x1b[>1u\x1b[?1049h\x1b[>5u"),
            "\x1b[>1u\x1b[?1049h\x1b[>5u"
        );
        // Leaving the alternate screen keeps its stack for next time.
        assert_eq!(
            preamble(b"\x1b[>1u\x1b[?1049h\x1b[>5u\x1b[?1049l"),
            "\x1b[>1u\x1b[?1049h\x1b[>5u\x1b[?1049l"
        );
        assert_eq!(preamble(b"\x1b[>1u\x1b[>3u\x1b[<u"), "\x1b[>1u");
        assert_eq!(preamble(b"\x1b[>1u\x1b[>3u\x1b[<5u"), "");
    }

    #[test]
    fn kitty_set_changes_only_the_active_flags() {
        // fish: `CSI = 5 u` with an empty stack.
        assert_eq!(preamble(b"\x1b[=5u"), "\x1b[=5;1u");
        assert_eq!(preamble(b"\x1b[>1u\x1b[=4;2u"), "\x1b[>1u\x1b[=5;1u");
        assert_eq!(preamble(b"\x1b[>7u\x1b[=2;3u"), "\x1b[>7u\x1b[=5;1u");
        // A screen switch reloads from the stack, losing the `=` change.
        assert_eq!(preamble(b"\x1b[=5u\x1b[?1049h\x1b[?1049l"), "");
    }

    #[test]
    fn the_kitty_stack_drops_its_oldest_entry() {
        let mut stream = b"\x1b[>2u".to_vec();
        for _ in 0..KITTY_STACK {
            stream.extend_from_slice(b"\x1b[>1u");
        }
        let m = modes(&stream);
        assert_eq!(m.tracked.kitty[0].len(), KITTY_STACK);
        assert!(m.tracked.kitty[0].iter().all(|&f| f == 1));
    }

    #[test]
    fn escape_ends_strings_and_sequences() {
        // An unterminated OSC, then a mode change: the OSC ends at the ESC.
        assert_eq!(preamble(b"\x1b]0;title\x1b[?1049h"), "\x1b[?1049h");
        assert_eq!(preamble(b"\x1bPq#0;1\x1b[?2004h"), "\x1b[?2004h");
        assert_eq!(preamble(b"\x1b_apc text\x1b[?2004h"), "\x1b[?2004h");
        // Inside a string, a mode change is text.
        assert_eq!(preamble(b"\x1b]0;\x9b?1049h\x07"), "");
        // BEL ends an OSC but not a DCS.
        assert_eq!(preamble(b"\x1bPq\x07\x1b[?1h"), "\x1b[?1h");
        let m = modes(b"\x1bPq\x07");
        assert_eq!(m.scan.state, State::DcsPassthrough);
        assert_eq!(modes(b"\x1bPq\x9c").scan.state, State::Ground);
        assert_eq!(modes(b"\x1b]0;x\x9c").scan.state, State::Osc);
    }

    #[test]
    fn can_and_sub_abort() {
        assert_eq!(preamble(b"\x1b[?1049\x18h"), "");
        assert_eq!(preamble(b"\x1b]0;x\x1a\x1b[?1h"), "\x1b[?1h");
    }

    #[test]
    fn controls_inside_a_csi_do_not_end_it() {
        assert_eq!(preamble(b"\x1b[?20\n04h"), "\x1b[?2004h");
    }

    #[test]
    fn too_many_parameters_drop_the_sequence() {
        let mut s = b"\x1b[?".to_vec();
        s.extend_from_slice(&b"2004;".repeat(40));
        s.extend_from_slice(b"2004h");
        assert_eq!(preamble(&s), "");
        // 32 values exactly is still fine.
        let mut s = b"\x1b[?".to_vec();
        s.extend_from_slice(&b"1;".repeat(31));
        s.extend_from_slice(b"2004h");
        assert_eq!(preamble(&s), "\x1b[?1h\x1b[?2004h");
    }

    #[test]
    fn subparameters_count_but_only_the_first_value_is_read() {
        assert_eq!(preamble(b"\x1b[?2004:5h"), "\x1b[?2004h");
    }

    #[test]
    fn too_many_intermediates_drop_the_sequence() {
        assert_eq!(preamble(b"\x1b[?  1h"), "");
    }

    #[test]
    fn chunking_does_not_matter() {
        let corpus: &[u8] = "héllo \u{1F600} wörld\r\n\x1b[?1049h\x1b[>1u\x1b]0;tïtle\x07\
                              \x1bPq#0\x1b\\\x1b[?1;2004h\x1b[=3;2u\x1b[5 q\x1b=\x1b[?1002h\
                              \x1b[?1006h text \x1b[38;2;1;2;3m\x1b[?12l\x1b[4h\x1b_x\x1b\\"
            .as_bytes();
        let whole = modes(corpus);
        for i in 0..=corpus.len() {
            let mut m = Modes::new();
            m.feed(&corpus[..i]);
            m.feed(&corpus[i..]);
            assert_eq!(m, whole, "split at {i}");
        }
        let mut m = Modes::new();
        for b in corpus {
            m.feed(std::slice::from_ref(b));
        }
        assert_eq!(m, whole);
    }

    fn start(bytes: &[u8]) -> (usize, Modes) {
        let mut m = Modes::new();
        let p = m.find_start(bytes);
        (p, m)
    }

    #[test]
    fn the_start_follows_a_line_break() {
        assert_eq!(start(b"partial line\nwhole line\n").0, 13);
    }

    #[test]
    fn the_start_skips_a_cut_sequence() {
        // The ring was cut inside `CSI ? 1049 h`; the rest is its tail.
        let mut m = Modes::new();
        m.feed(b"\x1b[?10");
        let p = m.find_start(b"49hscreen\x1b[2;1Hmore");
        assert_eq!(p, 3);
        assert!(m.tracked.alt_screen);
    }

    #[test]
    fn the_start_skips_continuation_bytes() {
        // Cut inside "é" (C3 A9) and "😀" (F0 9F 98 80), with no line break.
        assert_eq!(start(b"\xa9x").0, 1);
        assert_eq!(start(b"\x9f\x98\x80x").0, 3);
    }

    #[test]
    fn the_start_is_after_a_string_cut_in_two() {
        let mut m = Modes::new();
        m.feed(b"\x1b]0;a long ti");
        let p = m.find_start(b"tle\x07\x1b[?1hdraw");
        assert_eq!(p, 4);
        assert_eq!(m.scan.state, State::Ground);
        // The mode after the start is not read: the window will read it.
        assert_eq!(m.preamble(), b"");
    }

    #[test]
    fn with_no_line_break_the_start_is_the_first_clean_place() {
        assert_eq!(start(b"\x1b[3;4Hdraw\x1b[5;6Hmore").0, 0);
    }

    #[test]
    fn a_line_break_after_the_search_window_is_not_waited_for() {
        let mut s = vec![b'x'; LINE_SEARCH + 10];
        s.extend_from_slice(b"\nrest");
        assert_eq!(start(&s).0, 0);
    }

    #[test]
    fn a_line_break_at_the_very_end_is_not_a_start() {
        assert_eq!(start(b"text\n").0, 0);
    }

    #[test]
    fn nothing_clean_means_nothing_to_replay() {
        let mut m = Modes::new();
        m.feed(b"\x1b]0;");
        assert_eq!(m.find_start(b"never ends"), 10);
    }

    #[test]
    fn find_start_leaves_the_state_at_the_start() {
        let bytes = b"\x1b[?1h\x1b[?2004";
        let mut m = Modes::new();
        let p = m.find_start(bytes);
        assert_eq!(p, 0);
        assert_eq!(m, Modes::new());
        let mut m = Modes::new();
        m.feed(b"\x1b[?25l");
        let p = m.find_start(b"ab\ncd");
        assert_eq!(p, 3);
        assert_eq!(m.preamble(), b"\x1b[?25l");
    }
}
