//! What the C++ grid item calls in Rust, and the one session behind it.
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use kyko_term::{Colors, keys};

use crate::appearance::Look;
use kyko_core::config::Action;

use crate::session::{Act, Launch, MouseEv, MouseKind, SearchOp, Workspace};
use std::sync::atomic::{AtomicI32, Ordering};

#[cxx::bridge]
pub mod ffi {
    struct FfiCell {
        ch: u32,
        fg: u32,
        bg: u32,
        flags: u8,
        /// kyko-term's `UL_*` style.
        underline: u8,
        /// 0x01RRGGBB for the underline's own colour, 0 for the text's.
        underline_color: u32,
    }

    /// The combining marks after the cell at `index` (a grapheme cluster).
    struct FfiExtra {
        index: u32,
        text: String,
    }

    /// An image placement in view (kitty's graphics protocol): which image, the part of it shown,
    /// and where, in cells plus a pixel offset, at what drawn size.
    struct FfiImage {
        image: u32,
        generation: u64,
        width: u32,
        height: u32,
        sx: u32,
        sy: u32,
        sw: u32,
        sh: u32,
        col: i32,
        row: i32,
        ox: u32,
        oy: u32,
        dw: u32,
        dh: u32,
        z: i32,
    }

    extern "Rust" {
        fn kyko_resize(pane: u32, cols: u16, rows: u16);
        fn kyko_cell_px(w: u16, h: u16);
        fn kyko_images(pane: u32, out: &mut Vec<FfiImage>);
        fn kyko_image_pixels(pane: u32, image: u32, generation: u64, out: &mut Vec<u8>) -> bool;
        #[allow(clippy::too_many_arguments)]
        fn kyko_snapshot(
            pane: u32,
            out: &mut Vec<FfiCell>,
            extras: &mut Vec<FfiExtra>,
            gutter: &mut Vec<u8>,
            cols: &mut u16,
            rows: &mut u16,
            cur_x: &mut i32,
            cur_y: &mut i32,
        ) -> u64;
        fn kyko_key(pane: u32, key: u32, mods: u8, text: &str, event: u8) -> bool;
        fn kyko_focus_report(pane: u32, focused: bool);
        fn kyko_link_spans(pane: u32, col: u16, row: u16, out: &mut Vec<u16>) -> bool;
        fn kyko_open_link(pane: u32, col: u16, row: u16, mods: u8) -> bool;
        fn kyko_middle_paste(pane: u32, mods: u8) -> bool;
        fn kyko_scroll_pos(pane: u32, offset: &mut u32, history: &mut u32);
        fn kyko_scroll_to(pane: u32, offset: u32);
        fn kyko_text(pane: u32, text: &str);
        fn kyko_paste(pane: u32, text: &str);
        #[allow(clippy::too_many_arguments)]
        fn kyko_mouse(
            pane: u32,
            kind: u8,
            button: u8,
            col: u16,
            row: u16,
            right: bool,
            mods: u8,
            clicks: u8,
        );
        fn kyko_confirm(yes: bool);
        fn kyko_unicode_search(query: &str) -> String;
        fn kyko_unicode_done(text: &str, typed: bool);
        fn kyko_palette_items(query: &str) -> String;
        fn kyko_palette_done(id: &str);
        fn kyko_sticky_header(pane: u32) -> String;
        fn kyko_search(op: u8, text: &str);
        fn kyko_fixed_title() -> String;
        fn kyko_exit_status() -> i32;
        fn kyko_test_shot() -> String;
        fn kyko_layout() -> String;
        fn kyko_action(name: &str);
        fn kyko_focus_pane(pane: u32);
        fn kyko_focus_tab(tab: u32);
        fn kyko_close_tab(tab: u32);
        fn kyko_close_pane(pane: u32);
        fn kyko_dismiss(pane: u32);
        fn kyko_set_ratio(tab: u32, path: &str, ratio: f64);
        fn kyko_rename_tab(tab: u32, name: &str);
        fn kyko_setting(name: &str) -> i32;
        fn kyko_prefs_pages() -> String;
        fn kyko_prefs_values() -> String;
        fn kyko_prefs_set(key: &str, json: &str) -> String;
        fn kyko_prefs_reset(page: &str) -> String;
        fn kyko_prefs_key(key: u32, mods: u32, text: &str) -> String;
        fn kyko_set_visible(visible: bool);
        fn kyko_palette_bg() -> u32;
        fn kyko_palette_fg() -> u32;
        fn kyko_palette_cursor() -> u32;
        fn kyko_palette_gutter(kind: u8) -> u32;
        fn kyko_palette_font() -> String;
        fn kyko_palette_font_size() -> u16;
    }

