//! What the C++ grid item calls in Rust, and the one session behind it.
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use kyko_term::{Colors, keys};

use crate::appearance::Look;
use kyko_core::config::Action;

use crate::session::{Act, Launch, MouseEv, MouseKind, Workspace};
use std::sync::atomic::{AtomicI32, Ordering};

#[cxx::bridge]
pub mod ffi {
    struct FfiCell {
        ch: u32,
        fg: u32,
        bg: u32,
        flags: u8,
    }

    extern "Rust" {
        fn kyko_resize(pane: u32, cols: u16, rows: u16);
        fn kyko_snapshot(
            pane: u32,
            out: &mut Vec<FfiCell>,
            cols: &mut u16,
            rows: &mut u16,
            cur_x: &mut i32,
            cur_y: &mut i32,
        ) -> u64;
        fn kyko_key(pane: u32, key: u32, mods: u8, text: &str) -> bool;
        fn kyko_text(pane: u32, text: &str);
        fn kyko_paste(pane: u32, text: &str);
        #[allow(clippy::too_many_arguments)]
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
        fn kyko_set_visible(visible: bool);
        fn kyko_palette_bg() -> u32;
        fn kyko_palette_fg() -> u32;
        fn kyko_palette_cursor() -> u32;
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
        fn controller_copy(text: &str);
        fn controller_paste(pane: u32);
        fn app_set_id(id: &str);
    }
}

pub use ffi::{app_set_id, grid_register};

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
        Box::new(|text| ffi::controller_copy(&text)),
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
    let service =
        crate::control::serve(|c| ffi::controller_send(c.code())).map_err(|e| e.to_string())?;
    let _ = SERVICE.set(service);
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
        "width_pct" => n(l.width_pct),
        "above_fullscreen" => b(l.above_fullscreen),
        "hide_on_focus_loss" => b(l.hide_on_focus_loss),
        "animations" => b(l.animations),
        "tabs_bottom" => b(l.tabs_bottom),
        _ => 0,
    }
}

fn setup() -> &'static Setup {
    SETUP.get().expect("configure ran first")
}

fn look() -> Look {
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

fn kyko_snapshot(
    pane: u32,
    out: &mut Vec<ffi::FfiCell>,
    cols: &mut u16,
    rows: &mut u16,
    cur_x: &mut i32,
    cur_y: &mut i32,
) -> u64 {
    out.clear();
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
    out.extend(snap.cells.iter().map(|c| ffi::FfiCell {
        ch: c.ch as u32,
        fg: c.fg,
        bg: c.bg,
        flags: c.flags,
    }));
    1
}

/// A key press in `pane`: one of the window's actions, or bytes for the pane's program. True
/// when kyko took it.
fn kyko_key(pane: u32, key: u32, mods: u8, text: &str) -> bool {
    let Some(w) = WORKSPACE.get() else {
        return false;
    };
    if let Some(action) = crate::session::action_for(&look(), key, mods, text) {
        match action {
            Action::Copy => w.copy(pane),
            Action::Paste => ffi::controller_paste(pane),
            Action::FontBigger => zoom(1),
            Action::FontSmaller => zoom(-1),
            Action::FontReset => zoom(0),
            other => w.act(Act::Key(other)),
        }
        return true;
    }
    let app_cursor = w
        .shared
        .lock()
        .unwrap()
        .frames
        .get(&pane)
        .is_some_and(|f| f.app_cursor);
    let bytes = keys::encode(key, mods, text, app_cursor);
    if bytes.is_empty() {
        return false;
    }
    w.input(pane, bytes);
    true
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

fn kyko_confirm(yes: bool) {
    act(Act::Confirm(yes));
}

fn kyko_text(pane: u32, text: &str) {
    if let Some(w) = WORKSPACE.get() {
        w.input(pane, text.as_bytes().to_vec());
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
