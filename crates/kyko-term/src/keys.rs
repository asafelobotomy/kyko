//! Turns a key press into the bytes a terminal program expects (xterm's encoding). No Qt: the
//! window passes Qt's key code, the modifier bits and the text the key produced.

pub const SHIFT: u8 = 1;
pub const ALT: u8 = 2;
pub const CTRL: u8 = 4;
/// The Super (logo) key; xterm's encoding has no room for it, the kitty protocol does.
pub const SUPER: u8 = 8;

/// Key event kinds for the kitty protocol's "report event types".
pub const PRESS: u8 = 1;
pub const REPEAT: u8 = 2;
pub const RELEASE: u8 = 3;

/// The kitty keyboard protocol's flags (`CSI > flags u`).
pub const KITTY_DISAMBIGUATE: u8 = 1;
pub const KITTY_EVENT_TYPES: u8 = 2;
pub const KITTY_ALTERNATE_KEYS: u8 = 4;
pub const KITTY_ALL_AS_ESCAPES: u8 = 8;
pub const KITTY_TEXT: u8 = 16;

// Qt::Key values (Qt's `qnamespace.h`); the window passes them unchanged.
const ESCAPE: u32 = 0x0100_0000;
const TAB: u32 = 0x0100_0001;
const BACKTAB: u32 = 0x0100_0002;
const BACKSPACE: u32 = 0x0100_0003;
const RETURN: u32 = 0x0100_0004;
const ENTER: u32 = 0x0100_0005;
const INSERT: u32 = 0x0100_0006;
const DELETE: u32 = 0x0100_0007;
const HOME: u32 = 0x0100_0010;
const END: u32 = 0x0100_0011;
const LEFT: u32 = 0x0100_0012;
const UP: u32 = 0x0100_0013;
const RIGHT: u32 = 0x0100_0014;
const DOWN: u32 = 0x0100_0015;
const PAGE_UP: u32 = 0x0100_0016;
const PAGE_DOWN: u32 = 0x0100_0017;
const F1: u32 = 0x0100_0030;
const F12: u32 = 0x0100_003b;
const F35: u32 = 0x0100_0052;
const SHIFT_KEY: u32 = 0x0100_0020;
const CONTROL_KEY: u32 = 0x0100_0021;
const META_KEY: u32 = 0x0100_0022;
const ALT_KEY: u32 = 0x0100_0023;
const CAPS_LOCK: u32 = 0x0100_0024;
const NUM_LOCK: u32 = 0x0100_0025;
const SCROLL_LOCK: u32 = 0x0100_0026;
const SUPER_L: u32 = 0x0100_0053;
const SUPER_R: u32 = 0x0100_0054;
const MENU: u32 = 0x0100_0055;
const PAUSE: u32 = 0x0100_0008;
const PRINT: u32 = 0x0100_0009;
const ALT_GR: u32 = 0x0100_1103;
const SPACE: u32 = 0x20;
const AT: u32 = 0x40;
const UNDERSCORE: u32 = 0x5f;

/// xterm's modifier parameter: 1 + shift + 2 alt + 4 ctrl. 1 means none.
fn param(mods: u8) -> u8 {
    1 + (mods & SHIFT != 0) as u8 + 2 * (mods & ALT != 0) as u8 + 4 * (mods & CTRL != 0) as u8
}

fn csi(final_byte: char, mods: u8, app_cursor: bool) -> Vec<u8> {
    match param(mods) {
        1 if app_cursor => format!("\x1bO{final_byte}").into_bytes(),
        1 => format!("\x1b[{final_byte}").into_bytes(),
        m => format!("\x1b[1;{m}{final_byte}").into_bytes(),
    }
}

fn tilde(n: u8, mods: u8) -> Vec<u8> {
    match param(mods) {
        1 => format!("\x1b[{n}~").into_bytes(),
        m => format!("\x1b[{n};{m}~").into_bytes(),
    }
}

