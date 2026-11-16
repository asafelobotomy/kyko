//! Preferences (§2.6): reads and writes `config.toml` for the Preferences window. The checks
//! and the text editing are in `kyko_core::prefs`; this is the file and the process.
use std::ffi::OsString;
use std::path::PathBuf;

use kyko_core::config::Config;
use kyko_core::{paths, prefs};
use serde_json::json;

fn env(k: &str) -> Option<OsString> {
    std::env::var_os(k)
}

fn config_path() -> Result<PathBuf, String> {
    paths::config_path(env).ok_or_else(|| "no config folder (HOME is not set)".to_string())
}

/// The file's text; a missing file is empty.
fn read(path: &std::path::Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// `{values: {key: value}, error: ""}`: the settings now; `error` says why the file cannot be
/// read when it does not parse (the values are then the defaults).
pub fn values() -> String {
    let path = config_path();
    let loaded = path
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|p| read(p))
        .and_then(|t| {
            Config::parse(&t).map_err(|e| format!("config.toml line {}: {}", e.line, e.message))
        });
    match loaded {
        Ok(l) => json!({"values": prefs::values_json(&l.config), "error": ""}),
        Err(e) => json!({"values": prefs::values_json(&Config::default()), "error": e}),
    }
    .to_string()
}

/// Applies `change` to the file; "" when written, else why not.
fn edit(f: impl FnOnce(&str) -> Result<String, String>) -> String {
    let run = || -> Result<(), String> {
        let path = config_path()?;
        let new = f(&read(&path)?)?;
        kyko_core::configedit::write(&path, &new).map_err(|e| format!("cannot write: {e}"))
    };
    run().err().unwrap_or_default()
}

pub fn set(key: &str, json: &str) -> String {
    match serde_json::from_str(json) {
        Ok(v) => edit(|text| prefs::change(text, key, &v)),
        Err(e) => format!("bad value: {e}"),
    }
}

pub fn reset(page: &str) -> String {
    edit(|text| prefs::reset_page(text, page))
}

/// Opens the Preferences window as its own process.
pub fn open() {
    if let Ok(exe) = std::env::current_exe()
        && let Ok(mut child) = std::process::Command::new(exe)
            .arg("preferences")
            .stdin(std::process::Stdio::null())
            .spawn()
    {
        // Reaped when the window closes.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}
