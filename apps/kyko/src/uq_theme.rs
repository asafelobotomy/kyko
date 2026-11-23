//! uq-theme 1.1.0
//!
//! uQ's theme tokens (`$XDG_STATE_HOME/uq/theme/tokens.json`, written by uqd) for a Rust app,
//! and a live follow of that file. Vendor this file unchanged (see uQ's `native/uq-theme/README.md`);
//! it needs only `serde_json` and `libc`.
//!
//! `load` gives the tokens as one JSON object, the same shape every uQ native app's QML `Theme`
//! singleton reads:
//!
//! `{present, name, variant, colors{bg, surface, ...}, radius, gapIn, gapOut, borderWidth,
//!   fontUi, fontMono, fontSize, iconTheme, motion{fast, base, slow, reduce}}`
//!
//! `motion` (1.1.0) is in ms and all 0 when the user turned on Reduce motion; a uQ older than
//! 0.3.36 writes none, so it is uQ's defaults (100, 140, 220) then.
//!
//! or `{present: false}` when uQ isn't there, the file doesn't parse, or a colour is missing or
//! not `#rrggbb`/`#rrggbbaa` (half a theme looks worse than the desktop's own).
//!
//! `watch` follows the file with inotify, not a poll. uqd replaces the whole `theme/` folder at
//! once (it renames a filled staging folder over it), so the folder and its parent are watched,
//! or the nearest folder that exists when uQ hasn't written a theme yet.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// The version of this file; `check-uq-theme.sh` compares it with the one a repo pins.
pub const VERSION: &str = "1.1.0";

/// The colour tokens every uQ theme defines (see uq-core's `theme.rs`).
pub const COLORS: [&str; 12] = [
    "bg",
    "surface",
    "surface_alt",
    "overlay",
    "text",
    "text_muted",
    "accent",
    "accent_text",
    "border",
    "success",
    "warning",
    "error",
];

/// Files bigger than this are not tokens.
const MAX_BYTES: u64 = 256 * 1024;

/// `$<override_var>` when set (tests and screenshots point it at a fixture), else
/// `$XDG_STATE_HOME/uq/theme/tokens.json`, else `~/.local/state/uq/theme/tokens.json`.
pub fn tokens_path(override_var: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(override_var).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(state.join("uq/theme/tokens.json"))
}

/// The tokens at `path` (see the module docs), `{present: false}` without them.
pub fn load(path: Option<&Path>) -> Value {
    let text = path
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= MAX_BYTES))
        .and_then(|p| std::fs::read_to_string(p).ok());
    text.map_or_else(absent, |t| parse(&t))
}

fn absent() -> Value {
    json!({ "present": false })
}

