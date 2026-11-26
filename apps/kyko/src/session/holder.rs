//! Starting and finding `kykod`: a private one for this window, or the shared one.

use super::*;

/// Finds `kykod`: next to this executable, else on `PATH`.
pub(crate) fn kykod_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("kykod")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| "kykod".into())
}

/// A `kykod` serving `socket` for this window alone: started here, ended with the window. The
/// drop-down will use the shared one the systemd unit starts; a window opened with `kyko -e`
/// must not take that one's single connection.
pub fn start_private_holder(socket: &Path) -> std::io::Result<Child> {
    let mut child = Command::new(kykod_path())
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Ok(child);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "kykod did not start",
    ))
}

/// Waits briefly for the private `kykod` to end after `EndSessions`, so it is not left a zombie;
/// kills it if it does not.
pub fn reap(mut holder: Child) {
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        if matches!(holder.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = holder.kill();
    let _ = holder.wait();
}

/// Makes sure a `kykod` serves `socket`, starting one of its own when none does: the drop-down's
/// shared holder. With the systemd unit installed `kykod` is already there and this does nothing.
pub fn ensure_shared_holder(socket: &Path) -> std::io::Result<()> {
    if std::os::unix::net::UnixStream::connect(socket).is_ok() {
        return Ok(());
    }
    use std::os::unix::process::CommandExt;
    // Its own process group: the holder must outlive this window and its terminal.
    let _child = Command::new(kykod_path())
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "kykod did not start",
    ))
}
