//! Keys, the mouse, pastes and scrolling from the grid item, on their way to the session.

use super::*;

/// A key press in `pane`: one of the window's actions, or bytes for the pane's program. True
/// when kyko took it.
pub(crate) fn kyko_key(pane: u32, key: u32, mods: u8, text: &str, event: u8) -> bool {
    let Some(w) = WORKSPACE.get() else {
        return false;
    };
    // Keys kyko took for itself: their release is not the program's either.
    static TAKEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    if event == keys::RELEASE {
        let mut taken = TAKEN.lock().unwrap();
        if let Some(i) = taken.iter().position(|k| *k == key) {
            taken.swap_remove(i);
            return true;
        }
    }
    if pick_link(w, pane, key, text, event) {
        return true;
    }
    if event != keys::RELEASE
        && let Some(action) = crate::session::action_for(&look(), key, mods, text)
    {
        if event == keys::PRESS {
            let mut taken = TAKEN.lock().unwrap();
            if !taken.contains(&key) {
                taken.push(key);
            }
        }
        run_action(w, pane, action);
        return true;
    }
    let (app_cursor, kitty) = w
        .shared
        .lock()
        .unwrap()
        .frames
        .get(&pane)
        .map_or((false, 0), |f| (f.app_cursor, f.kitty));
    let bytes = keys::encode_kitty(key, mods, text, app_cursor, kitty, event);
    if bytes.is_empty() {
        return false;
    }
    w.typed(pane, bytes);
    true
}

/// Does what a key bound to `action` does, in `pane`.
pub(crate) fn run_action(w: &Workspace, pane: u32, action: Action) {
    match action {
        Action::Copy => w.copy(pane),
        Action::PrevPrompt => w.jump_prompt(pane, -1),
        Action::NextPrompt => w.jump_prompt(pane, 1),
        Action::PrevFailed => w.jump_failed(pane, -1),
        Action::NextFailed => w.jump_failed(pane, 1),
        Action::CopyLastOutput => w.copy_last_output(pane),
        Action::CopyLastCommand => w.copy_last_command(pane),
        Action::ShowScrollback => w.show_scrollback(pane),
        Action::OpenLink => {
            let mut shared = w.shared.lock().unwrap();
            let hints = shared
                .frames
                .get(&pane)
                .and_then(|f| f.snapshot.as_ref())
                .and_then(kyko_term::hints::Hints::new);
            shared.hints = hints.map(|h| (pane, h));
            drop(shared);
            ffi::grid_request_repaint();
        }
        Action::Paste => ffi::controller_paste(pane),
        Action::FontBigger => zoom(1),
        Action::FontSmaller => zoom(-1),
        Action::FontReset => zoom(0),
        other => w.act(Act::Key(other)),
    }
}

/// How far the view is scrolled back, and how much history there is: the scrollbar.
pub(crate) fn kyko_scroll_pos(pane: u32, offset: &mut u32, history: &mut u32) {
    (*offset, *history) = WORKSPACE
        .get()
        .and_then(|w| w.shared.lock().unwrap().frames.get(&pane).map(|f| f.scroll))
        .unwrap_or((0, 0));
}

pub(crate) fn kyko_scroll_to(pane: u32, offset: u32) {
    if let Some(w) = WORKSPACE.get() {
        w.scroll_to(pane, offset);
    }
}

/// A middle click pastes the primary selection, unless the program takes clicks itself (then
/// Shift+middle-click does). True when the window should paste.
pub(crate) fn kyko_middle_paste(pane: u32, mods: u8) -> bool {
    let Some(w) = WORKSPACE.get() else {
        return false;
    };
    let tracking = w
        .shared
        .lock()
        .unwrap()
        .frames
        .get(&pane)
        .is_some_and(|f| f.mouse_tracking);
    !tracking || mods & 1 != 0
}

/// The pane gained or lost the keyboard: tells a program that asked (`CSI ? 1004 h`).
pub(crate) fn kyko_focus_report(pane: u32, focused: bool) {
    let Some(w) = WORKSPACE.get() else { return };
    {
        let mut shared = w.shared.lock().unwrap();
        if focused {
            shared.looking_at = Some(pane);
        } else if shared.looking_at == Some(pane) {
            shared.looking_at = None;
        }
    }
    let wants = w
        .shared
        .lock()
        .unwrap()
        .frames
        .get(&pane)
        .is_some_and(|f| f.focus_reporting);
    if wants {
        w.input(
            pane,
            if focused {
                b"\x1b[I".to_vec()
            } else {
                b"\x1b[O".to_vec()
            },
        );
    }
}

/// The font size, as steps from the configured one; `0` takes it back.
pub(crate) static ZOOM: AtomicI32 = AtomicI32::new(0);

pub(crate) fn zoom(step: i32) {
    let now = if step == 0 {
        0
    } else {
        let base = i32::from(look().font_size);
        (ZOOM.load(Ordering::SeqCst) + step).clamp(6 - base, 72 - base)
    };
    ZOOM.store(now, Ordering::SeqCst);
    ffi::grid_reload_look();
}

pub(crate) fn kyko_paste(pane: u32, text: &str) {
    if let Some(w) = WORKSPACE.get() {
        w.paste(pane, text.to_string());
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn kyko_mouse(
    pane: u32,
    kind: u8,
    button: u8,
    col: u16,
    row: u16,
    right: bool,
    mods: u8,
    clicks: u8,
) {
    let kind = match kind {
        0 => MouseKind::Press,
        1 => MouseKind::Move,
        2 => MouseKind::Release,
        _ => MouseKind::Wheel,
    };
    if let Some(w) = WORKSPACE.get() {
        w.mouse(
            pane,
            MouseEv {
                kind,
                button,
                col,
                row,
                right,
                mods,
                clicks,
            },
        );
    }
}
