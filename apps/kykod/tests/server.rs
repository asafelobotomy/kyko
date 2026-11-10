mod common;

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::Stdio;
use std::time::Duration;

use common::{Client, Holder, LIMIT, bin, contains, count, exists, gone_within, spec, wait_exit};
use kyko_proto::{MAX_FRAME, ToHolder, ToWindow, VERSION};

#[test]
fn hello_reports_the_version_and_no_panes() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    let (v, panes, layout) = c.hello();
    assert_eq!((v, panes.len(), layout.len()), (VERSION, 0, 0));
}

#[test]
fn socket_and_folder_are_private() {
    let h = Holder::start();
    let sock = std::fs::metadata(h.socket()).unwrap();
    assert_eq!(sock.permissions().mode() & 0o777, 0o600);
    let dir = std::fs::metadata(h.socket().parent().unwrap()).unwrap();
    assert_eq!(dir.permissions().mode() & 0o777, 0o700);
}

#[test]
fn spawn_then_output_arrives() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "printf ready; sleep 30"],
    )));
    c.output_until(1, b"ready", LIMIT);
}

#[test]
fn input_is_delivered() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "cat", &[])));
    c.send(ToHolder::Input {
        pane: 1,
        bytes: b"ping\n".to_vec(),
    });
    // Once the terminal's echo, once what cat wrote back.
    c.output_until_n(1, b"ping", 2, LIMIT);
}

#[test]
fn resize_reaches_the_shell() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "/bin/sh", &[])));
    c.send(ToHolder::Resize {
        pane: 1,
        cols: 100,
        rows: 30,
    });
    c.send(ToHolder::Input {
        pane: 1,
        bytes: b"stty size\n".to_vec(),
    });
    c.output_until(1, b"30 100", LIMIT);
}

#[test]
fn exit_status_is_reported_and_pane_stays_listed() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "/bin/sh", &["-c", "exit 3"])));
    let f = c.wait_for(|f| matches!(f, ToWindow::Exited { .. }), LIMIT);
    assert_eq!(f, Some(ToWindow::Exited { pane: 1, status: 3 }));
    drop(c);
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert_eq!((panes[0].pane, panes[0].exited), (1, Some(3)));
}

#[test]
fn close_ends_the_shell_and_forgets_the_pane() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "sleep", &["300"])));
    let pid = h.child_running("sleep 300");
    c.send(ToHolder::Close { pane: 1 });
    assert!(
        gone_within(pid, Duration::from_secs(2)),
        "the shell is still there"
    );
    drop(c);
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert!(panes.is_empty(), "{panes:?}");
}

#[test]
fn missing_shell_gives_spawn_failed() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "/bin/nope", &[])));
    match c.wait_for(|f| matches!(f, ToWindow::SpawnFailed { .. }), LIMIT) {
        Some(ToWindow::SpawnFailed { pane, message }) => {
            assert_eq!(pane, 1);
            assert!(message.contains("/bin/nope"), "{message}");
        }
        other => panic!("expected SpawnFailed, got {other:?}"),
    }
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "printf fine; sleep 30"],
    )));
    c.output_until(1, b"fine", LIMIT);
}

#[test]
fn duplicate_pane_id_gives_spawn_failed() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "sleep", &["300"])));
    c.send(ToHolder::Spawn(spec(1, "sleep", &["300"])));
    let f = c.wait_for(|f| matches!(f, ToWindow::SpawnFailed { .. }), LIMIT);
    assert_eq!(
        f,
        Some(ToWindow::SpawnFailed {
            pane: 1,
            message: "pane 1 already exists".to_string()
        })
    );
    assert_eq!(h.children().len(), 1, "{:?}", h.children());
}

#[test]
fn too_many_panes_is_refused() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    for pane in 1..=257 {
        c.send(ToHolder::Spawn(spec(pane, "sleep", &["300"])));
    }
    let f = c.wait_for(
        |f| matches!(f, ToWindow::SpawnFailed { .. }),
        Duration::from_secs(20),
    );
    assert_eq!(
        f,
        Some(ToWindow::SpawnFailed {
            pane: 257,
            message: "too many panes".to_string()
        })
    );
    drop(c);
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert_eq!(panes.len(), 256);
    let pids: Vec<i32> = h.children().into_iter().map(|(p, _)| p).collect();
    assert_eq!(pids.len(), 256);
    drop(c);
    drop(h);
    for pid in pids {
        assert!(gone_within(pid, LIMIT), "shell {pid} outlived the test");
    }
}

