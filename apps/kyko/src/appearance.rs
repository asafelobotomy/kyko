//! The colours and font the window draws with, from `config.toml` and uQ's theme.
use std::ffi::OsString;

use kyko_core::config::Config;
use kyko_core::paths;
use kyko_core::theme::{self, Theme};
use kyko_term::Colors;

pub struct Look {
    pub colors: Colors,
    pub font: String,
    pub font_size: u16,
}

fn env(k: &str) -> Option<OsString> {
    std::env::var_os(k)
}

/// uQ's theme from its `tokens.json`, when the file is there and parses.
fn uq_theme() -> Option<Theme> {
    let text = std::fs::read_to_string(paths::uq_tokens_path(env)?).ok()?;
    Theme::from_tokens(&text).ok().map(|(t, _)| t)
}

/// What to draw with now. A config that does not load leaves the defaults; the window shows
/// kyko's fallback theme when uQ's is not available.
pub fn load() -> Look {
    let config = paths::config_path(env)
        .and_then(|p| Config::load(&p).ok())
        .map(|l| l.config)
        .unwrap_or_default();
    let uq = uq_theme();
    let (look, warnings) = theme::resolve(&config, uq.is_some(), uq.as_ref());
    for w in warnings {
        eprintln!("kyko: {}: {}", w.key, w.message);
    }
    let p = &look.theme.term;
    Look {
        colors: Colors {
            fg: p.foreground,
            bg: p.background,
            cursor: p.cursor,
            ansi: p.ansi,
        },
        font: look.font,
        font_size: u16::try_from(look.font_size).unwrap_or(11),
    }
}
