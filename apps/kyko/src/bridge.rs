//! What the C++ grid item calls in Rust, and the one session behind it.
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use kyko_term::{Colors, keys};

use crate::appearance::Look;
use crate::session::{Frame, Launch, Session};

#[cxx::bridge]
pub mod ffi {
    struct FfiCell {
        ch: u32,
        fg: u32,
        bg: u32,
        flags: u8,
    }

    extern "Rust" {
        fn kyko_start(cols: u16, rows: u16);
        fn kyko_resize(cols: u16, rows: u16);
        fn kyko_snapshot(
            out: &mut Vec<FfiCell>,
            cols: &mut u16,
            rows: &mut u16,
            cur_x: &mut i32,
            cur_y: &mut i32,
        ) -> u64;
        fn kyko_key(key: u32, mods: u8, text: &str) -> bool;
        fn kyko_text(text: &str);
        fn kyko_title() -> String;
        fn kyko_fixed_title() -> String;
        fn kyko_test_shot() -> String;
        fn kyko_notice() -> String;
        fn kyko_exit_status() -> i32;
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
static SESSION: OnceLock<Session> = OnceLock::new();
static LOOK: OnceLock<Mutex<Look>> = OnceLock::new();
static SERVICE: OnceLock<crate::control::Service> = OnceLock::new();

/// Called once by `main` before the window exists.
pub fn configure(socket: PathBuf, launch: Launch, look: Look, title: String) {
    let _ = SETUP.set(Setup {
        title,
        socket,
        launch: Mutex::new(Some(launch)),
    });
    let _ = LOOK.set(Mutex::new(look));
}

/// Shows the drop-down once the window exists (`kyko dropdown --show`).
pub fn show_at_start() {
    ffi::controller_send(crate::control::Command::Show.code());
}

/// A new look (the config or theme changed): recolour the panes and have the item reload.
pub fn set_look(look: Look) {
    let colors = look.colors.clone();
    *LOOK.get().expect("configure ran first").lock().unwrap() = look;
    if let Some(s) = SESSION.get() {
        s.set_colors(colors);
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
        "radius" => n(l.radius),
        "gap_out" => n(l.gap_out),
        "border_width" => n(l.border_width),
        "sheet" => b(l.sheet),
        "height_pct" => n(l.height_pct),
        "width_pct" => n(l.width_pct),
        "above_fullscreen" => b(l.above_fullscreen),
        "hide_on_focus_loss" => b(l.hide_on_focus_loss),
        "animations" => b(l.animations),
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

/// The window's exit code: the shell's status, once it has ended.
pub fn exit_status() -> Option<i32> {
    SESSION.get().and_then(|s| s.frame.lock().unwrap().exited)
}

/// Ends the pane and `kykod` with it. Called after the Qt event loop returns.
pub fn shutdown() {
    if let Some(s) = SESSION.get() {
        s.request_end();
        s.wait();
    }
}

fn frame() -> Option<Frame> {
    SESSION.get().map(|s| s.frame.lock().unwrap().clone())
}

fn kyko_start(cols: u16, rows: u16) {
    if SESSION.get().is_some() {
        return kyko_resize(cols, rows);
    }
    let s = setup();
    let Some(launch) = s.launch.lock().unwrap().take() else {
        return;
    };
    let session = Session::start(
        s.socket.clone(),
        launch,
        cols,
        rows,
        colors(),
        Box::new(ffi::grid_request_repaint),
    );
    let _ = SESSION.set(session);
}

fn kyko_resize(cols: u16, rows: u16) {
    if let Some(s) = SESSION.get() {
        s.resize(cols, rows);
    }
}

fn kyko_snapshot(
    out: &mut Vec<ffi::FfiCell>,
    cols: &mut u16,
    rows: &mut u16,
    cur_x: &mut i32,
    cur_y: &mut i32,
) -> u64 {
    out.clear();
    *cols = 0;
    *rows = 0;
    let Some(snap) = frame().and_then(|f| f.snapshot) else {
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

fn kyko_key(key: u32, mods: u8, text: &str) -> bool {
    let Some(s) = SESSION.get() else { return false };
    let app_cursor = s.frame.lock().unwrap().app_cursor;
    let bytes = keys::encode(key, mods, text, app_cursor);
    if bytes.is_empty() {
        return false;
    }
    s.input(bytes);
    true
}

fn kyko_text(text: &str) {
    if let Some(s) = SESSION.get() {
        s.input(text.as_bytes().to_vec());
    }
}

fn kyko_title() -> String {
    frame().map(|f| f.program).unwrap_or_default()
}

fn kyko_fixed_title() -> String {
    setup().title.clone()
}

fn kyko_notice() -> String {
    frame().and_then(|f| f.notice).unwrap_or_default()
}

fn kyko_exit_status() -> i32 {
    exit_status().unwrap_or(-1)
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
    look().font_size
}

/// A test aid: when `KYKO_TEST_SHOT` names a file, the window saves its screen there after a
/// second and exits, so the window can be checked without a display.
fn kyko_test_shot() -> String {
    std::env::var("KYKO_TEST_SHOT").unwrap_or_default()
}
