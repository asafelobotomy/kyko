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