/// The tokens in `text` (the contents of tokens.json).
pub fn parse(text: &str) -> Value {
    let Ok(t) = serde_json::from_str::<Value>(text) else {
        return absent();
    };
    let mut colors = Map::new();
    for key in COLORS {
        match t["colors"][key].as_str().filter(|c| is_color(c)) {
            Some(c) => colors.insert(key.to_string(), json!(c)),
            None => return absent(),
        };
    }
    let num =
        |p: &str, def: u64, max: u64| t.pointer(p).and_then(Value::as_u64).unwrap_or(def).min(max);
    let text_at = |p: &str| {
        t.pointer(p)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let reduce = t.pointer("/motion/reduce").and_then(Value::as_bool) == Some(true);
    let ms = |k: &str, def: u64| {
        let v = t
            .pointer(&format!("/motion/{k}"))
            .and_then(Value::as_u64)
            .unwrap_or(def);
        if reduce { 0 } else { v.min(1000) }
    };
    let variant = if t["variant"].as_str() == Some("light") {
        "light"
    } else {
        "dark"
    };
    json!({
        "present": true,
        "name": t["name"].as_str().unwrap_or("uQ"),
        "variant": variant,
        "colors": colors,
        "radius": num("/ui/radius", 8, 32),
        "gapIn": num("/ui/gap_in", 5, 64),
        "gapOut": num("/ui/gap_out", 12, 64),
        "borderWidth": num("/ui/border_width", 1, 8),
        "fontUi": text_at("/fonts/ui"),
        "fontMono": text_at("/fonts/mono"),
        "fontSize": num("/fonts/size", 0, 32),
        "iconTheme": text_at("/icons/theme"),
        "motion": {
            "fast": ms("fast", 100),
            "base": ms("base", 140),
            "slow": ms("slow", 220),
            "reduce": reduce,
        },
    })
}

fn is_color(c: &str) -> bool {
    (c.len() == 7 || c.len() == 9)
        && c.starts_with('#')
        && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Follow the tokens file on a thread of its own: each time it changes, hand the new tokens (the
/// JSON text of `load`) to `send`, until `send` returns false (the window is gone). Only real
/// changes are sent. Without inotify (or without a path) nothing is sent and the app keeps the
/// tokens it loaded at start.
pub fn watch(path: Option<PathBuf>, send: impl Fn(String) -> bool + Send + 'static) {
    let Some(path) = path else { return };
    let _ = std::thread::Builder::new()
        .name("uq-theme".into())
        .spawn(move || {
            let Some(ino) = Inotify::new() else { return };
            let mut last = load(Some(&path)).to_string();
            loop {
                // watch first, then read: a file written in between is still seen
                ino.watch_nearest(&path);
                let now = load(Some(&path)).to_string();
                if now != last {
                    last = now.clone();
                    if !send(now) {
                        return;
                    }
                }
                if !ino.wait() {
                    return;
                }
                // uqd writes several files in a burst; let it finish, then read once
                std::thread::sleep(std::time::Duration::from_millis(150));
                ino.drain();
            }
        });
}

/// A small inotify handle: one watch on the folder that holds (or will hold) the file, and one on
/// the file itself while it exists (for an editor that rewrites it in place).
struct Inotify {
    fd: i32,
    watches: std::cell::RefCell<Vec<i32>>,
}

impl Inotify {
    fn new() -> Option<Self> {
        // SAFETY: plain syscall; a negative result is an error and is checked.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        (fd >= 0).then(|| Self {
            fd,
            watches: Default::default(),
        })
    }

    fn add(&self, p: &Path, mask: u32) {
        let Ok(c) = CString::new(p.as_os_str().as_bytes()) else {
            return;
        };
        // SAFETY: fd is ours and c is a valid NUL-terminated path.
        let wd = unsafe { libc::inotify_add_watch(self.fd, c.as_ptr(), mask) };
        if wd >= 0 {
            self.watches.borrow_mut().push(wd);
        }
    }

    /// Re-point the watches: the file's folder, or the nearest ancestor that exists, plus the file.
    fn watch_nearest(&self, file: &Path) {
        for wd in self.watches.borrow_mut().drain(..) {
            // SAFETY: wd came from inotify_add_watch on this fd; a stale one just returns an error.
            unsafe { libc::inotify_rm_watch(self.fd, wd) };
        }
        let dir_mask = libc::IN_CREATE
            | libc::IN_MOVED_TO
            | libc::IN_CLOSE_WRITE
            | libc::IN_DELETE
            | libc::IN_MOVED_FROM
            | libc::IN_DELETE_SELF
            | libc::IN_MOVE_SELF;
        let mut dir = file.parent();
        while let Some(d) = dir {
            if d.is_dir() {
                self.add(d, dir_mask);
                // uqd swaps the whole theme/ folder (rename), so its parent is watched too
                if let Some(up) = d.parent() {
                    self.add(up, dir_mask);
                }
                break;
            }
            dir = d.parent();
        }
        if file.exists() {
            self.add(file, libc::IN_CLOSE_WRITE | libc::IN_MODIFY);
        }
    }

    /// Block until something happens; false when the handle is broken.
    fn wait(&self) -> bool {
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: reads into our own buffer of the stated length.
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                return true;
            }
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
    }

    /// Throw away events that arrived meanwhile (they are covered by the read that follows).
    fn drain(&self) {
        let mut buf = [0u8; 4096];
        loop {
            let mut p = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd, no wait.
            if unsafe { libc::poll(&mut p, 1, 0) } <= 0 {
                return;
            }
            // SAFETY: as in wait().
            if unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) } <= 0 {
                return;
            }
        }
    }
}

