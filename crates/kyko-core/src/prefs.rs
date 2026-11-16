//! What Preferences shows (§2.6): the pages and their settings, the current values for the
//! window, and checked changes to `config.toml` text. The window itself is QML; everything
//! that can be tested without Qt lives here.
use serde_json::{Value as Json, json};

use crate::config::{Config, NewTabDir, Shape, TabsAt};
use crate::configedit::{self, Value};

/// What kind of control a setting gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bool,
    Int(i64, i64),
    /// A number that 0 unsets (back to the theme's, say).
    OptInt(i64, i64),
    Choice(&'static [&'static str]),
    /// A text; empty unsets.
    Text,
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub id: &'static str,
    pub title: &'static str,
    pub items: &'static [Item],
}

pub const PAGES: &[Page] = &[
    Page {
        id: "window",
        title: "Window",
        items: &[
            Item {
                key: "window.shape",
                label: "Shape",
                help: "Island floats with a gap; sheet spans the screen's edge.",
                kind: Kind::Choice(&["island", "sheet"]),
            },
            Item {
                key: "window.tabs",
                label: "Tab bar",
                help: "Where the tabs sit.",
                kind: Kind::Choice(&["top", "bottom"]),
            },
            Item {
                key: "window.height_pct",
                label: "Height (%)",
                help: "Of the screen.",
                kind: Kind::Int(20, 100),
            },
            Item {
                key: "window.width_pct",
                label: "Width (%)",
                help: "Of the screen; the island shape only.",
                kind: Kind::Int(40, 100),
            },
            Item {
                key: "window.above_fullscreen",
                label: "Show over full-screen windows",
                help: "",
                kind: Kind::Bool,
            },
            Item {
                key: "window.hide_on_focus_loss",
                label: "Hide when it loses focus",
                help: "",
                kind: Kind::Bool,
            },
            Item {
                key: "window.animations",
                label: "Slide in and out",
                help: "",
                kind: Kind::Bool,
            },
        ],
    },
    Page {
        id: "terminal",
        title: "Terminal",
        items: &[
            Item {
                key: "terminal.font",
                label: "Font",
                help: "A font family; empty follows the theme.",
                kind: Kind::Text,
            },
            Item {
                key: "terminal.font_size",
                label: "Font size",
                help: "In points; 0 follows the theme.",
                kind: Kind::OptInt(6, 48),
            },
            Item {
                key: "terminal.scrollback",
                label: "Scrollback (lines)",
                help: "",
                kind: Kind::Int(0, 100_000),
            },
            Item {
                key: "terminal.new_tab_dir",
                label: "New tabs start in",
                help: "The home folder, or the focused pane's folder.",
                kind: Kind::Choice(&["home", "current"]),
            },
            Item {
                key: "terminal.copy_on_select",
                label: "Copy on select",
                help: "",
                kind: Kind::Bool,
            },
        ],
    },
    Page {
        id: "advanced",
        title: "Advanced",
        items: &[
            Item {
                key: "advanced.programs_write_clipboard",
                label: "Programs may set the clipboard",
                help: "A program in the terminal can copy to your clipboard.",
                kind: Kind::Bool,
            },
            Item {
                key: "advanced.allow_run",
                label: "Other apps may run commands in kyko",
                help: "Turning this off disables Run in kyko.",
                kind: Kind::Bool,
            },
        ],
    },
];

fn item(key: &str) -> Option<&'static Item> {
    PAGES.iter().flat_map(|p| p.items).find(|i| i.key == key)
}

fn kind_json(kind: Kind) -> Json {
    match kind {
        Kind::Bool => json!({"type": "bool"}),
        Kind::Int(min, max) => json!({"type": "int", "min": min, "max": max}),
        Kind::OptInt(min, max) => json!({"type": "optint", "min": min, "max": max}),
        Kind::Choice(opts) => json!({"type": "choice", "options": opts}),
        Kind::Text => json!({"type": "text"}),
    }
}

/// The pages for the window: `[{id, title, items: [{key, label, help, type, …}]}]`.
pub fn pages_json() -> Json {
    PAGES
        .iter()
        .map(|p| {
            let items: Vec<Json> = p
                .items
                .iter()
                .map(|i| {
                    let mut v = kind_json(i.kind);
                    v["key"] = json!(i.key);
                    v["label"] = json!(i.label);
                    v["help"] = json!(i.help);
                    v
                })
                .collect();
            json!({"id": p.id, "title": p.title, "items": items})
        })
        .collect()
}

/// The settings now, by key: what the file says, defaults filled in; unset texts and numbers
/// are `""` and `0`.
pub fn values_json(c: &Config) -> Json {
    let (w, t, a) = (&c.window, &c.terminal, &c.advanced);
    json!({
        "window.shape": if w.shape == Shape::Island { "island" } else { "sheet" },
        "window.tabs": if w.tabs == TabsAt::Top { "top" } else { "bottom" },
        "window.height_pct": w.height_pct,
        "window.width_pct": w.width_pct,
        "window.above_fullscreen": w.above_fullscreen,
        "window.hide_on_focus_loss": w.hide_on_focus_loss,
        "window.animations": w.animations,
        "terminal.font": t.font.clone().unwrap_or_default(),
        "terminal.font_size": t.font_size.unwrap_or(0),
        "terminal.scrollback": t.scrollback,
        "terminal.new_tab_dir": if t.new_tab_dir == NewTabDir::Home { "home" } else { "current" },
        "terminal.copy_on_select": t.copy_on_select,
        "advanced.programs_write_clipboard": a.programs_write_clipboard,
        "advanced.allow_run": a.allow_run,
    })
}

