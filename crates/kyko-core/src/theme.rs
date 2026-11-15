//! The theme: uQ's `tokens.json`, one of the 14 built-ins (uQ's own themes, rendered by uQ and
//! copied in by `scripts/sync-uq-themes.sh`), or custom terminal colours over a built-in.
//! [`resolve`] turns the configured source into what the window draws with.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::Warning;
use crate::config::Config;

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
    ("dracula-dark", include_str!("../themes/dracula-dark.json")),
    (
        "dracula-light",
        include_str!("../themes/dracula-light.json"),
    ),
    ("forest-dark", include_str!("../themes/forest-dark.json")),
    ("forest-light", include_str!("../themes/forest-light.json")),
    (
        "graphite-dark",
        include_str!("../themes/graphite-dark.json"),
    ),
    (
        "graphite-light",
        include_str!("../themes/graphite-light.json"),
    ),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.json")),
    (
        "gruvbox-light",
        include_str!("../themes/gruvbox-light.json"),
    ),
    ("matcha-dark", include_str!("../themes/matcha-dark.json")),
    ("matcha-light", include_str!("../themes/matcha-light.json")),
    (
        "rosewood-dark",
        include_str!("../themes/rosewood-dark.json"),
    ),
    (
        "rosewood-light",
        include_str!("../themes/rosewood-light.json"),
    ),
    ("velvet-dark", include_str!("../themes/velvet-dark.json")),
    ("velvet-light", include_str!("../themes/velvet-light.json")),
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

/// The theme font sizes accepted, as `terminal.font_size`.
const FONT_SIZES: std::ops::RangeInclusive<i64> = 6..=48;
/// The `ui` sizes accepted, in pixels.
const METRICS: std::ops::RangeInclusive<i64> = 0..=1000;

impl Theme {
    /// uQ's `tokens.json`. Each missing or bad field takes the built-in gruvbox of the same
    /// variant (dark when the variant is missing or bad too) and adds a warning.
    pub fn from_tokens(json: &str) -> Result<(Theme, Vec<Warning>), TokensError> {
        let value: Value = serde_json::from_str(json).map_err(|e| TokensError {
            message: format!("not JSON: {e}"),
        })?;
        let Value::Object(map) = value else {
            return Err(TokensError {
                message: "not a JSON object".into(),
            });
        };
        let mut warnings = Vec::new();
        let variant = match map.get("variant").and_then(Value::as_str) {
            Some("dark") => Variant::Dark,
            Some("light") => Variant::Light,
            _ => {
                warnings.push(Warning::new("variant", "not \"dark\" or \"light\""));
                Variant::Dark
            }
        };
        let mut theme = decode(&map, &fallback_for(variant), &mut warnings);
        theme.variant = variant;
        Ok((theme, warnings))
    }
}

