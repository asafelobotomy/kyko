//! Key sequences as `config.toml` and the Keys page write them: `Ctrl+Shift+T`, `F12`, `Ctrl++`.
//!
//! One key with its modifiers; no chords. Parsing is case-insensitive and accepts a few aliases
//! (`PageUp`, `Escape`, `Return`, `Del`, `Ins`, `Control`, `Meta`, `Win`, `Logo`); printing
//! always gives the canonical text, `Ctrl+Alt+Shift+Super+Key`, which parses back to the same
//! sequence.

use std::fmt;
use std::str::FromStr;

/// The longest key sequence accepted, in bytes.
pub const MAX_KEY_SEQ: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable character. ASCII letters are stored upper-case, everything else as given.
    Char(char),
    /// F1 to F24.
    F(u8),
    Up,
    Down,
    Left,
    Right,
    PgUp,
    PgDn,
    Home,
    End,
    Insert,
    Delete,
    Tab,
    Enter,
    Esc,
    Space,
    Backspace,
}

/// The named keys with their canonical names, then the aliases.
const NAMES: &[(&str, Key)] = &[
    ("Up", Key::Up),
    ("Down", Key::Down),
    ("Left", Key::Left),
    ("Right", Key::Right),
    ("PgUp", Key::PgUp),
    ("PgDn", Key::PgDn),
    ("Home", Key::Home),
    ("End", Key::End),
    ("Insert", Key::Insert),
    ("Delete", Key::Delete),
    ("Tab", Key::Tab),
    ("Enter", Key::Enter),
    ("Esc", Key::Esc),
    ("Space", Key::Space),
    ("Backspace", Key::Backspace),
    // Aliases: parsed, never printed (the canonical name above comes first).
    ("PageUp", Key::PgUp),
    ("PageDown", Key::PgDn),
    ("Escape", Key::Esc),
    ("Return", Key::Enter),
    ("Del", Key::Delete),
    ("Ins", Key::Insert),
];

impl Key {
    /// Every named key and F1 to F24, for tests and the Keys page.
    pub fn named() -> impl Iterator<Item = Key> {
        NAMES[..15]
            .iter()
            .map(|(_, k)| *k)
            .chain((1..=24).map(Key::F))
    }

    /// The same key with an ASCII letter upper-cased, as `KeySeq` stores it.
    fn normal(self) -> Key {
        match self {
            Key::Char(c) => Key::Char(c.to_ascii_uppercase()),
            k => k,
        }
    }

    /// A character whose Shift the keyboard layout decides: anything but an ASCII letter.
    fn shift_free(self) -> bool {
        matches!(self, Key::Char(c) if !c.is_ascii_alphabetic())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Char(c) => write!(f, "{c}"),
            Key::F(n) => write!(f, "F{n}"),
            k => {
                let name = NAMES.iter().find(|(_, n)| n == k).map(|(s, _)| *s);
                f.write_str(name.unwrap_or("?"))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeySeq {
    pub mods: Mods,
    pub key: Key,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyError(pub String);

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for KeyError {}

fn err<T>(message: impl Into<String>) -> Result<T, KeyError> {
    Err(KeyError(message.into()))
}

impl FromStr for KeySeq {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, KeyError> {
        if s.is_empty() {
            return err("empty key sequence");
        }
        if s.len() > MAX_KEY_SEQ {
            return err(format!("longer than {MAX_KEY_SEQ} bytes"));
        }
        if s.chars().any(char::is_control) {
            return err("contains a control character");
        }
        // A trailing empty segment is the `+` key: `Ctrl++`, or `+` alone.
        let (mods_part, key_text) = if s == "+" {
            (None, "+")
        } else if let Some(rest) = s.strip_suffix("++") {
            (Some(rest), "+")
        } else if s.ends_with('+') {
            return err(format!("{s:?} has no key after its last +"));
        } else {
            match s.rsplit_once('+') {
                Some((m, k)) => (Some(m), k),
                None => (None, s),
            }
        };
        let mut mods = Mods::default();
        if let Some(mods_part) = mods_part {
            for name in mods_part.split('+') {
                let flag = match name.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => &mut mods.ctrl,
                    "alt" => &mut mods.alt,
                    "shift" => &mut mods.shift,
                    "super" | "meta" | "win" | "logo" => &mut mods.super_,
                    "" => return err(format!("{s:?} has an empty part")),
                    _ if parse_key(name).is_ok() => {
                        return err(format!("{s:?} has more than one key"));
                    }
                    _ => return err(format!("unknown modifier {name:?}")),
                };
                if *flag {
                    return err(format!("{s:?} names a modifier twice"));
                }
                *flag = true;
            }
        }
        Ok(KeySeq {
            mods,
            key: parse_key(key_text)?,
        })
    }
}

fn parse_key(text: &str) -> Result<Key, KeyError> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (None, _) => return err("no key"),
        (Some(c), None) => {
            return if c.is_whitespace() {
                err("a blank key: write Space")
            } else {
                Ok(Key::Char(c).normal())
            };
        }
        _ => {}
    }
    let lower = text.to_ascii_lowercase();
    if let Some((_, k)) = NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(&lower)) {
        return Ok(*k);
    }
    if let Some(num) = lower.strip_prefix('f')
        && !num.is_empty()
        && num.bytes().all(|b| b.is_ascii_digit())
    {
        return match num.parse::<u8>() {
            Ok(n @ 1..=24) if !num.starts_with('0') => Ok(Key::F(n)),
            _ => err(format!("{text:?}: function keys are F1 to F24")),
        };
    }
    err(format!("unknown key {text:?}"))
}