/// `text` with `key` set to `new` (a JSON value from the window), checked: a value the config
/// would refuse is an error naming why, and nothing is written. An unset text or number removes
/// the key.
pub fn change(text: &str, key: &str, new: &Json) -> Result<String, String> {
    let it = item(key).ok_or_else(|| format!("{key} is not a setting Preferences changes"))?;
    let bad = || format!("{new} is not a valid {}", it.label);
    let value = match it.kind {
        Kind::Bool => Some(Value::Bool(new.as_bool().ok_or_else(bad)?)),
        Kind::Int(min, max) => {
            let n = new.as_i64().ok_or_else(bad)?;
            if !(min..=max).contains(&n) {
                return Err(format!("{} must be {min} to {max}", it.label));
            }
            Some(Value::Int(n))
        }
        Kind::OptInt(min, max) => match new.as_i64().ok_or_else(bad)? {
            0 => None,
            n if (min..=max).contains(&n) => Some(Value::Int(n)),
            _ => return Err(format!("{} must be 0 or {min} to {max}", it.label)),
        },
        Kind::Choice(opts) => {
            let s = new.as_str().ok_or_else(bad)?;
            if !opts.contains(&s) {
                return Err(format!("{} must be one of {}", it.label, opts.join(", ")));
            }
            Some(Value::Str(s.to_string()))
        }
        Kind::Text => match new.as_str().ok_or_else(bad)?.trim() {
            "" => None,
            s if s.contains(['\n', '\0']) => return Err(format!("{} is one line", it.label)),
            s => Some(Value::Str(s.to_string())),
        },
    };
    let out = configedit::set(text, key, value)?;
    // The config has the last word: a value it warns about is not written.
    let loaded = Config::parse(&out).map_err(|e| e.message)?;
    if let Some(w) = loaded.warnings.iter().find(|w| w.key == key) {
        return Err(w.message.clone());
    }
    Ok(out)
}

/// `text` with every setting of the page removed ("Reset this page"). Other keys stay.
pub fn reset_page(text: &str, page: &str) -> Result<String, String> {
    let page = PAGES
        .iter()
        .find(|p| p.id == page)
        .ok_or_else(|| format!("no page {page}"))?;
    let mut out = text.to_string();
    for it in page.items {
        out = configedit::set(&out, it.key, None)?;
    }
    Ok(out.trim_start_matches('\n').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn loaded(text: &str) -> Config {
        let l = Config::parse(text).unwrap();
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        l.config
    }

    #[test]
    fn every_setting_has_a_value_and_changes_what_the_config_reads() {
        let c = Config::default();
        let values = values_json(&c);
        for p in PAGES {
            for i in p.items {
                assert!(values.get(i.key).is_some(), "{} has no value", i.key);
                // Writing a setting's own value changes nothing the config reads.
                let text = change("", i.key, &values[i.key]).unwrap();
                assert_eq!(loaded(&text), c, "{}", i.key);
            }
        }
    }

    #[test]
    fn a_change_is_read_back() {
        let t = change("", "window.height_pct", &json!(60)).unwrap();
        assert_eq!(values_json(&loaded(&t))["window.height_pct"], 60);
        let t = change(&t, "window.shape", &json!("sheet")).unwrap();
        assert_eq!(values_json(&loaded(&t))["window.shape"], "sheet");
        let t = change(&t, "terminal.font", &json!(" Fira Code ")).unwrap();
        assert_eq!(values_json(&loaded(&t))["terminal.font"], "Fira Code");
    }

    #[test]
    fn unset_values_remove_the_key() {
        let t = change("", "terminal.font_size", &json!(14)).unwrap();
        assert!(t.contains("font_size"));
        let t = change(&t, "terminal.font_size", &json!(0)).unwrap();
        assert_eq!(t, "");
        let t = change("[terminal]\nfont = \"x\"\n", "terminal.font", &json!("")).unwrap();
        assert_eq!(t, "");
    }

    #[test]
    fn bad_values_are_refused_with_a_reason() {
        for (k, v) in [
            ("window.height_pct", json!(5)),
            ("window.height_pct", json!("tall")),
            ("window.shape", json!("blob")),
            ("window.animations", json!(1)),
            ("terminal.font_size", json!(3)),
            ("terminal.font", json!("a\nb")),
            ("no.such_key", json!(1)),
        ] {
            assert!(change("", k, &v).is_err(), "{k} = {v}");
        }
    }

    #[test]
    fn a_hand_edited_file_keeps_its_other_content() {
        let text = "# mine\n[window]\nheight_pct = 50 # tall\n\n[hotkey]\ntoggle = \"F11\"\n";
        let out = change(text, "window.shape", &json!("sheet")).unwrap();
        assert!(out.starts_with("# mine\n[window]\nheight_pct = 50 # tall\n"));
        assert!(out.contains("[hotkey]\ntoggle = \"F11\""));
    }

    #[test]
    fn reset_removes_only_that_page() {
        let text = "[window]\nshape = \"sheet\"\nheight_pct = 50\n\n[terminal]\nscrollback = 5\n";
        let out = reset_page(text, "window").unwrap();
        assert_eq!(out, "[terminal]\nscrollback = 5\n");
        assert!(reset_page(text, "nope").is_err());
    }

    #[test]
    fn the_pages_are_described_for_the_window() {
        let pages = pages_json();
        assert_eq!(pages[0]["id"], "window");
        assert_eq!(pages[0]["items"][0]["type"], "choice");
        assert_eq!(pages[0]["items"][2]["max"], 100);
    }
}
