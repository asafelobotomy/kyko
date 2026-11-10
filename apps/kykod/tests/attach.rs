mod common;

use std::time::{Duration, Instant};

use common::{Client, Holder, LIMIT, contains, cpu_ticks, exists, spec};
use kyko_proto::{PaneId, SpawnSpec, ToHolder, ToWindow};

/// A window that spawns `s` and leaves at once, so the pane runs with no
/// window from its first byte.
fn spawn_and_leave(h: &Holder, s: SpawnSpec) {
    let mut a = Client::connect(h);
    a.hello();
    a.send(ToHolder::Spawn(s));
}

/// Every `Output` for `pane` that arrives within `period`.
fn collect_for(c: &mut Client, pane: PaneId, period: Duration) -> Vec<u8> {
    let end = Instant::now() + period;
    let mut got = Vec::new();
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return got;
        }
        if let Some(ToWindow::Output { pane: p, bytes }) = c.recv(left)
            && p == pane
        {
            got.extend(bytes);
        }
    }
}

/// The numbers in `bytes`, one per line (`<prefix><n>\r\n`). A first line that
/// may have been cut by the ring (`cut`) and an unfinished last line are left
/// out; any other line that is not a number fails the test.
fn numbers(bytes: &[u8], prefix: &str, cut: bool) -> Vec<u64> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines: Vec<&str> = text.split("\r\n").collect();
    lines.pop(); // after the last "\r\n": empty or unfinished
    if cut && !lines.is_empty() {
        lines.remove(0);
    }
    lines
        .iter()
        .map(|l| {
            l.strip_prefix(prefix)
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("not a counter line: {l:?}"))
        })
        .collect()
}

#[test]
fn output_before_any_window_is_replayed_on_attach() {
    let h = Holder::start();
    spawn_and_leave(
        &h,
        spec(
            1,
            "/bin/sh",
            &["-c", "echo one; sleep 1; echo two; sleep 300.1"],
        ),
    );
    std::thread::sleep(Duration::from_secs(2));
    let mut b = Client::connect(&h);
    let (_, panes, _) = b.hello();
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert_eq!((panes[0].pane, panes[0].exited), (1, None));
    // A pane from before this connection sends nothing until Attach.
    assert_eq!(b.recv(Duration::from_millis(300)), None);
    let (bytes, truncated) = b.attach(1);
    let text = String::from_utf8_lossy(&bytes);
    let (one, two) = (text.find("one"), text.find("two"));
    assert!(
        one.is_some() && two.is_some() && one < two,
        "replay: {text:?}"
    );
    assert!(!truncated);
}

#[test]
fn attach_to_an_ended_pane_replays_it_then_reports_the_exit() {
    let h = Holder::start();
    spawn_and_leave(&h, spec(1, "/bin/sh", &["-c", "echo last-words; exit 4"]));
    assert!(h.log_has("pane 1 ended with status 4", 1), "{}", h.log());
    let mut b = Client::connect(&h);
    b.hello();
    let (bytes, truncated) = b.attach(1);
    assert!(contains(&bytes, b"last-words"));
    assert!(!truncated);
    // Also what a window needs when the shell ended between Hello and Attach.
    assert_eq!(b.recv(LIMIT), Some(ToWindow::Exited { pane: 1, status: 4 }));
}

#[test]
fn live_output_follows_the_replay_without_loss() {
    let h = Holder::start();
    // As fast as the shell can count: a byte lost or sent twice between the
    // replay and the live stream shows as a gap or a repeat.
    spawn_and_leave(
        &h,
        spec(
            1,
            "/bin/sh",
            &["-c", "i=0; while :; do echo L$i; i=$((i+1)); done"],
        ),
    );
    let mut b = Client::connect(&h);
    b.hello();
    // The counter is running: had the pane been live already, Output would come.
    assert_eq!(b.recv(Duration::from_millis(400)), None);
    let (replay, truncated) = b.attach(1);
    let live = collect_for(&mut b, 1, Duration::from_secs(1));
    let before = numbers(&replay, "L", truncated);
    assert!(
        before.len() >= 100,
        "only {} numbers replayed: the attach was not mid-stream",
        before.len()
    );
    let mut all = replay.clone();
    all.extend(&live);
    let seen = numbers(&all, "L", truncated);
    assert!(
        seen.len() >= before.len() + 100,
        "only {} numbers arrived live",
        seen.len() - before.len()
    );
    if !truncated {
        assert_eq!(seen[0], 0);
    }
    for (i, pair) in seen.windows(2).enumerate() {
        assert_eq!(
            pair[1],
            pair[0] + 1,
            "after L{} came L{} ({} of them replayed, the break is at {i})",
            pair[0],
            pair[1],
            before.len()
        );
    }
}

