//! The modes tracked for a reattaching window, and how each control sequence changes them.

use super::*;

/// The tracked modes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tracked {
    pub(crate) flags: u16,
    pub(crate) alt_screen: bool,
    pub(crate) mouse: Mouse,
    pub(crate) encoding: Encoding,
    pub(crate) cursor: Option<Cursor>,
    /// Kitty stacks: `[main, alternate]`, oldest first.
    pub(crate) kitty: [VecDeque<u8>; 2],
    pub(crate) kitty_active: u8,
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
    pub(crate) fn stack(&mut self) -> &mut VecDeque<u8> {
        &mut self.kitty[usize::from(self.alt_screen)]
    }

    pub(crate) fn reload_kitty(&mut self) {
        self.kitty_active = self.stack().back().copied().unwrap_or(0);
    }

    pub(crate) fn set_flag(&mut self, bit: u16, on: bool) {
        if on {
            self.flags |= bit;
        } else {
            self.flags &= !bit;
        }
    }

    pub(crate) fn private_mode(&mut self, number: u16, on: bool) {
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

    pub(crate) fn ansi_mode(&mut self, number: u16, on: bool) {
        if let Some(f) = FLAGS.iter().find(|f| !f.private && f.number == number) {
            self.set_flag(f.bit, on);
        }
    }

    pub(crate) fn csi(&mut self, final_byte: u8, intermediates: &[u8], params: &[u16]) {
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

    pub(crate) fn esc(&mut self, final_byte: u8) {
        match final_byte {
            b'=' => self.flags |= APP_KEYPAD,
            b'>' => self.flags &= !APP_KEYPAD,
            b'c' => *self = Tracked::default(),
            _ => {}
        }
    }
}
