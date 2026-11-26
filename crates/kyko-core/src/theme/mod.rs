//! The theme: uQ's `tokens.json`, one of the 14 built-ins (uQ's own themes, rendered by uQ and
//! copied in by `scripts/sync-uq-themes.sh`), or custom terminal colours over a built-in.
//! [`resolve`] turns the configured source into what the window draws with.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::Warning;
use crate::config::Config;

mod tokens;

use tokens::decode;

/// A colour, `0xRRGGBB`, as `kyko_term::Colors` takes it.
pub type Rgb = u32;

/// The built-in used when nothing else is available, and the base of `custom` by default.
pub const FALLBACK: &str = "gruvbox-dark";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Dark,
    Light,
}

impl Variant {
    pub fn as_str(self) -> &'static str {
        match self {
            Variant::Dark => "dark",
            Variant::Light => "light",
        }
    }
}

/// uQ's semantic colours, for the window's own chrome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiColors {
    pub bg: Rgb,
    pub surface: Rgb,
    pub surface_alt: Rgb,
    pub overlay: Rgb,
    pub text: Rgb,
    pub text_muted: Rgb,
    pub accent: Rgb,
    pub accent_text: Rgb,
    pub border: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
}

/// uQ's `ui` sizes, in pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiMetrics {
    pub radius: u32,
    pub gap_in: u32,
    pub gap_out: u32,
    pub border_width: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeFonts {
    pub ui: String,
    pub mono: String,
    pub size: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermPalette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub selection_text: Rgb,
    pub ansi: [Rgb; 16],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// uQ's id (`gruvbox`); with the variant it names a built-in.
    pub id: String,
    pub name: String,
    pub variant: Variant,
    pub ui: UiColors,
    pub metrics: UiMetrics,
    pub fonts: ThemeFonts,
    pub term: TermPalette,
}

impl Theme {
    /// The built-in key this theme would have: `<id>-<variant>`.
    pub fn key(&self) -> String {
        format!("{}-{}", self.id, self.variant.as_str())
    }
}

/// `theme.source`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ThemeSource {
    /// Unset: uQ's theme when uQ is running, else [`FALLBACK`].
    #[default]
    Auto,
    Uq,
    /// A built-in key, `gruvbox-dark`.
    Builtin(String),
    Custom,
}

/// `[theme.custom]`: colours laid over the terminal palette of the built-in `base`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomColors {
    pub base: String,
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    pub cursor: Option<Rgb>,
    pub selection: Option<Rgb>,
    pub selection_text: Option<Rgb>,
    pub ansi: [Option<Rgb>; 16],
}

impl Default for CustomColors {
    fn default() -> Self {
        CustomColors {
            base: FALLBACK.to_string(),
            foreground: None,
            background: None,
            cursor: None,
            selection: None,
            selection_text: None,
            ansi: [None; 16],
        }
    }
}

/// `#rrggbb`, either case. Nothing else.
pub fn parse_rgb(s: &str) -> Option<Rgb> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// The built-in theme files, by key, sorted.
const BUILTIN_FILES: [(&str, &str); 14] = [
    (
        "dracula-dark",
        include_str!("../../themes/dracula-dark.json"),
    ),
    (
        "dracula-light",
        include_str!("../../themes/dracula-light.json"),
    ),
    ("forest-dark", include_str!("../../themes/forest-dark.json")),
    (
        "forest-light",
        include_str!("../../themes/forest-light.json"),
    ),
    (
        "graphite-dark",
        include_str!("../../themes/graphite-dark.json"),
    ),
    (
        "graphite-light",
        include_str!("../../themes/graphite-light.json"),
    ),
    (
        "gruvbox-dark",
        include_str!("../../themes/gruvbox-dark.json"),
    ),
    (
        "gruvbox-light",
        include_str!("../../themes/gruvbox-light.json"),
    ),
    ("matcha-dark", include_str!("../../themes/matcha-dark.json")),
    (
        "matcha-light",
        include_str!("../../themes/matcha-light.json"),
    ),
    (
        "rosewood-dark",
        include_str!("../../themes/rosewood-dark.json"),
    ),
    (
        "rosewood-light",
        include_str!("../../themes/rosewood-light.json"),
    ),
    ("velvet-dark", include_str!("../../themes/velvet-dark.json")),
    (
        "velvet-light",
        include_str!("../../themes/velvet-light.json"),
    ),
];

/// The raw text of a built-in, for tests.
#[doc(hidden)]
pub fn builtin_source(key: &str) -> Option<&'static str> {
    BUILTIN_FILES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, s)| *s)
}

