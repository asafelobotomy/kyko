//! How kyko reports problems and crashes, in uQ's format (uQ's `native/uq-diag`, vendored
//! unchanged as [`uq_diag`]).
//!
//! A panic leaves a crash record that uQ notices and lists on Settings › Problems. A problem
//! starts with its code, `[KYKO-003] …`, listed in `uq/diag/kyko.toml`: under systemd the line
//! is `<4>kyko: [KYKO-003] …`, so the journal files it at its level; in a terminal it reads
//! `kyko: [KYKO-003] …`. kyko writes these lines itself rather than through
//! tracing-subscriber, which would make every binary bigger for a handful of lines.

pub mod uq_diag;

pub use tracing::Level;
use uq_diag::Style;

/// Whether stderr is systemd's journal (kyko or kykod runs as a service).
pub fn journal() -> bool {
    Style::detect() == Style::Journal
}

/// One line for `program`'s `msg`, as [`problem`] writes it.
pub fn line(program: &str, level: Level, msg: &str, journal: bool) -> String {
    if journal {
        format!("<{}>{program}: {msg}", uq_diag::priority(&level))
    } else {
        format!("{program}: {msg}")
    }
}

/// Write a problem to stderr. Never panics: a closed stderr drops the line.
pub fn problem(program: &str, level: Level, msg: &str) {
    use std::io::Write as _;
    let _ = writeln!(
        std::io::stderr().lock(),
        "{}",
        line(program, level, msg, journal())
    );
}

/// Save a crash record if `program` panics. Call it first thing in `main`.
pub fn start(program: &'static str) {
    uq_diag::install_panic_hook(program);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_the_output() {
        assert_eq!(
            line("kyko", Level::WARN, "[KYKO-003] hotkey", true),
            "<4>kyko: [KYKO-003] hotkey"
        );
        assert_eq!(
            line("kykod", Level::ERROR, "[KYKO-011] x", false),
            "kykod: [KYKO-011] x"
        );
    }
}
