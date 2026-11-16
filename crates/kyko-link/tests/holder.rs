//! The link against a real `kykod`.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use kyko_link::{Event, Link};
use kyko_proto::SpawnSpec;
use kyko_term::Colors;

struct Holder {
    child: Child,
    socket: PathBuf,
    _dir: tempfile::TempDir,
}

fn kykod() -> PathBuf {
    // Built by `cargo build --workspace`, which the tests run after; `cargo test --workspace`
    // builds every binary first.
    let exe = std::env::current_exe().unwrap();
    let target = exe.parent().unwrap().parent().unwrap();
    let path = target.join("kykod");
    if !path.exists() {
        let ok = Command::new(env!("CARGO"))
            .args(["build", "-p", "kykod"])
            .status()
            .unwrap()
            .success();
        assert!(ok, "could not build kykod");
    }
    path
}

impl Holder {
    fn start() -> Holder {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("kyko").join("hold.sock");
        let child = Command::new(kykod())
            .arg("--socket")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("kykod is built (run `cargo build --workspace` first)");
        let end = Instant::now() + Duration::from_secs(5);
        while !Path::new(&socket).exists() {
            assert!(Instant::now() < end, "kykod did not listen");
            std::thread::sleep(Duration::from_millis(10));
        }
        Holder {
            child,
            socket,
            _dir: dir,
        }
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spec(pane: u32, shell: &str, args: &[&str]) -> SpawnSpec {
    SpawnSpec {
        pane,
        shell: shell.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        cwd: String::new(),
        env: vec![("TERM".into(), "xterm-256color".into())],
        cols: 40,
        rows: 6,
        ring_bytes: 0,
    }
}

fn screen(link: &Link, pane: u32) -> String {
    let snap = link.pane(pane).unwrap().emulator().snapshot(&Colors {
        fg: 0xffffff,
        bg: 0,
        cursor: 0xffffff,
        ansi: [0; 16],
    });
    snap.cells
        .chunks(usize::from(snap.cols))
        .map(|row| {
            row.iter()
                .map(|c| c.ch)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pump_until(link: &mut Link, what: &str, done: impl Fn(&Link) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(link) {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        link.pump(Duration::from_millis(50)).unwrap();
    }
}

#[test]
fn spawn_type_and_see_the_screen() {
    let h = Holder::start();
    let mut link = Link::connect(&h.socket, 100).unwrap();
    link.spawn(spec(1, "/bin/sh", &[])).unwrap();
    link.attach(1).unwrap();
    link.input(1, b"echo kyko-link-$((6*7))\n").unwrap();
    pump_until(&mut link, "the echo", |l| {
        screen(l, 1).contains("kyko-link-42")
    });
}

#[test]
fn a_new_window_finds_the_pane_and_its_screen() {
    let h = Holder::start();
    {
        let mut first = Link::connect(&h.socket, 100).unwrap();
        first.spawn(spec(7, "/bin/sh", &[])).unwrap();
        first.attach(7).unwrap();
        first.input(7, b"echo before-restart\n").unwrap();
        pump_until(&mut first, "the echo", |l| {
            screen(l, 7).contains("before-restart")
        });
        first.save_layout(b"layout".to_vec()).unwrap();
    }
    // Reconnect: kykod refuses a second window while the first is live, so retry briefly.
    let end = Instant::now() + Duration::from_secs(5);
    let mut second = loop {
        match Link::connect(&h.socket, 100) {
            Ok(l) => break l,
            Err(e) => {
                assert!(Instant::now() < end, "no reconnect: {e}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    assert_eq!(second.pane_ids(), vec![7]);
    assert_eq!(second.layout, b"layout");
    second.attach(7).unwrap();
    pump_until(&mut second, "the replay", |l| {
        screen(l, 7).contains("before-restart")
    });
}

#[test]
fn an_exit_is_reported() {
    let h = Holder::start();
    let mut link = Link::connect(&h.socket, 100).unwrap();
    link.spawn(spec(2, "/bin/sh", &["-c", "exit 3"])).unwrap();
    link.attach(2).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < end, "no Exited");
        if link
            .pump(Duration::from_millis(50))
            .unwrap()
            .contains(&Event::Exited { pane: 2, status: 3 })
        {
            break;
        }
    }
    assert_eq!(link.pane(2).unwrap().exited, Some(3));
}

#[test]
fn a_dead_socket_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Link::connect(&dir.path().join("none.sock"), 10).is_err());
}
