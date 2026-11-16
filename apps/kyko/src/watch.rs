//! Live reload: a thread looks at `config.toml` and uQ's `tokens.json` twice a second and calls
//! back when either changed. Polling keeps kyko free of an inotify dependency, and a hidden
//! kyko costs two `stat` calls per half second.
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn stamp(p: &PathBuf) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

pub fn spawn(files: Vec<PathBuf>, on_change: impl Fn() + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("kyko-watch".into())
        .spawn(move || {
            let mut seen: Vec<_> = files.iter().map(stamp).collect();
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let now: Vec<_> = files.iter().map(stamp).collect();
                if now != seen {
                    seen = now;
                    on_change();
                }
            }
        });
}
