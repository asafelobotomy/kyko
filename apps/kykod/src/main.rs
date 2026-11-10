use std::io::Write;
use std::path::PathBuf;

use clap::Parser;

/// kykod: the kyko session holder, which keeps terminal sessions running.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The socket to serve [default: $XDG_RUNTIME_DIR/kyko/hold.sock]
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    let path = match cli.socket {
        Some(path) => path,
        None => match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
            Some(dir) => PathBuf::from(dir).join("kyko").join("hold.sock"),
            None => {
                fail("XDG_RUNTIME_DIR is not set; pass --socket PATH");
            }
        },
    };
    let Err(e) = kykod::server::run(&path);
    fail(&e);
}

/// Logs why kykod cannot run and exits 1; a closed stderr does not change the exit code.
fn fail(why: &str) -> ! {
    let _ = writeln!(std::io::stderr().lock(), "kykod: {why}");
    std::process::exit(1);
}
