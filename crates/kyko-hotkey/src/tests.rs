use super::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

fn key(s: &str) -> KeySeq {
    s.parse().unwrap()
}

/// How the fake compositor reads its config.
#[derive(Clone, Copy)]
enum Config {
    /// Hyprland 0.57+: `eval` works, `keyword` is gone.
    Lua,
    /// hyprlang: `eval` says it needs Lua, `keyword` works.
    Hyprlang,
    /// Hyprland from before `eval`.
    Old,
}

/// A Hyprland control socket that records what it is told (but not `j/binds`, which it answers
/// with `binds`) and answers like a compositor reading `config`; any line containing `refuse`
/// gets an error.
fn fake(dir: &Path, config: Config, binds: &'static str) -> (PathBuf, Arc<Mutex<Vec<String>>>) {
    let path = dir.join(".socket.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            let mut buf = [0u8; 1024];
            let n = s.read(&mut buf).unwrap_or(0);
            let line = String::from_utf8_lossy(&buf[..n]).to_string();
            if line == "j/binds" {
                let _ = s.write_all(binds.as_bytes());
                continue;
            }
            let eval = line.starts_with("/eval ");
            let answer = match config {
                _ if line.contains("refuse") => "bad key",
                Config::Lua if eval => "ok",
                Config::Lua => "unknown request",
                Config::Hyprlang if eval => "eval is only supported with the lua config manager",
                Config::Old if eval => "unknown request",
                Config::Hyprlang | Config::Old => "ok",
            };
            log.lock().unwrap().push(line);
            let _ = s.write_all(answer.as_bytes());
        }
    });
    (path, seen)
}

fn hypr(sock: PathBuf) -> Hotkeys {
    Hotkeys::new(Box::new(Hyprland::new(sock, "kyko toggle")))
}

#[test]
fn keys_are_spelled_the_way_hyprland_reads_them() {
    assert_eq!(hypr_keyspec(&key("F12")), ",F12");
    assert_eq!(hypr_keyspec(&key("Ctrl+Shift+T")), "CTRL SHIFT,t");
    assert_eq!(hypr_keyspec(&key("Super+Alt+PgUp")), "SUPER ALT,prior");
    assert_eq!(hypr_keyspec(&key("Ctrl+Space")), "CTRL,space");
    assert_eq!(hypr_keyspec(&key("Super+Esc")), "SUPER,escape");
    assert_eq!(hypr_lua_keys(&key("F12")), "F12");
    assert_eq!(hypr_lua_keys(&key("Ctrl+Shift+T")), "CTRL + SHIFT + t");
    assert_eq!(hypr_lua_keys(&key("Super+Alt+PgUp")), "SUPER + ALT + prior");
    assert_eq!(lua_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
}

#[test]
fn a_lua_config_gets_an_hl_bind_through_eval() {
    let dir = tempfile::tempdir().unwrap();
    let (sock, seen) = fake(dir.path(), Config::Lua, "[]");
    let mut h = hypr(sock);
    h.set(Some(key("F12"))).unwrap();
    h.set(Some(key("Ctrl+F11"))).unwrap();
    h.set(None).unwrap();
    let seen = seen.lock().unwrap().clone();
    let bind = |k: &str| {
        format!(
            r#"/eval if __kyko_hotkey then __kyko_hotkey:remove() end __kyko_hotkey = hl.bind("{k}", hl.dsp.exec_cmd("kyko toggle"), {{ description = "kyko:toggle" }})"#
        )
    };
    let unbind = "/eval if __kyko_hotkey then __kyko_hotkey:remove() __kyko_hotkey = nil end";
    assert_eq!(
        seen,
        [
            bind("F12"),
            unbind.into(),
            bind("CTRL + F11"),
            unbind.into()
        ]
    );
    assert_eq!(h.key(), None);
}

#[test]
fn a_hyprlang_config_gets_a_keyword_bind() {
    for config in [Config::Hyprlang, Config::Old] {
        let dir = tempfile::tempdir().unwrap();
        let (sock, seen) = fake(dir.path(), config, "[]");
        let mut h = hypr(sock);
        h.set(Some(key("F12"))).unwrap();
        h.set(Some(key("Ctrl+F11"))).unwrap();
        h.set(None).unwrap();
        let seen = seen.lock().unwrap().clone();
        // Only the first bind asks for Lua; the answer is kept.
        assert!(seen[0].starts_with("/eval "), "{seen:?}");
        assert_eq!(
            seen[1..],
            [
                "/keyword bind ,F12,exec,kyko toggle",
                "/keyword unbind ,F12",
                "/keyword bind CTRL,F11,exec,kyko toggle",
                "/keyword unbind CTRL,F11",
            ]
        );
    }
}

/// uQ's generated.lua: the drop-down on F12, known by its description (a Lua bind shows only a
/// function), next to an unrelated bind.
const UQ_LUA_BINDS: &str = r#"[
    {"modmask": 0, "submap": "", "key": "F12", "description": "uq:uq.terminal.dropdown", "dispatcher": "function: 0x55d0", "arg": "17"},
    {"modmask": 64, "submap": "", "key": "Return", "description": "uq:uq.terminal", "dispatcher": "function: 0x55d1", "arg": "18"}
]"#;

