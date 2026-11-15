mod common;

use common::{Rng, cases};
use kyko_core::config::Config;
use kyko_core::theme::*;
use serde_json::{Value, json};

fn rgb(v: &Value) -> Rgb {
    parse_rgb(v.as_str().unwrap()).unwrap()
}

#[test]
fn every_builtin_loads_without_a_warning_and_matches_its_file() {
    for key in builtin_keys() {
        let text = builtin_source(key).unwrap();
        let (t, w) = Theme::from_tokens(text).unwrap();
        assert!(w.is_empty(), "{key}: {w:?}");
        assert_eq!(t.key(), key);
        assert_eq!(builtin(key).as_ref(), Some(&t));
        let v: Value = serde_json::from_str(text).unwrap();
        let ansi: Vec<Rgb> = v["terminal"]["ansi"]
            .as_array()
            .unwrap()
            .iter()
            .map(rgb)
            .collect();
        assert_eq!(t.term.ansi.to_vec(), ansi, "{key}");
        // uQ's kitty.conf.j2 mapping.
        let c = &v["colors"];
        assert_eq!(t.term.foreground, rgb(&c["text"]), "{key}");
        assert_eq!(t.term.background, rgb(&c["bg"]), "{key}");
        assert_eq!(t.term.cursor, rgb(&c["accent"]), "{key}");
        assert_eq!(t.term.selection, rgb(&c["accent"]), "{key}");
        assert_eq!(t.term.selection_text, rgb(&c["accent_text"]), "{key}");
        assert_eq!(t.fonts.mono, v["fonts"]["mono"].as_str().unwrap());
        assert_eq!(
            u64::from(t.fonts.size),
            v["fonts"]["size"].as_u64().unwrap()
        );
    }
}

/// The shape of the user's tokens.json: every field, as uQ writes it.
fn user_tokens(variant: &str) -> Value {
    let key = format!("velvet-{variant}");
    serde_json::from_str(builtin_source(&key).unwrap()).unwrap()
}

/// Every field path in `v`, leaves only (`colors.bg`, `terminal.ansi`).
fn leaves(v: &Value) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for (k, child) in v.as_object().unwrap() {
        if k == "icons" {
            continue; // not read
        }
        match child {
            Value::Object(m) => {
                for k2 in m.keys() {
                    out.push(vec![k.clone(), k2.clone()]);
                }
            }
            _ => out.push(vec![k.clone()]),
        }
    }
    out
}

fn at<'a>(v: &'a mut Value, path: &[String]) -> &'a mut Value {
    path.iter().fold(v, |v, k| &mut v[k.as_str()])
}

