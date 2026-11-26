//! Limits and failures: frames out of order, the socket and its lock, stuck readers and busy panes.

use super::*;

#[test]
fn frames_before_hello_close_the_connection() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.send(ToHolder::Spawn(spec(1, "sleep", &["300"])));
    assert!(c.closed_within(LIMIT), "the connection stayed open");
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert!(panes.is_empty(), "{panes:?}");
    assert!(h.children().is_empty());
}

#[test]
fn wrong_version_gets_hello_then_close() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.send(ToHolder::Hello { version: 999 });
    match c.recv(LIMIT) {
        Some(ToWindow::Hello { version, .. }) => assert_eq!(version, VERSION),
        other => panic!("expected Hello, got {other:?}"),
    }
    assert!(c.closed_within(LIMIT), "the connection stayed open");
}

#[test]
fn a_file_that_is_not_a_socket_is_refused_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hold.sock");
    std::fs::write(&path, b"not a socket").unwrap();
    let mut child = bin()
        .arg("--socket")
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait_exit(&mut child, LIMIT), Some(1));
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert!(err.contains("is not a socket"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), b"not a socket");
}

#[test]
fn a_busy_pane_sends_output_in_few_frames() {
    // A program that writes a line at a time is read a few lines at a time.
    // A frame per read would mean a socket write and a window wake-up per
    // ~100 bytes (spec 6.2 item 6), so what is already waiting is read into
    // the same frame.
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "seq 1 300000; printf END; sleep 30"],
    )));
    let end = std::time::Instant::now() + Duration::from_secs(20);
    let (mut got, mut frames) = (Vec::new(), 0);
    while !got.ends_with(b"END") {
        let left = end.saturating_duration_since(std::time::Instant::now());
        match c.recv(left) {
            Some(ToWindow::Output { pane: 1, bytes }) => {
                frames += 1;
                got.extend(bytes);
            }
            Some(_) => {}
            None => panic!("no END after {} bytes", got.len()),
        }
    }
    let average = got.len() / frames;
    assert!(
        average >= 1024,
        "{frames} Output frames averaging {average} bytes"
    );
}

#[test]
fn a_held_lock_makes_kykod_exit_1_and_leave_the_stale_file() {
    use nix::fcntl::{Flock, FlockArg};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hold.sock");
    std::fs::write(&path, b"stale").unwrap();
    // This test plays the kykod that holds the lock.
    let file = std::fs::File::create(dir.path().join("hold.sock.lock")).unwrap();
    let _lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).unwrap();
    let mut child = bin()
        .arg("--socket")
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait_exit(&mut child, LIMIT), Some(1));
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert!(err.contains("another kykod is serving"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), b"stale");
}

#[test]
fn a_lock_file_that_is_a_symbolic_link_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hold.sock");
    let target = dir.path().join("elsewhere");
    std::os::unix::fs::symlink(&target, dir.path().join("hold.sock.lock")).unwrap();
    let mut child = bin()
        .arg("--socket")
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait_exit(&mut child, LIMIT), Some(1));
    assert!(!target.exists(), "kykod created the link's target");
    assert!(!path.exists(), "kykod made the socket");
}

#[test]
fn a_socket_path_too_long_to_bind_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("x".repeat(120));
    let path = folder.join("hold.sock");
    let mut child = bin()
        .arg("--socket")
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait_exit(&mut child, LIMIT), Some(1));
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert!(err.contains("a socket path must be at most 107"), "{err}");
    assert!(!folder.exists(), "kykod created the socket's folder");
}

#[test]
fn a_mebibyte_of_input_arrives_complete() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "stty -echo; printf go; wc -c; sleep 30"],
    )));
    c.output_until(1, b"go", LIMIT);
    // Lines well under the terminal's 4095-byte line limit: 1024 lines of 1024 bytes.
    let mut line = vec![b'a'; 1023];
    line.push(b'\n');
    let all = line.repeat(1024);
    for chunk in all.chunks(64 * 1024) {
        c.send(ToHolder::Input {
            pane: 1,
            bytes: chunk.to_vec(),
        });
    }
    // End of file for wc, at the start of a line.
    c.send(ToHolder::Input {
        pane: 1,
        bytes: vec![4],
    });
    c.output_until(1, b"1048576", Duration::from_secs(20));
}

