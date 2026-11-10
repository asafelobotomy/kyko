//! The kyko session holder: the output ring, the PTY, panes and the socket server.

/// One line per event to stderr, which systemd sends to the journal. Never
/// panics: a log that cannot be written (stderr closed) is dropped.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr().lock(), "kykod: {}", format_args!($($arg)*));
    }};
}

pub mod pane;
pub mod pty;
pub mod ring;
pub mod server;
