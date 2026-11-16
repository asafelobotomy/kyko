//! The drop-down's toggle hotkey: bound through the compositor when kyko knows how, else
//! kyko prints the line to bind by hand.
use std::sync::Mutex;

use kyko_core::keys::KeySeq;
use kyko_hotkey::Hotkeys;

static HOTKEYS: Mutex<Option<Hotkeys>> = Mutex::new(None);

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// Binds the hotkey, and follows the compositor's config reloads, which forget it.
pub fn start(key: Option<KeySeq>) {
    let (backend, events) = kyko_hotkey::pick(&env, "kyko toggle");
    let mut hotkeys = Hotkeys::new(backend);
    if let Err(e) = hotkeys.set(key) {
        eprintln!("kyko: hotkey ({}): {e}", hotkeys.backend_name());
    }
    *HOTKEYS.lock().unwrap() = Some(hotkeys);
    if let Some(events) = events {
        kyko_hotkey::watch_reloads(events, || {
            if let Some(h) = HOTKEYS.lock().unwrap().as_mut()
                && let Err(e) = h.rebind()
            {
                eprintln!("kyko: hotkey ({}): {e}", h.backend_name());
            }
        });
    }
}

/// The config changed: move the binding when the key did.
pub fn apply(key: Option<KeySeq>) {
    if let Some(h) = HOTKEYS.lock().unwrap().as_mut()
        && h.key() != key
        && let Err(e) = h.set(key)
    {
        eprintln!("kyko: hotkey ({}): {e}", h.backend_name());
    }
}
