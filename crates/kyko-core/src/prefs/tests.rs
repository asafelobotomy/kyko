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
    assert_eq!(pages[0]["items"][1]["key"], "window.position");
    assert_eq!(pages[0]["items"][3]["max"], 100);
}
