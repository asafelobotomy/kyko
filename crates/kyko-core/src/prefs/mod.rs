//! What Preferences shows (§2.6): the pages and their settings, the current values for the
//! window, and checked changes to `config.toml` text. The window itself is QML; everything
//! that can be tested without Qt lives here.
use serde_json::{Value as Json, json};

use crate::config::{Action, Bell, Config, NewTabDir, NotifyOnFinish, Position, Shape, TabsAt};
use crate::configedit::{self, Value};
use crate::keys::KeySeq;
use crate::theme::{ThemeSource, builtin_keys};

mod pages;

pub use pages::PAGES;

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
        "window.position": w.position.name(),
        "window.tabs": if w.tabs == TabsAt::Top { "top" } else { "bottom" },
        "window.height_pct": w.height_pct,
        "window.width_pct": w.width_pct,
        "window.side_width_pct": w.side_width_pct,
        "window.side_height_pct": w.side_height_pct,
        "window.center_width_pct": w.center_width_pct,
        "window.center_height_pct": w.center_height_pct,
        "window.dim_pct": w.dim_pct,
        "window.opacity_pct": w.opacity_pct,
        "window.above_fullscreen": w.above_fullscreen,
        "window.hide_on_focus_loss": w.hide_on_focus_loss,
        "window.animations": w.animations,
        "terminal.font": t.font.clone().unwrap_or_default(),
        "terminal.font_size": t.font_size.unwrap_or(0),
        "terminal.scrollback": t.scrollback,
        "terminal.new_tab_dir": if t.new_tab_dir == NewTabDir::Home { "home" } else { "current" },
        "terminal.bell": match t.bell {
            Bell::None => "none",
            Bell::Visual => "visual",
            Bell::Sound => "sound",
        },
        "terminal.notify_on_finish": match t.notify_on_finish {
            NotifyOnFinish::Never => "never",
            NotifyOnFinish::Unfocused => "unfocused",
            NotifyOnFinish::Always => "always",
        },
        "terminal.notify_after": t.notify_after,
        "terminal.copy_on_select": t.copy_on_select,
        "terminal.ligatures": t.ligatures,
        "terminal.shell_integration": t.shell_integration,
        "terminal.shell_extras": t.shell_extras,
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
mod tests;