#[test]
fn malformed_frame_closes_only_that_connection() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "sleep", &["300"])));
    let pid = h.child_running("sleep 300");
    // A frame of an unknown type, then one whose inner length overruns it.
    c.send_raw(&[5, 0, 0, 0, 200, 1, 2, 3, 4]);
    assert!(c.closed_within(LIMIT), "the connection stayed open");
    let mut c = Client::connect(&h);
    c.hello();
    c.send_raw(&[9, 0, 0, 0, 3, 1, 0, 0, 0, 255, 255, 0, 0]);
    assert!(c.closed_within(LIMIT), "the connection stayed open");
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].exited, None);
    assert!(exists(pid));
}

#[test]
fn oversized_frame_closes_the_connection() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send_raw(&((MAX_FRAME + 1) as u32).to_le_bytes());
    assert!(c.closed_within(LIMIT), "the connection stayed open");
    let mut c = Client::connect(&h);
    c.hello();
}

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
        Some(ToWindow::Hello { version, .. }) => assert_eq!(version, 1),
        other => panic!("expected Hello, got {other:?}"),
    }
    assert!(c.closed_within(LIMIT), "the connection stayed open");
}

#[test]
fn stale_socket_file_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hold.sock");
    std::fs::write(&path, b"not a socket").unwrap();
    let h = Holder::start_at(dir, path);
    let mut c = Client::connect(&h);
    assert_eq!(c.hello().0, VERSION);
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
fn socket_folder_is_created_private_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deeper").join("hold.sock");
    let h = Holder::start_at(dir, path);
    let mode = std::fs::metadata(h.socket().parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
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
fn unknown_pane_frames_are_ignored() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Input {
        pane: 9,
        bytes: b"x".to_vec(),
    });
    c.send(ToHolder::Resize {
        pane: 9,
        cols: 10,
        rows: 10,
    });
    c.send(ToHolder::Close { pane: 9 });
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "printf alive; sleep 30"],
    )));
    c.output_until(1, b"alive", LIMIT);
}

#[test]
fn existing_folder_mode_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("shared");
    std::fs::DirBuilder::new()
        .mode(0o755)
        .create(&folder)
        .unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let h = Holder::start_at(dir, folder.join("hold.sock"));
    let mode = std::fs::metadata(&folder).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    let sock = std::fs::metadata(h.socket()).unwrap().permissions().mode();
    assert_eq!(sock & 0o777, 0o600);
}

#[test]
fn close_kills_a_shell_that_ignores_hangup() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let before = h.threads();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "trap '' HUP; printf armed; exec sleep 300.25"],
    )));
    c.output_until(1, b"armed", LIMIT);
    let pid = h.child_running("sleep 300.25");
    c.send(ToHolder::Close { pane: 1 });
    assert!(
        gone_within(pid, Duration::from_secs(4)),
        "the shell outlived Close"
    );
    let end = std::time::Instant::now() + Duration::from_secs(4);
    while h.threads() != before && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(h.threads(), before, "a pane thread outlived Close");
}

