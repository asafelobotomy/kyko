//! Live reload, without polling. uQ's theme is followed by uQ's shared watcher (`uq_theme`,
//! inotify on the theme folder, which uqd swaps whole); `config.toml` by an inotify watch on its
//! folder (or the nearest one that exists yet), so an editor's save-by-rename is seen too. Either
//! change calls back once; a burst of events is read once, 150 ms after the first.
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::uq_theme;

pub fn spawn(
    config: Option<PathBuf>,
    tokens: Option<PathBuf>,
    on_change: impl Fn() + Send + Sync + 'static,
) {
    let on_change = Arc::new(on_change);
    let theme = Arc::clone(&on_change);
    uq_theme::watch(tokens, move |_| {
        theme();
        true
    });
    if let Some(config) = config {
        let _ = std::thread::Builder::new()
            .name("kyko-watch".into())
            .spawn(move || follow(&config, &*on_change));
    }
}

fn stamp(p: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn follow(file: &Path, on_change: &dyn Fn()) {
    // SAFETY: plain syscall; a negative result is an error and is checked.
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    if fd < 0 {
        return;
    }
    let mut seen = stamp(file);
    let mut wds: Vec<i32> = Vec::new();
    loop {
        // Watch first, then look: a write in between is still seen.
        for wd in wds.drain(..) {
            // SAFETY: wd came from inotify_add_watch on fd.
            unsafe { libc::inotify_rm_watch(fd, wd) };
        }
        let mut dir = file.parent();
        while let Some(d) = dir {
            if d.is_dir() {
                let mask = libc::IN_CREATE
                    | libc::IN_MOVED_TO
                    | libc::IN_CLOSE_WRITE
                    | libc::IN_MODIFY
                    | libc::IN_DELETE
                    | libc::IN_MOVED_FROM
                    | libc::IN_DELETE_SELF
                    | libc::IN_MOVE_SELF;
                if let Ok(c) = CString::new(d.as_os_str().as_bytes()) {
                    // SAFETY: fd is ours and c is a valid NUL-terminated path.
                    let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), mask) };
                    if wd >= 0 {
                        wds.push(wd);
                    }
                }
                break;
            }
            dir = d.parent();
        }
        let now = stamp(file);
        if now != seen {
            seen = now;
            on_change();
        }
        if !wait(fd) {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
        drain(fd);
    }
    // SAFETY: closing our own descriptor once.
    unsafe { libc::close(fd) };
}

/// Block until an event; false when the descriptor is broken.
fn wait(fd: i32) -> bool {
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY: reads into our own buffer of the stated length.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            return true;
        }
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return false;
    }
}

/// Drop the events that came meanwhile; the look that follows covers them.
fn drain(fd: i32) {
    let mut buf = [0u8; 4096];
    loop {
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd, no wait; then a read into our own buffer.
        if unsafe { libc::poll(&mut p, 1, 0) } <= 0
            || unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) } <= 0
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn a_saved_or_replaced_config_is_seen_without_polling() {
        let dir = std::env::temp_dir().join(format!("kyko-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("kyko/config.toml");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        spawn(Some(file.clone()), None, move || {
            let _ = tx.lock().unwrap().send(());
        });
        std::thread::sleep(Duration::from_millis(100));
        // The folder does not exist yet.
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(&file, "[window]\n").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Saved by rename, as editors do.
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, "[window]\nheight_pct = 50\n").unwrap();
        std::fs::rename(&tmp, &file).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