#[test]
fn ring_wrap_sets_truncated_and_bounds_the_replay() {
    let h = Holder::start();
    let mut s = spec(1, "/bin/sh", &["-c", "timeout 1 yes ringwrap; sleep 300.3"]);
    s.ring_bytes = 262_144;
    spawn_and_leave(&h, s);
    std::thread::sleep(Duration::from_millis(1500));
    let mut b = Client::connect(&h);
    b.hello();
    let (bytes, truncated) = b.attach(1);
    assert_eq!(bytes.len(), 262_144);
    assert!(truncated);
    assert!(contains(&bytes, b"ringwrap\r\n"));
}

#[test]
fn writer_with_no_window_is_never_blocked_and_memory_is_bounded() {
    let h = Holder::start();
    let mut s = spec(1, "yes", &["neverblocked"]);
    s.ring_bytes = 262_144;
    spawn_and_leave(&h, s);
    let pid = h.child_running("yes neverblocked");
    let mut cpu = cpu_ticks(pid);
    for second in 1..=3 {
        std::thread::sleep(Duration::from_secs(1));
        let now = cpu_ticks(pid);
        assert!(
            now > cpu,
            "yes used no CPU in second {second}: blocked on a full terminal"
        );
        cpu = now;
        let rss = h.rss_anon();
        assert!(
            rss < 32 * 1024 * 1024,
            "kykod's RssAnon is {rss} bytes after {second} s"
        );
    }
}

#[test]
fn window_killed_mid_output_leaves_shells_running() {
    let h = Holder::start();
    let script = "i=0; while :; do echo k$i; i=$((i+1)); sleep 0.01; done";
    let mut a = Client::connect(&h);
    a.hello();
    a.send(ToHolder::Spawn(spec(1, "/bin/sh", &["-c", script])));
    let pid = h.child_running(&format!("/bin/sh -c {script}"));
    let got = a.output_until(1, b"k20\r\n", LIMIT);
    let last = *numbers(&got, "k", false).last().unwrap();
    // Gone without a word, in the middle of the output.
    drop(a);
    std::thread::sleep(Duration::from_secs(1));
    assert!(exists(pid), "the shell died with its window");
    let mut b = Client::connect(&h);
    b.hello();
    let (replay, truncated) = b.attach(1);
    let replayed = numbers(&replay, "k", truncated);
    let newest = *replayed.last().unwrap();
    assert!(
        newest > last + 10,
        "the replay ends at k{newest}; the first window saw k{last}"
    );
    // And it is live for the new window.
    b.output_until(1, format!("k{}\r\n", newest + 3).as_bytes(), LIMIT);
}

#[test]
fn second_window_is_refused_while_one_is_live() {
    let h = Holder::start();
    let mut a = Client::connect(&h);
    a.hello();
    let mut b = Client::connect(&h);
    assert_eq!(b.hello_once(), None);
    assert!(
        b.ended() && b.frames() == 0,
        "the second window got a reply"
    );
    assert!(h.log_has("refused", 1), "{}", h.log());
    // The first window still works.
    a.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "printf still-here; sleep 300.6"],
    )));
    a.output_until(1, b"still-here", LIMIT);
}

#[test]
fn second_window_is_accepted_after_the_first_leaves() {
    let h = Holder::start();
    let mut a = Client::connect(&h);
    a.hello();
    drop(a);
    assert!(h.log_has("disconnected", 1), "{}", h.log());
    // Once kykod has seen the first window go, the next one is let in at once.
    let mut b = Client::connect(&h);
    assert!(b.hello_once().is_some(), "{}", h.log());
    b.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "printf welcome; sleep 300.7"],
    )));
    b.output_until(1, b"welcome", LIMIT);
}

#[test]
fn layout_is_stored_and_returned() {
    let h = Holder::start();
    let mut a = Client::connect(&h);
    assert_eq!(a.hello().2, b"");
    a.send(ToHolder::SaveLayout {
        blob: b"layout-1".to_vec(),
    });
    drop(a);
    let mut b = Client::connect(&h);
    assert_eq!(b.hello().2, b"layout-1");
    b.send(ToHolder::SaveLayout {
        blob: b"layout-2".to_vec(),
    });
    drop(b);
    let mut c = Client::connect(&h);
    assert_eq!(c.hello().2, b"layout-2");
}

