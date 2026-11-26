//! Test helpers that play the window's part against a real `kykod`.
#![allow(dead_code)] // each test file uses a different part of this module

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kyko_proto::{Decoder, Frame, PaneId, PaneInfo, SpawnSpec, ToHolder, ToWindow, VERSION};
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;

mod client;
mod procs;

pub use client::*;
pub use procs::*;

/// How long any single wait in these tests may take before it counts as a failure.
pub const LIMIT: Duration = Duration::from_secs(5);

pub fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kykod"))
}

/// A `kykod` serving a socket in its own temporary folder. Dropping it kills
/// every session its shells started, then `kykod` itself, so a failed assertion
/// leaves nothing running.
pub struct Holder {
    child: Child,
    socket: PathBuf,
    log: Arc<Mutex<String>>,
    /// Every shell pid [`Holder::children`] has seen. A shell that outlives a
    /// `kykod` which has exited is no longer its child, so cleanup needs these.
    seen: Mutex<Vec<i32>>,
    _dir: tempfile::TempDir,
}

impl Holder {
    /// Starts `kykod` on `<temp>/kyko/hold.sock`, a folder it has to create.
    pub fn start() -> Holder {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("kyko").join("hold.sock");
        Holder::start_at(dir, socket)
    }

    /// Starts `kykod` on `socket`, which lies inside `dir`; returns once it listens.
    pub fn start_at(dir: tempfile::TempDir, socket: PathBuf) -> Holder {
        let mut cmd = bin();
        cmd.arg("--socket").arg(&socket);
        Holder::launch(dir, socket, cmd, None)
    }

    /// Starts `cmd`, which serves `socket`. With `stderr` given, kykod's log
    /// goes there and readiness is found by connecting; otherwise the log is
    /// kept and readiness is its "listening on" line.
    pub fn launch(
        dir: tempfile::TempDir,
        socket: PathBuf,
        mut cmd: Command,
        stderr: Option<Stdio>,
    ) -> Holder {
        let piped = stderr.is_none();
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr.unwrap_or_else(Stdio::piped))
            .spawn()
            .unwrap();
        let log = Arc::new(Mutex::new(String::new()));
        let (tx, rx) = channel();
        if piped {
            let stderr = child.stderr.take().unwrap();
            let sink = log.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    if line.contains("listening on") {
                        let _ = tx.send(());
                    }
                    let mut log = sink.lock().unwrap();
                    log.push_str(&line);
                    log.push('\n');
                }
            });
        }
        let holder = Holder {
            child,
            socket,
            log,
            seen: Mutex::new(Vec::new()),
            _dir: dir,
        };
        let ready = if piped {
            rx.recv_timeout(LIMIT).is_ok()
        } else {
            let end = Instant::now() + LIMIT;
            loop {
                if UnixStream::connect(&holder.socket).is_ok() {
                    break true;
                }
                if Instant::now() >= end {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        assert!(
            ready,
            "kykod did not start listening; log:\n{}",
            holder.log()
        );
        holder
    }

    /// True while `kykod` itself is running.
    pub fn alive(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    /// The number of threads `kykod` has now.
    pub fn threads(&self) -> usize {
        let status = std::fs::read_to_string(format!("/proc/{}/status", self.pid())).unwrap();
        status
            .lines()
            .find_map(|l| l.strip_prefix("Threads:"))
            .and_then(|n| n.trim().parse().ok())
            .unwrap()
    }

    pub fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    /// `kykod`'s anonymous resident memory (`RssAnon`), in bytes.
    pub fn rss_anon(&self) -> u64 {
        let status = std::fs::read_to_string(format!("/proc/{}/status", self.pid())).unwrap();
        let kib: u64 = status
            .lines()
            .find_map(|l| l.strip_prefix("RssAnon:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
            .unwrap();
        kib * 1024
    }

    /// Voluntary plus involuntary context switches of every `kykod` thread.
    pub fn context_switches(&self) -> u64 {
        let mut total = 0;
        for task in std::fs::read_dir(format!("/proc/{}/task", self.pid())).unwrap() {
            let Ok(status) = std::fs::read_to_string(task.unwrap().path().join("status")) else {
                continue;
            };
            for line in status.lines() {
                if let Some(v) = line
                    .strip_prefix("voluntary_ctxt_switches:")
                    .or_else(|| line.strip_prefix("nonvoluntary_ctxt_switches:"))
                {
                    total += v.trim().parse::<u64>().unwrap();
                }
            }
        }
        total
    }

    /// Waits until the log has `needle` in it at least `times` times.
    pub fn log_has(&self, needle: &str, times: usize) -> bool {
        let end = Instant::now() + LIMIT;
        loop {
            if count(self.log().as_bytes(), needle.as_bytes()) >= times {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// What `kykod` has written to stderr so far.
    pub fn log(&self) -> String {
        self.log.lock().unwrap().clone()
    }

    /// `kykod`'s direct children (its shells) as (pid, command line with spaces).
    pub fn children(&self) -> Vec<(i32, String)> {
        let found: Vec<(i32, String)> = processes()
            .into_iter()
            .filter(|p| p.ppid == self.pid())
            .map(|p| (p.pid, p.cmdline))
            .collect();
        let mut seen = self.seen.lock().unwrap();
        for (pid, _) in &found {
            if !seen.contains(pid) {
                seen.push(*pid);
            }
        }
        found
    }

    /// Sends `signal` to `kykod`, after noting its shells for cleanup.
    pub fn signal(&self, signal: Signal) {
        self.children();
        kill(Pid::from_raw(self.pid()), signal).unwrap();
    }

    /// Waits up to `limit` for `kykod` to exit by itself and returns its exit
    /// code (`None` if it was killed by a signal or is still running at
    /// `limit`). It is not killed here; dropping the holder does that.
    pub fn wait_exit(&mut self, limit: Duration) -> Option<i32> {
        self.children();
        let end = Instant::now() + limit;
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                return s.code();
            }
            if Instant::now() >= end {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The pid of the one child whose command line is exactly `cmdline`,
    /// waiting for it to appear.
    pub fn child_running(&self, cmdline: &str) -> i32 {
        let end = Instant::now() + LIMIT;
        loop {
            let found: Vec<i32> = self
                .children()
                .into_iter()
                .filter(|(_, c)| c == cmdline)
                .map(|(p, _)| p)
                .collect();
            match found.as_slice() {
                [pid] => return *pid,
                [] if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
                _ => panic!("expected one child {cmdline:?}, found {found:?}"),
            }
        }
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        // Every shell is a session leader: end its whole session, let kykod reap
        // it, then end kykod.
        self.children();
        let sessions = self.seen.lock().unwrap().clone();
        kill_sessions(&sessions);
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end && sessions.iter().any(|p| exists(*p)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        kill_sessions(&sessions);
    }
}

/// A `Spawn` for `shell` with `args` at 80x24, in the home folder, default ring.
pub fn spec(pane: PaneId, shell: &str, args: &[&str]) -> SpawnSpec {
    SpawnSpec {
        pane,
        shell: shell.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        cwd: String::new(),
        env: vec![],
        cols: 80,
        rows: 24,
        ring_bytes: 0,
    }
}

pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    count(hay, needle) > 0
}

pub fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Waits on a child with a timeout, killing it if it overstays.
pub fn wait_exit(child: &mut Child, limit: Duration) -> Option<i32> {
    let end = Instant::now() + limit;
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code();
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