    unsafe extern "C++" {
        include!("kyko/cpp/gridview.h");
        fn grid_register();
        fn grid_request_repaint();
        fn grid_reload_look();
        fn controller_send(code: i32);
        fn controller_layout_changed();
        fn controller_copy(text: &str, primary: bool);
        fn controller_paste(pane: u32);
        fn grid_bell(pane: u32, mode: i32);
        fn app_set_id(id: &str);
        fn app_set_translucent();
        fn grid_cell_px() -> u32;
    }
}

pub use ffi::{app_set_id, app_set_translucent, grid_register};

struct Setup {
    title: String,
    socket: PathBuf,
    launch: Mutex<Option<Launch>>,
}

static SETUP: OnceLock<Setup> = OnceLock::new();
static WORKSPACE: OnceLock<Workspace> = OnceLock::new();
static LOOK: OnceLock<crate::session::SharedLook> = OnceLock::new();
static SERVICE: OnceLock<crate::control::Service> = OnceLock::new();

/// Called once by `main` before the window exists.
pub fn configure(socket: PathBuf, launch: Launch, look: Look, title: String) {
    let _ = SETUP.set(Setup {
        title,
        socket,
        launch: Mutex::new(Some(launch)),
    });
    let _ = LOOK.set(std::sync::Arc::new(Mutex::new(look)));
}

/// Starts the sessions; the window's items report their sizes as they appear.
pub fn start() {
    let s = setup();
    let Some(launch) = s.launch.lock().unwrap().take() else {
        return;
    };
    let ws = Workspace::start(
        s.socket.clone(),
        launch,
        LOOK.get().expect("configure ran first").clone(),
        Box::new(ffi::grid_request_repaint),
        Box::new(ffi::controller_layout_changed),
        Box::new(|| ffi::controller_send(crate::control::Command::Hide.code())),
        Box::new(|| ffi::controller_send(3)),
        Box::new(|text, primary| ffi::controller_copy(&text, primary)),
        Box::new(|pane| ffi::grid_bell(pane, look().bell)),
    );
    let _ = WORKSPACE.set(ws);
}

/// Shows the drop-down once the window exists (`kyko dropdown --show`).
pub fn show_at_start() {
    ffi::controller_send(crate::control::Command::Show.code());
}

/// A new look (the config or theme changed): recolour the panes and have the item reload.
pub fn set_look(look: Look) {
    let colors = look.colors.clone();
    *LOOK.get().expect("configure ran first").lock().unwrap() = look;
    if let Some(w) = WORKSPACE.get() {
        w.set_colors(colors);
    }
    ffi::grid_reload_look();
}

/// Serves `org.kyko.Terminal1` and routes its commands to the window.
pub fn serve_bus() -> Result<(), String> {
    let service = crate::control::serve(std::sync::Arc::new(BusHost)).map_err(|e| e.to_string())?;
    let _ = SERVICE.set(service);
    Ok(())
}

struct BusHost;

impl crate::control::Host for BusHost {
    fn command(&self, command: crate::control::Command) {
        ffi::controller_send(command.code());
    }

    fn request(&self, request: crate::control::Request) -> Result<(), String> {
        handle_request(request)
    }

    fn tabs(&self) -> Vec<kyko_core::tabsearch::TabInfo> {
        let layout = WORKSPACE
            .get()
            .map(|w| w.shared.lock().unwrap().layout.clone())
            .unwrap_or_default();
        tabs_from_layout(&layout)
    }

