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
    }

    impl cxx_qt::Threading for Toggler {}
}

#[derive(Default)]
pub struct TogglerRust;

impl qobject::Toggler {
    pub fn start(self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGUSR1])
                .expect("install SIGUSR1 handler");
            for _ in signals.forever() {
                if qt.queue(|t| t.toggled()).is_err() {
                    break;
                }
            }
        });
    }
}
