//! The find bar, the close confirmation, Unicode input and the command palette.

use super::*;

/// The find bar: 0 look for `text`, 1 next, 2 previous, 3 close.
pub(crate) fn kyko_search(op: u8, text: &str) {
    let op = match op {
        0 => SearchOp::Find(text.to_string()),
        1 => SearchOp::Next,
        2 => SearchOp::Prev,
        _ => SearchOp::Close,
    };
    if let Some(w) = WORKSPACE.get() {
        w.search(op);
    }
}

pub(crate) fn kyko_confirm(yes: bool) {
    act(Act::Confirm(yes));
}

/// Characters for the Unicode input box, as JSON: `[{ch, code, name}]`.
pub(crate) fn kyko_unicode_search(query: &str) -> String {
    static NAMES: OnceLock<Vec<(char, String)>> = OnceLock::new();
    let names = NAMES.get_or_init(kyko_core::unicode::load_names);
    let found: Vec<_> = kyko_core::unicode::search(query, names, 12)
        .into_iter()
        .map(|f| {
            serde_json::json!({"ch": f.ch.to_string(), "code": format!("U+{:04X}", u32::from(f.ch)),
                               "name": f.name.to_lowercase()})
        })
        .collect();
    serde_json::Value::from(found).to_string()
}

/// The Unicode input box closed; `typed` when `text` should go to the pane.
pub(crate) fn kyko_unicode_done(text: &str, typed: bool) {
    act(Act::Unicode(typed.then(|| text.to_string())));
}

/// The command palette's entries for `query`, best first, as JSON:
/// `[{"id", "label", "hint", "kind"}]`. An id is `key:NAME` (an action), `tab:ID`, `cmd:TEXT`
/// (a recent command line, typed at the prompt without Enter) or `prefs`.
pub(crate) fn kyko_palette_items(query: &str) -> String {
    let Some(w) = WORKSPACE.get() else {
        return "[]".into();
    };
    let l = look();
    let mut items: Vec<(String, String, String, &str)> = Vec::new();
    for page in kyko_core::prefs::PAGES {
        for item in page.items {
            let Some(name) = item.key.strip_prefix("keys.") else {
                continue;
            };
            let Some(action) = Action::all().find(|a| a.key_name() == name) else {
                continue;
            };
            let hint = l
                .keys
                .get(action)
                .map(|k| k.to_string())
                .unwrap_or_default();
            items.push((
                format!("key:{name}"),
                item.label.to_string(),
                hint,
                "action",
            ));
        }
    }
    items.push((
        "prefs".into(),
        "Preferences".into(),
        String::new(),
        "action",
    ));
    let (layout, recent) = {
        let sh = w.shared.lock().unwrap();
        (sh.layout.clone(), sh.recent.clone())
    };
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&layout) {
        for t in v["tabs"].as_array().into_iter().flatten() {
            let name = t["name"].as_str().filter(|n| !n.is_empty());
            let label = name.or(t["program"].as_str()).unwrap_or("Tab").to_string();
            items.push((format!("tab:{}", t["id"]), label, "Tab".into(), "tab"));
        }
    }
    for c in recent {
        items.push((format!("cmd:{c}"), c, "Recent".into(), "command"));
    }
    let mut scored: Vec<_> = items
        .into_iter()
        .filter_map(|(id, label, hint, kind)| {
            kyko_core::fuzzy::score(query, &label).map(|s| (s, id, label, hint, kind))
        })
        .collect();
    // Stable: equal scores keep actions, then tabs, then the newest commands first.
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    let out: Vec<_> = scored
        .into_iter()
        .take(30)
        .map(|(_, id, label, hint, kind)| {
            serde_json::json!({"id": id, "label": label, "hint": hint, "kind": kind})
        })
        .collect();
    serde_json::Value::Array(out).to_string()
}

/// The palette closed: on `id` (see [`kyko_palette_items`]), or on nothing (empty, Escape).
pub(crate) fn kyko_palette_done(id: &str) {
    let Some(w) = WORKSPACE.get() else {
        return;
    };
    w.act(Act::PaletteClosed);
    let pane = w.shared.lock().unwrap().focused;
    if let Some(name) = id.strip_prefix("key:") {
        if let Some(action) = Action::all().find(|a| a.key_name() == name)
            && action != Action::CommandPalette
        {
            run_action(w, pane, action);
        }
    } else if let Some(tab) = id.strip_prefix("tab:").and_then(|t| t.parse().ok()) {
        w.act(Act::FocusTab(tab));
    } else if let Some(text) = id.strip_prefix("cmd:") {
        w.typed(pane, text.as_bytes().to_vec());
    } else if id == "prefs" {
        crate::prefs::open();
    }
}
