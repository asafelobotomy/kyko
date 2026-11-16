use std::process::ExitCode;

use kyko_prompt::{Input, Shell, git_status, render};

const USAGE: &str =
    "usage: kyko-prompt render --shell bash|zsh|fish [--status N] [--duration MS] [--jobs N]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("kyko-prompt {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("render") => {
            let (mut shell, mut status, mut duration_ms, mut jobs) = (None, 0i32, 0u64, 0u32);
            while let Some(flag) = args.next() {
                let value = args.next();
                match flag.as_str() {
                    "--shell" => shell = value.as_deref().and_then(Shell::parse),
                    "--status" => {
                        status = value.as_deref().and_then(|v| v.parse().ok()).unwrap_or(0)
                    }
                    "--duration" => {
                        duration_ms = value.as_deref().and_then(|v| v.parse().ok()).unwrap_or(0)
                    }
                    "--jobs" => jobs = value.as_deref().and_then(|v| v.parse().ok()).unwrap_or(0),
                    _ => {
                        eprintln!("kyko-prompt: unknown option {flag}\n{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            let Some(shell) = shell else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let cwd = std::env::current_dir().unwrap_or_default();
            let venv = std::env::var("VIRTUAL_ENV")
                .ok()
                .and_then(|v| v.rsplit('/').next().map(str::to_string))
                .filter(|v| !v.is_empty());
            let input = Input {
                shell,
                status,
                duration_ms,
                jobs,
                cwd: cwd.to_string_lossy().into_owned(),
                home: std::env::var("HOME").unwrap_or_default(),
                venv,
                git: git_status(&cwd),
            };
            print!("{}", render(&input));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
