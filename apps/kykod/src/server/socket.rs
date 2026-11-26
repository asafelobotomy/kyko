//! The socket and its lock file: taking the lock, replacing a stale socket, and binding.

use super::*;

/// The longest socket path Linux binds: `sun_path` is 108 bytes, one of them
/// the terminating NUL.
pub(crate) const MAX_SOCKET_PATH: usize = 107;

/// The lock file beside `socket`: `<socket path>.lock`.
pub(crate) fn lock_path(socket: &Path) -> PathBuf {
    let mut p = socket.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

/// Makes the socket: its folder 0700 if it has to be created, then an
/// exclusive lock on `<socket path>.lock` (0600), then a stale socket file
/// replaced (any other kind of file there is refused, never removed), then the socket itself, 0600. Only the holder of the lock touches
/// the socket file, so two kykods starting together cannot both decide the
/// socket is stale and take it from each other. The caller keeps the returned
/// lock for the life of the process.
pub(crate) fn bind(path: &Path) -> Result<(UnixListener, Flock<File>), String> {
    let shown = path.display();
    // Checked before anything is created, so a path too long to bind leaves
    // no folder or lock file behind.
    if std::os::unix::net::SocketAddr::from_pathname(path).is_err() {
        return Err(format!(
            "{shown} is {} bytes long; a socket path must be at most {MAX_SOCKET_PATH}",
            path.as_os_str().len()
        ));
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && fs::symlink_metadata(dir).is_err()
    {
        DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .and_then(|()| fs::set_permissions(dir, Permissions::from_mode(0o700)))
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let lock_file = lock_path(path);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        // A link here is not ours to follow: refused (ELOOP), not locked.
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&lock_file)
        .map_err(|e| format!("cannot open {}: {e}", lock_file.display()))?;
    let lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| {
        if e == Errno::EWOULDBLOCK {
            format!("another kykod is serving {shown}")
        } else {
            format!("cannot lock {}: {}", lock_file.display(), e.desc())
        }
    })?;
    match fs::symlink_metadata(path) {
        // Only a socket is ours to replace: any other file there was put by
        // someone else (a mistyped --socket, say) and is left alone.
        Ok(meta) if !meta.file_type().is_socket() => {
            return Err(format!("{shown} exists and is not a socket"));
        }
        Ok(_) => match UnixStream::connect(path) {
            Ok(_) => return Err(format!("{shown} is already served by a running kykod")),
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                fs::remove_file(path).map_err(|e| format!("cannot replace {shown}: {e}"))?;
                log!("replaced a stale socket file at {shown}");
            }
            Err(e) => return Err(format!("cannot use {shown}: {e}")),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot use {shown}: {e}")),
    }
    // The socket is created with mode 0600 from the start: no moment in which
    // another user could connect. This runs before any thread is started, so
    // changing the process-wide umask here is safe.
    let old = umask(Mode::from_bits_truncate(0o177));
    let bound = UnixListener::bind(path);
    umask(old);
    let listener = bound.map_err(|e| format!("cannot listen on {shown}: {e}"))?;
    fs::set_permissions(path, Permissions::from_mode(0o600))
        .map_err(|e| format!("cannot set the mode of {shown}: {e}"))?;
    Ok((listener, lock))
}
