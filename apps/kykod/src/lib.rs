//! The kyko session holder: the output ring, the PTY, panes and the socket server.

/// One line per event to stderr, which systemd sends to the journal. Never
/// panics: a log that cannot be written (stderr closed) is dropped.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr().lock(), "kykod: {}", format_args!($($arg)*));
    }};
}

/// A problem, with its code from `uq/diag/kyko.toml` first: `problem!(WARN, "[KYKO-014] …")`.
/// Under systemd the journal files it at that level.
macro_rules! problem {
    ($level:ident, $($arg:tt)*) => {
        kyko_diag::problem("kykod", kyko_diag::Level::$level, &format!($($arg)*))
    };
}

pub mod modes;
pub mod pane;
pub mod pty;
pub mod ring;
pub mod server;
