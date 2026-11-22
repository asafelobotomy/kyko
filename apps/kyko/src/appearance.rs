//! The colours and font the window draws with, from `config.toml` and uQ's theme.
use std::ffi::OsString;

use kyko_core::config::{
    Bell, Config, Cursor, Keys, NewTabDir, NotifyOnFinish, Position, Shape, TabsAt,
};
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
    /// `window.position`: 0 top, 1 bottom, 2 left, 3 right, 4 center.
    pub position: u8,
    /// The drop-down's size where it is now, in percent of the screen's free area.
    pub height_pct: u32,
    pub opacity_pct: u32,
    pub width_pct: u32,
    pub dim_pct: u32,
    /// uQ's motion tokens in ms (fast, base, slow): all 0 under Reduce motion or with
    /// `window.animations` off.
    pub motion: (u32, u32, u32),
    pub above_fullscreen: bool,
    pub hide_on_focus_loss: bool,
    pub animations: bool,
    pub tabs_bottom: bool,
    pub new_tab_dir: NewTabDir,
    pub scrollback: usize,
    pub copy_on_select: bool,
    /// Programs may set the clipboard with OSC 52.
    pub programs_write_clipboard: bool,
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
pub fn watched_files() -> (Option<std::path::PathBuf>, Option<std::path::PathBuf>) {
    (paths::config_path(env), paths::uq_tokens_path(env))
}

fn env(k: &str) -> Option<OsString> {
    std::env::var_os(k)
}

/// uQ's theme from its `tokens.json`, when the file is there and parses.
fn uq_theme() -> Option<Theme> {
    let text = std::fs::read_to_string(paths::uq_tokens_path(env)?).ok()?;
    Theme::from_tokens(&text).ok().map(|(t, _)| t)
}

/// uQ's motion tokens (`tokens.json` `motion`, uQ 0.3.36): fast, base and slow in ms, all 0
/// when the user turned on Reduce motion. Without uQ, or from an older uQ, uQ's defaults.
pub fn uq_motion(tokens: Option<&str>) -> (u32, u32, u32) {
    let v: serde_json::Value = tokens
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_default();
    if v.pointer("/motion/reduce")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return (0, 0, 0);
    }
    let ms = |k: &str, def: u32| {
        v.pointer(&format!("/motion/{k}"))
            .and_then(serde_json::Value::as_u64)
            .map_or(def, |n| n.min(1000) as u32)
    };
    (ms("fast", 100), ms("base", 140), ms("slow", 220))
}

/// What to draw with now. A config that does not load leaves the defaults; the window shows
/// kyko's fallback theme when uQ's is not available.
pub fn load() -> Look {
    let config = paths::config_path(env)
        .and_then(|p| Config::load(&p).ok())
        .map(|l| l.config)
        .unwrap_or_default();
    let tokens = paths::uq_tokens_path(env).and_then(|p| std::fs::read_to_string(p).ok());
    let uq = uq_theme();
    let (look, warnings) = theme::resolve(&config, uq.is_some(), uq.as_ref());
    for w in warnings {
        kyko_diag::problem(
            "kyko",
            kyko_diag::Level::WARN,
            &format!("[KYKO-001] {}: {}", w.key, w.message),
        );
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
        position: match w.position {
            Position::Top => 0,
            Position::Bottom => 1,
            Position::Left => 2,
            Position::Right => 3,
            Position::Center => 4,
        },
        height_pct: w.size_pct().1,
        opacity_pct: w.opacity_pct,
        width_pct: w.size_pct().0,
        dim_pct: w.dim_pct,
        motion: if w.animations {
            uq_motion(tokens.as_deref())
        } else {
            (0, 0, 0)
        },
        above_fullscreen: w.above_fullscreen,
        hide_on_focus_loss: w.hide_on_focus_loss,
        animations: w.animations,
        tabs_bottom: w.tabs == TabsAt::Bottom,
        new_tab_dir: config.terminal.new_tab_dir,
        scrollback: config.terminal.scrollback as usize,
        copy_on_select: config.terminal.copy_on_select,
        programs_write_clipboard: config.advanced.programs_write_clipboard,
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

#[cfg(test)]
mod tests {
    use super::uq_motion;

    #[test]
    fn motion_follows_uq_and_reduce_motion() {
        assert_eq!(uq_motion(None), (100, 140, 220));
        assert_eq!(uq_motion(Some(r#"{"radius": 10}"#)), (100, 140, 220));
        assert_eq!(
            uq_motion(Some(
                r#"{"motion": {"fast": 90, "base": 120, "slow": 99999, "reduce": false}}"#
            )),
            (90, 120, 1000)
        );
        assert_eq!(
            uq_motion(Some(
                r#"{"motion": {"fast": 100, "base": 140, "slow": 220, "reduce": true}}"#
            )),
            (0, 0, 0)
        );
    }
}
