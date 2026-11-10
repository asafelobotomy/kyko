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

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// What `kykod` has written to stderr so far.
    pub fn log(&self) -> String {
        self.log.lock().unwrap().clone()
    }

    /// `kykod`'s direct children (its shells) as (pid, command line with spaces).
    pub fn children(&self) -> Vec<(i32, String)> {
        processes()
            .into_iter()
            .filter(|p| p.ppid == self.pid())
            .map(|p| (p.pid, p.cmdline))
            .collect()
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
        let sessions: Vec<i32> = self.children().into_iter().map(|(p, _)| p).collect();
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

pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub session: i32,
    pub state: char,
    pub cmdline: String,
}

/// Every process this user can see, read from `/proc`.
pub fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // The fields after the command name, which is in parentheses and may hold spaces.
        let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
            continue;
        };
        let f: Vec<&str> = rest.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|b| {
                String::from_utf8_lossy(&b)
                    .trim_end_matches('\0')
                    .replace('\0', " ")
            })
            .unwrap_or_default();
        out.push(Proc {
            pid,
            state: f[0].chars().next().unwrap_or('?'),
            ppid: f[1].parse().unwrap_or(0),
            session: f[3].parse().unwrap_or(0),
            cmdline,
        });
    }
    out
}

/// SIGKILL to every process in any of these sessions.
pub fn kill_sessions(sessions: &[i32]) {
    if sessions.is_empty() {
        return;
    }
    for &s in sessions {
        let _ = killpg(Pid::from_raw(s), Signal::SIGKILL);
    }
    for p in processes() {
        if sessions.contains(&p.session) {
            let _ = kill(Pid::from_raw(p.pid), Signal::SIGKILL);
        }
    }
}

/// True while the process exists, zombie included.
pub fn exists(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Waits until `pid` no longer exists, not even as a zombie.
pub fn gone_within(pid: i32, limit: Duration) -> bool {
    let end = Instant::now() + limit;
    while exists(pid) {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
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

/// One connection to a `kykod`, speaking the protocol as the window does.
pub struct Client {
    stream: UnixStream,
    decoder: Decoder,
    eof: bool,
}

impl Client {
    pub fn connect(h: &Holder) -> Client {
        Client::connect_path(h.socket())
    }

    pub fn connect_path(path: &Path) -> Client {
        let stream = UnixStream::connect(path).unwrap();
        // A kykod that stops reading fails the test instead of hanging it.
        stream.set_write_timeout(Some(LIMIT)).unwrap();
        Client {
            stream,
            decoder: Decoder::new(),
            eof: false,
        }
    }

    pub fn send(&mut self, frame: ToHolder) {
        let mut out = Vec::new();
        frame.encode(&mut out);
        self.send_raw(&out);
    }

    pub fn send_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).unwrap();
    }

    /// The next frame, or `None` on timeout or end of stream.
    pub fn recv(&mut self, timeout: Duration) -> Option<ToWindow> {
        let end = Instant::now() + timeout;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            if let Some(f) = self
                .decoder
                .next::<ToWindow>()
                .expect("kykod sent a bad frame")
            {
                return Some(f);
            }
            if self.eof {
                return None;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            self.stream.set_read_timeout(Some(left)).unwrap();
            match self.stream.read(&mut buf) {
                Ok(0) => self.eof = true,
                Ok(n) => self.decoder.push(&buf[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return None;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                // A reset is how an abruptly closed connection can end.
                Err(_) => self.eof = true,
            }
        }
    }

    /// The first frame that satisfies `pred`, skipping the others.
    pub fn wait_for(
        &mut self,
        pred: impl Fn(&ToWindow) -> bool,
        timeout: Duration,
    ) -> Option<ToWindow> {
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let f = self.recv(left)?;
            if pred(&f) {
                return Some(f);
            }
        }
    }

    /// Sends `Hello` of this protocol version and returns the reply.
    pub fn hello(&mut self) -> (u32, Vec<PaneInfo>, Vec<u8>) {
        self.send(ToHolder::Hello { version: VERSION });
        match self.wait_for(|f| matches!(f, ToWindow::Hello { .. }), LIMIT) {
            Some(ToWindow::Hello {
                version,
                panes,
                layout,
            }) => (version, panes, layout),
            _ => panic!("no Hello came back"),
        }
    }

    /// Collects `pane`'s output until it contains `needle`; panics on timeout.
    pub fn output_until(&mut self, pane: PaneId, needle: &[u8], timeout: Duration) -> Vec<u8> {
        self.output_until_n(pane, needle, 1, timeout)
    }

    /// Collects `pane`'s output until `needle` has appeared `times` times.
    pub fn output_until_n(
        &mut self,
        pane: PaneId,
        needle: &[u8],
        times: usize,
        timeout: Duration,
    ) -> Vec<u8> {
        let end = Instant::now() + timeout;
        let mut got = Vec::new();
        let mut seen = 0;
        while seen < times {
            let left = end.saturating_duration_since(Instant::now());
            match self.recv(left) {
                Some(ToWindow::Output { pane: p, bytes }) if p == pane => {
                    assert!(bytes.len() <= 64 * 1024, "Output of {} bytes", bytes.len());
                    // Count only matches that end in the new bytes.
                    let from = got.len().saturating_sub(needle.len() - 1);
                    got.extend(bytes);
                    seen += count(&got[from..], needle);
                }
                Some(_) => {}
                None => panic!(
                    "waiting for {:?} from pane {pane}: got {:?}",
                    String::from_utf8_lossy(needle),
                    String::from_utf8_lossy(&got)
                ),
            }
        }
        got
    }

    /// True if the connection reaches its end within `timeout`, whatever arrives first.
    pub fn closed_within(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        while !self.eof {
            if Instant::now() >= end {
                return false;
            }
            let left = end.saturating_duration_since(Instant::now());
            self.recv(left);
        }
        true
    }
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
