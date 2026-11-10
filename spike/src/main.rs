//! kyko-spike: throwaway. `kyko-spike` runs the window; `kyko-spike toggle` signals it.
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

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
    let _ = std::fs::write(pid_file(), std::process::id().to_string());
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(e) = engine.as_mut() {
        e.load(&QUrl::from("qrc:/qt/qml/org/kyko/spike/qml/Main.qml"));
    }
    let code = app.as_mut().map_or(1, |a| a.exec());
    let _ = std::fs::remove_file(pid_file());
    ExitCode::from(code as u8)
}
