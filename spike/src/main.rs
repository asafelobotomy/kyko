//! kyko-spike: throwaway. `kyko-spike` runs the window; `kyko-spike toggle` signals it.
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

mod bridge;
mod engine;
mod hotkey;
mod theme;
mod toggle;

fn pid_file() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into());
    PathBuf::from(dir).join("kyko-spike.pid")
}

fn send_toggle() -> ExitCode {
    let pid = std::fs::read_to_string(pid_file()).ok().and_then(|s| s.trim().parse::<u32>().ok());
    let alive = pid.is_some_and(|p| {
        std::fs::read_to_string(format!("/proc/{p}/comm")).is_ok_and(|c| c.trim() == "kyko-spike")
    });
    if !alive {
        eprintln!("kyko-spike is not running");
        return ExitCode::from(1);
    }
    let ok = Command::new("kill").args(["-USR1", &pid.unwrap().to_string()]).status().is_ok_and(|s| s.success());
    if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("toggle") {
        return send_toggle();
    }
    toggle::claim_signal();
    let _ = std::fs::write(pid_file(), std::process::id().to_string());
    if std::env::var_os("KYKO_SPIKE_NO_PORTAL").is_none() {
        // The portal may wait on a dialog, so bind off the main thread.
        std::thread::spawn(|| {
            let key = std::env::var("KYKO_SPIKE_HOTKEY").unwrap_or_else(|_| "F12".into());
            let result = hotkey::start_portal_hotkey(&key, || {
                let _ = signal_hook::low_level::raise(signal_hook::consts::SIGUSR1);
            });
            match result {
                Ok(s) => eprintln!("hotkey: bound ({s}), asked for {key}"),
                Err(e) => eprintln!("hotkey: {e}"),
            }
        });
    }
    let mut app = QGuiApplication::new();
    bridge::ffi::grid_register();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(e) = engine.as_mut() {
        e.load(&QUrl::from("qrc:/qt/qml/org/kyko/spike/qml/Main.qml"));
    }
    let code = app.as_mut().map_or(1, |a| a.exec());
    let _ = std::fs::remove_file(pid_file());
    ExitCode::from(code as u8)
}
