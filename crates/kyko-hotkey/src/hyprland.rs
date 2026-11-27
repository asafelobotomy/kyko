//! Hyprland: a runtime bind through the control socket. Hyprland 0.57 and later read a Lua
//! config and take Lua through `eval` (`hl.bind`, kept in a Lua global so it can be removed
//! again without touching the config's own binds on the same key); older releases take
//! `keyword bind`. Which one works is found on the first bind and kept.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use kyko_core::keys::{Key, KeySeq};

use crate::Backend;

/// The `description` kyko gives its own bind, so `binds` can tell it from the config's.
pub const DESCRIPTION: &str = "kyko:toggle";
/// The description uQ's Settings › Shortcuts gives its F12 drop-down bind.
const UQ_DROPDOWN: &str = "uq:uq.terminal.dropdown";
/// The Lua global holding kyko's bind object.
const LUA_SLOT: &str = "__kyko_hotkey";

/// Hyprland's modifier bits (`modmask` in `binds`): Shift, Ctrl, Alt, Super. Caps Lock and
/// Num Lock (bits 1 and 4) are left out of a comparison.
const SHIFT: u64 = 1;
const CTRL: u64 = 4;
const ALT: u64 = 8;
const SUPER: u64 = 64;

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

fn mod_names(seq: &KeySeq) -> Vec<&'static str> {
    let m = seq.mods;
    [
        (m.super_, "SUPER"),
        (m.ctrl, "CTRL"),
        (m.alt, "ALT"),
        (m.shift, "SHIFT"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect()
}

fn modmask(seq: &KeySeq) -> u64 {
    let m = seq.mods;
    [
        (m.shift, SHIFT),
        (m.ctrl, CTRL),
        (m.alt, ALT),
        (m.super_, SUPER),
    ]
    .into_iter()
    .filter_map(|(on, bit)| on.then_some(bit))
    .sum()
}

/// `MODS,key`, the two fields of a hyprlang bind that name the key.
pub fn hypr_keyspec(seq: &KeySeq) -> String {
    format!("{},{}", mod_names(seq).join(" "), hypr_key(seq.key))
}

/// `MODS + key`, the key string of Lua's `hl.bind`.
pub fn hypr_lua_keys(seq: &KeySeq) -> String {
    let mut parts = mod_names(seq);
    let key = hypr_key(seq.key);
    parts.push(&key);
    parts.join(" + ")
}

/// `s` as a Lua string literal.
pub fn lua_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\{}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dialect {
    Lua,
    Keyword,
}

pub struct Hyprland {
    socket: PathBuf,
    /// What the key runs, as a shell command.
    command: String,
    /// The compositor's config runs `command` on this key itself, so kyko binds nothing.
    deferred: bool,
    dialect: Option<Dialect>,
}

impl Hyprland {
    pub fn new(socket: PathBuf, command: &str) -> Hyprland {
        Hyprland {
            socket,
            command: command.to_string(),
            deferred: false,
            dialect: None,
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

    /// Whether a bind in the compositor's config already runs our command on `key`. A hyprlang
    /// bind shows its command (`arg`); a Lua bind shows only a function, so uQ's is known by its
    /// description. kyko's own bind is never counted.
    fn bound_elsewhere(&self, key: &KeySeq) -> bool {
        let Ok(answer) = self.send("j/binds") else {
            return false;
        };
        let Ok(serde_json::Value::Array(binds)) = serde_json::from_str(&answer) else {
            return false;
        };
        let text = |b: &serde_json::Value, f: &str| b[f].as_str().unwrap_or("").to_string();
        binds.iter().any(|b| {
            let description = text(b, "description");
            let mask = b["modmask"].as_u64().unwrap_or(0) & (SHIFT | CTRL | ALT | SUPER);
            description != DESCRIPTION
                && mask == modmask(key)
                && text(b, "key").eq_ignore_ascii_case(&hypr_key(key.key))
                && text(b, "submap").is_empty()
                && (description == UQ_DROPDOWN || text(b, "arg").contains(&self.command))
        })
    }

    /// Runs `lua` on a Lua config, else `keyword` on a hyprlang one.
    fn run(&mut self, lua: &str, keyword: &str) -> Result<(), String> {
        if self.dialect != Some(Dialect::Keyword) {
            let answer = self.send(&format!("/eval {lua}"))?;
            match answer.as_str() {
                "ok" => {
                    self.dialect = Some(Dialect::Lua);
                    return Ok(());
                }
                // A hyprlang config, or a Hyprland from before `eval`.
                a if self.dialect.is_none()
                    && (a == "unknown request" || a.starts_with("eval is only supported")) =>
                {
                    self.dialect = Some(Dialect::Keyword);
                }
                other => return Err(format!("Hyprland refused `{lua}`: {other}")),
            }
        }
        match self.send(&format!("/keyword {keyword}"))?.as_str() {
            "ok" => Ok(()),
            other => Err(format!("Hyprland refused `{keyword}`: {other}")),
        }
    }
}

impl Backend for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    fn bind(&mut self, key: &KeySeq) -> Result<(), String> {
        self.deferred = self.bound_elsewhere(key);
        if self.deferred {
            return Ok(());
        }
        // A bind still held (a reload that kept the Lua state) is replaced, not doubled.
        let lua = format!(
            "if {LUA_SLOT} then {LUA_SLOT}:remove() end {LUA_SLOT} = hl.bind({}, hl.dsp.exec_cmd({}), {{ description = {} }})",
            lua_string(&hypr_lua_keys(key)),
            lua_string(&self.command),
            lua_string(DESCRIPTION),
        );
        let keyword = format!("bind {},exec,{}", hypr_keyspec(key), self.command);
        self.run(&lua, &keyword)
    }

    fn unbind(&mut self, key: &KeySeq) -> Result<(), String> {
        if self.deferred {
            return Ok(());
        }
        let lua = format!("if {LUA_SLOT} then {LUA_SLOT}:remove() {LUA_SLOT} = nil end");
        self.run(&lua, &format!("unbind {}", hypr_keyspec(key)))
    }
}
