//! The Hyprland backend against a real Hyprland (scripts/hotkey-live.sh starts one headless with
//! a Lua config holding uQ's F12 bind, sets KYKO_LIVE_HYPRLAND, and gives KYKO_LIVE_PRESS, a
//! program that presses Ctrl+F11). Without it this test passes
//! without checking anything.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use kyko_hotkey::{DESCRIPTION, Hotkeys, pick};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn ask(socket: &Path, line: &str) -> String {
    let mut s = UnixStream::connect(socket).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn binds(socket: &Path) -> Vec<serde_json::Value> {
    serde_json::from_str(&ask(socket, "j/binds")).unwrap()
}

/// kyko's own binds, as (modmask, key).
fn kykos(socket: &Path) -> Vec<(u64, String)> {
    binds(socket)
        .iter()
        .filter(|b| b["description"] == DESCRIPTION)
        .map(|b| {
            let mask = b["modmask"].as_u64().unwrap();
            (mask, b["key"].as_str().unwrap().to_string())
        })
        .collect()
}

fn f12s(socket: &Path) -> usize {
    binds(socket).iter().filter(|b| b["key"] == "F12").count()
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn binds_on_a_real_hyprland() {
    if env("KYKO_LIVE_HYPRLAND").is_none() {
        eprintln!("KYKO_LIVE_HYPRLAND not set: no Hyprland to test against");
        return;
    }
    let marker = PathBuf::from(env("KYKO_LIVE_MARKER").expect("KYKO_LIVE_MARKER"));
    let command = format!("touch {}", marker.display());
    let (backend, _) = pick(&env, &command);
    assert_eq!(backend.name(), "hyprland");
    let (socket, _) = kyko_hotkey::hyprland_sockets(&env).unwrap();
    let mut h = Hotkeys::new(backend);
    assert_eq!(f12s(&socket), 1, "uQ's F12 bind is in the config");

    // F12 is uQ's: kyko binds nothing and says nothing.
    h.set(Some("F12".parse().unwrap())).unwrap();
    assert!(kykos(&socket).is_empty());
    assert_eq!(f12s(&socket), 1);

    // Another key is kyko's own, through Lua.
    h.set(Some("Ctrl+F11".parse().unwrap())).unwrap();
    assert_eq!(kykos(&socket), [(4, "F11".to_string())]);
    assert_eq!(f12s(&socket), 1, "uQ's F12 is still there");

    // Pressing it runs the command.
    let typed = Command::new(env("KYKO_LIVE_PRESS").expect("KYKO_LIVE_PRESS"))
        .status()
        .unwrap();
    assert!(typed.success());
    wait_for("Ctrl+F11 ran the command", || marker.exists());

    // A reload forgets it; binding again gives one bind, not two.
    assert_eq!(ask(&socket, "/reload").trim(), "ok");
    wait_for("the reload forgot kyko's bind", || {
        kykos(&socket).is_empty()
    });
    h.rebind().unwrap();
    h.rebind().unwrap();
    assert_eq!(kykos(&socket), [(4, "F11".to_string())]);

    // Moving and dropping the key leave only the config's binds.
    h.set(Some("Super+Shift+K".parse().unwrap())).unwrap();
    assert_eq!(kykos(&socket), [(65, "k".to_string())]);
    h.set(None).unwrap();
    assert!(kykos(&socket).is_empty());
    assert_eq!(f12s(&socket), 1);
}