    fn focus_tab(&self, id: u32) -> bool {
        if !self.tabs().iter().any(|t| t.id == id) {
            return false;
        }
        let Some(w) = WORKSPACE.get() else {
            return false;
        };
        w.act(Act::FocusTab(id));
        ffi::controller_send(crate::control::Command::Show.code());
        true
    }
}

/// The tabs in the layout JSON the window publishes.
fn tabs_from_layout(layout: &str) -> Vec<kyko_core::tabsearch::TabInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(layout) else {
        return Vec::new();
    };
    let text = |t: &serde_json::Value, k: &str| t[k].as_str().unwrap_or_default().to_string();
    v["tabs"]
        .as_array()
        .map(|tabs| {
            tabs.iter()
                .filter_map(|t| {
                    Some(kyko_core::tabsearch::TabInfo {
                        id: u32::try_from(t["id"].as_u64()?).ok()?,
                        name: text(t, "name"),
                        program: text(t, "program"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `Run` and `NewTab` from the bus: a new tab in the window, then the window shown.
fn handle_request(req: crate::control::Request) -> Result<(), String> {
    use crate::control::Request;
    let Some(w) = WORKSPACE.get() else {
        return Err("kyko is still starting".into());
    };
    let act = match req {
        Request::Run { command, cwd } => {
            if !look().allow_run {
                return Err("kyko has `advanced.allow_run` turned off".into());
            }
            kyko_core::runq::validate(&command).map_err(|e| e.to_string())?;
            Act::Run {
                command: Some(command),
                cwd: kyko_core::runq::cwd(&cwd),
            }
        }
        Request::NewTab { cwd } => Act::Run {
            command: None,
            cwd: kyko_core::runq::cwd(&cwd),
        },
    };
    w.act(act);
    ffi::controller_send(crate::control::Command::Show.code());
    Ok(())
}

fn kyko_set_visible(visible: bool) {
    if let Some(s) = SERVICE.get() {
        s.set_visible(visible);
    }
}

fn kyko_setting(name: &str) -> i32 {
    let l = look();
    let b = |v: bool| i32::from(v);
    let n = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    match name {
        "accent" => n(l.accent),
        "background" => n(l.colors.bg),
        "foreground" => n(l.colors.fg),
        "radius" => n(l.radius),
        "gap_out" => n(l.gap_out),
        "border_width" => n(l.border_width),
        "sheet" => b(l.sheet),
        "height_pct" => n(l.height_pct),
        "opacity_pct" => n(l.opacity_pct),
        "width_pct" => n(l.width_pct),
        "position" => i32::from(l.position),
        "dim_pct" => n(l.dim_pct),
        "motion_fast" => n(l.motion.0),
        "motion_base" => n(l.motion.1),
        "motion_slow" => n(l.motion.2),
        "above_fullscreen" => b(l.above_fullscreen),
        "hide_on_focus_loss" => b(l.hide_on_focus_loss),
        "animations" => b(l.animations),
        "tabs_bottom" => b(l.tabs_bottom),
        "ligatures" => b(l.ligatures),
        "line_height_pct" => n(l.line_height_pct),
        "padding" => n(l.padding),
        "cursor" => i32::from(l.cursor),
        "cursor_blink" => b(l.cursor_blink),
        _ => 0,
    }
}

fn setup() -> &'static Setup {
    SETUP.get().expect("configure ran first")
}

pub(crate) fn look() -> Look {
    LOOK.get()
        .expect("configure ran first")
        .lock()
        .unwrap()
        .clone()
}

fn colors() -> Colors {
    look().colors
}

/// The window's exit code: the last shell's status, once the window has nothing left to show.
pub fn exit_status() -> Option<i32> {
    WORKSPACE.get().and_then(|w| w.shared.lock().unwrap().exit)
}

/// Ends the sessions of a windowed kyko. Called after the Qt event loop returns.
pub fn shutdown() {
    if let Some(w) = WORKSPACE.get() {
        w.request_end();
        w.wait();
    }
}

fn kyko_resize(pane: u32, cols: u16, rows: u16) {
    if let Some(w) = WORKSPACE.get() {
        w.resize(pane, cols, rows);
    }
}

/// The window's cell size in pixels: images are sized in cells from it.
fn kyko_cell_px(w: u16, h: u16) {
    if let Some(ws) = WORKSPACE.get() {
        ws.cell_px(w, h);
    }
}

/// Measures the cell as soon as the application exists, so a program that draws an image at
/// once (`kyko -e` with an image viewer) has its size before the first item is made.
pub fn measure_cells() {
    let px = ffi::grid_cell_px();
    kyko_cell_px((px >> 16) as u16, (px & 0xffff) as u16);
}

fn kyko_images(pane: u32, out: &mut Vec<ffi::FfiImage>) {
    out.clear();
    let Some(w) = WORKSPACE.get() else { return };
    let shared = w.shared.lock().unwrap();
    let Some(snap) = shared.frames.get(&pane).and_then(|f| f.snapshot.as_ref()) else {
        return;
    };
    out.extend(snap.images.iter().map(|v| ffi::FfiImage {
        image: v.image,
        generation: v.generation,
        width: v.width,
        height: v.height,
        sx: v.src.0,
        sy: v.src.1,
        sw: v.src.2,
        sh: v.src.3,
        col: v.col,
        row: v.row,
        ox: v.offset.0,
        oy: v.offset.1,
        dw: v.size.0,
        dh: v.size.1,
        z: v.z,
    }));
}

/// An image's RGBA pixels, once, for the window's texture cache.
fn kyko_image_pixels(pane: u32, image: u32, generation: u64, out: &mut Vec<u8>) -> bool {
    out.clear();
    let Some(w) = WORKSPACE.get() else {
        return false;
    };
    let rgba = {
        let shared = w.shared.lock().unwrap();
        let Some(snap) = shared.frames.get(&pane).and_then(|f| f.snapshot.as_ref()) else {
            return false;
        };
        match snap
            .images
            .iter()
            .find(|v| v.image == image && v.generation == generation)
        {
            Some(v) => v.rgba.clone(),
            None => return false,
        }
    };
    out.extend_from_slice(&rgba);
    true
}

#[allow(clippy::too_many_arguments)]
fn kyko_snapshot(
    pane: u32,
    out: &mut Vec<ffi::FfiCell>,
    extras: &mut Vec<ffi::FfiExtra>,
    gutter: &mut Vec<u8>,
    cols: &mut u16,
    rows: &mut u16,
    cur_x: &mut i32,
    cur_y: &mut i32,
) -> u64 {
    out.clear();
    extras.clear();
    gutter.clear();
    *cols = 0;
    *rows = 0;
    let Some(w) = WORKSPACE.get() else { return 0 };
    let shared = w.shared.lock().unwrap();
    let Some(snap) = shared.frames.get(&pane).and_then(|f| f.snapshot.as_ref()) else {
        return 0;
    };
    *cols = snap.cols;
    *rows = snap.rows;
    (*cur_x, *cur_y) = snap
        .cursor
        .map_or((-1, -1), |(x, y)| (i32::from(x), i32::from(y)));
    gutter.extend_from_slice(&snap.gutter);
    out.extend(snap.cells.iter().map(|c| ffi::FfiCell {
        ch: c.ch as u32,
        fg: c.fg,
        bg: c.bg,
        flags: c.flags,
        underline: c.underline,
        underline_color: c.underline_color.map_or(0, |c| 0x0100_0000 | c),
    }));
    extras.extend(snap.combining.iter().map(|(index, text)| ffi::FfiExtra {
        index: *index,
        text: text.clone(),
    }));
    if let Some((_, hints)) = shared.hints.as_ref().filter(|(p, _)| *p == pane) {
        let l = look();
        let cols = usize::from(snap.cols);
        let shown = hints.shown();
        // Underline the links still in play, then write each one's label over its start.
        for h in &hints.items {
            if !h.label.starts_with(&hints.typed) {
                continue;
            }
            for &(r, a, b) in &h.spans {
                for c in a..b {
                    if let Some(cell) = out.get_mut(usize::from(r) * cols + usize::from(c)) {
                        cell.underline = kyko_term::UL_SINGLE;
                        cell.underline_color = 0x0100_0000 | l.accent;
                    }
                }
            }
        }
        for (r, c, label) in shown {
            for (k, ch) in label.chars().enumerate() {
                if let Some(cell) = out.get_mut(usize::from(r) * cols + usize::from(c) + k) {
                    cell.ch = ch as u32;
                    cell.fg = l.colors.bg;
                    cell.bg = l.accent;
                    cell.flags = kyko_term::BOLD;
                    cell.underline = kyko_term::UL_NONE;
                }
                extras.retain(|e| e.index as usize != usize::from(r) * cols + usize::from(c) + k);
            }
        }
    }
    1
}

/// A key press in `pane`: one of the window's actions, or bytes for the pane's program. True
/// when kyko took it.
fn kyko_key(pane: u32, key: u32, mods: u8, text: &str, event: u8) -> bool {
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
fn run_action(w: &Workspace, pane: u32, action: Action) {
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

/// While links are labelled in `pane`, keys pick a label instead of reaching the program:
/// Escape or a wrong letter drops the labels, a label opens its link and its capital copies
/// it. True when the key was taken.
fn pick_link(w: &Workspace, pane: u32, key: u32, text: &str, event: u8) -> bool {
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

/// How far the view is scrolled back, and how much history there is: the scrollbar.
fn kyko_scroll_pos(pane: u32, offset: &mut u32, history: &mut u32) {
    (*offset, *history) = WORKSPACE
        .get()
        .and_then(|w| w.shared.lock().unwrap().frames.get(&pane).map(|f| f.scroll))
        .unwrap_or((0, 0));
}

fn kyko_scroll_to(pane: u32, offset: u32) {
    if let Some(w) = WORKSPACE.get() {
        w.scroll_to(pane, offset);
    }
}

/// The link under a cell, for the hover underline: its cells as (row, start, end) triples.
fn kyko_link_spans(pane: u32, col: u16, row: u16, out: &mut Vec<u16>) -> bool {
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
fn kyko_open_link(pane: u32, col: u16, row: u16, mods: u8) -> bool {
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
fn existing_file(t: &kyko_term::links::FileTarget, cwd: Option<&String>) -> Option<PathBuf> {
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
fn open_file(
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
fn editor_command(
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

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Opens `url` with the desktop's handler (`xdg-open`), without waiting for it.
fn open_url(url: &str) -> bool {
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

/// A middle click pastes the primary selection, unless the program takes clicks itself (then
/// Shift+middle-click does). True when the window should paste.
fn kyko_middle_paste(pane: u32, mods: u8) -> bool {
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
fn kyko_focus_report(pane: u32, focused: bool) {
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
static ZOOM: AtomicI32 = AtomicI32::new(0);

fn zoom(step: i32) {
    let now = if step == 0 {
        0
    } else {
        let base = i32::from(look().font_size);
        (ZOOM.load(Ordering::SeqCst) + step).clamp(6 - base, 72 - base)
    };
    ZOOM.store(now, Ordering::SeqCst);
    ffi::grid_reload_look();
}

fn kyko_paste(pane: u32, text: &str) {
    if let Some(w) = WORKSPACE.get() {
        w.paste(pane, text.to_string());
    }
}

#[allow(clippy::too_many_arguments)]
fn kyko_mouse(
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

/// The find bar: 0 look for `text`, 1 next, 2 previous, 3 close.
fn kyko_search(op: u8, text: &str) {
    let op = match op {
        0 => SearchOp::Find(text.to_string()),
        1 => SearchOp::Next,
        2 => SearchOp::Prev,
        _ => SearchOp::Close,
    };
    if let Some(w) = WORKSPACE.get() {
        w.search(op);
    }
}

fn kyko_confirm(yes: bool) {
    act(Act::Confirm(yes));
}

/// Characters for the Unicode input box, as JSON: `[{ch, code, name}]`.
fn kyko_unicode_search(query: &str) -> String {
    static NAMES: OnceLock<Vec<(char, String)>> = OnceLock::new();
    let names = NAMES.get_or_init(kyko_core::unicode::load_names);
    let found: Vec<_> = kyko_core::unicode::search(query, names, 12)
        .into_iter()
        .map(|f| {
            serde_json::json!({"ch": f.ch.to_string(), "code": format!("U+{:04X}", u32::from(f.ch)),
                               "name": f.name.to_lowercase()})
        })
        .collect();
    serde_json::Value::from(found).to_string()
}

/// The Unicode input box closed; `typed` when `text` should go to the pane.
fn kyko_unicode_done(text: &str, typed: bool) {
    act(Act::Unicode(typed.then(|| text.to_string())));
}

/// The command palette's entries for `query`, best first, as JSON:
/// `[{"id", "label", "hint", "kind"}]`. An id is `key:NAME` (an action), `tab:ID`, `cmd:TEXT`
/// (a recent command line, typed at the prompt without Enter) or `prefs`.
fn kyko_palette_items(query: &str) -> String {
    let Some(w) = WORKSPACE.get() else {
        return "[]".into();
    };
    let l = look();
    let mut items: Vec<(String, String, String, &str)> = Vec::new();
    for page in kyko_core::prefs::PAGES {
        for item in page.items {
            let Some(name) = item.key.strip_prefix("keys.") else {
                continue;
            };
            let Some(action) = Action::all().find(|a| a.key_name() == name) else {
                continue;
            };
            let hint = l
                .keys
                .get(action)
                .map(|k| k.to_string())
                .unwrap_or_default();
            items.push((
                format!("key:{name}"),
                item.label.to_string(),
                hint,
                "action",
            ));
        }
    }
    items.push((
        "prefs".into(),
        "Preferences".into(),
        String::new(),
        "action",
    ));
    let (layout, recent) = {
        let sh = w.shared.lock().unwrap();
        (sh.layout.clone(), sh.recent.clone())
    };
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&layout) {
        for t in v["tabs"].as_array().into_iter().flatten() {
            let name = t["name"].as_str().filter(|n| !n.is_empty());
            let label = name.or(t["program"].as_str()).unwrap_or("Tab").to_string();
            items.push((format!("tab:{}", t["id"]), label, "Tab".into(), "tab"));
        }
    }
    for c in recent {
        items.push((format!("cmd:{c}"), c, "Recent".into(), "command"));
    }
    let mut scored: Vec<_> = items
        .into_iter()
        .filter_map(|(id, label, hint, kind)| {
            kyko_core::fuzzy::score(query, &label).map(|s| (s, id, label, hint, kind))
        })
        .collect();
    // Stable: equal scores keep actions, then tabs, then the newest commands first.
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    let out: Vec<_> = scored
        .into_iter()
        .take(30)
        .map(|(_, id, label, hint, kind)| {
            serde_json::json!({"id": id, "label": label, "hint": hint, "kind": kind})
        })
        .collect();
    serde_json::Value::Array(out).to_string()
}

/// The palette closed: on `id` (see [`kyko_palette_items`]), or on nothing (empty, Escape).
fn kyko_palette_done(id: &str) {
    let Some(w) = WORKSPACE.get() else {
        return;
    };
    w.act(Act::PaletteClosed);
    let pane = w.shared.lock().unwrap().focused;
    if let Some(name) = id.strip_prefix("key:") {
        if let Some(action) = Action::all().find(|a| a.key_name() == name)
            && action != Action::CommandPalette
        {
            run_action(w, pane, action);
        }
    } else if let Some(tab) = id.strip_prefix("tab:").and_then(|t| t.parse().ok()) {
        w.act(Act::FocusTab(tab));
    } else if let Some(text) = id.strip_prefix("cmd:") {
        w.typed(pane, text.as_bytes().to_vec());
    } else if id == "prefs" {
        crate::prefs::open();
    }
}

/// The command pinned at the top of `pane`, or "" for none.
fn kyko_sticky_header(pane: u32) -> String {
    WORKSPACE
        .get()
        .and_then(|w| {
            w.shared
                .lock()
                .unwrap()
                .frames
                .get(&pane)
                .and_then(|f| f.header.clone())
        })
        .unwrap_or_default()
}

fn kyko_text(pane: u32, text: &str) {
    if let Some(w) = WORKSPACE.get() {
        w.typed(pane, text.as_bytes().to_vec());
    }
}

fn kyko_layout() -> String {
    WORKSPACE
        .get()
        .map(|w| w.shared.lock().unwrap().layout.clone())
        .unwrap_or_default()
}

fn act(a: Act) {
    if let Some(w) = WORKSPACE.get() {
        w.act(a);
    }
}

/// The tab bar's buttons, by the action's name in `[keys]`.
fn kyko_action(name: &str) {
    if name == "preferences" {
        crate::prefs::open();
        return;
    }
    let action = match name {
        "new_tab" => Action::NewTab,
        "split_side" => Action::SplitSide,
        "split_stacked" => Action::SplitStacked,
        "close_pane" => Action::ClosePane,
        _ => return,
    };
    act(Act::Key(action));
}

fn kyko_focus_pane(pane: u32) {
    act(Act::FocusPane(pane));
}

fn kyko_focus_tab(tab: u32) {
    act(Act::FocusTab(tab));
}

fn kyko_close_tab(tab: u32) {
    act(Act::CloseTab(tab));
}

fn kyko_close_pane(pane: u32) {
    act(Act::ClosePane(pane));
}

fn kyko_dismiss(pane: u32) {
    act(Act::Dismiss(pane));
}

/// `path` is the divider's place in the split tree, as the layout gives it: `0` and `1`.
fn kyko_set_ratio(tab: u32, path: &str, ratio: f64) {
    let path = path.chars().map(|c| c == '1').collect();
    act(Act::Ratio(tab, path, ratio));
}

fn kyko_rename_tab(tab: u32, name: &str) {
    act(Act::Rename(
        tab,
        (!name.is_empty()).then(|| name.to_string()),
    ));
}

/// The status the window quits with, or -1 while it has panes to show.
fn kyko_prefs_pages() -> String {
    kyko_core::prefs::pages_json().to_string()
}

fn kyko_prefs_values() -> String {
    crate::prefs::values()
}

fn kyko_prefs_set(key: &str, json: &str) -> String {
    crate::prefs::set(key, json)
}

/// A key press as the sequence's text, for the capture control; "" while only a modifier is down.
fn kyko_prefs_key(key: u32, mods: u32, text: &str) -> String {
    kyko_core::keys::KeySeq::from_qt(key, mods, text)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn kyko_prefs_reset(page: &str) -> String {
    crate::prefs::reset(page)
}

fn kyko_exit_status() -> i32 {
    exit_status().unwrap_or(-1)
}

fn kyko_fixed_title() -> String {
    setup().title.clone()
}

fn kyko_palette_bg() -> u32 {
    colors().bg
}

fn kyko_palette_fg() -> u32 {
    colors().fg
}

fn kyko_palette_cursor() -> u32 {
    colors().cursor
}

/// The colour of the bar beside a command: running, succeeded or failed (kyko-term's `GUTTER_*`).
fn kyko_palette_gutter(kind: u8) -> u32 {
    let c = colors();
    match kind {
        kyko_term::GUTTER_RUNNING => c.ansi[11],
        kyko_term::GUTTER_OK => c.ansi[10],
        _ => c.ansi[9],
    }
}

fn kyko_palette_font() -> String {
    look().font
}

fn kyko_palette_font_size() -> u16 {
    let size = i32::from(look().font_size) + ZOOM.load(Ordering::SeqCst);
    u16::try_from(size.clamp(6, 72)).unwrap_or(11)
}

/// A test aid: when `KYKO_TEST_SHOT` names a file, the window saves its screen there after a
/// second and exits, so the window can be checked without a display.
fn kyko_test_shot() -> String {
    std::env::var("KYKO_TEST_SHOT").unwrap_or_default()
}