#[test]
fn eight_kykods_at_once_leave_exactly_one_serving() {
    struct Kill(Vec<std::process::Child>);
    impl Drop for Kill {
        fn drop(&mut self) {
            for c in &mut self.0 {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hold.sock");
    std::fs::write(&path, b"stale").unwrap();
    let mut all = Kill(
        (0..8)
            .map(|_| {
                bin()
                    .arg("--socket")
                    .arg(&path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect(),
    );
    let end = std::time::Instant::now() + LIMIT;
    let mut codes = vec![None; 8];
    while std::time::Instant::now() < end && codes.iter().filter(|c| c.is_some()).count() < 7 {
        for (i, c) in all.0.iter_mut().enumerate() {
            if codes[i].is_none() {
                codes[i] = c.try_wait().unwrap().map(|s| s.code());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Give a wrongly surviving second server a moment to show itself.
    std::thread::sleep(Duration::from_millis(300));
    for (i, c) in all.0.iter_mut().enumerate() {
        if codes[i].is_none() {
            codes[i] = c.try_wait().unwrap().map(|s| s.code());
        }
    }
    let running = codes.iter().filter(|c| c.is_none()).count();
    let refused = codes.iter().filter(|c| **c == Some(Some(1))).count();
    assert_eq!((running, refused), (1, 7), "{codes:?}");
    let mut c = Client::connect_path(&path);
    assert_eq!(c.hello().0, VERSION);
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
fn input_to_a_program_that_never_reads_does_not_block_close() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "sleep", &["300.5"])));
    let pid = h.child_running("sleep 300.5");
    let line = b"line\n";
    for _ in 0..10 {
        c.send(ToHolder::Input {
            pane: 1,
            bytes: line.repeat(10_000),
        });
    }
    c.send(ToHolder::Close { pane: 1 });
    assert!(
        gone_within(pid, Duration::from_secs(4)),
        "Close was not handled"
    );
    drop(c);
    let mut c = Client::connect(&h);
    let (_, panes, _) = c.hello();
    assert!(panes.is_empty(), "{panes:?}");
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
fn input_beyond_the_queue_bound_is_dropped_until_the_queue_empties() {
    let dir = tempfile::tempdir().unwrap();
    let go = dir.path().join("go");
    // Reads nothing until `go` exists, then exactly the 4,000,000 bytes that
    // fit, then whatever else came (for a second; `--foreground` keeps `cat`
    // in the terminal's foreground group, so it may read), then one line.
    let script = format!(
        "stty raw -echo; printf ready; while [ ! -e {} ]; do sleep 0.05; done; \
         head -c 4000000 >/dev/null; n=$(timeout --foreground 1 cat | wc -c); printf 'extra=%s;' $n; \
         read -r line; printf 'line=%s;' \"$line\"; sleep 30",
        go.display()
    );
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "/bin/sh", &["-c", &script])));
    c.output_until(1, b"ready", LIMIT);
    for _ in 0..4 {
        c.send(ToHolder::Input {
            pane: 1,
            bytes: vec![b'x'; 1_000_000],
        });
    }
    // Past the bound: dropped. So is anything after it, however small, while
    // the queue still holds input: a paste is cut short, never left with a
    // hole in it.
    c.send(ToHolder::Input {
        pane: 1,
        bytes: vec![b'x'; 1_000_000],
    });
    c.send(ToHolder::Input {
        pane: 1,
        bytes: b"yy".to_vec(),
    });
    // Frames are handled in order: once this pane talks, the ones above have
    // been handled too.
    c.send(ToHolder::Spawn(spec(
        2,
        "/bin/sh",
        &["-c", "printf after; sleep 30"],
    )));
    c.output_until(2, b"after", LIMIT);
    std::fs::write(&go, b"").unwrap();
    let out = c.output_until(1, b";", Duration::from_secs(20));
    assert!(
        contains(&out, b"extra=0;"),
        "{}",
        String::from_utf8_lossy(&out)
    );
    // Once the queue has emptied, input is taken again.
    c.send(ToHolder::Input {
        pane: 1,
        bytes: b"taken\n".to_vec(),
    });
    c.output_until(1, b"line=taken;", LIMIT);
    let log = h.log();
    assert_eq!(count(log.as_bytes(), b"input queue full"), 1, "{log}");
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
fn default_socket_is_under_xdg_runtime_dir() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("kyko").join("hold.sock");
    let mut cmd = bin();
    cmd.env("XDG_RUNTIME_DIR", dir.path());
    let h = Holder::launch(dir, socket.clone(), cmd, None);
    let mut c = Client::connect_path(&socket);
    assert_eq!(c.hello().0, VERSION);
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(socket.parent().unwrap()), 0o700);
    assert_eq!(mode(&socket), 0o600);
    drop(h);
}

/// Collects `pane`'s output up to its `Exited`, and every frame for it in
/// `after` more. Returns the output, the exit status and how many `Exited`
/// came in all.
fn output_and_exits(c: &mut Client, pane: u32, after: Duration) -> (Vec<u8>, i32, usize) {
    let end = std::time::Instant::now() + LIMIT;
    let mut out = Vec::new();
    let status = loop {
        let left = end.saturating_duration_since(std::time::Instant::now());
        match c.recv(left) {
            Some(ToWindow::Output { pane: p, bytes }) if p == pane => out.extend(bytes),
            Some(ToWindow::Exited { pane: p, status }) if p == pane => break status,
            Some(_) => {}
            None => panic!(
                "no Exited for pane {pane}; output {:?}",
                String::from_utf8_lossy(&out)
            ),
        }
    };
    let mut exits = 1;
    let end = std::time::Instant::now() + after;
    while let Some(f) = c.recv(end.saturating_duration_since(std::time::Instant::now())) {
        if matches!(f, ToWindow::Exited { pane: p, .. } if p == pane) {
            exits += 1;
        }
    }
    (out, status, exits)
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
