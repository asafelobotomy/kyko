//! The kyko session holder: the output ring, the PTY, panes and the socket server.

/// One line per event to stderr, which systemd sends to the journal.
macro_rules! log {
    ($($arg:tt)*) => { eprintln!("kykod: {}", format_args!($($arg)*)) };
}

pub mod pane;
pub mod pty;
pub mod ring;
pub mod server;
