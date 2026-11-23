//! The toggle hotkey (§2.9). On Wayland a program cannot grab a key: the compositor owns the
//! keyboard, so kyko registers its key with it. This crate holds the backends and the rules that
//! hold for all of them, with no Qt and no window:
//!
//! - [`Hyprland`]: a runtime `bind` through Hyprland's control socket, put back after a config
//!   reload (the reload forgets runtime binds). When the compositor's own config already runs
//!   kyko's command on a key (uQ binds F12 in Settings › Shortcuts), kyko leaves the key to it:
//!   two binds would toggle twice;
//! - [`Manual`]: nothing can be bound, so it says which line to put in the compositor's config.
//!
//! Further backends (`uq`, `kde`, the desktop portal) implement the same [`Backend`].
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kyko_core::keys::{Key, KeySeq};

/// What binding a key means on one compositor.
pub trait Backend: Send {
    /// Short name for logs and Preferences: `hyprland`, `manual`.
    fn name(&self) -> &'static str;
    /// Makes `key` run `kyko toggle`.
    fn bind(&mut self, key: &KeySeq) -> Result<(), String>;
    /// Takes the binding away.
    fn unbind(&mut self, key: &KeySeq) -> Result<(), String>;
}

/// The key kyko holds and the backend it holds it with.
pub struct Hotkeys {
    backend: Box<dyn Backend>,
    key: Option<KeySeq>,
}

