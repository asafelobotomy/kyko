mod common;

use std::time::{Duration, Instant};

use common::{Client, Holder, LIMIT, exists, gone_within, processes, spec};
use kyko_proto::{PaneId, ToHolder, ToWindow};
use nix::sys::signal::Signal;

/// How long `kykod` may take to exit once it has decided to: the 2 s grace
/// plus a second of slack.
const EXIT: Duration = Duration::from_secs(3);

/// [`EXIT`] for a test that waits the whole grace and then times the exit.
const SLOW_EXIT: Duration = Duration::from_secs(4);

/// Starts an interactive bash with job control in `pane`, waits for its
/// prompt and returns its pid. `tag` makes its command line unique.
fn idle_shell(h: &Holder, c: &mut Client, pane: PaneId, tag: &str) -> i32 {
    let mut s = spec(
        pane,
        "/bin/bash",
        &["--norc", "--noprofile", "-i", "-s", tag],
    );
    s.env = vec![("PS1".to_string(), "KYKO$ ".to_string())];
    c.send(ToHolder::Spawn(s));
    c.output_until(pane, b"KYKO$ ", LIMIT);
    h.child_running(&format!("/bin/bash --norc --noprofile -i -s {tag}"))
}

/// Types `command` into `pane` and returns the pid of the process whose
/// command line is exactly `command`, once it is running.
fn run_in(c: &mut Client, pane: PaneId, command: &str) -> i32 {
    c.send(ToHolder::Input {
        pane,
        bytes: format!("{command}\n").into_bytes(),
    });
    let end = Instant::now() + LIMIT;
    loop {
        let found: Vec<i32> = processes()
            .into_iter()
            .filter(|p| p.cmdline == command)
            .map(|p| p.pid)
            .collect();
        match found.as_slice() {
            [pid] => return *pid,
            [] if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            _ => panic!("expected one {command:?}, found {found:?}"),
        }
    }
}

#[test]
fn end_sessions_with_idle_shells_exits_0_and_ends_them() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let one = idle_shell(&h, &mut c, 1, "end-idle-1");
    let two = idle_shell(&h, &mut c, 2, "end-idle-2");
    c.send(ToHolder::EndSessions { force: false });
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(gone_within(one, LIMIT), "shell 1 outlived kykod");
    assert!(gone_within(two, LIMIT), "shell 2 outlived kykod");
    assert!(!h.socket().exists(), "the socket file is still there");
}

#[test]
fn end_sessions_is_refused_while_a_program_runs() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let one = idle_shell(&h, &mut c, 1, "end-busy-1");
    let two = idle_shell(&h, &mut c, 2, "end-busy-2");
    let sleep = run_in(&mut c, 2, "sleep 3062");
    c.send(ToHolder::EndSessions { force: false });
    let reply = c.wait_for(|f| matches!(f, ToWindow::Busy { .. }), LIMIT);
    assert_eq!(reply, Some(ToWindow::Busy { panes: vec![2] }));
    assert!(h.alive(), "kykod exited");
    assert!(
        exists(one) && exists(two) && exists(sleep),
        "a process ended"
    );
    // Nothing was hung up: the window can still use both panes.
    c.send(ToHolder::Input {
        pane: 1,
        bytes: b"echo still-$((40+2))\n".to_vec(),
    });
    c.output_until(1, b"still-42", LIMIT);
    assert!(h.socket().exists(), "the socket file was removed");
}

#[test]
fn busy_lists_every_busy_pane_in_ascending_order() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    // Spawned out of order, so the table's order is not the spawn order.
    idle_shell(&h, &mut c, 7, "end-order-7");
    idle_shell(&h, &mut c, 3, "end-order-3");
    idle_shell(&h, &mut c, 5, "end-order-5");
    run_in(&mut c, 7, "sleep 3067");
    run_in(&mut c, 3, "sleep 3063");
    c.send(ToHolder::EndSessions { force: false });
    let reply = c.wait_for(|f| matches!(f, ToWindow::Busy { .. }), LIMIT);
    assert_eq!(reply, Some(ToWindow::Busy { panes: vec![3, 7] }));
    assert!(h.alive(), "kykod exited");
}

#[test]
fn forced_end_sessions_overrides_busy() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let one = idle_shell(&h, &mut c, 1, "end-force-1");
    let two = idle_shell(&h, &mut c, 2, "end-force-2");
    let sleep = run_in(&mut c, 2, "sleep 3064");
    c.send(ToHolder::EndSessions { force: true });
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(gone_within(sleep, LIMIT), "the sleep outlived kykod");
    assert!(gone_within(one, LIMIT), "shell 1 outlived kykod");
    assert!(gone_within(two, LIMIT), "shell 2 outlived kykod");
    assert!(!h.socket().exists(), "the socket file is still there");
}