/// The Left or Right arrow with no modifier, in the program's DECCKM mode.
pub fn arrow(right: bool, app_cursor: bool) -> Vec<u8> {
    csi(if right { 'C' } else { 'D' }, 0, app_cursor)
}

/// The bytes for one key press; empty when the key sends nothing (a bare modifier).
/// `app_cursor` is the program's DECCKM mode, which changes the unmodified arrow keys.
pub fn encode(key: u32, mods: u8, text: &str, app_cursor: bool) -> Vec<u8> {
    let out = match key {
        RETURN | ENTER => b"\r".to_vec(),
        BACKSPACE => b"\x7f".to_vec(),
        TAB => b"\t".to_vec(),
        BACKTAB => b"\x1b[Z".to_vec(),
        ESCAPE => b"\x1b".to_vec(),
        UP => csi('A', mods, app_cursor),
        DOWN => csi('B', mods, app_cursor),
        RIGHT => csi('C', mods, app_cursor),
        LEFT => csi('D', mods, app_cursor),
        HOME => csi('H', mods, app_cursor),
        END => csi('F', mods, app_cursor),
        INSERT => tilde(2, mods),
        DELETE => tilde(3, mods),
        PAGE_UP => tilde(5, mods),
        PAGE_DOWN => tilde(6, mods),
        F1..=F12 => {
            let n = (key - F1) as u8;
            if n < 4 {
                let name = ['P', 'Q', 'R', 'S'][n as usize];
                match param(mods) {
                    1 => format!("\x1bO{name}").into_bytes(),
                    m => format!("\x1b[1;{m}{name}").into_bytes(),
                }
            } else {
                // F5..F12 are 15, 17, 18, 19, 20, 21, 23, 24.
                let code = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 4)];
                tilde(code, mods)
            }
        }
        _ if mods & CTRL != 0 && (AT..=UNDERSCORE).contains(&key) => vec![(key - AT) as u8],
        _ if mods & CTRL != 0 && key == SPACE => vec![0],
        // Keys without a text (a bare Shift or Ctrl) send nothing.
        _ => text.as_bytes().to_vec(),
    };
    // Alt sends an escape in front of what the key would send; the cursor and function keys
    // already carry Alt in their modifier parameter.
    let special = key >= 0x0100_0000 && !matches!(key, BACKSPACE | TAB | RETURN | ENTER | ESCAPE);
    if mods & ALT != 0 && !special && !out.is_empty() {
        let mut v = vec![0x1b];
        v.extend(out);
        return v;
    }
    out
}

/// How the kitty protocol names a key: a number with `u`, or a legacy final byte with a number.
enum Kitty {
    /// `CSI number ; mods u`
    U(u32),
    /// `CSI 1 ; mods X`, or `CSI X` with no modifiers or event.
    Letter(char),
    /// `CSI number ; mods ~`
    Tilde(u32),
}

