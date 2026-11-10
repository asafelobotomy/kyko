//! The emulator through the holder: these tests play the window. Every
//! `Output` for a pane is fed to a `kyko_term::Emulator`, and what the
//! emulator has to answer (`take_replies`) goes back to the pane as `Input`.
mod common;

use std::time::{Duration, Instant};

use common::{Client, Holder, LIMIT, spec};
use kyko_proto::{PaneId, SpawnSpec, ToHolder, ToWindow};
use kyko_term::{Colors, Emulator, Snapshot, WIDE_SPACER};

const COLS: u16 = 80;
const ROWS: u16 = 24;
const SCROLLBACK: usize = 1000;

const GRUVBOX: Colors = Colors {
    fg: 0xebdbb2,
    bg: 0x282828,
    cursor: 0xebdbb2,
    ansi: [
        0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374,
        0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
    ],
};

/// How long a screen must stay unchanged to count as settled.
const QUIET: Duration = Duration::from_millis(300);

/// Row `r` as text: wide-char spacers skipped, trailing blanks trimmed.
fn row(s: &Snapshot, r: usize) -> String {
    let cols = s.cols as usize;
    s.cells[r * cols..(r + 1) * cols]
        .iter()
        .filter(|c| c.flags & WIDE_SPACER == 0)
        .map(|c| c.ch)
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn rows(s: &Snapshot) -> Vec<String> {
    (0..s.rows as usize).map(|r| row(s, r)).collect()
}

fn dump(s: &Snapshot) -> String {
    rows(s)
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{i:2}|{r}\n"))
        .collect()
}

/// A shell started without the user's start-up files, with a plain prompt.
fn plain_shell(pane: PaneId) -> SpawnSpec {
    let mut s = spec(pane, "/bin/bash", &["--norc", "--noprofile", "-i"]);
    s.env = vec![("PS1".into(), "$ ".into())];
    s
}

/// The window's side of one pane: a connection and the pane's emulator.
struct Window {
    client: Client,
    emu: Emulator,
    pane: PaneId,
}

impl Window {
    /// Connects and says `Hello`; the emulator starts blank.
    fn open(h: &Holder, pane: PaneId) -> Window {
        let mut client = Client::connect(h);
        client.hello();
        Window {
            client,
            emu: Emulator::new(COLS, ROWS, SCROLLBACK),
            pane,
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.emu.snapshot(&GRUVBOX)
    }

    /// Feeds one frame, if it is this pane's output, and sends the replies
    /// it provokes back as `Input`. True if it was output.
    fn take(&mut self, frame: &ToWindow) -> bool {
        match frame {
            ToWindow::Output { pane, bytes } if *pane == self.pane => {
                self.emu.feed(bytes);
                let replies = self.emu.take_replies();
                if !replies.is_empty() {
                    self.client.send(ToHolder::Input {
                        pane: self.pane,
                        bytes: replies,
                    });
                }
                true
            }
            _ => false,
        }
    }

    /// Plays the window until the screen satisfies `done`; panics with the
    /// screen after `timeout`.
    fn until(&mut self, done: impl Fn(&Snapshot) -> bool, timeout: Duration) -> Snapshot {
        let end = Instant::now() + timeout;
        loop {
            let s = self.snapshot();
            if done(&s) {
                return s;
            }
            let left = end.saturating_duration_since(Instant::now());
            match self.client.recv(left) {
                Some(f) => {
                    self.take(&f);
                }
                None => panic!("the screen never got there:\n{}", dump(&s)),
            }
        }
    }

    /// Plays the window until no output has come for [`QUIET`].
    fn settle(&mut self) -> Snapshot {
        while let Some(f) = self.client.recv(QUIET) {
            self.take(&f);
        }
        self.snapshot()
    }

    fn type_text(&mut self, text: &str) {
        self.client.send(ToHolder::Input {
            pane: self.pane,
            bytes: text.as_bytes().to_vec(),
        });
    }

    /// Attaches this window to its pane and replays the pane into its
    /// emulator; returns it with the replayed bytes and the `truncated` flag.
    fn reattach_here(mut self) -> (Window, Vec<u8>, bool) {
        let (bytes, truncated) = self.client.attach(self.pane);
        self.emu.replay(&bytes, truncated);
        (self, bytes, truncated)
    }

    /// Leaves, connects again, attaches and replays the pane into a new
    /// emulator, as a window started afresh does. The bytes replayed are every
    /// `Output` that came before `Replayed`; they are returned with the
    /// `truncated` flag.
    fn reattach(self, h: &Holder) -> (Window, Vec<u8>, bool) {
        let pane = self.pane;
        drop(self);
        Window::open(h, pane).reattach_here()
    }
}

#[test]
fn a_shell_session_renders_through_the_holder() {
    let h = Holder::start();
    let mut w = Window::open(&h, 1);
    w.client.send(ToHolder::Spawn(spec(1, "/bin/sh", &[])));
    w.type_text("printf '\\033[31mred\\033[0m plain\\n'\n");
    let s = w.until(|s| rows(s).iter().any(|r| r == "red plain"), LIMIT);
    let r = rows(&s).iter().position(|r| r == "red plain").unwrap();
    let first = s.cells[r * s.cols as usize];
    assert_eq!((first.ch, first.fg), ('r', 0xcc241d), "\n{}", dump(&s));
    let plain = s.cells[r * s.cols as usize + 4];
    assert_eq!((plain.ch, plain.fg), ('p', GRUVBOX.fg), "\n{}", dump(&s));
}

#[test]
fn terminal_replies_reach_the_program() {
    let h = Holder::start();
    let mut w = Window::open(&h, 1);
    // Asks where the cursor is, reads the answer from its terminal and prints
    // it without the leading ESC. Only the emulator can answer, and only
    // through Input.
    let script = "stty -icanon -echo; printf '\\033[6n'; \
                  IFS= read -r -d R reply; printf 'report=%s;\\n' \"${reply#?}\"; \
                  sleep 300.7";
    w.client
        .send(ToHolder::Spawn(spec(1, "/bin/bash", &["-c", script])));
    let s = w.until(|s| rows(s).iter().any(|r| r.starts_with("report=")), LIMIT);
    // The query came first, at the top left: row 1, column 1.
    assert!(
        rows(&s).iter().any(|r| r == "report=[1;1;"),
        "\n{}",
        dump(&s)
    );
}

#[test]
fn reattach_restores_a_shell_screen() {
    let h = Holder::start();
    let mut w = Window::open(&h, 1);
    w.client.send(ToHolder::Spawn(plain_shell(1)));
    w.until(|s| row(s, 0) == "$", LIMIT);
    w.type_text("echo alpha\n");
    w.until(|s| rows(s).iter().any(|r| r == "alpha"), LIMIT);
    w.type_text("printf '\\033[1;32mbeta\\033[0m %s\\n' gamma\n");
    w.until(|s| rows(s).iter().any(|r| r == "beta gamma"), LIMIT);
    w.type_text("seq 3 | tr '\\n' ,; echo\n");
    w.until(|s| rows(s).iter().any(|r| r == "1,2,3,"), LIMIT);
    let s1 = w.settle();
    assert_eq!(
        row(&s1, 6),
        "$",
        "the last prompt is not where expected\n{}",
        dump(&s1)
    );

    let (w, _, truncated) = w.reattach(&h);
    assert!(!truncated);
    let s2 = w.snapshot();
    assert_eq!(rows(&s2), rows(&s1));
    assert_eq!(s2, s1, "the replayed screen differs cell for cell");
}

/// Starts `less` on 200 numbered lines in pane 1, pages down once and leaves.
/// Returns a test for the paged screen.
fn paged_less(h: &Holder, dir: &tempfile::TempDir) -> impl Fn(&Snapshot) -> bool + use<> {
    let file = dir.path().join("lines.txt");
    let text: String = (1..=200).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&file, text).unwrap();
    let file = file.to_str().unwrap().to_string();

    let mut w = Window::open(h, 1);
    let mut s = spec(1, "/usr/bin/less", &[&file]);
    // The user's own less settings must not change the screen.
    s.env = vec![
        ("LESS".into(), String::new()),
        ("LESSOPEN".into(), String::new()),
        ("LESSHISTFILE".into(), "-".into()),
    ];
    w.client.send(ToHolder::Spawn(s));
    let last = ROWS as usize - 1;
    let on_first_page = move |s: &Snapshot| row(s, 0) == "line 1" && row(s, last - 1) == "line 23";
    w.until(on_first_page, LIMIT);
    // Page down once, so the screen is not the program's first drawing.
    w.type_text(" ");
    let paged = move |s: &Snapshot| row(s, 0) == "line 24" && row(s, last) == ":";
    w.until(paged, LIMIT);
    w.settle();
    paged
}

/// Every `Output` for `pane` that arrives within `period`, in bytes.
fn output_within(c: &mut Client, pane: PaneId, period: Duration) -> usize {
    let end = Instant::now() + period;
    let mut n = 0;
    while let Some(f) = c.recv(end.saturating_duration_since(Instant::now())) {
        if let ToWindow::Output { pane: p, bytes } = f
            && p == pane
        {
            n += bytes.len();
        }
    }
    n
}

#[test]
fn a_same_size_resize_after_attach_makes_a_full_screen_program_redraw() {
    let h = Holder::start();
    let dir = tempfile::tempdir().unwrap();
    let paged = paged_less(&h, &dir);

    // A window started afresh that ignores the replay: everything it shows
    // must come from the redraw the nudge provokes.
    let mut w = Window::open(&h, 1);
    w.client.attach(1);
    assert!(!paged(&w.snapshot()));
    w.client.send(ToHolder::Resize {
        pane: 1,
        cols: COLS,
        rows: ROWS,
    });
    w.until(&paged, LIMIT);
    w.settle();
    // Only the first same-size Resize after Attach nudges: a window that
    // sends its size again does not make the program draw again.
    w.client.send(ToHolder::Resize {
        pane: 1,
        cols: COLS,
        rows: ROWS,
    });
    let n = output_within(&mut w.client, 1, Duration::from_millis(500));
    assert_eq!(n, 0, "a second same-size Resize made less draw {n} bytes");
    // Still the live program: it answers a key.
    w.type_text("q");
    w.client
        .wait_for(|f| matches!(f, ToWindow::Exited { pane: 1, .. }), LIMIT)
        .expect("less did not quit");
}

#[test]
fn a_real_resize_after_attach_uses_up_the_nudge() {
    let h = Holder::start();
    let dir = tempfile::tempdir().unwrap();
    let _ = paged_less(&h, &dir);

    let mut w = Window::open(&h, 1);
    w.client.attach(1);
    // A real change of size: the kernel signals less, which draws.
    w.client.send(ToHolder::Resize {
        pane: 1,
        cols: COLS,
        rows: ROWS - 4,
    });
    w.client
        .wait_for(|f| matches!(f, ToWindow::Output { pane: 1, .. }), LIMIT)
        .expect("less did not draw after a change of size");
    w.settle();
    // The same size again is not the first Resize since Attach: no redraw.
    w.client.send(ToHolder::Resize {
        pane: 1,
        cols: COLS,
        rows: ROWS - 4,
    });
    let n = output_within(&mut w.client, 1, Duration::from_millis(500));
    assert_eq!(n, 0, "a same-size Resize after a real one drew {n} bytes");
}

#[test]
fn a_same_size_resize_to_a_shell_at_its_prompt_is_harmless() {
    let h = Holder::start();
    let mut w = Window::open(&h, 1);
    w.client.send(ToHolder::Spawn(plain_shell(1)));
    w.until(|s| row(s, 0) == "$", LIMIT);
    w.settle();
    drop(w);
    let (mut w, _, _) = Window::open(&h, 1).reattach_here();
    w.client.send(ToHolder::Resize {
        pane: 1,
        cols: COLS,
        rows: ROWS,
    });
    w.type_text("echo nudged-$((6*7))\n");
    w.until(|s| rows(s).iter().any(|r| r == "nudged-42"), LIMIT);
    assert!(!w.client.ended(), "kykod closed the connection");
}

/// Escape-sequence debris: what a cut SGR sequence leaves at a row's start.
fn sgr_debris(row: &str) -> bool {
    let rest = row.trim_start_matches(|c: char| c.is_ascii_digit());
    let Some(rest) = rest.strip_prefix(';') else {
        return false;
    };
    rest.trim_start_matches(|c: char| c.is_ascii_digit())
        .starts_with('m')
}

#[test]
fn reattach_after_the_ring_wrapped_is_clean() {
    let h = Holder::start();
    let mut w = Window::open(&h, 1);
    // 20,000 numbered lines in colour, about 980 KB: the 256 KiB ring wraps
    // more than twice, and wherever it is cut there is an escape sequence near.
    let script = "awk 'BEGIN { for (i = 1; i <= 20000; i++) \
                  printf \"\\033[1;3%dmnumbered line %05d\\033[0m of the ring test\\n\", i % 8, i }'; \
                  echo MARKER; sleep 300.9";
    let mut s = spec(1, "/bin/sh", &["-c", script]);
    s.ring_bytes = 262_144;
    w.client.send(ToHolder::Spawn(s));
    w.client.output_until(1, b"MARKER", Duration::from_secs(20));

    let (w, bytes, truncated) = w.reattach(&h);
    assert!(truncated, "the ring did not wrap");
    let s = w.snapshot();
    for r in rows(&s) {
        assert!(
            !sgr_debris(&r) && !r.contains('\u{fffd}'),
            "debris in {r:?}:\n{}",
            dump(&s)
        );
    }
    assert!(rows(&s).iter().any(|r| r == "MARKER"), "\n{}", dump(&s));
    assert!(
        rows(&s)
            .iter()
            .any(|r| r == "numbered line 20000 of the ring test"),
        "\n{}",
        dump(&s)
    );

    // The window shows only the last rows; the cut is far above them. Replay
    // the same bytes into a terminal tall enough to show all of it: every row
    // is a whole numbered line, in order, up to the marker.
    let mut tall = Emulator::new(COLS, 6000, 0);
    tall.replay(&bytes, truncated);
    let all = rows(&tall.snapshot(&GRUVBOX));
    let text: Vec<&String> = all.iter().filter(|r| !r.is_empty()).collect();
    assert!(text.len() > 1000, "only {} rows replayed", text.len());
    let (marker, lines) = text.split_last().unwrap();
    assert_eq!(marker.as_str(), "MARKER");
    let numbers: Vec<u32> = lines
        .iter()
        .map(|r| {
            assert!(!sgr_debris(r) && !r.contains('\u{fffd}'), "debris in {r:?}");
            r.strip_prefix("numbered line ")
                .and_then(|r| r.strip_suffix(" of the ring test"))
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("not a whole numbered line: {r:?}"))
        })
        .collect();
    assert_eq!(*numbers.last().unwrap(), 20000);
    for pair in numbers.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "a line is missing or repeated");
    }
}

#[test]
fn debris_is_recognised() {
    assert!(sgr_debris("1;32mnumbered line"));
    assert!(sgr_debris(";m"));
    assert!(!sgr_debris("numbered line 00001"));
    assert!(!sgr_debris("1,2,3,"));
}
