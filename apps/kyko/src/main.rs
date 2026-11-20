//! kyko: a drop-down and windowed terminal for the uQ desktop.
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

mod appearance;
mod bridge;
mod control;
mod hotkey;
mod icat;
mod prefs;
mod remote;
mod session;
mod watch;

/// kyko: a terminal for the uQ desktop.
#[derive(Parser)]
#[command(version, about, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Sub>,
    /// Window title, instead of the foreground program's name
    #[arg(long)]
    title: Option<String>,
    /// The window's application id (the Wayland app id), for window rules
    #[arg(long, value_name = "ID")]
    class: Option<String>,
    /// Start in this folder
    #[arg(long, short = 'd', alias = "directory", value_name = "DIR")]
    working_directory: Option<PathBuf>,
    /// Open the tabs a session file describes: one `kyko @` command per line (launch, set-tab-title...)
    #[arg(long, value_name = "FILE")]
    session: Option<PathBuf>,
    /// Run this program and its arguments instead of the shell; the window closes when it ends
    #[arg(short = 'e', long = "exec", num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
    exec: Vec<String>,
}

#[derive(Subcommand)]
enum Sub {
    /// Run the drop-down: a terminal that slides down from the top edge and keeps its sessions
    Dropdown {
        /// Show it at start instead of waiting for `kyko toggle`
        #[arg(long)]
        show: bool,
    },
    /// Show the drop-down, or hide it when it is shown
    Toggle,
    /// Show the drop-down
    Show,
    /// Hide the drop-down
    Hide,
    /// Run a command in a new tab of the drop-down; the tab stays open after it ends
    Run {
        /// The command, as the shell would read it
        #[arg(num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
        command: Vec<String>,
        /// Start in this folder
        #[arg(long, short = 'd', value_name = "DIR")]
        working_directory: Option<PathBuf>,
    },
    /// Open the Preferences window
    Preferences,
    /// Control a running kyko window: `kyko @ ls`, `kyko @ send-text`, `kyko @ launch`... (`kyko @ help`)
    #[command(name = "@", alias = "remote", disable_help_flag = true)]
    Remote {
        #[arg(num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
        words: Vec<String>,
    },
    /// Show images in the terminal (kitty's graphics protocol); PNG, or any format ImageMagick reads
    Icat {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
    },
    /// Open a new tab in the drop-down
    NewTab {
        /// Start in this folder
        #[arg(long, short = 'd', value_name = "DIR")]
        working_directory: Option<PathBuf>,
    },
}

/// The shell, as the user chose it.
fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into())
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// The folder to send: the one given, else this process's own, else none.
fn dir_arg(dir: Option<PathBuf>) -> String {
    dir.or_else(|| std::env::current_dir().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn tab_request(request: control::Request) -> ExitCode {
    match control::request(&request) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kyko: {e}");
            ExitCode::from(1)
        }
    }
}

fn control(command: control::Command) -> ExitCode {
    match control::call(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kyko: {e}");
            ExitCode::from(1)
        }
    }
}

/// The Preferences window: no sessions, no `kykod`; it only edits `config.toml`.
fn preferences_window() -> ExitCode {
    let launch = session::Launch {
        shell: login_shell(),
        args: Vec::new(),
        cwd: String::new(),
        login_shell: login_shell(),
        persistent: false,
        startup: Vec::new(),
    };
    bridge::configure(PathBuf::new(), launch, appearance::load(), String::new());
    let mut app = QGuiApplication::new();
    bridge::app_set_id("org.kyko.Kyko.Preferences");
    bridge::grid_register();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(e) = engine.as_mut() {
        e.load(&QUrl::from("qrc:/qt/qml/org/kyko/app/qml/Preferences.qml"));
    }
    let code = app.as_mut().map_or(1, |a| a.exec());
    ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1))
}

