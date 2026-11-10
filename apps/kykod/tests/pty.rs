use std::io::Read;
use std::ops::Deref;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use kykod::pty::Pty;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

const LIMIT: Duration = Duration::from_secs(5);

/// Owns a pty for one test and makes sure its shell is gone when the test ends,
/// whether or not an assertion failed.
struct Guard(Pty);

impl Deref for Guard {
    type Target = Pty;
    fn deref(&self) -> &Pty {
        &self.0
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.hangup();
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            if self.0.try_wait().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = killpg(Pid::from_raw(self.0.pid()), Signal::SIGKILL);
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end && self.0.try_wait().is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Everything read from a pty so far, fed by a thread so that every wait has a timeout.
struct Out {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    done: bool,
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    count(hay, needle) > 0
}

impl Out {
    fn new(p: &Pty) -> Out {
        let mut f = p.reader().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match f.read(&mut chunk) {
                    // EIO is how a Linux PTY master says the child side is gone.
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(chunk[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Out {
            rx,
            buf: Vec::new(),
            done: false,
        }
    }

    /// Reads until `needle` has appeared `times` times in all the output.
    fn until_n(&mut self, needle: &[u8], times: usize) -> &[u8] {
        let end = Instant::now() + LIMIT;
        while count(&self.buf, needle) < times {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(c) => self.buf.extend(c),
                Err(RecvTimeoutError::Timeout) => panic!(
                    "timed out waiting for {:?}; got {:?}",
                    String::from_utf8_lossy(needle),
                    String::from_utf8_lossy(&self.buf)
                ),
                Err(RecvTimeoutError::Disconnected) => panic!(
                    "output ended waiting for {:?}; got {:?}",
                    String::from_utf8_lossy(needle),
                    String::from_utf8_lossy(&self.buf)
                ),
            }
        }
        &self.buf
    }

    fn until(&mut self, needle: &[u8]) -> &[u8] {
        self.until_n(needle, 1)
    }

    /// Reads until the shell side closes.
    fn all(&mut self) -> &[u8] {
        let end = Instant::now() + LIMIT;
        while !self.done {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(c) => self.buf.extend(c),
                Err(RecvTimeoutError::Disconnected) => self.done = true,
                Err(RecvTimeoutError::Timeout) => panic!(
                    "timed out waiting for the end of output; got {:?}",
                    String::from_utf8_lossy(&self.buf)
                ),
            }
        }
        &self.buf
    }
}

fn wait(p: &Pty) -> Option<i32> {
    let end = Instant::now() + LIMIT;
    while Instant::now() < end {
        if let Some(s) = p.try_wait() {
            return Some(s);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn spawn_in(cmd: &str, cwd: &str, env: &[(&str, &str)]) -> Guard {
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Guard(Pty::spawn("/bin/sh", &strings(&["-c", cmd]), cwd, &env, 80, 24).unwrap())
}

fn sh(cmd: &str) -> Guard {
    spawn_in(cmd, "", &[])
}

/// An interactive bash with job control and a recognisable prompt.
fn interactive(cwd: &str) -> (Guard, Out) {
    let env = vec![("PS1".to_string(), "KYKO$ ".to_string())];
    let args = strings(&["--norc", "--noprofile", "-i"]);
    let p = Guard(Pty::spawn("/bin/bash", &args, cwd, &env, 80, 24).unwrap());
    let mut out = Out::new(&p);
    out.until(b"KYKO$ ");
    (p, out)
}

/// True while `pid` runs. A zombie has ended, so it does not count: the shell's
/// children are orphaned when it ends, and where PID 1 does not reap orphans
/// (a CI job's container) an ended one stays a zombie for good.
fn alive(pid: i32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next());
    !matches!(state, None | Some("Z"))
}

#[test]
fn runs_a_command_and_reports_its_status() {
    let p = sh("printf hello; exit 7");
    let mut out = Out::new(&p);
    assert!(contains(out.all(), b"hello"));
    assert_eq!(wait(&p), Some(7));
}

#[test]
fn exit_status_is_kept_and_the_child_is_reaped() {
    let p = sh("exit 3");
    assert_eq!(wait(&p), Some(3));
    assert_eq!(p.try_wait(), Some(3));
    assert_eq!(p.try_wait(), Some(3));
    assert!(
        !std::path::Path::new(&format!("/proc/{}", p.pid())).exists(),
        "the child is still there as a zombie"
    );
}

#[test]
fn environment_is_set() {
    let p = spawn_in("echo $TERM/$COLORTERM/$EXTRA", "", &[("EXTRA", "yes")]);
    let mut out = Out::new(&p);
    assert!(contains(out.all(), b"xterm-256color/truecolor/yes"));
}

#[test]
fn starts_in_the_requested_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap();
    let p = spawn_in("pwd", path.to_str().unwrap(), &[]);
    let mut out = Out::new(&p);
    assert!(contains(out.all(), path.to_str().unwrap().as_bytes()));
}

#[test]
fn size_is_what_was_asked_then_what_was_resized() {
    let p = sh("stty size; read x; stty size");
    let mut out = Out::new(&p);
    out.until(b"24 80");
    p.resize(100, 30);
    p.write_all(b"\n").unwrap();
    out.until(b"30 100");
    assert_eq!(wait(&p), Some(0));
}

#[test]
fn input_reaches_the_program() {
    let p = sh("cat");
    let mut out = Out::new(&p);
    p.write_all(b"ping\n").unwrap();
    // Once the terminal's echo, once what cat printed back.
    out.until_n(b"ping", 2);
}

#[test]
fn missing_shell_is_a_spawn_error() {
    let e = Pty::spawn("/bin/nope", &[], "", &[], 80, 24).err().unwrap();
    assert!(e.0.contains("/bin/nope"), "{}", e.0);
    assert!(e.0.contains("No such file or directory"), "{}", e.0);
}

#[test]
fn missing_folder_is_a_spawn_error() {
    let e = Pty::spawn("/bin/sh", &[], "/no/such/dir", &[], 80, 24)
        .err()
        .unwrap();
    assert!(e.0.contains("/no/such/dir"), "{}", e.0);
    assert!(e.0.contains("folder does not exist"), "{}", e.0);
}

#[test]
fn foreground_program_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap();
    let (p, mut out) = interactive(path.to_str().unwrap());
    assert!(!p.is_busy());
    p.write_all(b"sleep 31\n").unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    let mut seen = None;
    while Instant::now() < end {
        if let Some((pgid, name)) = p.foreground()
            && name == "sleep"
        {
            seen = Some(pgid);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let pgid = seen.expect("the foreground program never became sleep");
    assert_ne!(pgid, p.pid());
    assert!(p.is_busy());
    assert_eq!(p.cwd().as_deref(), path.to_str());
    out.buf.clear();
}

#[test]
fn idle_shell_is_not_busy() {
    let (p, _out) = interactive("/");
    assert!(!p.is_busy());
    let (pgid, name) = p.foreground().unwrap();
    assert_eq!(pgid, p.pid());
    assert_eq!(name, "bash");
}

#[test]
fn hangup_ends_the_shell_and_its_children() {
    let (p, mut out) = interactive("/");
    p.write_all(b"sleep 300\n").unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    let mut sleep_pid = None;
    while Instant::now() < end {
        if let Some((pgid, name)) = p.foreground()
            && name == "sleep"
        {
            sleep_pid = Some(pgid);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let sleep_pid = sleep_pid.expect("sleep never became the foreground program");
    p.hangup();
    let end = Instant::now() + Duration::from_secs(2);
    let mut status = None;
    while Instant::now() < end && status.is_none() {
        status = p.try_wait();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(status.is_some(), "the shell did not exit after hangup");
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end && alive(sleep_pid) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(sleep_pid), "the sleep outlived its shell");
    out.buf.clear();
}

#[test]
fn killed_by_signal_reports_128_plus_signal() {
    let p = sh("kill -9 $$");
    assert_eq!(wait(&p), Some(137));
}

#[test]
fn a_shell_does_not_inherit_another_shells_terminal() {
    let first = sh("cat");
    let second = sh("for f in /proc/$$/fd/*; do readlink $f; done");
    let mut out = Out::new(&second);
    let text = String::from_utf8_lossy(out.all()).into_owned();
    assert!(!text.contains("/dev/ptmx"), "master leaked: {text}");
    let mut pts: Vec<&str> = text.lines().filter(|l| l.contains("/dev/pts/")).collect();
    pts.dedup();
    assert_eq!(pts.len(), 1, "expected only its own terminal: {text}");
    drop(first);
}

#[test]
fn wait_blocks_until_the_shell_ends_and_reaps_it() {
    let p = sh("sleep 0.2; exit 4");
    assert_eq!(p.wait(), Some(4));
    assert_eq!(p.try_wait(), Some(4));
    assert!(
        !std::path::Path::new(&format!("/proc/{}", p.pid())).exists(),
        "the child is still there as a zombie"
    );
}

#[test]
fn dropping_a_pty_ends_and_reaps_its_shell() {
    let p = Pty::spawn("sleep", &strings(&["300"]), "", &[], 80, 24).unwrap();
    let pid = p.pid();
    let started = Instant::now();
    drop(p);
    assert!(started.elapsed() < Duration::from_secs(1), "drop blocked");
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "the shell is still there after drop"
    );
}

#[test]
fn dropping_a_pty_ends_a_shell_that_ignores_hangup() {
    let p = Pty::spawn(
        "/bin/sh",
        &strings(&["-c", "trap '' HUP; printf ready; sleep 300"]),
        "",
        &[],
        80,
        24,
    )
    .unwrap();
    let mut out = Out::new(&p);
    out.until(b"ready");
    let pid = p.pid();
    let started = Instant::now();
    drop(p);
    assert!(started.elapsed() < Duration::from_secs(1), "drop blocked");
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end && std::path::Path::new(&format!("/proc/{pid}")).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "the shell outlived its pty"
    );
}

#[test]
fn hangup_after_the_shell_was_reaped_signals_nothing() {
    let p = sh("exit 0");
    assert_eq!(wait(&p), Some(0));
    // The pid may be recycled by now; hangup must not send anything to it.
    p.hangup();
    assert_eq!(p.try_wait(), Some(0));
}
