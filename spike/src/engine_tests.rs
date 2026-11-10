use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use super::*;
use crate::theme::Palette;

fn sh(script: &str, on_dirty: Box<dyn Fn() + Send>) -> Engine {
    Engine::spawn_args("/bin/sh", &["-c", script], 80, 24, Palette::builtin(), on_dirty).unwrap()
}

fn wait_exit(e: &Engine) -> Option<i32> {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Some(c) = e.exited() {
            return Some(c);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

fn run(script: &str) -> Snapshot {
    let e = sh(script, Box::new(|| {}));
    assert!(wait_exit(&e).is_some(), "script did not exit: {script}");
    e.snapshot()
}

fn row_text(s: &Snapshot, row: usize) -> String {
    let cols = s.cols as usize;
    let t: String = s.cells[row * cols..(row + 1) * cols]
        .iter()
        .filter(|c| c.flags & WIDE_SPACER == 0)
        .map(|c| c.ch)
        .collect();
    t.trim_end().to_string()
}

fn spawn_cat() -> Engine {
    sh("cat", Box::new(|| {}))
}

fn wait_for(e: &Engine, f: impl Fn(&Snapshot) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if f(&e.snapshot()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn prints_text() {
    assert_eq!(row_text(&run("printf 'hi there\\n'"), 0), "hi there");
}

#[test]
fn ansi_red_resolves_through_palette() {
    let s = run("printf '\\033[31mX\\033[0m'");
    assert_eq!(s.cells[0].ch, 'X');
    assert_eq!(s.cells[0].fg, 0xcc241d);
}

#[test]
fn truecolor_passes_through() {
    assert_eq!(run("printf '\\033[38;2;1;2;3mX'").cells[0].fg, 0x010203);
}

#[test]
fn wide_char_takes_two_cells() {
    let s = run("printf '日x'");
    assert!(s.cells[0].flags & WIDE != 0 && s.cells[1].flags & WIDE_SPACER != 0);
    assert_eq!(s.cells[2].ch, 'x');
}

#[test]
fn invalid_utf8_does_not_kill_the_engine() {
    let s = run("printf '\\377\\376ok'");
    assert!(row_text(&s, 0).ends_with("ok"));
}

#[test]
fn exit_status_is_reported() {
    let e = sh("exit 7", Box::new(|| {}));
    assert_eq!(wait_exit(&e), Some(7));
}

#[test]
fn resize_clamps_and_survives_zero() {
    let e = spawn_cat();
    e.resize(0, 0);
    let s = e.snapshot();
    assert_eq!((s.cols, s.rows), (2, 1));
    e.resize(120, 40);
    assert_eq!(e.snapshot().cols, 120);
}

#[test]
fn input_reaches_the_shell() {
    let e = spawn_cat();
    e.write(b"echo-me\n");
    assert!(wait_for(&e, |s| row_text(s, 1) == "echo-me"));
}

#[test]
fn on_dirty_is_coalesced() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel::<()>();
    let c = calls.clone();
    let e = sh(
        "seq 1 20000",
        Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            let _ = tx.send(());
        }),
    );
    let end = Instant::now() + Duration::from_secs(10);
    while e.exited().is_none() && Instant::now() < end {
        if rx.recv_timeout(Duration::from_millis(20)).is_ok() {
            let _ = e.snapshot();
        }
    }
    assert!(e.exited().is_some());
    let n = calls.load(Ordering::SeqCst);
    assert!(n >= 1 && n < 2000, "on_dirty calls: {n}");
    assert_eq!(row_text(&e.snapshot(), 22), "20000");
}
