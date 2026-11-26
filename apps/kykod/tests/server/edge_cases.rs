//! Edge cases: unknown panes, an existing folder, many kykods at once and the default socket.

use super::*;

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
    // Bound and closed: a stale socket every one of them would like to replace.
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
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
