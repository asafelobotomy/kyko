//! The toggle hotkey (§2.9). On Wayland a program cannot grab a key: the compositor owns the
//! keyboard, so kyko registers its key with it. This crate holds the backends and the rules that
//! hold for all of them, with no Qt and no window:
//!
//! - [`Hyprland`]: a runtime bind through Hyprland's control socket (Lua `hl.bind` through
//!   `eval` on 0.57 and later, `keyword bind` before), put back after a config reload (the reload
//!   forgets runtime binds). When the compositor's own config already runs kyko's command on the
//!   same key (uQ binds F12 in Settings › Shortcuts), kyko leaves the key to it: two binds would
//!   toggle twice;
//! - [`Manual`]: nothing can be bound, so it says which line to put in the compositor's config.
//!
//! Further backends (`uq`, `kde`, the desktop portal) implement the same [`Backend`].
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kyko_core::keys::KeySeq;

mod hyprland;
pub use hyprland::{DESCRIPTION, Hyprland, hypr_keyspec, hypr_lua_keys, lua_string};

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
            "no way to bind {key} here; for Hyprland add `hl.bind({}, hl.dsp.exec_cmd({}))` to hyprland.lua (`bind = {}, exec, {}` to an older hyprland.conf), or bind the command `{}` in your compositor",
            lua_string(&hypr_lua_keys(key)),
            lua_string(&self.command),
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
mod tests;
