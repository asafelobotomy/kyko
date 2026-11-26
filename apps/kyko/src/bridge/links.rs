//! Links under the pointer: finding them, and opening a URL in the browser or a file in the editor.

use super::*;

/// While links are labelled in `pane`, keys pick a label instead of reaching the program:
/// Escape or a wrong letter drops the labels, a label opens its link and its capital copies
/// it. True when the key was taken.
pub(crate) fn pick_link(w: &Workspace, pane: u32, key: u32, text: &str, event: u8) -> bool {
    use kyko_term::hints::Outcome;
    let mut shared = w.shared.lock().unwrap();
    let Some((_, hints)) = shared.hints.as_mut().filter(|(p, _)| *p == pane) else {
        return false;
    };
    if event == keys::RELEASE {
        return true;
    }
    let ch = match key {
        0x0100_0000 => None,
        0x0100_0003 => Some('\u{8}'),
        _ => Some(text.chars().next().unwrap_or('\0')),
    };
    let outcome = hints.key(ch);
    if outcome != Outcome::Pending {
        shared.hints = None;
    }
    drop(shared);
    match outcome {
        Outcome::Open(url) => {
            open_url(&url);
        }
        Outcome::Copy(url) => ffi::controller_copy(&url, false),
        Outcome::Pending | Outcome::Cancel => {}
    }
    ffi::grid_request_repaint();
    true
}

/// The link under a cell, for the hover underline: its cells as (row, start, end) triples.
pub(crate) fn kyko_link_spans(pane: u32, col: u16, row: u16, out: &mut Vec<u16>) -> bool {
    out.clear();
    let Some(w) = WORKSPACE.get() else {
        return false;
    };
    let shared = w.shared.lock().unwrap();
    let Some(snap) = shared.frames.get(&pane).and_then(|f| f.snapshot.as_ref()) else {
        return false;
    };
    let spans = match snap.link_at(col, row) {
        Some((url, spans)) if kyko_term::links::openable(&url) => spans,
        Some(_) => return false,
        None => match snap.path_at(col, row) {
            Some((t, spans)) if existing_file(&t, shared.cwds.get(&pane)).is_some() => spans,
            _ => return false,
        },
    };
    for (r, a, b) in spans {
        out.extend([r, a, b]);
    }
    true
}

/// A click on a link opens it with the desktop's handler (`xdg-open`). A program that takes
/// mouse clicks itself keeps plain clicks; Ctrl+Shift+click opens the link there. True when a
/// link was opened.
pub(crate) fn kyko_open_link(pane: u32, col: u16, row: u16, mods: u8) -> bool {
    let Some(w) = WORKSPACE.get() else {
        return false;
    };
    let url = {
        let shared = w.shared.lock().unwrap();
        let Some(frame) = shared.frames.get(&pane) else {
            return false;
        };
        if frame.mouse_tracking && mods & 5 != 5 {
            return false;
        }
        let Some(snap) = frame.snapshot.as_ref() else {
            return false;
        };
        match snap.link_at(col, row) {
            Some((url, _)) if kyko_term::links::openable(&url) => url,
            Some(_) => return false,
            None => {
                let Some((t, _)) = snap.path_at(col, row) else {
                    return false;
                };
                let Some(path) = existing_file(&t, shared.cwds.get(&pane)) else {
                    return false;
                };
                drop(shared);
                return open_file(w, &path, t.line, t.column);
            }
        }
    };
    open_url(&url)
}

/// The file a path on screen names, if it exists: `~/` from the home folder, a relative one
/// from the pane's folder.
pub(crate) fn existing_file(
    t: &kyko_term::links::FileTarget,
    cwd: Option<&String>,
) -> Option<PathBuf> {
    let p = if let Some(rest) = t.path.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME")?).join(rest)
    } else if t.path.starts_with('/') {
        PathBuf::from(&t.path)
    } else {
        PathBuf::from(cwd.filter(|c| !c.is_empty())?).join(&t.path)
    };
    p.exists().then_some(p)
}

/// Opens a file from the screen. With a line number and a terminal editor in `$VISUAL` or
/// `$EDITOR`, the editor opens at that line in a new tab; anything else goes to the desktop's
/// handler.
pub(crate) fn open_file(
    w: &Workspace,
    path: &std::path::Path,
    line: Option<u32>,
    column: Option<u32>,
) -> bool {
    let editor = std::env::var("VISUAL")
        .ok()
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|e| !e.trim().is_empty());
    if path.is_file()
        && let Some(cmd) = editor.and_then(|e| editor_command(&e, path, line, column))
    {
        w.act(Act::Run {
            command: Some(cmd),
            cwd: path.parent().map(|d| d.to_string_lossy().into_owned()),
        });
        return true;
    }
    open_url(&format!("file://{}", path.display()))
}

/// The command line that opens `path` at `line` in a terminal editor kyko knows how to place
/// (vi-likes, helix, nano, micro, kak, emacs -nw); `None` for others (a GUI editor gets the
/// file through `xdg-open`).
pub(crate) fn editor_command(
    editor: &str,
    path: &std::path::Path,
    line: Option<u32>,
    column: Option<u32>,
) -> Option<String> {
    let prog = editor.split_whitespace().next()?;
    let name = prog.rsplit('/').next().unwrap_or(prog);
    let q = shell_quote(&path.to_string_lossy());
    let (l, c) = (line.unwrap_or(1), column.unwrap_or(1));
    let at = match name {
        "vi" | "vim" | "nvim" | "nano" | "micro" | "kak" => {
            if line.is_some() {
                format!("+{l} {q}")
            } else {
                q
            }
        }
        "hx" | "helix" => {
            if line.is_some() {
                format!("{q}:{l}:{c}")
            } else {
                q
            }
        }
        "emacs" if editor.contains("-nw") => {
            if line.is_some() {
                format!("+{l}:{c} {q}")
            } else {
                q
            }
        }
        _ => return None,
    };
    Some(format!("{editor} {at}"))
}

pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Opens `url` with the desktop's handler (`xdg-open`), without waiting for it.
pub(crate) fn open_url(url: &str) -> bool {
    match std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(e) => {
            kyko_diag::problem(
                "kyko",
                kyko_diag::Level::WARN,
                &format!("[KYKO-006] cannot open {url}: xdg-open: {e}"),
            );
            false
        }
    }
}
