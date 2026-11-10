//! A shell on a pseudo-terminal: spawn it, talk to it, ask what it is running.

use std::fs::File;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use nix::pty::{Winsize, openpty};
use nix::sys::signal::{Signal, killpg};
use nix::sys::wait::{Id, WaitPidFlag, waitid, waitpid};
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
    /// Fails when this process is out of descriptors.
    pub fn reader(&self) -> io::Result<File> {
        self.master.try_clone().map(File::from)
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

    /// The terminal's foreground process group, or `None` if it has none
    /// (the kernel answers 0 then, for instance once the shell has ended).
    fn foreground_group(&self) -> Option<i32> {
        tcgetpgrp(&self.master)
            .ok()
            .map(|g| g.as_raw())
            .filter(|&g| g > 0)
    }

    /// A running process of the foreground group `pgid`: its leader, or, once
    /// the leader has ended (`dmesg | less` after `dmesg` is done), one of the
    /// shell's children in the group. `None` if neither is found, which is
    /// cheap to find out: the shell has few children.
    fn group_member(&self, pgid: i32) -> Option<i32> {
        let running_in_group =
            |pid: i32| matches!(stat(pid), Some((state, g)) if state != 'Z' && g == pgid);
        if running_in_group(pgid) {
            return Some(pgid);
        }
        let children =
            std::fs::read_to_string(format!("/proc/{0}/task/{0}/children", self.pid)).ok()?;
        children
            .split_whitespace()
            .filter_map(|c| c.parse().ok())
            .find(|&c| running_in_group(c))
    }

    /// The terminal's foreground process group and its program's name: the
    /// leader's, or another member's once the leader has ended (see
    /// [`Pty::group_member`]), or empty if no member is found.
    pub fn foreground(&self) -> Option<(i32, String)> {
        let pgid = self.foreground_group()?;
        let name = self
            .group_member(pgid)
            .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
            .map(|n| n.trim_end().to_string())
            .unwrap_or_default();
        Some((pgid, name))
    }

    /// The folder of a running member of the foreground group, or else of
    /// the shell.
    pub fn cwd(&self) -> Option<String> {
        let read = |pid: i32| {
            std::fs::read_link(format!("/proc/{pid}/cwd"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        };
        self.foreground_group()
            .and_then(|pgid| self.group_member(pgid))
            .and_then(read)
            .or_else(|| read(self.pid))
    }

    /// True when a process group other than the shell's holds the terminal.
    /// It compares the group ids alone, so a program whose group's leader has
    /// ended (`dmesg | less`) is still seen.
    pub fn is_busy(&self) -> bool {
        matches!(self.foreground_group(), Some(g) if g != self.pid)
    }

    /// Sends `SIGHUP` to the shell's process group.
    pub fn hangup(&self) {
        self.signal_group(Signal::SIGHUP);
    }

    /// Sends `SIGKILL` to the shell's process group unless the shell has
    /// already ended, and says whether it did. Never blocks: the check is a
    /// `poll` with no timeout.
    pub fn kill_if_running(&self) -> bool {
        if self.ends_within(Duration::ZERO) {
            return false;
        }
        self.signal_group(Signal::SIGKILL);
        true
    }

    /// Signals the shell's process group unless the shell has been reaped. The
    /// status lock is held across the kill: the shell is reaped only under that
    /// lock (by [`Pty::try_wait`]), so its pid, and with it the group id, cannot
    /// be recycled between the check and the kill.
    fn signal_group(&self, signal: Signal) {
        let status = self.status.lock().unwrap_or_else(|p| p.into_inner());
        if status.is_none() {
            let _ = killpg(Pid::from_raw(self.pid), signal);
        }
    }

    /// Blocks until the shell ends, then reaps it and returns what
    /// [`Pty::try_wait`] does. The wait itself does not reap (`WNOWAIT`), so
    /// reaping still happens only under the status lock.
    pub fn wait(&self) -> Option<i32> {
        loop {
            match waitid(
                Id::Pid(Pid::from_raw(self.pid)),
                WaitPidFlag::WEXITED | WaitPidFlag::WNOWAIT,
            ) {
                Err(Errno::EINTR) => continue,
                // Ended, or already reaped by try_wait: either way the status is known.
                _ => return self.try_wait(),
            }
        }
    }

    /// Hangs the shell up and, if it has not ended within `grace`, kills its
    /// process group. Blocks for up to `grace`; never signals a reaped shell.
    pub fn end(&self, grace: Duration) {
        self.hangup();
        if !self.ends_within(grace) {
            self.signal_group(Signal::SIGKILL);
        }
    }

    /// True once the shell has ended (reaped or not), waiting at most `limit`.
    /// Waits on [`Pty::exit_fd`], so the wait sleeps in `poll` rather than
    /// ticking.
    fn ends_within(&self, limit: Duration) -> bool {
        let fd = match self.exit_fd() {
            Ok(Some(fd)) => fd,
            Ok(None) => return true,
            Err(_) => return false,
        };
        let Ok(timeout) = rustix::event::Timespec::try_from(limit) else {
            return false;
        };
        let mut fds = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        loop {
            match rustix::event::poll(&mut fds, Some(&timeout)) {
                Ok(n) => return n > 0,
                // A signal is rare here; waiting the full limit again is harmless.
                Err(rustix::io::Errno::INTR) => {}
                Err(_) => return false,
            }
        }
    }

    /// A pidfd for the shell, which `poll` reports readable once the shell has
    /// ended; `None` if it has already been reaped. It is opened under the
    /// status lock while the shell is unreaped, so it refers to the shell and
    /// not to a process that reused its pid.
    pub fn exit_fd(&self) -> io::Result<Option<OwnedFd>> {
        let status = self.status.lock().unwrap_or_else(|p| p.into_inner());
        if status.is_some() {
            return Ok(None);
        }
        let pid = rustix::process::Pid::from_raw(self.pid)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())
            .map(Some)
            .map_err(io::Error::from)
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

/// The state letter and process group of `pid`, from `/proc/<pid>/stat`.
fn stat(pid: i32) -> Option<(char, i32)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The fields after the program name, which is in parentheses and may hold
    // anything: state, parent, group.
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let group = fields.nth(1)?.parse().ok()?;
    Some((state, group))
}

/// How long a dropped [`Pty`] gives its shell after `SIGHUP`, and again after `SIGKILL`.
const DROP_GRACE: Duration = Duration::from_millis(100);

impl Drop for Pty {
    /// A dropped `Pty` takes its shell with it and leaves no zombie: `SIGHUP` to the
    /// group, up to 100 ms to end, then `SIGKILL` to the group and up to 100 ms
    /// more. A shell still not gone after that (stuck in the kernel) is reaped by
    /// a short-lived thread, so the caller blocks for at most about 200 ms.
    fn drop(&mut self) {
        if self.try_wait().is_some() {
            return;
        }
        self.hangup();
        if self.ends_within(DROP_GRACE) && self.try_wait().is_some() {
            return;
        }
        self.signal_group(Signal::SIGKILL);
        if self.ends_within(DROP_GRACE) && self.try_wait().is_some() {
            return;
        }
        let pid = Pid::from_raw(self.pid);
        let reap = move || {
            while let Err(Errno::EINTR) = waitpid(pid, None) {}
        };
        if std::thread::Builder::new()
            .name("reaper".into())
            .spawn(reap)
            .is_err()
        {
            reap();
        }
    }
}