/// Every field of `map`, or `fb`'s with a warning. `variant` is read here too, for built-ins.
fn decode(map: &Map<String, Value>, fb: &Theme, w: &mut Vec<Warning>) -> Theme {
    let empty = Map::new();
    let section = |name: &str, w: &mut Vec<Warning>| -> &Map<String, Value> {
        match map.get(name) {
            Some(Value::Object(m)) => m,
            Some(_) => {
                w.push(Warning::new(name, "not an object"));
                &empty
            }
            None => {
                w.push(Warning::new(name, "missing"));
                &empty
            }
        }
    };
    let string = |m: &Map<String, Value>, key: &str, fb: &str, w: &mut Vec<Warning>| -> String {
        match m.get(key).and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                w.push(Warning::new(key, "missing or not a non-empty string"));
                fb.to_string()
            }
        }
    };
    let id = string(map, "id", &fb.id, w);
    let name = string(map, "name", &fb.name, w);
    let variant = match map.get("variant").and_then(Value::as_str) {
        Some("light") => Variant::Light,
        Some("dark") => Variant::Dark,
        _ => fb.variant,
    };

    let colors = section("colors", w);
    let col = |key: &str, fb: Rgb, w: &mut Vec<Warning>| -> Rgb {
        match colors.get(key).and_then(Value::as_str).and_then(parse_rgb) {
            Some(c) => c,
            None => {
                w.push(Warning::new(
                    format!("colors.{key}"),
                    "missing or not #rrggbb",
                ));
                fb
            }
        }
    };
    let f = &fb.ui;
    let ui = UiColors {
        bg: col("bg", f.bg, w),
        surface: col("surface", f.surface, w),
        surface_alt: col("surface_alt", f.surface_alt, w),
        overlay: col("overlay", f.overlay, w),
        text: col("text", f.text, w),
        text_muted: col("text_muted", f.text_muted, w),
        accent: col("accent", f.accent, w),
        accent_text: col("accent_text", f.accent_text, w),
        border: col("border", f.border, w),
        success: col("success", f.success, w),
        warning: col("warning", f.warning, w),
        error: col("error", f.error, w),
    };

    let int = |m: &Map<String, Value>,
               dotted: &str,
               range: std::ops::RangeInclusive<i64>,
               fb: u32,
               w: &mut Vec<Warning>|
     -> u32 {
        let key = dotted.rsplit('.').next().unwrap_or(dotted);
        match m.get(key).and_then(Value::as_i64) {
            Some(n) if range.contains(&n) => n as u32,
            _ => {
                w.push(Warning::new(
                    dotted,
                    format!(
                        "missing or not an integer in {}–{}",
                        range.start(),
                        range.end()
                    ),
                ));
                fb
            }
        }
    };
    let metrics_map = section("ui", w);
    let fm = &fb.metrics;
    let metrics = UiMetrics {
        radius: int(metrics_map, "ui.radius", METRICS, fm.radius, w),
        gap_in: int(metrics_map, "ui.gap_in", METRICS, fm.gap_in, w),
        gap_out: int(metrics_map, "ui.gap_out", METRICS, fm.gap_out, w),
        border_width: int(metrics_map, "ui.border_width", METRICS, fm.border_width, w),
    };

    let fonts_map = section("fonts", w);
    let font = |key: &str, fb: &str, w: &mut Vec<Warning>| -> String {
        match fonts_map.get(key).and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                w.push(Warning::new(
                    format!("fonts.{key}"),
                    "missing or not a non-empty string",
                ));
                fb.to_string()
            }
        }
    };
    let fonts = ThemeFonts {
        ui: font("ui", &fb.fonts.ui, w),
        mono: font("mono", &fb.fonts.mono, w),
        size: int(fonts_map, "fonts.size", FONT_SIZES, fb.fonts.size, w),
    };

    let mut ansi = fb.term.ansi;
    match section("terminal", w).get("ansi") {
        Some(Value::Array(list)) => {
            if list.len() != 16 {
                w.push(Warning::new(
                    "terminal.ansi",
                    format!("{} colours, not 16", list.len()),
                ));
            }
            for (i, slot) in ansi.iter_mut().enumerate() {
                match list.get(i).and_then(Value::as_str).and_then(parse_rgb) {
                    Some(c) => *slot = c,
                    None if i < list.len() => {
                        w.push(Warning::new(format!("terminal.ansi[{i}]"), "not #rrggbb"));
                    }
                    None => {}
                }
            }
        }
        _ => w.push(Warning::new("terminal.ansi", "missing or not an array")),
    }

    Theme {
        id,
        name,
        variant,
        term: palette_from(&ui, ansi),
        ui,
        metrics,
        fonts,
    }
}

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
mod tests {
    use super::*;

    #[test]
    fn rgb() {
        assert_eq!(parse_rgb("#282828"), Some(0x282828));
        assert_eq!(parse_rgb("#AbCdEf"), Some(0xabcdef));
        for bad in [
            "282828",
            "#282",
            "#2828281",
            "#28282g",
            "red",
            "#28282880",
            "",
            "#",
        ] {
            assert_eq!(parse_rgb(bad), None, "{bad}");
        }
    }

    #[test]
    fn fourteen_sorted_builtins() {
        let keys: Vec<_> = builtin_keys().collect();
        assert_eq!(keys.len(), 14);
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert!(keys.contains(&FALLBACK));
        assert_eq!(builtin("nope"), None);
    }
}
