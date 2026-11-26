//! Reading uQ's `tokens.json` into a [`Theme`], field by field, with a fallback and a warning
//! for each missing or bad one.

use super::*;

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
pub(crate) fn decode(map: &Map<String, Value>, fb: &Theme, w: &mut Vec<Warning>) -> Theme {
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