impl Hotkeys {
    pub fn new(backend: Box<dyn Backend>) -> Hotkeys {
        Hotkeys { backend, key: None }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// The key that is bound now.
    pub fn key(&self) -> Option<KeySeq> {
        self.key
    }

    /// Changes the key (`None` for no hotkey). The old key is unbound first; if the new one
    /// cannot be bound the old one is bound again and the reason comes back.
    pub fn set(&mut self, new: Option<KeySeq>) -> Result<(), String> {
        if new == self.key {
            return Ok(());
        }
        let old = self.key.take();
        if let Some(old) = old {
            self.backend.unbind(&old)?;
        }
        let Some(new) = new else {
            return Ok(());
        };
        match self.backend.bind(&new) {
            Ok(()) => {
                self.key = Some(new);
                Ok(())
            }
            Err(e) => {
                if let Some(old) = old {
                    // The old key stays, as far as the backend lets it.
                    if self.backend.bind(&old).is_ok() {
                        self.key = Some(old);
                    }
                }
                Err(e)
            }
        }
    }

    /// Binds the held key again: after the compositor reloaded its config.
    pub fn rebind(&mut self) -> Result<(), String> {
        match self.key {
            Some(key) => self.backend.bind(&key),
            None => Ok(()),
        }
    }
}

// ---- Hyprland ---------------------------------------------------------------------------------

/// Hyprland's name for a key.
fn hypr_key(key: Key) -> String {
    match key {
        Key::Char('+') => "plus".into(),
        Key::Char('-') => "minus".into(),
        Key::Char('=') => "equal".into(),
        Key::Char('.') => "period".into(),
        Key::Char(',') => "comma".into(),
        Key::Char('/') => "slash".into(),
        Key::Char('`') => "grave".into(),
        Key::Char(c) => c.to_ascii_lowercase().to_string(),
        Key::F(n) => format!("F{n}"),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::PgUp => "prior".into(),
        Key::PgDn => "next".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::Insert => "insert".into(),
        Key::Delete => "delete".into(),
        Key::Tab => "tab".into(),
        Key::Enter => "return".into(),
        Key::Esc => "escape".into(),
        Key::Space => "space".into(),
        Key::Backspace => "backspace".into(),
    }
}

/// `MODS,key`, the two fields of a Hyprland bind that name the key.
pub fn hypr_keyspec(seq: &KeySeq) -> String {
    let m = seq.mods;
    let mods: Vec<&str> = [
        (m.super_, "SUPER"),
        (m.ctrl, "CTRL"),
        (m.alt, "ALT"),
        (m.shift, "SHIFT"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    format!("{},{}", mods.join(" "), hypr_key(seq.key))
}

pub struct Hyprland {
    socket: PathBuf,
    /// What the key runs, as a shell command.
    command: String,
    /// The compositor's config runs `command` itself, so kyko binds nothing.
    deferred: bool,
}

impl Hyprland {
    pub fn new(socket: PathBuf, command: &str) -> Hyprland {
        Hyprland {
            socket,
            command: command.to_string(),
            deferred: false,
        }
    }

    /// Sends one command to the control socket and returns its answer.
    fn send(&self, line: &str) -> Result<String, String> {
        let mut s = UnixStream::connect(&self.socket)
            .map_err(|e| format!("Hyprland's control socket: {e}"))?;
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
        s.write_all(line.as_bytes())
            .map_err(|e| format!("Hyprland's control socket: {e}"))?;
        let mut answer = String::new();
        s.read_to_string(&mut answer)
            .map_err(|e| format!("Hyprland's control socket: {e}"))?;
        Ok(answer.trim().to_string())
    }

    /// Whether a bind in the compositor's config already runs our command.
    fn bound_elsewhere(&self) -> bool {
        self.send("j/binds")
            .is_ok_and(|binds| binds.contains(&self.command))
    }

    fn keyword(&self, text: &str) -> Result<(), String> {
        match self.send(&format!("/keyword {text}"))?.as_str() {
            "ok" => Ok(()),
            other => Err(format!("Hyprland refused `{text}`: {other}")),
        }
    }
}

impl Backend for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    fn bind(&mut self, key: &KeySeq) -> Result<(), String> {
        // Asked before each bind: kyko's own runtime bind is gone by then (unbound, or forgotten
        // by a reload), so a match is the config's.
        self.deferred = self.bound_elsewhere();
        if self.deferred {
            return Ok(());
        }
        self.keyword(&format!("bind {},exec,{}", hypr_keyspec(key), self.command))
    }

    fn unbind(&mut self, key: &KeySeq) -> Result<(), String> {
        if self.deferred {
            return Ok(());
        }
        self.keyword(&format!("unbind {}", hypr_keyspec(key)))
    }
}

/// Where Hyprland's control socket and event socket are, from the environment.
pub fn hyprland_sockets(env: &dyn Fn(&str) -> Option<String>) -> Option<(PathBuf, PathBuf)> {
    let sig = env("HYPRLAND_INSTANCE_SIGNATURE").filter(|s| !s.is_empty())?;
    let run = env("XDG_RUNTIME_DIR").filter(|s| !s.is_empty())?;
    let dir = Path::new(&run).join("hypr").join(sig);
    Some((dir.join(".socket.sock"), dir.join(".socket2.sock")))
}

/// Calls `on_reload` each time Hyprland says it reloaded its config, for as long as the process
/// lives; a lost event socket is tried again every two seconds.
pub fn watch_reloads(events: PathBuf, on_reload: impl Fn() + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("kyko-hypr".into())
        .spawn(move || {
            loop {
                if let Ok(s) = UnixStream::connect(&events) {
                    for line in BufReader::new(s).lines().map_while(Result::ok) {
                        if line.starts_with("configreloaded>>") {
                            on_reload();
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
}

// ---- manual -----------------------------------------------------------------------------------

/// No way to bind a key: the user does it in the compositor's config.
pub struct Manual {
    command: String,
}

impl Manual {
    pub fn new(command: &str) -> Manual {
        Manual {
            command: command.to_string(),
        }
    }
}

impl Backend for Manual {
    fn name(&self) -> &'static str {
        "manual"
    }

    fn bind(&mut self, key: &KeySeq) -> Result<(), String> {
        Err(format!(
            "no way to bind {key} here; for Hyprland add `bind = {}, exec, {}` to the config, or bind the command `{}` in your compositor",
            hypr_keyspec(key).replacen(',', ", ", 1),
            self.command,
            self.command
        ))
    }

    fn unbind(&mut self, _key: &KeySeq) -> Result<(), String> {
        Ok(())
    }
}

/// The first backend that fits this environment, and for Hyprland where to watch for reloads.
pub fn pick(
    env: &dyn Fn(&str) -> Option<String>,
    command: &str,
) -> (Box<dyn Backend>, Option<PathBuf>) {
    match hyprland_sockets(env) {
        Some((control, events)) if control.exists() => {
            (Box::new(Hyprland::new(control, command)), Some(events))
        }
        _ => (Box::new(Manual::new(command)), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc::{Receiver, channel};
    use std::sync::{Arc, Mutex};

    fn key(s: &str) -> KeySeq {
        s.parse().unwrap()
    }

    /// A Hyprland control socket that records what it is told and answers `ok`, or an error
    /// for any line containing `refuse`.
    fn fake_control(dir: &Path) -> (PathBuf, Arc<Mutex<Vec<String>>>) {
        fake_control_with(dir, "[]")
    }

    /// [`fake_control`] that answers `j/binds` with `binds`, and leaves that question out of
    /// what it records.
    fn fake_control_with(dir: &Path, binds: &'static str) -> (PathBuf, Arc<Mutex<Vec<String>>>) {
        let path = dir.join(".socket.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let mut line = String::new();
                let mut buf = [0u8; 512];
                let n = s.read(&mut buf).unwrap_or(0);
                line.push_str(&String::from_utf8_lossy(&buf[..n]));
                if line == "j/binds" {
                    let _ = s.write_all(binds.as_bytes());
                    continue;
                }
                let answer = if line.contains("refuse") {
                    "bad key"
                } else {
                    "ok"
                };
                log.lock().unwrap().push(line);
                let _ = s.write_all(answer.as_bytes());
            }
        });
        (path, seen)
    }

    #[test]
    fn keys_are_spelled_the_way_hyprland_reads_them() {
        assert_eq!(hypr_keyspec(&key("F12")), ",F12");
        assert_eq!(hypr_keyspec(&key("Ctrl+Shift+T")), "CTRL SHIFT,t");
        assert_eq!(hypr_keyspec(&key("Super+Alt+PgUp")), "SUPER ALT,prior");
        assert_eq!(hypr_keyspec(&key("Ctrl+Space")), "CTRL,space");
        assert_eq!(hypr_keyspec(&key("Super+Esc")), "SUPER,escape");
    }

    #[test]
    fn binding_sends_a_runtime_bind_and_unbinding_takes_it_away() {
        let dir = tempfile::tempdir().unwrap();
        let (sock, seen) = fake_control(dir.path());
        let mut h = Hotkeys::new(Box::new(Hyprland::new(sock, "kyko toggle")));
        h.set(Some(key("F12"))).unwrap();
        h.set(Some(key("Ctrl+F11"))).unwrap();
        h.set(None).unwrap();
        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen,
            [
                "/keyword bind ,F12,exec,kyko toggle",
                "/keyword unbind ,F12",
                "/keyword bind CTRL,F11,exec,kyko toggle",
                "/keyword unbind CTRL,F11",
            ]
        );
        assert_eq!(h.key(), None);
    }

    #[test]
    fn a_key_the_config_already_binds_is_left_to_it() {
        let dir = tempfile::tempdir().unwrap();
        // uQ's Settings › Shortcuts binds F12 to the toggle.
        let (sock, seen) = fake_control_with(
            dir.path(),
            r#"[{"key": "F12", "dispatcher": "exec", "arg": "sh -c 'command -v kyko >/dev/null && exec kyko toggle'"}]"#,
        );
        let mut h = Hotkeys::new(Box::new(Hyprland::new(sock, "kyko toggle")));
        h.set(Some(key("F12"))).unwrap();
        h.rebind().unwrap();
        h.set(None).unwrap();
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_refused_key_puts_the_old_one_back() {
        let dir = tempfile::tempdir().unwrap();
        let (sock, seen) = fake_control(dir.path());
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
        let (sock, seen) = fake_control(dir.path());
        let mut h = Hotkeys::new(Box::new(Hyprland::new(sock, "kyko toggle")));
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
        assert!(err.contains("bind = , F12, exec, kyko toggle"), "{err}");
        assert!(m.unbind(&key("F12")).is_ok());
    }
}
