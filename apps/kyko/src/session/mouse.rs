//! The mouse over a pane's grid: selecting, links, and the reports a program asked for.

use super::*;

/// A mouse event on a pane: sent to the program when it asked for mouse reports, else it selects
/// text or scrolls. True when the pane's picture changed.
pub(crate) fn mouse(
    link: &mut Link,
    pane: PaneId,
    ev: MouseEv,
    copy_on_select: bool,
    on_copy: &dyn Fn(String, bool),
) -> Result<bool, LinkError> {
    let Some(p) = link.pane(pane) else {
        return Ok(false);
    };
    let modes = p.emulator().modes();
    let shift = ev.mods & 1 != 0;
    let tracking = modes.mouse_click || modes.mouse_drag || modes.mouse_motion;
    if tracking && !shift {
        let button = match ev.kind {
            MouseKind::Wheel => 64 + ev.button,
            _ => ev.button.min(3),
        };
        let motion = matches!(ev.kind, MouseKind::Move);
        if motion && !(modes.mouse_motion || (modes.mouse_drag && ev.button < 3)) {
            return Ok(false);
        }
        let mut cb = u32::from(button)
            + if ev.mods & 2 != 0 { 8 } else { 0 }
            + if ev.mods & 4 != 0 { 16 } else { 0 }
            + if motion { 32 } else { 0 };
        let (x, y) = (u32::from(ev.col) + 1, u32::from(ev.row) + 1);
        let release = matches!(ev.kind, MouseKind::Release);
        let bytes = if modes.sgr_mouse {
            format!("\x1b[<{cb};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes()
        } else {
            if release {
                cb = (cb & !3) | 3;
            }
            let mut out = b"\x1b[M".to_vec();
            for v in [cb + 32, x + 32, y + 32] {
                if modes.utf8_mouse {
                    out.extend(char::from_u32(v).unwrap_or(' ').to_string().into_bytes());
                } else {
                    out.push(u8::try_from(v).unwrap_or(255));
                }
            }
            out
        };
        link.input(pane, &bytes)?;
        return Ok(false);
    }
    let Some(p) = link.pane_mut(pane) else {
        return Ok(false);
    };
    match ev.kind {
        MouseKind::Wheel => {
            let lines = if ev.button == 0 { 3 } else { -3 };
            if modes.alt_screen && modes.alternate_scroll {
                // A full-screen program without mouse reports: the wheel is the arrow keys.
                let key = match (ev.button == 0, modes.app_cursor) {
                    (true, true) => "\x1bOA",
                    (true, false) => "\x1b[A",
                    (false, true) => "\x1bOB",
                    (false, false) => "\x1b[B",
                };
                link.input(pane, key.repeat(3).as_bytes())?;
                return Ok(false);
            }
            p.emulator_mut().scroll(lines);
            Ok(true)
        }
        MouseKind::Press if ev.button == 0 => {
            let kind = match ev.clicks {
                2 => SelectKind::Word,
                3.. => SelectKind::Line,
                // Ctrl+Alt+drag selects a rectangle, as in kitty.
                _ if ev.mods & 6 == 6 => SelectKind::Block,
                _ => SelectKind::Simple,
            };
            p.emulator_mut()
                .select_start(ev.col, ev.row, kind, ev.right);
            Ok(true)
        }
        MouseKind::Move if ev.button == 0 => {
            p.emulator_mut().select_update(ev.col, ev.row, ev.right);
            Ok(true)
        }
        MouseKind::Release if ev.button == 0 => {
            // What was selected becomes the primary selection (middle-click pastes it), and
            // the clipboard as well when the setting asks.
            if let Some(t) = p.emulator().selection_text() {
                if copy_on_select {
                    on_copy(t.clone(), false);
                }
                on_copy(t, true);
                return Ok(false);
            }
            // A plain click (no drag) on the command line being typed moves the shell's cursor
            // there, as kitty and fish do.
            if ev.clicks == 1
                && ev.mods == 0
                && let Some(bytes) = p.emulator().click_to_cursor(ev.col, ev.row)
            {
                if !bytes.is_empty() {
                    link.input(pane, &bytes)?;
                }
                return Ok(false);
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}
