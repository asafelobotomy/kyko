//! The C++ grid item's view of the one global engine.
use std::sync::OnceLock;

use crate::engine::Engine;
use crate::theme::Palette;

#[cxx::bridge]
pub mod ffi {
    struct FfiCell {
        ch: u32,
        fg: u32,
        bg: u32,
        flags: u8,
    }

    extern "Rust" {
        fn spike_start(cols: u16, rows: u16);
        fn spike_snapshot(out: &mut Vec<FfiCell>, cols: &mut u16, rows: &mut u16, cur_x: &mut i32, cur_y: &mut i32) -> u64;
        fn spike_write(bytes: &[u8]);
        fn spike_resize(cols: u16, rows: u16);
        fn spike_exit_status() -> i32;
        fn spike_palette_bg() -> u32;
        fn spike_palette_fg() -> u32;
        fn spike_palette_cursor() -> u32;
        fn spike_palette_font() -> String;
        fn spike_palette_font_size() -> u16;
        fn spike_stats_enabled() -> bool;
    }

    unsafe extern "C++" {
        include!("kyko-spike/cpp/griditem.h");
        fn grid_register();
        fn grid_request_repaint();
    }
}

static ENGINE: OnceLock<Engine> = OnceLock::new();
static PALETTE: OnceLock<Palette> = OnceLock::new();

fn palette() -> &'static Palette {
    PALETTE.get_or_init(Palette::load)
}

fn spike_start(cols: u16, rows: u16) {
    if ENGINE.get().is_some() {
        return spike_resize(cols, rows);
    }
    let shell = std::env::var("KYKO_SPIKE_SHELL")
        .or_else(|_| std::env::var("SHELL"))
        .unwrap_or_else(|_| "/bin/sh".into());
    match Engine::spawn(&shell, cols, rows, palette().clone(), Box::new(ffi::grid_request_repaint)) {
        Ok(e) => {
            let _ = ENGINE.set(e);
            // Test aid: KYKO_SPIKE_TYPE is written to the shell half a second after start (\n = Enter).
            if let Ok(text) = std::env::var("KYKO_SPIKE_TYPE") {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    spike_write(text.replace("\\n", "\r").as_bytes());
                });
            }
        },
        Err(e) => eprintln!("kyko-spike: could not start {shell}: {e}"),
    }
}

fn spike_snapshot(out: &mut Vec<ffi::FfiCell>, cols: &mut u16, rows: &mut u16, cur_x: &mut i32, cur_y: &mut i32) -> u64 {
    out.clear();
    let Some(e) = ENGINE.get() else {
        *cols = 0;
        *rows = 0;
        return 0;
    };
    let s = e.snapshot();
    *cols = s.cols;
    *rows = s.rows;
    (*cur_x, *cur_y) = s.cursor.map_or((-1, -1), |(x, y)| (x as i32, y as i32));
    out.extend(s.cells.iter().map(|c| ffi::FfiCell { ch: c.ch as u32, fg: c.fg, bg: c.bg, flags: c.flags }));
    s.seq
}

fn spike_write(bytes: &[u8]) {
    if let Some(e) = ENGINE.get() {
        e.write(bytes);
    }
}

fn spike_resize(cols: u16, rows: u16) {
    if let Some(e) = ENGINE.get() {
        e.resize(cols, rows);
    }
}

fn spike_exit_status() -> i32 {
    ENGINE.get().and_then(Engine::exited).unwrap_or(-1)
}

fn spike_palette_bg() -> u32 {
    palette().bg
}

fn spike_palette_fg() -> u32 {
    palette().fg
}

fn spike_palette_cursor() -> u32 {
    palette().cursor
}

fn spike_palette_font() -> String {
    palette().font.clone()
}

fn spike_palette_font_size() -> u16 {
    palette().font_size
}

fn spike_stats_enabled() -> bool {
    std::env::var_os("KYKO_SPIKE_STATS").is_some()
}
