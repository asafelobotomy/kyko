//! Changing one setting in `config.toml` text without disturbing the rest: Preferences writes
//! through this, so a hand-edited file keeps its comments, order and unknown keys.
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, value};

/// A value Preferences can write.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Sets the dotted `key` (`window.height_pct`) to `new`, or removes it when `new` is `None`
/// (back to its default). A table left empty by a removal goes too. Text that does not parse is
/// an error and nothing is changed.
pub fn set(text: &str, key: &str, new: Option<Value>) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let path: Vec<&str> = key.split('.').collect();
    let (last, parents) = path.split_last().ok_or("an empty key")?;
    if last.is_empty() || parents.iter().any(|p| p.is_empty()) {
        return Err(format!("bad key {key:?}"));
    }
    match new {
        Some(v) => {
            let mut table = doc.as_table_mut();
            for p in parents {
                let item = table.entry(p).or_insert_with(|| Item::Table(Table::new()));
                table = item
                    .as_table_mut()
                    .ok_or_else(|| format!("{p} is not a table"))?;
            }
            // Keeps the comment and spacing around an existing value.
            let item = match &v {
                Value::Str(s) => value(s.as_str()),
                Value::Int(n) => value(*n),
                Value::Float(f) => value(*f),
                Value::Bool(b) => value(*b),
            };
            match table.get_mut(last) {
                Some(Item::Value(old)) => {
                    let decor = old.decor().clone();
                    if let Item::Value(mut nv) = item {
                        *nv.decor_mut() = decor;
                        *old = nv;
                    }
                }
                _ => {
                    table.insert(last, item);
                }
            }
        }
        None => remove(doc.as_table_mut(), &path),
    }
    Ok(doc.to_string())
}

/// Removes a whole section (`window`), for "Reset this page".
pub fn reset_section(text: &str, section: &str) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    doc.as_table_mut().remove(section);
    Ok(doc.to_string().trim_start_matches('\n').to_string())
}

fn remove(table: &mut Table, path: &[&str]) {
    match path {
        [last] => {
            table.remove(last);
        }
        [first, rest @ ..] => {
            if let Some(child) = table.get_mut(first).and_then(Item::as_table_mut) {
                remove(child, rest);
                if child.is_empty() {
                    table.remove(first);
                }
            }
        }
        [] => {}
    }
}

/// Writes `text` to `path` through a temporary file in the same folder, so a reader never sees
/// half a file; the folder is made when it is missing.
pub fn write(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".config.toml.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_a_new_key_and_makes_its_table() {
        let out = set("", "window.height_pct", Some(Value::Int(60))).unwrap();
        assert_eq!(out, "[window]\nheight_pct = 60\n");
    }

    #[test]
    fn keeps_comments_and_other_keys() {
        let text =
            "# mine\n[window]\nshape = \"sheet\" # tall\nheight_pct = 45\n\n[extra]\nx = 1\n";
        let out = set(text, "window.height_pct", Some(Value::Int(70))).unwrap();
        assert_eq!(
            out,
            "# mine\n[window]\nshape = \"sheet\" # tall\nheight_pct = 70\n\n[extra]\nx = 1\n"
        );
    }

    #[test]
    fn keeps_the_comment_after_the_changed_value() {
        let out = set("[w]\na = 1 # note\n", "w.a", Some(Value::Int(2))).unwrap();
        assert_eq!(out, "[w]\na = 2 # note\n");
    }

    #[test]
    fn writes_each_kind() {
        let mut t = String::new();
        for (k, v) in [
            ("a.s", Value::Str("x\"y".into())),
            ("a.n", Value::Int(-3)),
            ("a.f", Value::Float(0.5)),
            ("a.b", Value::Bool(true)),
        ] {
            t = set(&t, k, Some(v)).unwrap();
        }
        let parsed: toml::Table = t.parse().unwrap();
        assert_eq!(parsed["a"]["s"].as_str(), Some("x\"y"));
        assert_eq!(parsed["a"]["n"].as_integer(), Some(-3));
        assert_eq!(parsed["a"]["f"].as_float(), Some(0.5));
        assert_eq!(parsed["a"]["b"].as_bool(), Some(true));
    }

    #[test]
    fn removing_the_last_key_removes_the_table() {
        let out = set("[window]\nshape = \"sheet\"\n", "window.shape", None).unwrap();
        assert_eq!(out, "");
        let out = set("[w]\na = 1\nb = 2\n", "w.a", None).unwrap();
        assert_eq!(out, "[w]\nb = 2\n");
    }

    #[test]
    fn removing_a_missing_key_changes_nothing() {
        let text = "[w]\na = 1\n";
        assert_eq!(set(text, "w.zzz", None).unwrap(), text);
        assert_eq!(set(text, "q.zzz", None).unwrap(), text);
    }

    #[test]
    fn broken_text_and_keys_are_errors() {
        assert!(set("[w\n", "w.a", Some(Value::Int(1))).is_err());
        assert!(set("", "a..b", Some(Value::Int(1))).is_err());
        assert!(set("w = 1\n", "w.a", Some(Value::Int(1))).is_err());
    }

    #[test]
    fn resets_a_page() {
        let out = reset_section("[window]\na = 1\n\n[terminal]\nb = 2\n", "window").unwrap();
        assert_eq!(out, "[terminal]\nb = 2\n");
    }

    #[test]
    fn writes_through_a_temporary_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("kyko").join("config.toml");
        write(&p, "a = 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a = 1\n");
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn what_is_written_is_what_the_config_reads() {
        let t = set("", "window.height_pct", Some(Value::Int(60))).unwrap();
        let t = set(&t, "terminal.copy_on_select", Some(Value::Bool(true))).unwrap();
        let c = crate::config::Config::parse(&t).unwrap();
        assert!(c.warnings.is_empty());
        assert_eq!(c.config.window.height_pct, 60);
        assert!(c.config.terminal.copy_on_select);
    }
}
