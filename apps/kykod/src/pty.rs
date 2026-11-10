//! A shell on a pseudo-terminal: spawn it, talk to it, ask what it is running.

use std::fs::File;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use nix::pty::{Winsize, openpty};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::{Pid, tcgetpgrp};

/// Why a shell could not be started, as a sentence fit to show the user.
#[derive(Debug)]
pub struct SpawnError(pub String);

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SpawnError {}

/// A running shell and the master side of its terminal.
pub struct Pty {
    master: OwnedFd,
    child: Mutex<Child>,
    pid: i32,
    status: Mutex<Option<i32>>,
}

/// Held while a terminal is created and its child spawned, so that no other
/// spawn in this process can fork while a descriptor is not yet close-on-exec.
static SPAWN: Mutex<()> = Mutex::new(());

fn winsize(cols: u16, rows: u16) -> Winsize {
    Winsize {
        ws_row: rows.max(1),
        ws_col: cols.max(2),
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

fn set_cloexec(fd: impl AsFd) -> Result<(), Errno> {
    fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)).map(|_| ())
}

fn errno_text(e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => Errno::from_raw(n).desc().to_string(),
        None => e.to_string(),
    }
}

impl Pty {
    /// Starts `shell` with `args` in `cwd` (empty means the home folder), on top of
    /// this process's environment plus `TERM`, `COLORTERM` and `env`.
    pub fn spawn(
        shell: &str,
        args: &[String],
        cwd: &str,
        env: &[(String, String)],
        cols: u16,
        rows: u16,
    ) -> Result<Pty, SpawnError> {
        let dir = if cwd.is_empty() {
            std::env::var("HOME")
                .ok()
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| "/".to_string())
        } else {
            cwd.to_string()
        };
        match std::fs::metadata(Path::new(&dir)) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(SpawnError(format!("{dir}: not a folder"))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SpawnError(format!("{dir}: folder does not exist")));
            }
            Err(e) => return Err(SpawnError(format!("{dir}: {}", errno_text(&e)))),
        }

        let _guard = SPAWN.lock().unwrap_or_else(|p| p.into_inner());
        let size = winsize(cols, rows);
        let pty = openpty(&size, None)
            .map_err(|e| SpawnError(format!("cannot open a terminal: {}", e.desc())))?;
        let (master, slave) = (pty.master, pty.slave);
        set_cloexec(&master)
            .and_then(|()| set_cloexec(&slave))
            .map_err(|e| SpawnError(format!("cannot open a terminal: {}", e.desc())))?;

        let dup = |fd: &OwnedFd| {
            fd.try_clone()
                .map_err(|e| SpawnError(format!("cannot open a terminal: {}", errno_text(&e))))
        };
        let mut cmd = Command::new(shell);
        cmd.args(args)
            .current_dir(&dir)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::from(dup(&slave)?))
            .stdout(Stdio::from(dup(&slave)?))
            .stderr(Stdio::from(dup(&slave)?));
        // SAFETY: the closure runs in the forked child before exec, so it may only call
        // async-signal-safe functions and must not allocate or take locks. It calls
        // `setsid(2)` and `ioctl(2)` with `TIOCSCTTY`, both async-signal-safe, touches no
        // Rust-managed state, and builds its error from a raw errno, which does not
        // allocate. Descriptor 0 is the slave by then, because `Command` has already
        // duplicated the stdio descriptors onto 0, 1 and 2.
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid().map_err(|e| io::Error::from_raw_os_error(e as i32))?;
                if nix::libc::ioctl(0, nix::libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd
            .spawn()
            .map_err(|e| SpawnError(format!("{shell}: {}", errno_text(&e))))?;
        // The parent must not keep the slave: its presence would stop the master
        // from ever seeing the end of the shell's output.
        drop(cmd);
        drop(slave);
        let pid = child.id() as i32;
        Ok(Pty {
            master,
            child: Mutex::new(child),
            pid,
            status: Mutex::new(None),
        })
    }

    /// A blocking duplicate of the master. A read returns `EIO` once the shell is gone.
    pub fn reader(&self) -> File {
        File::from(
            self.master
                .try_clone()
                .expect("duplicating a descriptor failed"),
        )
    }

    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        let mut rest = bytes;
        while !rest.is_empty() {
            match nix::unistd::write(&self.master, rest) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => rest = &rest[n..],
                Err(Errno::EINTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Tells the terminal its new size; the foreground program gets `SIGWINCH`.
    pub fn resize(&self, cols: u16, rows: u16) {
        let s = winsize(cols, rows);
        let _ = rustix::termios::tcsetwinsize(
            &self.master,
            rustix::termios::Winsize {
                ws_row: s.ws_row,
                ws_col: s.ws_col,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
        );
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// The terminal's foreground process group and its leader's program name.
    pub fn foreground(&self) -> Option<(i32, String)> {
        let pgid = tcgetpgrp(&self.master).ok()?.as_raw();
        let name = std::fs::read_to_string(format!("/proc/{pgid}/comm")).ok()?;
        Some((pgid, name.trim_end().to_string()))
    }

    /// The folder of the foreground group's leader, or else of the shell.
    pub fn cwd(&self) -> Option<String> {
        let read = |pid: i32| {
            std::fs::read_link(format!("/proc/{pid}/cwd"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        };
        self.foreground()
            .and_then(|(pgid, _)| read(pgid))
            .or_else(|| read(self.pid))
    }

    /// True when something other than the shell itself holds the terminal.
    pub fn is_busy(&self) -> bool {
        matches!(self.foreground(), Some((pgid, _)) if pgid != self.pid)
    }

    /// Sends `SIGHUP` to the shell's process group.
    pub fn hangup(&self) {
        if self
            .status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
        {
            return;
        }
        let _ = killpg(Pid::from_raw(self.pid), Signal::SIGHUP);
    }

    /// The exit status once the shell has ended (128 plus the signal if it was
    /// killed), reaping it on the first call and answering the same ever after.
    pub fn try_wait(&self) -> Option<i32> {
        let mut status = self.status.lock().unwrap_or_else(|p| p.into_inner());
        if status.is_none() {
            let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
            if let Ok(Some(s)) = child.try_wait() {
                *status = Some(
                    s.code()
                        .or_else(|| s.signal().map(|n| 128 + n))
                        .unwrap_or(1),
                );
            }
        }
        *status
    }
}
