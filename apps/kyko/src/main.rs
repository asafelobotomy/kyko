//! kyko: a drop-down and windowed terminal for the uQ desktop.
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

mod appearance;
mod bridge;
mod session;

/// kyko: a terminal for the uQ desktop.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Window title, instead of the foreground program's name
    #[arg(long)]
    title: Option<String>,
    /// The window's application id (the Wayland app id), for window rules
    #[arg(long, value_name = "ID")]
    class: Option<String>,
    /// Start in this folder
    #[arg(long, short = 'd', alias = "directory", value_name = "DIR")]
    working_directory: Option<PathBuf>,
    /// Run this program and its arguments instead of the shell; the window closes when it ends
    #[arg(short = 'e', long = "exec", num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
    exec: Vec<String>,
}

/// The shell, as the user chose it.
fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (shell, args) = match cli.exec.split_first() {
        Some((program, rest)) => (program.clone(), rest.to_vec()),
        None => (login_shell(), Vec::new()),
    };
    let cwd = cli
        .working_directory
        .or_else(|| std::env::current_dir().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    // This window's own holder: kykod keeps one window at a time, and a window opened with
    // `kyko -e` must not take the drop-down's.
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let socket = runtime
        .join("kyko")
        .join(format!("win-{}.sock", std::process::id()));
    let holder = match session::start_private_holder(&socket) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("kyko: cannot start kykod: {e}");
            return ExitCode::from(1);
        }
    };

    bridge::configure(
        socket,
        session::Launch { shell, args, cwd },
        appearance::load(),
        cli.title.unwrap_or_default(),
    );
    let mut app = QGuiApplication::new();
    if let Some(class) = &cli.class {
        bridge::app_set_id(class);
    }
    bridge::grid_register();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(e) = engine.as_mut() {
        e.load(&QUrl::from("qrc:/qt/qml/org/kyko/app/qml/Main.qml"));
    }
    let code = app.as_mut().map_or(1, |a| a.exec());
    bridge::shutdown();
    session::reap(holder);
    // The shell's own status when it ended the window, else Qt's.
    let status = bridge::exit_status().unwrap_or(code);
    ExitCode::from(u8::try_from(status & 0xff).unwrap_or(1))
}