/// `kyko @ ...`: one request to the window in `$KYKO_LISTEN_ON` (or `--to`).
fn remote_cli(words: &[String]) -> ExitCode {
    use kyko_core::remote::{Match, Request, parse_args};
    let parsed = match parse_args(words) {
        Ok(p) => p,
        Err(e) => {
            let usage = e.starts_with("usage");
            eprintln!("{}{e}", if usage { "" } else { "kyko @: " });
            return ExitCode::from(if usage && words.first().is_some_and(|w| w == "help") {
                0
            } else {
                2
            });
        }
    };
    let Some(to) = parsed.to.clone().or_else(|| {
        std::env::var("KYKO_LISTEN_ON")
            .ok()
            .filter(|s| !s.is_empty())
    }) else {
        eprintln!("kyko @: not inside kyko: $KYKO_LISTEN_ON is not set; give --to unix:PATH");
        return ExitCode::from(1);
    };
    let mut request = parsed.request;
    // Inside kyko and talking to its own window, "here" is the pane the command runs in.
    let here = parsed
        .to
        .is_none()
        .then(|| std::env::var("KYKO_PANE_ID").ok()?.parse::<u32>().ok())
        .flatten();
    if let Some(id) = here {
        match &mut request {
            Request::SendText { target, .. }
            | Request::GetText { target, .. }
            | Request::ClosePane { target }
                if *target == Match::Here =>
            {
                *target = Match::Pane(id);
            }
            _ => {}
        }
    }
    if parsed.stdin
        && let Request::SendText { text, .. } = &mut request
    {
        let mut more = String::new();
        if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut more) {
            eprintln!("kyko @: reading standard input: {e}");
            return ExitCode::from(1);
        }
        text.push_str(&more);
    }
    match remote::call(&to, &request) {
        Ok(reply) if reply.ok => {
            match (&request, &reply.data) {
                (Request::GetText { .. }, d) => print!("{}", d["text"].as_str().unwrap_or("")),
                (Request::Launch { .. }, d) => println!("{}", d["pane"]),
                (Request::Ls, d) => {
                    println!("{}", serde_json::to_string_pretty(d).unwrap_or_default())
                }
                _ => {}
            }
            ExitCode::SUCCESS
        }
        Ok(reply) => {
            eprintln!(
                "kyko @: {}",
                reply.error.unwrap_or_else(|| "refused".into())
            );
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("kyko @: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(Sub::Remote { words }) = &cli.command {
        return remote_cli(words);
    }
    if let Some(Sub::Icat { files }) = &cli.command {
        return icat::run(files);
    }
    match cli.command {
        Some(Sub::Toggle) => return control(control::Command::Toggle),
        Some(Sub::Show) => return control(control::Command::Show),
        Some(Sub::Hide) => return control(control::Command::Hide),
        Some(Sub::Run {
            command,
            working_directory,
        }) => {
            let command = command.join(" ");
            if let Err(e) = kyko_core::runq::validate(&command) {
                eprintln!("kyko: {e}");
                return ExitCode::from(2);
            }
            return tab_request(control::Request::Run {
                command,
                cwd: dir_arg(working_directory),
            });
        }
        Some(Sub::NewTab { working_directory }) => {
            return tab_request(control::Request::NewTab {
                cwd: dir_arg(working_directory),
            });
        }
        _ => {}
    }
    if matches!(cli.command, Some(Sub::Preferences)) {
        return preferences_window();
    }
    let dropdown = match cli.command {
        Some(Sub::Dropdown { show }) => Some(show),
        _ => None,
    };

    let startup = match &cli.session {
        Some(path) => {
            let home = std::env::var("HOME").unwrap_or_default();
            match std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|t| kyko_core::remote::parse_session(&t, &home))
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("kyko: {}: {e}", path.display());
                    return ExitCode::from(2);
                }
            }
        }
        None => Vec::new(),
    };
    let (shell, args) = match cli.exec.split_first() {
        Some((program, rest)) => (program.clone(), rest.to_vec()),
        None => (login_shell(), Vec::new()),
    };
    let cwd = cli
        .working_directory
        .or_else(|| std::env::current_dir().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    // A window opened with `kyko -e` has a `kykod` of its own: `kykod` keeps one window at a
    // time, and the drop-down owns the shared one.
    let (socket, holder) = if dropdown.is_some() {
        let socket = runtime_dir().join("kyko").join("hold.sock");
        if let Err(e) = session::ensure_shared_holder(&socket) {
            eprintln!("kyko: cannot start kykod: {e}");
            return ExitCode::from(1);
        }
        (socket, None)
    } else {
        let socket = runtime_dir()
            .join("kyko")
            .join(format!("win-{}.sock", std::process::id()));
        match session::start_private_holder(&socket) {
            Ok(h) => (socket, Some(h)),
            Err(e) => {
                eprintln!("kyko: cannot start kykod: {e}");
                return ExitCode::from(1);
            }
        }
    };

    let launch = session::Launch {
        shell,
        args,
        cwd,
        login_shell: login_shell(),
        persistent: dropdown.is_some(),
        startup,
    };
    bridge::configure(
        socket,
        launch,
        appearance::load(),
        cli.title.unwrap_or_default(),
    );
    if dropdown.is_some()
        && let Err(e) = bridge::serve_bus()
    {
        eprintln!(
            "kyko: cannot serve {}: {e} (is the drop-down already running?)",
            control::NAME
        );
        return ExitCode::from(1);
    }
    bridge::start();
    if dropdown.is_some() {
        hotkey::start(appearance::load().hotkey);
    }
    watch::spawn(appearance::watched_files(), move || {
        let look = appearance::load();
        if dropdown.is_some() {
            hotkey::apply(look.hotkey);
        }
        bridge::set_look(look)
    });

    bridge::app_set_translucent();
    let mut app = QGuiApplication::new();
    bridge::measure_cells();
    // Matches `org.kyko.Kyko.desktop`, so the compositor shows the desktop entry's icon.
    bridge::app_set_id(cli.class.as_deref().unwrap_or("org.kyko.Kyko"));
    bridge::grid_register();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(e) = engine.as_mut() {
        let file = if dropdown.is_some() {
            "DropDown"
        } else {
            "Main"
        };
        e.load(&QUrl::from(&format!(
            "qrc:/qt/qml/org/kyko/app/qml/{file}.qml"
        )));
    }
    if dropdown == Some(true) {
        bridge::show_at_start();
    }
    let code = app.as_mut().map_or(1, |a| a.exec());
    bridge::shutdown();
    if let Some(holder) = holder {
        session::reap(holder);
    }
    // The shell's own status when it ended the window, else Qt's.
    let status = bridge::exit_status().unwrap_or(code);
    ExitCode::from(u8::try_from(status & 0xff).unwrap_or(1))
}