#[test]
fn a_closed_stderr_does_not_stop_kykod() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("hold.sock");
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut cmd = bin();
    cmd.arg("--socket").arg(&socket);
    let mut h = Holder::launch(dir, socket, cmd, Some(Stdio::from(writer)));
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "/bin/sh", &["-c", "exit 2"])));
    let f = c.wait_for(|f| matches!(f, ToWindow::Exited { .. }), LIMIT);
    assert_eq!(f, Some(ToWindow::Exited { pane: 1, status: 2 }));
    drop(c);
    assert!(h.alive(), "kykod died writing its log");
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert_eq!(panes.len(), 1);
    assert!(h.alive());
}

#[test]
fn a_shell_exit_is_reported_once_after_its_last_output() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "printf last-words-once; exit 3"],
    )));
    let (out, status, exits) = output_and_exits(&mut c, 1, Duration::from_millis(500));
    assert!(contains(&out, b"last-words-once"), "{out:?}");
    assert_eq!((status, exits), (3, 1));
}

#[test]
fn a_shell_that_exits_while_a_background_job_holds_the_terminal_reports_exited() {
    // `firefox & exit`: the shell is gone but its job keeps the terminal open,
    // so the terminal never reports the end. The job ignores SIGHUP, as one
    // in a background process group would never get it, and keeps writing.
    let job = "while printf .; do sleep 0.2; done";
    let script = format!("trap '' HUP; sh -c '{job}' bg-job-3077 & printf before-exit; exit 7");
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let before = h.threads();
    let started = std::time::Instant::now();
    c.send(ToHolder::Spawn(spec(1, "/bin/sh", &["-c", &script])));
    let (out, status, exits) = output_and_exits(&mut c, 1, Duration::from_millis(300));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "Exited came {:?} after Spawn",
        started.elapsed()
    );
    assert!(contains(&out, b"before-exit"), "{out:?}");
    assert_eq!((status, exits), (7, 1));
    let cmdline = format!("sh -c {job} bg-job-3077");
    let jobs: Vec<i32> = common::processes()
        .into_iter()
        .filter(|p| p.cmdline == cmdline)
        .map(|p| p.pid)
        .collect();
    let [job_pid] = jobs[..] else {
        panic!("expected one background job, found {jobs:?}");
    };
    // Closing the pane closes the terminal, which hangs the job up: its next
    // write fails and it ends.
    c.send(ToHolder::Close { pane: 1 });
    let ended = gone_within(job_pid, Duration::from_secs(4));
    if !ended {
        common::kill_sessions(&[job_pid]);
    }
    assert!(ended, "the background job outlived Close");
    let end = std::time::Instant::now() + Duration::from_secs(4);
    while h.threads() != before && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(h.threads(), before, "a pane thread outlived Close");
}

#[test]
fn stale_unix_socket_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hold.sock");
    // Bound and closed: the file stays, but nothing listens on it.
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    let h = Holder::start_at(dir, path);
    let mut c = Client::connect(&h);
    assert_eq!(c.hello().0, VERSION);
}

#[test]
fn second_kykod_on_a_live_socket_exits_1() {
    let h = Holder::start();
    let mut second = bin()
        .arg("--socket")
        .arg(h.socket())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait_exit(&mut second, LIMIT), Some(1));
    let mut c = Client::connect(&h);
    assert_eq!(c.hello().0, VERSION);
}

#[test]
fn no_runtime_dir_and_no_socket_flag_exits_1() {
    let mut child = bin()
        .env_remove("XDG_RUNTIME_DIR")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(wait_exit(&mut child, LIMIT), Some(1));
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert!(err.contains("XDG_RUNTIME_DIR"), "{err}");
}

#[test]
fn unread_window_does_not_block_the_shell() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    // Far more output than the socket can buffer, while this window reads nothing.
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "yes | head -c 30000000; exit 5"],
    )));
    let pid = h.child_running("/bin/sh -c yes | head -c 30000000; exit 5");
    assert!(
        gone_within(pid, Duration::from_secs(20)),
        "the shell did not finish; log:\n{}",
        h.log()
    );
    // The stalled window was dropped rather than waited for.
    assert!(c.closed_within(LIMIT), "the stalled connection stayed open");
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert_eq!(panes[0].exited, Some(5));
}

#[test]
fn output_frames_for_a_busy_pane_stay_within_64_kib() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &[
            "-c",
            "head -c 3000000 /dev/zero | tr '\\0' x; printf END; sleep 30",
        ],
    )));
    // output_until asserts the size of every Output frame.
    let out = c.output_until(1, b"END", Duration::from_secs(20));
    assert!(count(&out, b"x") >= 3_000_000 - 1);
    assert!(contains(&out, b"END"));
}