impl Drop for Inotify {
    fn drop(&mut self) {
        // SAFETY: closing our own descriptor once.
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn tokens(accent: &str) -> String {
        let colors: Map<String, Value> = COLORS
            .iter()
            .map(|k| {
                (
                    k.to_string(),
                    json!(if *k == "accent" { accent } else { "#202020" }),
                )
            })
            .collect();
        json!({"name": "Test", "variant": "light", "colors": colors, "ui": {"radius": 10, "gap_in": 4}, "fonts": {"ui": "Inter", "size": 11}})
            .to_string()
    }

    #[test]
    fn a_full_theme_is_read() {
        let t = parse(&tokens("#d4737f"));
        assert_eq!(t["present"], true);
        assert_eq!(t["variant"], "light");
        assert_eq!(t["colors"]["accent"], "#d4737f");
        assert_eq!(t["radius"], 10);
        assert_eq!(t["gapIn"], 4);
        assert_eq!(t["gapOut"], 12);
        assert_eq!(t["fontUi"], "Inter");
    }

    #[test]
    fn a_partial_or_broken_file_is_no_theme() {
        assert_eq!(parse("not json"), absent());
        assert_eq!(parse(r##"{"colors":{"bg":"#000000"}}"##), absent());
        assert_eq!(parse(&tokens("red")), absent());
        assert_eq!(parse(&tokens("#12345")), absent());
        assert_eq!(load(None), absent());
        assert_eq!(load(Some(Path::new("/nonexistent/tokens.json"))), absent());
    }

    #[test]
    fn silly_numbers_are_clamped() {
        let t = parse(&tokens("#ffffff").replace("\"radius\":10", "\"radius\":500"));
        assert_eq!(t["radius"], 32);
    }

    #[test]
    fn motion_defaults_follows_and_reduces() {
        let old = parse(&tokens("#ffffff"));
        assert_eq!(
            old["motion"],
            json!({"fast": 100, "base": 140, "slow": 220, "reduce": false})
        );
        let with = |m: &str| {
            let mut v: Value = serde_json::from_str(&tokens("#ffffff")).unwrap();
            v["motion"] = serde_json::from_str(m).unwrap();
            parse(&v.to_string())["motion"].clone()
        };
        assert_eq!(
            with(r#"{"fast": 90, "base": 120, "slow": 99999, "reduce": false}"#),
            json!({"fast": 90, "base": 120, "slow": 1000, "reduce": false})
        );
        assert_eq!(
            with(r#"{"fast": 100, "base": 140, "slow": 220, "reduce": true}"#),
            json!({"fast": 0, "base": 0, "slow": 0, "reduce": true})
        );
    }

    #[test]
    fn a_replaced_file_is_seen_without_polling() {
        let dir = std::env::temp_dir().join(format!("uq-theme-test-{}", std::process::id()));
        let file = dir.join("uq/theme/tokens.json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = mpsc::channel();
        watch(Some(file.clone()), move |t| tx.send(t).is_ok());
        std::thread::sleep(Duration::from_millis(100));
        // first theme ever: the folders do not exist yet
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, tokens("#111111")).unwrap();
        std::fs::rename(&tmp, &file).unwrap();
        let got: Value =
            serde_json::from_str(&rx.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
        assert_eq!(got["colors"]["accent"], "#111111");
        // replaced, as uqd does it
        std::fs::write(&tmp, tokens("#222222")).unwrap();
        std::fs::rename(&tmp, &file).unwrap();
        let got: Value =
            serde_json::from_str(&rx.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
        assert_eq!(got["colors"]["accent"], "#222222");
        // the whole folder swapped, as uqd's atomic_replace_dir does it
        let theme = file.parent().unwrap();
        let staging = theme.with_file_name("theme.staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("tokens.json"), tokens("#333333")).unwrap();
        std::fs::rename(theme, theme.with_file_name("theme.old")).unwrap();
        std::fs::rename(&staging, theme).unwrap();
        std::fs::remove_dir_all(theme.with_file_name("theme.old")).unwrap();
        let got: Value =
            serde_json::from_str(&rx.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
        assert_eq!(got["colors"]["accent"], "#333333");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