/// An end-to-end check of the brief's rule. It does not tell whether
/// `Pane::is_busy` checks for an ended shell: when the shell exits, the kernel
/// already clears the terminal's foreground group, so the pane reads as idle
/// either way.
#[test]
fn exited_panes_are_not_busy() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(1, "/bin/sh", &["-c", "exit 3"])));
    let exited = c.wait_for(|f| matches!(f, ToWindow::Exited { pane: 1, .. }), LIMIT);
    assert_eq!(exited, Some(ToWindow::Exited { pane: 1, status: 3 }));
    let two = idle_shell(&h, &mut c, 2, "end-exited-2");
    c.send(ToHolder::EndSessions { force: false });
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(gone_within(two, LIMIT), "shell 2 outlived kykod");
    assert!(!h.socket().exists(), "the socket file is still there");
}

#[test]
fn sigterm_hangs_up_the_shells() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let one = idle_shell(&h, &mut c, 1, "end-term-1");
    let two = idle_shell(&h, &mut c, 2, "end-term-2");
    let sleep = run_in(&mut c, 2, "sleep 3065");
    h.signal(Signal::SIGTERM);
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(gone_within(one, LIMIT), "shell 1 outlived kykod");
    assert!(gone_within(two, LIMIT), "shell 2 outlived kykod");
    assert!(gone_within(sleep, LIMIT), "the sleep outlived kykod");
    assert!(!h.socket().exists(), "the socket file is still there");
}

#[test]
fn sigint_ends_sessions_like_sigterm() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let one = idle_shell(&h, &mut c, 1, "end-int-1");
    h.signal(Signal::SIGINT);
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(gone_within(one, LIMIT), "the shell outlived kykod");
    assert!(!h.socket().exists(), "the socket file is still there");
}

#[test]
fn sigterm_without_panes_exits_at_once() {
    let mut h = Holder::start();
    let started = Instant::now();
    h.signal(Signal::SIGTERM);
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "kykod waited {:?} with no shells",
        started.elapsed()
    );
    assert!(!h.socket().exists(), "the socket file is still there");
}

#[test]
fn a_shell_ignoring_hangup_does_not_delay_exit_past_2s() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "trap '' HUP; printf armed; exec sleep 3066"],
    )));
    c.output_until(1, b"armed", LIMIT);
    h.child_running("sleep 3066");
    let started = Instant::now();
    h.signal(Signal::SIGTERM);
    // A loaded machine may be slow to run kykod's last steps: 2 s of slack.
    assert_eq!(h.wait_exit(SLOW_EXIT), Some(0), "log:\n{}", h.log());
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(2) && took < SLOW_EXIT,
        "kykod exited {took:?} after SIGTERM"
    );
    assert!(!h.socket().exists(), "the socket file is still there");
    // Observed, not required: kykod does not kill this shell on the way out,
    // so it outlives kykod (reparented, still running) until the holder's
    // cleanup ends it.
}

#[test]
fn a_pane_closed_just_before_sigterm_is_still_killed() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    c.send(ToHolder::Spawn(spec(
        1,
        "/bin/sh",
        &["-c", "trap '' HUP; printf armed; exec sleep 3068"],
    )));
    c.output_until(1, b"armed", LIMIT);
    let shell = h.child_running("sleep 3068");
    c.send(ToHolder::Close { pane: 1 });
    h.signal(Signal::SIGTERM);
    assert_eq!(h.wait_exit(SLOW_EXIT), Some(0), "log:\n{}", h.log());
    assert!(
        gone_within(shell, LIMIT),
        "the closed pane's shell outlived kykod; log:\n{}",
        h.log()
    );
}

#[test]
fn a_closing_pane_that_exits_quickly_does_not_delay_shutdown() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let shell = idle_shell(&h, &mut c, 1, "end-closing-1");
    c.send(ToHolder::Close { pane: 1 });
    let started = Instant::now();
    h.signal(Signal::SIGTERM);
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "kykod took {:?} to exit",
        started.elapsed()
    );
    assert!(gone_within(shell, LIMIT), "the shell outlived kykod");
}

#[test]
fn end_sessions_and_sigterm_together_end_once() {
    let mut h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let one = idle_shell(&h, &mut c, 1, "end-both-1");
    c.send(ToHolder::EndSessions { force: true });
    h.signal(Signal::SIGTERM);
    assert_eq!(h.wait_exit(EXIT), Some(0), "log:\n{}", h.log());
    assert!(gone_within(one, LIMIT), "the shell outlived kykod");
    assert!(h.log_has("kykod: exiting", 1), "log:\n{}", h.log());
    assert_eq!(
        common::count(h.log().as_bytes(), b"ending every session"),
        1,
        "log:\n{}",
        h.log()
    );
}