impl fmt::Display for KeySeq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.mods;
        for (on, name) in [
            (m.ctrl, "Ctrl+"),
            (m.alt, "Alt+"),
            (m.shift, "Shift+"),
            (m.super_, "Super+"),
        ] {
            if on {
                f.write_str(name)?;
            }
        }
        write!(f, "{}", self.key)
    }
}

impl KeySeq {
    pub const fn new(mods: Mods, key: Key) -> Self {
        KeySeq { mods, key }
    }

    /// Whether a key press, as the window has translated it, is this sequence. For a
    /// character other than an ASCII letter, Shift is ignored on both sides: the layout decides
    /// whether `+` needs it, so `Ctrl++` is pressed as Ctrl, Shift and `=` on most layouts.
    pub fn matches(&self, mods: Mods, key: Key) -> bool {
        let key = key.normal();
        if key != self.key {
            return false;
        }
        if key.shift_free() {
            let strip = |m: Mods| Mods { shift: false, ..m };
            strip(mods) == strip(self.mods)
        } else {
            mods == self.mods
        }
    }

    /// Whether two sequences would be taken for each other by [`KeySeq::matches`].
    pub fn clashes(&self, other: &KeySeq) -> bool {
        self.matches(other.mods, other.key) || other.matches(self.mods, self.key)
    }
}

/// Qt's modifier flags, as `QKeyEvent::modifiers()` reports them.
const QT_SHIFT: u32 = 0x0200_0000;
const QT_CTRL: u32 = 0x0400_0000;
const QT_ALT: u32 = 0x0800_0000;
const QT_META: u32 = 0x1000_0000;

/// A Qt key code (`Qt::Key`) and its text as a [`Key`]; `None` for a key kyko has no name for.
pub fn key_from_qt(key: u32, text: &str) -> Option<Key> {
    Some(match key {
        0x0100_0012 => Key::Left,
        0x0100_0013 => Key::Up,
        0x0100_0014 => Key::Right,
        0x0100_0015 => Key::Down,
        0x0100_0016 => Key::PgUp,
        0x0100_0017 => Key::PgDn,
        0x0100_0010 => Key::Home,
        0x0100_0011 => Key::End,
        0x0100_0006 => Key::Insert,
        0x0100_0007 => Key::Delete,
        0x0100_0001 => Key::Tab,
        0x0100_0004 | 0x0100_0005 => Key::Enter,
        0x0100_0000 => Key::Esc,
        0x0100_0003 => Key::Backspace,
        0x20 => Key::Space,
        0x0100_0030..=0x0100_0047 => Key::F((key - 0x0100_0030 + 1) as u8),
        0x21..=0x7e => Key::Char(char::from_u32(key)?),
        // Shift, Ctrl, Alt, Meta and the lock keys alone are no key.
        0x0100_0020..=0x0100_0026 => return None,
        _ => Key::Char(text.chars().next()?),
    })
}

impl KeySeq {
    /// A key press as Qt reports it (`key`, `modifiers`, `text`), for Preferences to capture a
    /// sequence with; `None` while only a modifier is down.
    pub fn from_qt(key: u32, qt_mods: u32, text: &str) -> Option<KeySeq> {
        let key = key_from_qt(key, text)?;
        let mods = Mods {
            ctrl: qt_mods & QT_CTRL != 0,
            alt: qt_mods & QT_ALT != 0,
            shift: qt_mods & QT_SHIFT != 0,
            super_: qt_mods & QT_META != 0,
        };
        Some(KeySeq { mods, key })
    }
}

#[cfg(test)]
mod tests;