fn kitty_key(key: u32, text: &str) -> Option<Kitty> {
    use Kitty::*;
    Some(match key {
        ESCAPE => U(27),
        RETURN | ENTER => U(13),
        TAB | BACKTAB => U(9),
        BACKSPACE => U(127),
        INSERT => Tilde(2),
        DELETE => Tilde(3),
        PAGE_UP => Tilde(5),
        PAGE_DOWN => Tilde(6),
        UP => Letter('A'),
        DOWN => Letter('B'),
        RIGHT => Letter('C'),
        LEFT => Letter('D'),
        HOME => Letter('H'),
        END => Letter('F'),
        F1 => Letter('P'),
        0x0100_0031 => Letter('Q'),
        0x0100_0032 => Tilde(13),
        0x0100_0033 => Letter('S'),
        k if (F1 + 4..=F12).contains(&k) => {
            Tilde([15, 17, 18, 19, 20, 21, 23, 24][(k - F1 - 4) as usize])
        }
        k if (F12 + 1..=F35).contains(&k) => U(57376 + (k - F12 - 1)),
        CAPS_LOCK => U(57358),
        SCROLL_LOCK => U(57359),
        NUM_LOCK => U(57360),
        PRINT => U(57361),
        PAUSE => U(57362),
        MENU => U(57363),
        SHIFT_KEY => U(57441),
        CONTROL_KEY => U(57442),
        ALT_KEY => U(57443),
        SUPER_L | META_KEY => U(57444),
        SUPER_R => U(57450),
        ALT_GR => U(57453),
        SPACE => U(32),
        // Letters are named by their lower-case form, whatever Shift or Caps Lock did.
        k if (0x41..=0x5a).contains(&k) => U(k + 0x20),
        k if k < 0x0100_0000 => {
            // Other text keys by the character they type without Shift, as far as Qt tells:
            // its key code is that character for symbols on the base layer.
            let c = text
                .chars()
                .next()
                .filter(|c| !c.is_control())
                .map(|c| c as u32)
                .unwrap_or(k);
            U(c.to_lowercase_u32())
        }
        _ => return None,
    })
}

trait LowerU32 {
    fn to_lowercase_u32(self) -> u32;
}

impl LowerU32 for u32 {
    fn to_lowercase_u32(self) -> u32 {
        char::from_u32(self)
            .and_then(|c| c.to_lowercase().next())
            .map_or(self, |c| c as u32)
    }
}

