//! The basics: hello, the socket's folder, spawning, input, resizing, exits and closing, and bad frames.

use super::*;

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