#[test]
fn a_missing_or_bad_field_costs_only_that_field() {
    for variant in ["dark", "light"] {
        let full = user_tokens(variant);
        let (good, w) = Theme::from_tokens(&full.to_string()).unwrap();
        assert!(w.is_empty());
        let fb = builtin(&format!("gruvbox-{variant}")).unwrap();
        for path in leaves(&full) {
            let dotted = path.join(".");
            for bad in [
                None,
                Some(json!(42)),
                Some(json!("#nothex")),
                Some(json!(null)),
            ] {
                let mut v = full.clone();
                match &bad {
                    None => {
                        let parent = at(&mut v, &path[..path.len() - 1]);
                        parent
                            .as_object_mut()
                            .unwrap()
                            .remove(&path[path.len() - 1]);
                    }
                    Some(b) => *at(&mut v, &path) = b.clone(),
                }
                // 42 is a good value for the integer fields.
                if bad == Some(json!(42)) && (path[0] == "ui" || dotted == "fonts.size") {
                    continue;
                }
                // Any non-empty string is a good name or font.
                let text_field =
                    ["id", "name", "fonts.ui", "fonts.mono"].contains(&dotted.as_str());
                if bad == Some(json!("#nothex")) && text_field {
                    continue;
                }
                let (t, w) = Theme::from_tokens(&v.to_string()).unwrap();
                assert_eq!(w.len(), 1, "{variant} {dotted} {bad:?}: {w:?}");
                assert_eq!(w[0].key, dotted, "{bad:?}");
                // Only that field changed, and to the gruvbox of the same variant.
                let mut want = good.clone();
                match dotted.as_str() {
                    "id" => want.id = fb.id.clone(),
                    "name" => want.name = fb.name.clone(),
                    "variant" => {
                        want.variant = Variant::Dark;
                        // The dark fallback is used for nothing else: every other field is good.
                    }
                    "colors.bg" => {
                        want.ui.bg = fb.ui.bg;
                        want.term.background = fb.ui.bg;
                    }
                    "colors.text" => {
                        want.ui.text = fb.ui.text;
                        want.term.foreground = fb.ui.text;
                    }
                    "colors.accent" => {
                        want.ui.accent = fb.ui.accent;
                        want.term.cursor = fb.ui.accent;
                        want.term.selection = fb.ui.accent;
                    }
                    "colors.accent_text" => {
                        want.ui.accent_text = fb.ui.accent_text;
                        want.term.selection_text = fb.ui.accent_text;
                    }
                    "terminal.ansi" => want.term.ansi = fb.term.ansi,
                    "fonts.ui" => want.fonts.ui = fb.fonts.ui.clone(),
                    "fonts.mono" => want.fonts.mono = fb.fonts.mono.clone(),
                    "fonts.size" => want.fonts.size = fb.fonts.size,
                    "ui.radius" => want.metrics.radius = fb.metrics.radius,
                    "ui.gap_in" => want.metrics.gap_in = fb.metrics.gap_in,
                    "ui.gap_out" => want.metrics.gap_out = fb.metrics.gap_out,
                    "ui.border_width" => want.metrics.border_width = fb.metrics.border_width,
                    other => {
                        let name = other.strip_prefix("colors.").unwrap();
                        let (wu, fu) = (&mut want.ui, &fb.ui);
                        let slot = match name {
                            "surface" => (&mut wu.surface, fu.surface),
                            "surface_alt" => (&mut wu.surface_alt, fu.surface_alt),
                            "overlay" => (&mut wu.overlay, fu.overlay),
                            "text_muted" => (&mut wu.text_muted, fu.text_muted),
                            "border" => (&mut wu.border, fu.border),
                            "success" => (&mut wu.success, fu.success),
                            "warning" => (&mut wu.warning, fu.warning),
                            "error" => (&mut wu.error, fu.error),
                            n => panic!("unexpected field {n}"),
                        };
                        *slot.0 = slot.1;
                    }
                }
                assert_eq!(t, want, "{variant} {dotted} {bad:?}");
            }
        }
    }
}

#[test]
fn ansi_entries_fall_back_one_by_one() {
    let mut v = user_tokens("light");
    v["terminal"]["ansi"][3] = json!("blue");
    let (t, w) = Theme::from_tokens(&v.to_string()).unwrap();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].key, "terminal.ansi[3]");
    let fb = builtin("gruvbox-light").unwrap();
    assert_eq!(t.term.ansi[3], fb.term.ansi[3]);
    assert_eq!(t.term.ansi[4], rgb(&v["terminal"]["ansi"][4]));
    // Too few: the rest from the fallback.
    v["terminal"]["ansi"] = json!(["#000001", "#000002"]);
    let (t, w) = Theme::from_tokens(&v.to_string()).unwrap();
    assert_eq!(w[0].key, "terminal.ansi");
    assert_eq!(&t.term.ansi[..3], &[1, 2, fb.term.ansi[2]]);
}