/// The bytes for a key event when the program turned on the kitty keyboard protocol with
/// `flags`; `event` is [`PRESS`], [`REPEAT`] or [`RELEASE`]. Empty when the key sends nothing
/// (a release the program did not ask for). With `flags == 0` this is [`encode`] for presses.
pub fn encode_kitty(
    key: u32,
    mods: u8,
    text: &str,
    app_cursor: bool,
    flags: u8,
    event: u8,
) -> Vec<u8> {
    if flags == 0 {
        return if event == RELEASE {
            Vec::new()
        } else {
            encode(key, mods, text, app_cursor)
        };
    }
    let all = flags & KITTY_ALL_AS_ESCAPES != 0;
    let events = flags & KITTY_EVENT_TYPES != 0;
    if event == RELEASE && !events {
        return Vec::new();
    }
    let Some(k) = kitty_key(key, text) else {
        return Vec::new();
    };
    let is_modifier_key = matches!(
        key,
        SHIFT_KEY
            | CONTROL_KEY
            | META_KEY
            | ALT_KEY
            | SUPER_L
            | SUPER_R
            | ALT_GR
            | CAPS_LOCK
            | NUM_LOCK
            | SCROLL_LOCK
    );
    if is_modifier_key && !all {
        return Vec::new();
    }
    // Typed text stays text, unless every key is to be an escape code: a text key without
    // Ctrl, Alt or Super, and Enter, Tab and Backspace with no modifier at all.
    let printable = !text.is_empty() && text.chars().all(|c| !c.is_control());
    let legacy_text = matches!(k, Kitty::U(c) if c >= 32 && c != 127) && printable;
    if !all && event != RELEASE {
        if legacy_text && mods & (CTRL | ALT | SUPER) == 0 {
            return text.as_bytes().to_vec();
        }
        if matches!(key, RETURN | ENTER | TAB | BACKSPACE)
            && mods & (CTRL | ALT | SUPER | SHIFT) == 0
        {
            return encode(key, mods, text, app_cursor);
        }
        if key == BACKTAB && mods & (CTRL | ALT | SUPER) == 0 {
            return encode(key, mods, text, app_cursor);
        }
    }
    if !all && event == RELEASE && (legacy_text || matches!(key, RETURN | ENTER | TAB | BACKSPACE))
    {
        // Released text keys and Enter, Tab, Backspace are only reported with "all keys".
        return Vec::new();
    }
    let mut m = 1
        + u32::from(mods & SHIFT != 0)
        + 2 * u32::from(mods & ALT != 0)
        + 4 * u32::from(mods & CTRL != 0)
        + 8 * u32::from(mods & SUPER != 0);
    if key == BACKTAB && mods & SHIFT == 0 {
        m += 1;
    }
    let ev = if events && event != PRESS {
        format!(":{event}")
    } else {
        String::new()
    };
    let mods_part = if m != 1 || !ev.is_empty() {
        format!("{m}{ev}")
    } else {
        String::new()
    };
    match k {
        Kitty::U(code) => {
            let mut s = format!("\x1b[{code}");
            if flags & KITTY_ALTERNATE_KEYS != 0
                && mods & SHIFT != 0
                && printable
                && let Some(shifted) = text.chars().next().map(|c| c as u32).filter(|c| *c != code)
            {
                s.push_str(&format!(":{shifted}"));
            }
            let with_text = flags & KITTY_TEXT != 0 && all && printable && event != RELEASE;
            if !mods_part.is_empty() || with_text {
                s.push(';');
                s.push_str(&mods_part);
            }
            if with_text {
                let cps: Vec<String> = text.chars().map(|c| (c as u32).to_string()).collect();
                s.push(';');
                s.push_str(&cps.join(":"));
            }
            s.push('u');
            s.into_bytes()
        }
        Kitty::Letter(c) => {
            if mods_part.is_empty() {
                if app_cursor && matches!(c, 'A'..='D' | 'H' | 'F') && !all {
                    format!("\x1bO{c}").into_bytes()
                } else {
                    format!("\x1b[{c}").into_bytes()
                }
            } else {
                format!("\x1b[1;{mods_part}{c}").into_bytes()
            }
        }
        Kitty::Tilde(n) => {
            if mods_part.is_empty() {
                format!("\x1b[{n}~").into_bytes()
            } else {
                format!("\x1b[{n};{mods_part}~").into_bytes()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(key: u32, mods: u8, text: &str) -> Vec<u8> {
        encode(key, mods, text, false)
    }

    #[test]
    fn text_and_control_keys() {
        assert_eq!(k(0x61, 0, "a"), b"a");
        assert_eq!(k(0x41, SHIFT, "A"), b"A");
        assert_eq!(k(0x43, CTRL, "\x03"), vec![3]);
        assert_eq!(k(SPACE, CTRL, " "), vec![0]);
        assert_eq!(k(0x5b, CTRL, "\x1b"), vec![0x1b]);
        assert_eq!(k(RETURN, 0, "\r"), b"\r");
        assert_eq!(k(BACKSPACE, 0, ""), vec![0x7f]);
        assert_eq!(k(0xe9, 0, "é"), "é".as_bytes());
    }

    #[test]
    fn alt_is_an_escape_prefix() {
        assert_eq!(k(0x62, ALT, "b"), b"\x1bb");
        assert_eq!(k(BACKSPACE, ALT, ""), b"\x1b\x7f");
        assert_eq!(k(0x78, ALT | CTRL, "\x18"), b"\x1b\x18");
    }

    #[test]
    fn arrows_follow_the_cursor_mode_and_modifiers() {
        assert_eq!(encode(UP, 0, "", false), b"\x1b[A");
        assert_eq!(encode(UP, 0, "", true), b"\x1bOA");
        assert_eq!(encode(LEFT, CTRL, "", true), b"\x1b[1;5D");
        assert_eq!(encode(RIGHT, SHIFT | ALT, "", false), b"\x1b[1;4C");
        assert_eq!(k(HOME, 0, ""), b"\x1b[H");
        assert_eq!(k(END, 0, ""), b"\x1b[F");
    }

    #[test]
    fn editing_and_function_keys() {
        assert_eq!(k(DELETE, 0, ""), b"\x1b[3~");
        assert_eq!(k(PAGE_UP, CTRL, ""), b"\x1b[5;5~");
        assert_eq!(k(F1, 0, ""), b"\x1bOP");
        assert_eq!(k(F1 + 3, SHIFT, ""), b"\x1b[1;2S");
        assert_eq!(k(F1 + 4, 0, ""), b"\x1b[15~");
        assert_eq!(k(F12, 0, ""), b"\x1b[24~");
        assert_eq!(k(BACKTAB, SHIFT, ""), b"\x1b[Z");
    }

    #[test]
    fn a_bare_modifier_sends_nothing() {
        assert!(k(0x0100_0020, SHIFT, "").is_empty());
        assert!(k(0x0100_0021, CTRL, "").is_empty());
    }
}

#[cfg(test)]
mod kitty_tests {
    use super::*;

    fn d(key: u32, mods: u8, text: &str) -> String {
        String::from_utf8(encode_kitty(
            key,
            mods,
            text,
            false,
            KITTY_DISAMBIGUATE,
            PRESS,
        ))
        .unwrap()
    }

    #[test]
    fn disambiguate_keeps_plain_text_and_names_the_rest() {
        assert_eq!(d(0x41, 0, "a"), "a");
        assert_eq!(d(0x41, SHIFT, "A"), "A");
        assert_eq!(d(0x41, CTRL, "\x01"), "\x1b[97;5u");
        assert_eq!(d(0x49, CTRL, "\t"), "\x1b[105;5u");
        assert_eq!(d(ESCAPE, 0, "\x1b"), "\x1b[27u");
        assert_eq!(d(RETURN, 0, "\r"), "\r");
        assert_eq!(d(RETURN, CTRL, "\r"), "\x1b[13;5u");
        assert_eq!(d(TAB, 0, "\t"), "\t");
        assert_eq!(d(BACKTAB, SHIFT, ""), "\x1b[Z");
        assert_eq!(d(0x41, ALT, "a"), "\x1b[97;3u");
        assert_eq!(d(0x41, SUPER | CTRL, ""), "\x1b[97;13u");
        assert_eq!(d(UP, 0, ""), "\x1b[A");
        assert_eq!(d(UP, CTRL, ""), "\x1b[1;5A");
        assert_eq!(d(F1 + 2, 0, ""), "\x1b[13~");
        assert_eq!(d(DELETE, SHIFT, ""), "\x1b[3;2~");
        assert_eq!(d(SHIFT_KEY, SHIFT, ""), "");
    }

    #[test]
    fn all_keys_as_escapes_with_events_and_text() {
        let f = KITTY_DISAMBIGUATE
            | KITTY_EVENT_TYPES
            | KITTY_ALL_AS_ESCAPES
            | KITTY_TEXT
            | KITTY_ALTERNATE_KEYS;
        let e = |key, mods, text: &str, ev| {
            String::from_utf8(encode_kitty(key, mods, text, false, f, ev)).unwrap()
        };
        assert_eq!(e(0x41, 0, "a", PRESS), "\x1b[97;;97u");
        assert_eq!(e(0x41, SHIFT, "A", PRESS), "\x1b[97:65;2;65u");
        assert_eq!(e(0x41, 0, "a", RELEASE), "\x1b[97;1:3u");
        assert_eq!(e(0x41, 0, "a", REPEAT), "\x1b[97;1:2;97u");
        assert_eq!(e(RETURN, 0, "\r", PRESS), "\x1b[13u");
        assert_eq!(e(SHIFT_KEY, SHIFT, "", PRESS), "\x1b[57441;2u");
        assert_eq!(e(UP, 0, "", RELEASE), "\x1b[1;1:3A");
    }

    #[test]
    fn releases_need_event_types() {
        assert!(encode_kitty(ESCAPE, 0, "", false, KITTY_DISAMBIGUATE, RELEASE).is_empty());
        assert!(encode_kitty(0x41, 0, "a", false, 0, RELEASE).is_empty());
        assert_eq!(encode_kitty(0x41, CTRL, "", false, 0, PRESS), vec![1]);
    }
}