fn builtins() -> &'static [Theme] {
    static ALL: OnceLock<Vec<Theme>> = OnceLock::new();
    ALL.get_or_init(|| {
        BUILTIN_FILES
            .iter()
            .map(|(_, text)| {
                // Decoded against an all-zero theme: a built-in has every field (a test checks
                // that it decodes with no warning), so nothing of the zero theme shows.
                let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
                let map = value.as_object().cloned().unwrap_or_default();
                decode(&map, &zero_theme(), &mut Vec::new())
            })
            .collect()
    })
}

/// A built-in by key, `gruvbox-dark`.
pub fn builtin(key: &str) -> Option<Theme> {
    let i = BUILTIN_FILES.iter().position(|(k, _)| *k == key)?;
    Some(builtins()[i].clone())
}

/// Every built-in key, sorted.
pub fn builtin_keys() -> impl Iterator<Item = &'static str> {
    BUILTIN_FILES.iter().map(|(k, _)| *k)
}

fn fallback_for(variant: Variant) -> Theme {
    let key = match variant {
        Variant::Dark => "gruvbox-dark",
        Variant::Light => "gruvbox-light",
    };
    builtin(key).unwrap_or_else(zero_theme)
}

fn zero_theme() -> Theme {
    let ui = UiColors {
        bg: 0,
        surface: 0,
        surface_alt: 0,
        overlay: 0,
        text: 0,
        text_muted: 0,
        accent: 0,
        accent_text: 0,
        border: 0,
        success: 0,
        warning: 0,
        error: 0,
    };
    Theme {
        id: String::new(),
        name: String::new(),
        variant: Variant::Dark,
        term: palette_from(&ui, [0; 16]),
        ui,
        metrics: UiMetrics {
            radius: 0,
            gap_in: 0,
            gap_out: 0,
            border_width: 0,
        },
        fonts: ThemeFonts {
            ui: String::new(),
            mono: String::new(),
            size: 0,
        },
    }
}

/// The terminal palette uQ gives its own terminals (`templates/kitty.conf.j2`).
fn palette_from(ui: &UiColors, ansi: [Rgb; 16]) -> TermPalette {
    TermPalette {
        foreground: ui.text,
        background: ui.bg,
        cursor: ui.accent,
        selection: ui.accent,
        selection_text: ui.accent_text,
        ansi,
    }
}

/// `tokens.json` is not JSON, or not a JSON object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokensError {
    pub message: String,
}

impl std::fmt::Display for TokensError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TokensError {}

/// What the window draws with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub theme: Theme,
    pub font: String,
    pub font_size: u32,
}

/// The theme and fonts for `config`. `uq_present`: whether `org.uq.Desktop1` is on the bus.
/// `uq_theme`: the last theme the window loaded from uQ's `tokens.json`, if any (the window
/// keeps it across a bad read).
pub fn resolve(
    config: &Config,
    uq_present: bool,
    uq_theme: Option<&Theme>,
) -> (Appearance, Vec<Warning>) {
    let mut warnings = Vec::new();
    let named = |key: &str, what: &str, w: &mut Vec<Warning>| -> Theme {
        builtin(key).unwrap_or_else(|| {
            w.push(Warning::new(
                what,
                format!("no built-in theme {key:?}; using {FALLBACK}"),
            ));
            fallback_for(Variant::Dark)
        })
    };
    let from_uq = |w: &mut Vec<Warning>| -> Theme {
        match uq_theme {
            Some(t) => t.clone(),
            None => {
                w.push(Warning::new(
                    "theme.source",
                    format!("uQ's theme is not available; using {FALLBACK}"),
                ));
                fallback_for(Variant::Dark)
            }
        }
    };
    let theme = match &config.theme {
        ThemeSource::Auto if uq_present => from_uq(&mut warnings),
        ThemeSource::Auto => fallback_for(Variant::Dark),
        ThemeSource::Uq => from_uq(&mut warnings),
        ThemeSource::Builtin(key) => named(key, "theme.source", &mut warnings),
        ThemeSource::Custom => {
            let c = &config.custom;
            let mut t = named(&c.base, "theme.custom.base", &mut warnings);
            let p = &mut t.term;
            p.foreground = c.foreground.unwrap_or(p.foreground);
            p.background = c.background.unwrap_or(p.background);
            p.cursor = c.cursor.unwrap_or(p.cursor);
            p.selection = c.selection.unwrap_or(p.selection);
            p.selection_text = c.selection_text.unwrap_or(p.selection_text);
            for (slot, over) in p.ansi.iter_mut().zip(c.ansi) {
                *slot = over.unwrap_or(*slot);
            }
            t
        }
    };
    let font = config
        .terminal
        .font
        .clone()
        .unwrap_or_else(|| theme.fonts.mono.clone());
    let font_size = config.terminal.font_size.unwrap_or(theme.fonts.size);
    (
        Appearance {
            theme,
            font,
            font_size,
        },
        warnings,
    )
}

#[cfg(test)]
mod tests;
