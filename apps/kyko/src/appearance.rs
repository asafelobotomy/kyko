//! The colours and font the window draws with, from `config.toml` and uQ's theme.
use std::ffi::OsString;

use kyko_core::config::{Bell, Config, Cursor, Keys, NewTabDir, NotifyOnFinish, Shape, TabsAt};
use kyko_core::keys::KeySeq;
use kyko_core::paths;
use kyko_core::theme::{self, Theme};
use kyko_term::Colors;

#[derive(Clone)]
pub struct Look {
    pub colors: Colors,
    pub font: String,
    pub font_size: u16,
    pub accent: u32,
    pub radius: u32,
    pub gap_out: u32,
    pub border_width: u32,
    pub sheet: bool,
    pub height_pct: u32,
    pub width_pct: u32,
    pub above_fullscreen: bool,
    pub hide_on_focus_loss: bool,
    pub animations: bool,
    pub tabs_bottom: bool,
    pub new_tab_dir: NewTabDir,
    pub scrollback: usize,
    pub copy_on_select: bool,
    pub shell_integration: bool,
    pub ligatures: bool,
    pub confirm_multiline_paste: bool,
    /// Line height in percent of the font's (80 to 200).
    pub line_height_pct: u32,
    pub padding: u32,
    /// 0 block, 1 beam, 2 underline.
    pub cursor: u8,
    pub cursor_blink: bool,
    /// 0 none, 1 visual flash, 2 sound.
    pub bell: i32,
    pub notify_on_finish: NotifyOnFinish,
    pub notify_after: u32,
    pub keys: Keys,
    pub hotkey: Option<KeySeq>,
    pub allow_run: bool,
}

/// Where the config and uQ's tokens are, to watch them for changes.
pub fn watched_files() -> Vec<std::path::PathBuf> {
    paths::config_path(env)
        .into_iter()
        .chain(paths::uq_tokens_path(env))
        .collect()
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
    let w = &config.window;
    Look {
        colors: Colors {
            fg: p.foreground,
            bg: p.background,
            cursor: p.cursor,
            ansi: p.ansi,
        },
        font: look.font,
        font_size: u16::try_from(look.font_size).unwrap_or(11),
        accent: look.theme.ui.accent,
        radius: look.theme.metrics.radius,
        gap_out: look.theme.metrics.gap_out,
        border_width: look.theme.metrics.border_width,
        sheet: w.shape == Shape::Sheet,
        height_pct: w.height_pct,
        width_pct: w.width_pct,
        above_fullscreen: w.above_fullscreen,
        hide_on_focus_loss: w.hide_on_focus_loss,
        animations: w.animations,
        tabs_bottom: w.tabs == TabsAt::Bottom,
        new_tab_dir: config.terminal.new_tab_dir,
        scrollback: config.terminal.scrollback as usize,
        copy_on_select: config.terminal.copy_on_select,
        shell_integration: config.terminal.shell_integration,
        ligatures: config.terminal.ligatures,
        confirm_multiline_paste: config.terminal.confirm_multiline_paste,
        line_height_pct: (config.terminal.line_height * 100.0).round() as u32,
        padding: config.terminal.padding,
        cursor: match config.terminal.cursor {
            Cursor::Block => 0,
            Cursor::Beam => 1,
            Cursor::Underline => 2,
        },
        cursor_blink: config.terminal.cursor_blink,
        bell: match config.terminal.bell {
            Bell::None => 0,
            Bell::Visual => 1,
            Bell::Sound => 2,
        },
        notify_on_finish: config.terminal.notify_on_finish,
        notify_after: config.terminal.notify_after,
        keys: config.keys.clone(),
        hotkey: config.hotkey,
        allow_run: config.advanced.allow_run,
    }
}
