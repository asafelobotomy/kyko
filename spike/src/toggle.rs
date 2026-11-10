//! `Toggler`: emits `toggled()` on the Qt thread for each SIGUSR1.
use std::pin::Pin;

use cxx_qt::Threading;

#[cxx_qt::bridge]
pub mod qobject {
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type Toggler = super::TogglerRust;

        #[qsignal]
        fn toggled(self: Pin<&mut Toggler>);

        #[qinvokable]
        fn start(self: Pin<&mut Toggler>);

        /// True when KYKO_SPIKE_NOFOCUS is set: automated runs must not take the keyboard.
        #[qinvokable]
        fn no_focus(self: &Toggler) -> bool;

        /// True when KYKO_SPIKE_LAYER=top (default: overlay).
        #[qinvokable]
        fn layer_top(self: &Toggler) -> bool;

        /// True when KYKO_SPIKE_START_HIDDEN is set.
        #[qinvokable]
        fn start_hidden(self: &Toggler) -> bool;
    }

    impl cxx_qt::Threading for Toggler {}
}

/// SIGUSR1 is claimed in `main` before the pid file exists: an early `toggle` must not kill us.
pub static SIGNALS: std::sync::Mutex<Option<signal_hook::iterator::Signals>> = std::sync::Mutex::new(None);

pub fn claim_signal() {
    let s = signal_hook::iterator::Signals::new([signal_hook::consts::SIGUSR1]).expect("install SIGUSR1 handler");
    *SIGNALS.lock().unwrap() = Some(s);
}

#[derive(Default)]
pub struct TogglerRust;

impl qobject::Toggler {
    pub fn layer_top(&self) -> bool {
        std::env::var("KYKO_SPIKE_LAYER").is_ok_and(|v| v == "top")
    }

    pub fn start_hidden(&self) -> bool {
        std::env::var_os("KYKO_SPIKE_START_HIDDEN").is_some()
    }

    pub fn no_focus(&self) -> bool {
        std::env::var_os("KYKO_SPIKE_NOFOCUS").is_some()
    }

    pub fn start(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let Some(mut signals) = SIGNALS.lock().unwrap().take() else { return };
            for _ in signals.forever() {
                if qt.queue(|t| t.toggled()).is_err() {
                    break;
                }
            }
        });
    }
}
