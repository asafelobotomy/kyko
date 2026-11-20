//! What Preferences shows (§2.6): the pages and their settings, the current values for the
//! window, and checked changes to `config.toml` text. The window itself is QML; everything
//! that can be tested without Qt lives here.
use serde_json::{Value as Json, json};

use crate::config::{Action, Config, NewTabDir, Shape, TabsAt};
use crate::configedit::{self, Value};
use crate::keys::KeySeq;
use crate::theme::{ThemeSource, builtin_keys};

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
    /// A key sequence, `Ctrl+Shift+T`; empty is no key at all.
    Key,
    /// Where the theme comes from: `auto`, `uq`, `custom` or `builtin:<id>`.
    Theme,
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
                help: "home: the home folder. current: the folder of the focused pane.",
                kind: Kind::Choice(&["home", "current"]),
            },
            Item {
                key: "terminal.copy_on_select",
                label: "Copy on select",
                help: "",
                kind: Kind::Bool,
            },
            Item {
                key: "terminal.ligatures",
                label: "Ligatures",
                help: "Draw sequences such as => and != as one sign when the font has them. Never under the cursor.",
                kind: Kind::Bool,
            },
            Item {
                key: "terminal.shell_integration",
                label: "Shell integration",
                help: "The same prompt marks, folder tracking and prompt in bash, zsh and fish. Your own prompt is kept. Applies to new tabs.",
                kind: Kind::Bool,
            },
        ],
    },
    Page {
        id: "appearance",
        title: "Appearance",
        items: &[Item {
            key: "theme.source",
            label: "Colours",
            help: "Auto follows uQ when it is running.",
            kind: Kind::Theme,
        }],
    },
    Page {
        id: "keys",
        title: "Keys",
        items: &[
            Item {
                key: "hotkey.toggle",
                label: "Show or hide kyko",
                help: "Works from any app where the compositor allows it.",
                kind: Kind::Key,
            },
            Item {
                key: "keys.new_tab",
                label: "New tab",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.close_pane",
                label: "Close pane",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.prev_tab",
                label: "Previous tab",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.next_tab",
                label: "Next tab",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.split_side",
                label: "Split side by side",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.split_stacked",
                label: "Split top and bottom",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.focus_left",
                label: "Focus left",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.focus_right",
                label: "Focus right",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.focus_up",
                label: "Focus up",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.focus_down",
                label: "Focus down",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.copy",
                label: "Copy",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.paste",
                label: "Paste",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.search",
                label: "Search",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.prev_prompt",
                label: "Previous prompt",
                help: "Scroll back to the prompt before the one at the top.",
                kind: Kind::Key,
            },
            Item {
                key: "keys.next_prompt",
                label: "Next prompt",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.copy_last_output",
                label: "Copy last output",
                help: "Selects and copies what the last command printed.",
                kind: Kind::Key,
            },
            Item {
                key: "keys.font_bigger",
                label: "Bigger text",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.font_smaller",
                label: "Smaller text",
                help: "",
                kind: Kind::Key,
            },
            Item {
                key: "keys.font_reset",
                label: "Reset text size",
                help: "",
                kind: Kind::Key,
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
        Kind::Key => json!({"type": "key"}),
        Kind::Theme => {
            let mut opts = vec!["auto".to_string(), "uq".to_string(), "custom".to_string()];
            opts.extend(builtin_keys().map(|k| format!("builtin:{k}")));
            json!({"type": "choice", "options": opts})
        }
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
    let mut v = json!({
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
        "terminal.ligatures": t.ligatures,
        "terminal.shell_integration": t.shell_integration,
        "advanced.programs_write_clipboard": a.programs_write_clipboard,
        "advanced.allow_run": a.allow_run,
        "theme.source": match &c.theme {
            ThemeSource::Auto => "auto".to_string(),
            ThemeSource::Uq => "uq".to_string(),
            ThemeSource::Custom => "custom".to_string(),
            ThemeSource::Builtin(k) => format!("builtin:{k}"),
        },
        "hotkey.toggle": seq_text(c.hotkey),
    });
    for a in Action::all().filter(|a| !matches!(a, Action::Tab(_))) {
        v[format!("keys.{}", a.key_name())] = json!(seq_text(c.keys.get(a)));
    }
    v
}

fn seq_text(s: Option<KeySeq>) -> String {
    s.map(|s| s.to_string()).unwrap_or_default()
}

/// The sequence `key` has in `text`, defaults filled in.
fn value_seq(text: &str, key: &str) -> Option<KeySeq> {
    let c = Config::parse(text).ok()?.config;
    values_json(&c)[key].as_str()?.parse().ok()
}

/// What `key` is when the file says nothing: the default sequence, if `key` is a key setting.
fn default_seq(key: &str) -> Option<KeySeq> {
    if key == "hotkey.toggle" {
        return Config::default().hotkey;
    }
    let name = key.strip_prefix("keys.")?;
    Action::all()
        .find(|a| a.key_name() == name)
        .map(Action::default_seq)
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
        Kind::Theme => {
            let s = new.as_str().ok_or_else(bad)?;
            match s {
                "auto" => None,
                "uq" | "custom" => Some(Value::Str(s.to_string())),
                _ if s
                    .strip_prefix("builtin:")
                    .is_some_and(|k| builtin_keys().any(|b| b == k)) =>
                {
                    Some(Value::Str(s.to_string()))
                }
                _ => return Err(format!("{} is not a theme", it.label)),
            }
        }
        Kind::Key => {
            let s = new.as_str().ok_or_else(bad)?.trim();
            if s.is_empty() {
                Some(Value::Str(String::new()))
            } else {
                let seq: KeySeq = s.parse().map_err(|e: crate::keys::KeyError| e.0)?;
                // The default is the absence of the key, so a reset really resets.
                if default_seq(key).is_some_and(|d| d == seq) {
                    None
                } else {
                    Some(Value::Str(seq.to_string()))
                }
            }
        }
    };
    let out = configedit::set(text, key, value)?;
    // The config has the last word: a value it warns about is not written.
    let loaded = Config::parse(&out).map_err(|e| e.message)?;
    if let Some(w) = loaded.warnings.iter().find(|w| w.key == key) {
        // A clash names its holder by config key; the window talks in labels.
        let mut msg = w.message.clone();
        for o in PAGES
            .iter()
            .flat_map(|p| p.items)
            .filter(|o| o.kind == Kind::Key)
        {
            msg = msg.replace(o.key, o.label);
        }
        return Err(msg);
    }
    // A sequence another action holds is refused here, not left for the config to drop later.
    if it.kind == Kind::Key
        && let Some(seq) = value_seq(&out, key)
    {
        let before = Config::parse(text).map(|l| values_json(&l.config)).ok();
        let holders = PAGES
            .iter()
            .filter(|p| p.id == "keys")
            .flat_map(|p| p.items);
        for o in holders.filter(|o| o.key != key) {
            let theirs = before.as_ref().and_then(|v| v[o.key].as_str());
            if theirs
                .and_then(|t| t.parse::<KeySeq>().ok())
                .is_some_and(|t| t.clashes(&seq))
            {
                return Err(format!("{seq} is already used by {}", o.label));
            }
        }
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
    fn a_key_is_set_cleared_and_reset() {
        let t = change("", "keys.new_tab", &json!("Ctrl+Alt+N")).unwrap();
        assert_eq!(values_json(&loaded(&t))["keys.new_tab"], "Ctrl+Alt+N");
        let t = change(&t, "keys.new_tab", &json!("")).unwrap();
        assert_eq!(values_json(&loaded(&t))["keys.new_tab"], "");
        // Back to the default is the key's absence.
        let t = change(&t, "keys.new_tab", &json!("Ctrl+Shift+T")).unwrap();
        assert_eq!(t, "");
        let t = change("", "hotkey.toggle", &json!("Ctrl+Alt+K")).unwrap();
        assert_eq!(values_json(&loaded(&t))["hotkey.toggle"], "Ctrl+Alt+K");
        assert!(change("", "keys.copy", &json!("Hyper+Q")).is_err());
    }

    #[test]
    fn a_key_another_action_holds_is_refused() {
        let e = change("", "keys.copy", &json!("Ctrl+Shift+V")).unwrap_err();
        assert!(e.contains("Paste"), "{e}");
        let e = change("", "keys.copy", &json!("F12")).unwrap_err();
        assert!(e.contains("Show or hide"), "{e}");
        // Moving the hotkey onto a key's sequence is refused too.
        assert!(change("", "hotkey.toggle", &json!("Ctrl+Shift+T")).is_err());
    }

    #[test]
    fn the_theme_is_chosen_from_the_known_ones() {
        let t = change("", "theme.source", &json!("builtin:gruvbox-dark")).unwrap();
        assert_eq!(
            values_json(&loaded(&t))["theme.source"],
            "builtin:gruvbox-dark"
        );
        assert_eq!(change(&t, "theme.source", &json!("auto")).unwrap(), "");
        assert!(change("", "theme.source", &json!("builtin:nope")).is_err());
    }

    #[test]
    fn the_pages_are_described_for_the_window() {
        let pages = pages_json();
        assert_eq!(pages[0]["id"], "window");
        assert_eq!(pages[0]["items"][0]["type"], "choice");
        assert_eq!(pages[0]["items"][2]["max"], 100);
    }
}