#[test]
fn empty_object_falls_back_with_warnings() {
    let (t, w) = Theme::from_tokens("{}").unwrap();
    let mut want = builtin("gruvbox-dark").unwrap();
    want.variant = Variant::Dark;
    assert_eq!(t, want);
    assert!(w.len() > 10, "{w:?}");
    assert!(w.iter().any(|w| w.key == "variant"));
    let (t, _) = Theme::from_tokens(r#"{"variant":"light"}"#).unwrap();
    assert_eq!(t, builtin("gruvbox-light").unwrap());
}

#[test]
fn not_an_object_is_an_error() {
    for bad in ["[]", "3", "\"x\"", "null", "", "{", "{\"a\":}"] {
        assert!(Theme::from_tokens(bad).is_err(), "{bad}");
    }
}

fn config(text: &str) -> Config {
    let l = Config::parse(text).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    l.config
}

#[test]
fn resolve_each_source() {
    let gruvbox = builtin(FALLBACK).unwrap();
    let mine = builtin("velvet-light").unwrap();
    let auto = config("");
    // Auto: uQ's when uQ is present and has a theme.
    let (a, w) = resolve(&auto, true, Some(&mine));
    assert_eq!((a.theme, w.len()), (mine.clone(), 0));
    // uQ present, no theme yet: the fallback and a warning.
    let (a, w) = resolve(&auto, true, None);
    assert_eq!(a.theme, gruvbox);
    assert_eq!(w.len(), 1);
    // uQ absent: the fallback, quietly, even with a stale uQ theme.
    let (a, w) = resolve(&auto, false, Some(&mine));
    assert_eq!((a.theme, w.len()), (gruvbox.clone(), 0));
    // Uq asked for: the file wins even without uQ on the bus.
    let uq = config("[theme]\nsource = \"uq\"");
    let (a, w) = resolve(&uq, false, Some(&mine));
    assert_eq!((a.theme, w.len()), (mine.clone(), 0));
    let (a, w) = resolve(&uq, false, None);
    assert_eq!((a.theme, w.len()), (gruvbox.clone(), 1));
    // Builtin.
    let b = config("[theme]\nsource = \"builtin:dracula-light\"");
    let (a, w) = resolve(&b, true, Some(&mine));
    assert_eq!((a.theme, w.len()), (builtin("dracula-light").unwrap(), 0));
    let b = config("[theme]\nsource = \"builtin:solarized-dark\"");
    let (a, w) = resolve(&b, true, Some(&mine));
    assert_eq!(a.theme, gruvbox);
    assert_eq!(w[0].key, "theme.source");
}

#[test]
fn custom_over_a_base() {
    let c = config(
        "[theme]\nsource = \"custom\"\n[theme.custom]\nbase = \"matcha-light\"\n\
         foreground = \"#010203\"\nselection_text = \"#0a0b0c\"\n\
         ansi = [\"#000001\", \"#000002\", \"#000003\"]\n",
    );
    let base = builtin("matcha-light").unwrap();
    let (a, w) = resolve(&c, true, None);
    assert!(w.is_empty());
    assert_eq!(a.theme.ui, base.ui, "the chrome is the base's");
    assert_eq!(a.theme.variant, Variant::Light);
    assert_eq!(a.theme.term.foreground, 0x010203);
    assert_eq!(a.theme.term.selection_text, 0x0a0b0c);
    assert_eq!(a.theme.term.background, base.term.background);
    assert_eq!(&a.theme.term.ansi[..4], &[1, 2, 3, base.term.ansi[3]]);
    // An unknown base: the fallback, a warning, the colours still laid over it.
    let c = config(
        "[theme]\nsource = \"custom\"\n[theme.custom]\nbase = \"nope\"\ncursor = \"#ffffff\"\n",
    );
    let (a, w) = resolve(&c, false, None);
    assert_eq!(w[0].key, "theme.custom.base");
    assert_eq!(a.theme.ui, builtin(FALLBACK).unwrap().ui);
    assert_eq!(a.theme.term.cursor, 0xffffff);
    // Twenty entries: the first sixteen are used (config warned about the rest).
    let twenty: Vec<String> = (1..=20).map(|i| format!("\"#0000{i:02x}\"")).collect();
    let l = Config::parse(&format!(
        "[theme]\nsource = \"custom\"\n[theme.custom]\nansi = [{}]\n",
        twenty.join(",")
    ))
    .unwrap();
    let (a, _) = resolve(&l.config, false, None);
    assert_eq!(a.theme.term.ansi[15], 16);
}

#[test]
fn fonts_from_config_or_theme() {
    let mine = builtin("velvet-dark").unwrap();
    let (a, _) = resolve(&config(""), true, Some(&mine));
    assert_eq!(
        (a.font.as_str(), a.font_size),
        (mine.fonts.mono.as_str(), mine.fonts.size)
    );
    let (a, _) = resolve(
        &config("[terminal]\nfont = \"Iosevka\"\nfont_size = 15"),
        true,
        Some(&mine),
    );
    assert_eq!((a.font.as_str(), a.font_size), ("Iosevka", 15));
    let (a, _) = resolve(&config("[terminal]\nfont_size = 9"), true, Some(&mine));
    assert_eq!(
        (a.font.as_str(), a.font_size),
        (mine.fonts.mono.as_str(), 9)
    );
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng::new(0x7E3E);
    let source = builtin_source("forest-dark").unwrap().as_bytes();
    for i in 0..cases(10_000) {
        let bytes = if i % 2 == 0 {
            rng.bytes(300)
        } else {
            let mut b = source.to_vec();
            for _ in 0..=rng.below(6) {
                let j = rng.below(b.len());
                b[j] = *rng.pick(b"{}[]\":,#0afz \n");
            }
            b
        };
        let _ = Theme::from_tokens(&String::from_utf8_lossy(&bytes));
    }
}
