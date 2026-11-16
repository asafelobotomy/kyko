//! Turns a key press into the bytes a terminal program expects (xterm's encoding). No Qt: the
//! window passes Qt's key code, the modifier bits and the text the key produced.

pub const SHIFT: u8 = 1;
pub const ALT: u8 = 2;
pub const CTRL: u8 = 4;

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