#[test]
fn oversized_layout_closes_the_connection() {
    let h = Holder::start();
    let mut a = Client::connect(&h);
    a.hello();
    // Exactly the limit is kept.
    let biggest = vec![b'L'; 256 * 1024];
    a.send(ToHolder::SaveLayout {
        blob: biggest.clone(),
    });
    a.send(ToHolder::SaveLayout {
        blob: vec![b'M'; 256 * 1024 + 1],
    });
    assert!(a.closed_within(LIMIT), "the connection stayed open");
    let mut b = Client::connect(&h);
    assert!(
        b.hello().2 == biggest,
        "the oversized layout replaced the stored one"
    );
}

#[test]
fn info_reports_the_program_and_folder() {
    let h = Holder::start();
    let mut a = Client::connect(&h);
    a.hello();
    a.send(ToHolder::Spawn(spec(1, "/bin/sh", &[])));
    // Right after Spawn: the shell, in the home folder.
    let home = std::fs::canonicalize(std::env::var("HOME").unwrap()).unwrap();
    match a.wait_for(|f| matches!(f, ToWindow::Info { .. }), LIMIT) {
        Some(ToWindow::Info { pane, program, cwd }) => {
            assert_eq!((pane, program.as_str()), (1, "sh"));
            assert_eq!(std::path::Path::new(&cwd), home);
        }
        other => panic!("no Info after Spawn: {other:?}"),
    }
    a.send(ToHolder::Input {
        pane: 1,
        bytes: b"cd /tmp\n".to_vec(),
    });
    let f = a.wait_for(
        |f| matches!(f, ToWindow::Info { cwd, .. } if cwd == "/tmp"),
        LIMIT,
    );
    assert!(f.is_some(), "no Info for the new folder");
    a.send(ToHolder::Input {
        pane: 1,
        bytes: b"sleep 30.4\n".to_vec(),
    });
    let f = a.wait_for(
        |f| matches!(f, ToWindow::Info { program, .. } if program == "sleep"),
        LIMIT,
    );
    match f {
        Some(ToWindow::Info { pane, cwd, .. }) => assert_eq!((pane, cwd.as_str()), (1, "/tmp")),
        other => panic!("no Info for the running program: {other:?}"),
    }
}

#[test]
fn info_is_not_repeated_when_nothing_changed() {
    let h = Holder::start();
    let mut a = Client::connect(&h);
    a.hello();
    a.send(ToHolder::Spawn(spec(1, "/bin/sh", &[])));
    assert!(
        a.wait_for(|f| matches!(f, ToWindow::Info { .. }), LIMIT)
            .is_some(),
        "no Info after Spawn"
    );
    let mut infos = Vec::new();
    let mut output = Vec::new();
    // The prompt, then three commands that change neither program nor folder;
    // each time well past the 100 ms quiet check.
    for round in 0..4 {
        if round > 0 {
            a.send(ToHolder::Input {
                pane: 1,
                bytes: b"echo x-norepeat\n".to_vec(),
            });
        }
        let end = Instant::now() + Duration::from_millis(500);
        while let Some(f) = a.recv(end.saturating_duration_since(Instant::now())) {
            match f {
                ToWindow::Output { bytes, .. } => output.extend(bytes),
                i @ ToWindow::Info { .. } => infos.push(i),
                _ => {}
            }
        }
    }
    // Each echo was seen twice: the terminal's echo of the input, and its output.
    let text = String::from_utf8_lossy(&output);
    assert_eq!(text.matches("x-norepeat").count(), 6, "{text:?}");
    assert!(infos.is_empty(), "{infos:?}");
}

#[test]
fn idle_holder_does_not_wake() {
    let h = Holder::start();
    let mut a = Client::connect(&h);
    a.hello();
    a.send(ToHolder::Spawn(spec(1, "/bin/sh", &[])));
    // The shell's prompt, then quiet long enough for the one check after it.
    let end = Instant::now() + LIMIT;
    let mut prompt = false;
    while !prompt {
        assert!(Instant::now() < end, "no prompt");
        prompt = matches!(
            a.recv(end.saturating_duration_since(Instant::now())),
            Some(ToWindow::Output { .. })
        );
    }
    while a.recv(Duration::from_millis(500)).is_some() {}
    let before = h.context_switches();
    std::thread::sleep(Duration::from_secs(3));
    let after = h.context_switches();
    assert!(
        after - before <= 2,
        "kykod woke {} times in 3 s of quiet",
        after - before
    );
    // The window is still attached and the shell answers.
    a.send(ToHolder::Input {
        pane: 1,
        bytes: b"echo awake-$((6*7))\n".to_vec(),
    });
    a.output_until(1, b"awake-42", LIMIT);
}