#[test]
fn the_key_uq_already_binds_is_left_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let (sock, seen) = fake(dir.path(), Config::Lua, UQ_LUA_BINDS);
    let mut h = hypr(sock);
    h.set(Some(key("F12"))).unwrap();
    h.rebind().unwrap();
    h.set(None).unwrap();
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn a_hyprlang_bind_of_the_command_is_left_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let (sock, seen) = fake(
        dir.path(),
        Config::Hyprlang,
        r#"[{"modmask": 0, "key": "F12", "submap": "", "description": "", "dispatcher": "exec", "arg": "sh -c 'command -v kyko >/dev/null && exec kyko toggle'"}]"#,
    );
    let mut h = hypr(sock);
    h.set(Some(key("F12"))).unwrap();
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn another_key_is_bound_beside_uqs() {
    let dir = tempfile::tempdir().unwrap();
    let (sock, seen) = fake(dir.path(), Config::Lua, UQ_LUA_BINDS);
    let mut h = hypr(sock);
    // Super+Return is uQ's too, but it runs something else: only F12 is the toggle.
    h.set(Some(key("Ctrl+F11"))).unwrap();
    h.set(Some(key("Super+Enter"))).unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(seen[0].contains(r#"hl.bind("CTRL + F11""#), "{seen:?}");
    assert!(seen[2].contains(r#"hl.bind("SUPER + return""#), "{seen:?}");
    assert_eq!(h.key(), Some(key("Super+Enter")));
}

#[test]
fn kykos_own_bind_is_not_taken_for_the_configs() {
    let dir = tempfile::tempdir().unwrap();
    let (sock, seen) = fake(
        dir.path(),
        Config::Lua,
        r#"[{"modmask": 4, "submap": "", "key": "F11", "description": "kyko:toggle", "dispatcher": "function: 0x1", "arg": "3"}]"#,
    );
    let mut h = hypr(sock);
    h.set(Some(key("Ctrl+F11"))).unwrap();
    h.rebind().unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn a_refused_key_puts_the_old_one_back() {
    let dir = tempfile::tempdir().unwrap();
    let (sock, seen) = fake(dir.path(), Config::Lua, "[]");
    let mut h = Hotkeys::new(Box::new(Hyprland::new(sock, "kyko toggle --refuse")));
    // The fake refuses every bind: the old key cannot come back either, and says why.
    let err = h.set(Some(key("F12"))).unwrap_err();
    assert!(err.contains("bad key"), "{err}");
    assert_eq!(h.key(), None);
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn a_reload_binds_the_held_key_again() {
    let dir = tempfile::tempdir().unwrap();
    let (sock, seen) = fake(dir.path(), Config::Lua, "[]");
    let mut h = hypr(sock);
    h.set(Some(key("F12"))).unwrap();
    h.rebind().unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0], seen[1]);
}

#[test]
fn the_event_socket_reports_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let events = dir.path().join(".socket2.sock");
    let listener = UnixListener::bind(&events).unwrap();
    let (tx, rx): (_, Receiver<()>) = channel();
    let tx = Mutex::new(tx);
    watch_reloads(events, move || {
        let _ = tx.lock().unwrap().send(());
    });
    let (mut s, _) = listener.accept().unwrap();
    s.write_all(b"workspace>>1\nconfigreloaded>>\n").unwrap();
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn the_backend_follows_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().to_str().unwrap().to_string();
    let sig_dir = dir.path().join("hypr").join("abc");
    std::fs::create_dir_all(&sig_dir).unwrap();
    let none = |_: &str| None;
    let (b, ev) = pick(&none, "kyko toggle");
    assert_eq!(b.name(), "manual");
    assert!(ev.is_none());
    let env = |k: &str| match k {
        "HYPRLAND_INSTANCE_SIGNATURE" => Some("abc".to_string()),
        "XDG_RUNTIME_DIR" => Some(run.clone()),
        _ => None,
    };
    // The socket is not there yet: Hyprland is not really running.
    assert_eq!(pick(&env, "kyko toggle").0.name(), "manual");
    UnixListener::bind(sig_dir.join(".socket.sock")).unwrap();
    let (b, ev) = pick(&env, "kyko toggle");
    assert_eq!(b.name(), "hyprland");
    assert_eq!(ev.unwrap(), sig_dir.join(".socket2.sock"));
}

#[test]
fn manual_says_what_to_put_in_the_config() {
    let mut m = Manual::new("kyko toggle");
    let err = m.bind(&key("F12")).unwrap_err();
    assert!(
        err.contains(r#"hl.bind("F12", hl.dsp.exec_cmd("kyko toggle"))"#),
        "{err}"
    );
    assert!(err.contains("bind = , F12, exec, kyko toggle"), "{err}");
    assert!(m.unbind(&key("F12")).is_ok());
}
