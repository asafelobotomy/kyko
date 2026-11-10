# kyko Milestone 1b (session holder and terminal engine) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `kykod`, the process that owns the shells, and the two Qt-free libraries the window will use to talk to it and to emulate a terminal, all proven by tests with no window involved.

**Architecture:** Three pieces. `kyko-proto` is the wire format, pure encode and decode. `kyko-term` wraps `alacritty_terminal` as an emulator that is fed bytes and never touches a PTY. `kykod` spawns shells on PTYs, keeps a ring of each pane's recent output, and serves one window at a time over a private Unix socket. An end-to-end test plays the window's part: it drives `kykod` through `kyko-proto` and feeds the output into `kyko-term`.

**Tech Stack:** Rust stable (edition 2024). `alacritty_terminal` `=0.26.0` (its `vte::ansi::Processor::advance` feeds bytes straight into a `Term`). `nix` 0.31 for PTYs, signals, peer credentials and process groups. `signal-hook` 0.3. `clap` 4. `tempfile` 3 for tests. No async runtime: threads and blocking I/O.

**Spec:** `docs/superpowers/specs/2026-11-10-kyko-design.md` (§2.7 and §2.8 define the holder; §2.4 the terminal; §2.11 security and efficiency; §4 errors; §5 the tests; §6.2 open risks 5 and 6, which this slice closes).

**Depends on:** slice 1a merged (workspace, CI, packaging, release on bump).

## Global Constraints

- `kyko-proto` and `kyko-term` carry `#![forbid(unsafe_code)]`. `kykod` uses `unsafe` only where a PTY child is set up, each block with a comment saying why it is sound.
- Socket: `$XDG_RUNTIME_DIR/kyko/hold.sock`; the directory is created mode 0700, the socket 0600. A peer whose uid (from `SO_PEERCRED`) differs from `kykod`'s is dropped without a reply. `kykod --socket PATH` overrides the path (tests use it). If `XDG_RUNTIME_DIR` is unset and `--socket` is absent, exit 1 with a message.
- One window at a time: a second connection is refused while one is live.
- Frames: a `u32` little-endian length (of type plus body), a `u8` type, the body. Maximum length 1 MiB (1,048,576); a longer or unknown frame closes the connection. Integers little-endian; a string or byte string is a `u32` length then the bytes; a list is a `u32` count then the items.
- Protocol version: `1`. On a mismatch `kykod` answers with its own `Hello` (so the window can show the notice) and closes.
- Output ring: 2 MiB (2,097,152 bytes) per pane by default, settable per `Spawn` between 256 KiB and 16 MiB. `kykod` always reads every PTY, attached or not.
- Shell environment: `TERM=xterm-256color`, `COLORTERM=truecolor`, plus what `Spawn` sends, on top of `kykod`'s own environment.
- Sizes are clamped to at least 2 columns by 1 row.
- No timers while idle: nothing in `kykod` wakes on a clock when no shell is producing output and no client is talking. Foreground-program and folder checks run after output, not on a timer.
- On `SIGTERM` and on an accepted `EndSessions`: send `SIGHUP` to every shell's process group, wait up to 2 s for them to exit, then exit 0.
- Limits: at most 256 panes; the stored layout blob at most 256 KiB.
- Efficiency target (spec §6.2 item 6): `seq 1 2000000` through `kykod` finishes within 15% of the same run reading the PTY directly, each the median of five runs.
- Version after this slice: `0.0.2`, released by CI.
- Commit messages end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. **A frame that is truncated, oversized, of unknown type, or whose inner lengths overrun the frame:** the decoder returns an error and never panics or allocates the claimed size; `kykod` closes that connection and keeps its shells. Tests in Tasks 1 and 4.
2. **The window disappears mid-output** (killed, or its socket closed while a frame is half written): shells keep running, their output keeps filling the rings, and a new window can attach. Test in Task 5.
3. **A program that writes forever with no window attached** (`yes`): `kykod` neither blocks the program nor grows; memory stays bounded by the ring. Test in Task 5.
4. **A shell that cannot start** (missing binary, not executable, folder gone): the window gets `SpawnFailed` with the reason, no pane is left half-made, and `kykod` keeps running. Test in Task 4.
5. **Replay begins in the middle of an escape sequence or a multi-byte character** because the ring wrapped there: the emulator must not misread what follows. Tests in Tasks 2 and 7.

---

### Task 1: `kyko-proto`

**Files:**
- Create: `crates/kyko-proto/Cargo.toml`, `crates/kyko-proto/src/lib.rs`, `crates/kyko-proto/src/codec.rs`
- Modify: root `Cargo.toml` (member)
- Test: `crates/kyko-proto/tests/roundtrip.rs`

**Interfaces:**
- Produces:

```rust
pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 1 << 20;
pub type PaneId = u32;

pub struct SpawnSpec { pub pane: PaneId, pub shell: String, pub args: Vec<String>, pub cwd: String,
                       pub env: Vec<(String, String)>, pub cols: u16, pub rows: u16, pub ring_bytes: u32 }
pub struct PaneInfo  { pub pane: PaneId, pub program: String, pub cwd: String, pub cols: u16, pub rows: u16,
                       pub exited: Option<i32> }

pub enum ToHolder {            // type byte
    Hello { version: u32 },                 // 1
    Spawn(SpawnSpec),                       // 2
    Input { pane: PaneId, bytes: Vec<u8> }, // 3
    Resize { pane: PaneId, cols: u16, rows: u16 }, // 4
    Close { pane: PaneId },                 // 5
    Attach { pane: PaneId },                // 6
    SaveLayout { blob: Vec<u8> },           // 7
    EndSessions { force: bool },            // 8
}
pub enum ToWindow {
    Hello { version: u32, panes: Vec<PaneInfo>, layout: Vec<u8> }, // 65
    Output { pane: PaneId, bytes: Vec<u8> },                       // 66
    Info { pane: PaneId, program: String, cwd: String },           // 67
    Exited { pane: PaneId, status: i32 },                          // 68
    Busy { panes: Vec<PaneId> },                                   // 69
    SpawnFailed { pane: PaneId, message: String },                 // 70
    Replayed { pane: PaneId, truncated: bool },                    // 71
}
pub enum ProtoError { TooLong(usize), UnknownType(u8), Malformed(&'static str) }

pub trait Frame: Sized { fn encode(&self, out: &mut Vec<u8>); fn decode(kind: u8, body: &[u8]) -> Result<Self, ProtoError>; }
impl Frame for ToHolder; impl Frame for ToWindow;

pub struct Decoder { /* buffered bytes */ }
impl Decoder { pub fn new() -> Self; pub fn push(&mut self, bytes: &[u8]);
               pub fn next<T: Frame>(&mut self) -> Result<Option<T>, ProtoError>; }
```

  All types derive `Debug, Clone, PartialEq, Eq`. `exited: Option<i32>` is a `u8` flag then the `i32`.
- Two frames go beyond the spec's list, both required by it elsewhere: `SpawnFailed` (§4: the pane shows why the shell could not start) and `Replayed` (§2.8: the window must know when replay ends and whether the ring had wrapped, to skip to the first line break).

- [ ] **Step 1: Write the failing tests**

```rust
fn rt<T: Frame + PartialEq + std::fmt::Debug + Clone>(f: T) {
    let mut bytes = Vec::new(); f.encode(&mut bytes);
    let mut d = Decoder::new();
    for chunk in bytes.chunks(3) { d.push(chunk); }          // arrives in pieces
    assert_eq!(d.next::<T>().unwrap(), Some(f));
    assert_eq!(d.next::<T>().unwrap(), None);
}
#[test] fn every_frame_round_trips() { /* rt() for one value of each ToHolder and ToWindow variant,
    including empty strings and lists, non-ASCII strings, exited: Some(-9) and None, a 64 KiB Output */ }
#[test] fn two_frames_in_one_push_decode_in_order() { /* Input then Resize */ }
#[test] fn incomplete_frame_is_none_not_an_error() {
    let mut b = Vec::new(); ToHolder::Close { pane: 7 }.encode(&mut b);
    let mut d = Decoder::new(); d.push(&b[..b.len() - 1]);
    assert_eq!(d.next::<ToHolder>().unwrap(), None);
}
#[test] fn oversized_length_is_rejected_before_buffering() {
    let mut d = Decoder::new(); d.push(&(MAX_FRAME as u32 + 1).to_le_bytes()); d.push(&[3]);
    assert!(matches!(d.next::<ToHolder>(), Err(ProtoError::TooLong(_))));
}
#[test] fn unknown_type_is_rejected() {
    let mut d = Decoder::new(); d.push(&1u32.to_le_bytes()); d.push(&[200]);
    assert!(matches!(d.next::<ToHolder>(), Err(ProtoError::UnknownType(200))));
}
#[test] fn inner_length_overrunning_the_frame_is_malformed() {
    // Input { pane: 1, bytes: <claims 1000 bytes, frame holds 2> }
    let mut body = vec![3u8]; body.extend(1u32.to_le_bytes()); body.extend(1000u32.to_le_bytes()); body.extend([1, 2]);
    let mut d = Decoder::new(); d.push(&(body.len() as u32).to_le_bytes()); d.push(&body);
    assert!(matches!(d.next::<ToHolder>(), Err(ProtoError::Malformed(_))));
}
#[test] fn trailing_bytes_in_a_frame_are_malformed() { /* a valid Close body with one extra byte */ }
#[test] fn invalid_utf8_in_a_string_is_malformed() { /* Spawn with shell = [0xff] */ }
#[test] fn zero_length_frame_is_malformed() { /* length 0: no type byte */ }
#[test] fn a_window_frame_type_is_unknown_to_the_holder() { /* decode an encoded ToWindow::Busy as ToHolder */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p kyko-proto`
Expected: fails to compile.

- [ ] **Step 3: Implement.** A small cursor type in `codec.rs` with `u8/u16/u32/i32/bytes/string/list` readers that check the remaining length before every read (no allocation sized from untrusted input beyond what is present) and matching writers. `encode` writes the length last, once the body size is known.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p kyko-proto && cargo clippy -p kyko-proto --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 5: Commit** `feat(proto): the kykod wire protocol`.

### Task 2: `kyko-term`

**Files:**
- Create: `crates/kyko-term/Cargo.toml`, `crates/kyko-term/src/lib.rs`, `crates/kyko-term/src/colors.rs`
- Modify: root `Cargo.toml` (member)
- Test: `crates/kyko-term/tests/emulator.rs`

**Interfaces:**
- Produces:

```rust
pub const BOLD: u8 = 1; pub const ITALIC: u8 = 2; pub const UNDERLINE: u8 = 4; pub const INVERSE: u8 = 8;
pub const WIDE: u8 = 16; pub const WIDE_SPACER: u8 = 32; pub const STRIKE: u8 = 64;

pub struct Cell { pub ch: char, pub fg: u32, pub bg: u32, pub flags: u8 }   // 0xRRGGBB; inverse already swapped
pub struct Snapshot { pub cols: u16, pub rows: u16, pub cells: Vec<Cell>, pub cursor: Option<(u16, u16)> }
pub struct Colors { pub fg: u32, pub bg: u32, pub cursor: u32, pub ansi: [u32; 16] }

pub struct Emulator { /* Term<listener>, vte::ansi::Processor, reply buffer */ }
impl Emulator {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self;      // clamps to 2x1
    pub fn feed(&mut self, bytes: &[u8]);                             // output from the shell
    pub fn replay(&mut self, bytes: &[u8], truncated: bool);          // reset, then feed; if truncated, from just after the first b'\n'
    pub fn resize(&mut self, cols: u16, rows: u16);                   // clamps to 2x1
    pub fn snapshot(&self, colors: &Colors) -> Snapshot;
    pub fn take_replies(&mut self) -> Vec<u8>;                        // bytes the terminal must send back to the shell
    pub fn title(&self) -> Option<&str>;
    pub fn history_len(&self) -> usize;                               // scrollback lines held
    pub fn pending_sync(&self) -> bool;                               // a synchronised update is open
    pub fn flush_sync(&mut self);                                     // end it (the caller's timeout)
}
```

  `colors.rs` holds the palette resolution from the spike (`spike/src/engine.rs` on branch `spike/m0`: named, indexed 16 to 231 cube, 232 to 255 grey, true colour, dim variants), as a pure function `resolve(color, overrides, &Colors) -> u32`.
- The emulator's event listener collects `PtyWrite` text into the reply buffer and keeps the latest `Title`; clipboard and colour-request events are ignored in this slice (spec §2.11: programs cannot read the clipboard).

- [ ] **Step 1: Write the failing tests** (helper: `screen(bytes) -> Snapshot` on an 80×24 emulator with Gruvbox dark colours; `row(&snap, r) -> String` trimmed, skipping wide spacers):

```rust
#[test] fn plain_text()            { assert_eq!(row(&screen(b"hi there\r\n"), 0), "hi there"); }
#[test] fn ansi_red_uses_palette() { let s = screen(b"\x1b[31mX"); assert_eq!((s.cells[0].ch, s.cells[0].fg), ('X', 0xcc241d)); }
#[test] fn true_colour()           { assert_eq!(screen(b"\x1b[38;2;1;2;3mX").cells[0].fg, 0x010203); }
#[test] fn colour_cube_and_grey()  { assert_eq!(screen(b"\x1b[38;5;196mX").cells[0].fg, 0xff0000);
                                     assert_eq!(screen(b"\x1b[38;5;232mX").cells[0].fg, 0x080808); }
#[test] fn inverse_swaps()         { let s = screen(b"\x1b[7mX"); assert_eq!((s.cells[0].fg, s.cells[0].bg), (0x282828, 0xebdbb2)); }
#[test] fn styles_set_flags()      { let s = screen(b"\x1b[1;3;4;9mX"); assert_eq!(s.cells[0].flags & (BOLD|ITALIC|UNDERLINE|STRIKE), BOLD|ITALIC|UNDERLINE|STRIKE); }
#[test] fn wide_char_two_cells()   { let s = screen("日x".as_bytes()); assert!(s.cells[0].flags & WIDE != 0 && s.cells[1].flags & WIDE_SPACER != 0); assert_eq!(s.cells[2].ch, 'x'); }
#[test] fn invalid_utf8_shows_replacement() { let s = screen(b"\xff\xfeok"); assert_eq!(row(&s, 0), "\u{fffd}\u{fffd}ok"); }
#[test] fn cursor_position()       { assert_eq!(screen(b"ab").cursor, Some((2, 0))); }
#[test] fn cursor_hidden()         { assert_eq!(screen(b"\x1b[?25l").cursor, None); }
#[test] fn alternate_screen_and_back() { let mut e = emu(); e.feed(b"main"); e.feed(b"\x1b[?1049hALT"); assert_eq!(row(&snap(&e), 0), "ALT");
                                         e.feed(b"\x1b[?1049l"); assert_eq!(row(&snap(&e), 0), "main"); }
#[test] fn resize_clamps()         { let mut e = emu(); e.resize(0, 0); let s = snap(&e); assert_eq!((s.cols, s.rows), (2, 1)); e.resize(120, 40); assert_eq!(snap(&e).cols, 120); }
#[test] fn scrollback_is_bounded() { let mut e = Emulator::new(80, 24, 100); for i in 0..1000 { e.feed(format!("{i}\r\n").as_bytes()); } assert_eq!(e.history_len(), 100); }
#[test] fn device_status_report_is_answered() { let mut e = emu(); e.feed(b"\x1b[6n"); assert_eq!(e.take_replies(), b"\x1b[1;1R"); assert!(e.take_replies().is_empty()); }
#[test] fn title_is_kept()         { let mut e = emu(); e.feed(b"\x1b]0;hello\x07"); assert_eq!(e.title(), Some("hello")); }
#[test] fn replay_resets_first()   { let mut e = emu(); e.feed(b"old"); e.replay(b"new\r\n", false); assert_eq!(row(&snap(&e), 0), "new"); }
#[test] fn truncated_replay_skips_the_partial_first_line() {
    // The ring wrapped inside an escape sequence: "8;5;196mjunk\n" is the tail of a colour sequence.
    let mut e = emu(); e.replay(b"8;5;196mjunk\r\nclean\r\n", true);
    assert_eq!(row(&snap(&e), 0), "clean");
}
#[test] fn truncated_replay_with_no_newline_shows_nothing() { let mut e = emu(); e.replay(b"partial", true); assert_eq!(row(&snap(&e), 0), ""); }
#[test] fn truncated_replay_starting_mid_character_is_clean() {
    let mut e = emu(); e.replay(&[0x97, 0xa5, b'\n', b'o', b'k'], true);   // tail of a 3-byte character, then a line
    assert_eq!(row(&snap(&e), 0), "ok");
}
#[test] fn synchronised_update_is_held_until_flushed() {
    let mut e = emu(); e.feed(b"\x1b[?2026hhidden");
    assert!(e.pending_sync()); assert_eq!(row(&snap(&e), 0), "");
    e.flush_sync(); assert_eq!(row(&snap(&e), 0), "hidden");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p kyko-term`
Expected: fails to compile.

- [ ] **Step 3: Implement.** `Term::new(Config { scrolling_history: scrollback, ..Default::default() }, &size, listener)`; `feed` is `processor.advance(&mut term, bytes)`; `replay` builds a fresh `Term` and `Processor` of the current size before feeding. Carry the snapshot loop over from the spike's `engine.rs`, adding `STRIKE`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p kyko-term && cargo clippy -p kyko-term --all-targets -- -D warnings`
Expected: all pass. If `colour_cube_and_grey` or `device_status_report_is_answered` fails because `alacritty_terminal` 0.26 behaves differently from the value in the test, check the xterm specification for the right value and fix whichever is wrong, saying which in the commit message.

- [ ] **Step 5: Commit** `feat(term): emulator fed by bytes, with replay`.

### Task 3: `kykod` building blocks: ring and PTY

**Files:**
- Create: `apps/kykod/src/lib.rs`, `apps/kykod/src/ring.rs`, `apps/kykod/src/pty.rs`
- Modify: `apps/kykod/Cargo.toml` (a `[lib]` beside the `[[bin]]`), `apps/kykod/src/main.rs`
- Test: inline in `ring.rs`; `apps/kykod/tests/pty.rs`

**Interfaces:**
- Produces:

```rust
// ring.rs
pub struct Ring { /* VecDeque<u8>, capacity, wrapped */ }
impl Ring { pub fn new(capacity: usize) -> Self;            // clamped to 256 KiB ..= 16 MiB
            pub fn push(&mut self, bytes: &[u8]);           // drops the oldest bytes beyond capacity
            pub fn contents(&self) -> Vec<u8>;
            pub fn truncated(&self) -> bool;                // true once anything has been dropped
            pub fn len(&self) -> usize; }

// pty.rs
pub struct Pty { /* master OwnedFd, child pid */ }
pub struct SpawnError(pub String);                          // a sentence fit to show the user
impl Pty {
    pub fn spawn(shell: &str, args: &[String], cwd: &str, env: &[(String, String)], cols: u16, rows: u16) -> Result<Pty, SpawnError>;
    pub fn reader(&self) -> std::fs::File;                  // a duplicate of the master, blocking
    pub fn write_all(&self, bytes: &[u8]) -> std::io::Result<()>;
    pub fn resize(&self, cols: u16, rows: u16);
    pub fn pid(&self) -> i32;
    pub fn foreground(&self) -> Option<(i32, String)>;      // foreground process group and its program name (/proc/<pgid>/comm)
    pub fn cwd(&self) -> Option<String>;                    // /proc/<foreground pgid>/cwd, falling back to the shell's
    pub fn is_busy(&self) -> bool;                          // foreground group is not the shell's
    pub fn hangup(&self);                                   // SIGHUP to the shell's process group
    pub fn try_wait(&self) -> Option<i32>;                  // exit status, or 128 + signal
}
```

  `spawn` uses `nix::pty::openpty`, then `std::process::Command` with the slave as stdin, stdout and stderr and a `pre_exec` that calls `setsid` and makes the slave the controlling terminal. An empty `cwd` means the home directory; a `cwd` that does not exist is an error, not a silent fallback. `SpawnError` text examples: `"/bin/nope: No such file or directory"`, `"/tmp/gone: folder does not exist"`.

- [ ] **Step 1: Write the failing tests**

```rust
// ring.rs
#[test] fn keeps_everything_under_capacity() { let mut r = Ring::new(262_144); r.push(b"abc"); assert_eq!(r.contents(), b"abc"); assert!(!r.truncated()); }
#[test] fn drops_the_oldest_beyond_capacity() { let mut r = Ring::new(262_144); r.push(&vec![b'a'; 262_144]); r.push(b"xyz");
    let c = r.contents(); assert_eq!(c.len(), 262_144); assert!(c.ends_with(b"xyz")); assert!(r.truncated()); }
#[test] fn one_push_larger_than_capacity_keeps_its_tail() { let mut r = Ring::new(262_144); let mut big = vec![b'a'; 300_000]; big.extend(b"END"); r.push(&big);
    assert_eq!(r.len(), 262_144); assert!(r.contents().ends_with(b"END")); }
#[test] fn capacity_is_clamped() { assert_eq!(Ring::new(1).capacity(), 262_144); assert_eq!(Ring::new(usize::MAX).capacity(), 16 * 1024 * 1024); }

// tests/pty.rs  (helper: read_until(&mut file, needle, 5 s) -> Vec<u8>)
#[test] fn runs_a_command_and_reports_its_status() { let p = sh("printf hello; exit 7"); assert!(contains(read_all(&p), b"hello")); assert_eq!(wait(&p), Some(7)); }
#[test] fn environment_is_set() { let p = sh("echo $TERM/$COLORTERM/$EXTRA"); assert!(contains(read_all(&p), b"xterm-256color/truecolor/yes")); } // env: EXTRA=yes
#[test] fn starts_in_the_requested_folder() { /* cwd = a tempdir; `pwd` prints it */ }
#[test] fn size_is_what_was_asked_then_what_was_resized() { /* `stty size` prints "24 80"; after resize(100, 30) and another `stty size`: "30 100" */ }
#[test] fn input_reaches_the_program() { /* `cat`; write "ping\n"; output contains "ping" twice (echo and cat) */ }
#[test] fn missing_shell_is_a_spawn_error() { let e = Pty::spawn("/bin/nope", &[], "", &[], 80, 24).err().unwrap(); assert!(e.0.contains("/bin/nope")); }
#[test] fn missing_folder_is_a_spawn_error() { let e = Pty::spawn("/bin/sh", &[], "/no/such/dir", &[], 80, 24).err().unwrap(); assert!(e.0.contains("/no/such/dir")); }
#[test] fn foreground_program_is_reported() { /* shell runs `sleep 30`; within 2 s foreground().1 == "sleep" and is_busy(); cwd() is the shell's */ }
#[test] fn idle_shell_is_not_busy() { /* interactive `/bin/sh`, wait for the prompt; !is_busy() */ }
#[test] fn hangup_ends_the_shell_and_its_children() { /* `sleep 300` in the foreground; hangup(); try_wait() is Some within 2 s; the sleep pid is gone */ }
#[test] fn killed_by_signal_reports_128_plus_signal() { /* `kill -9 $$`; wait == Some(137) */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p kykod`
Expected: fails to compile.

- [ ] **Step 3: Implement** `ring.rs` (add `pub fn capacity(&self) -> usize`) and `pty.rs`. The one `unsafe` block is the `pre_exec` closure; its comment states that it calls only async-signal-safe functions (`setsid`, `ioctl(TIOCSCTTY)`).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p kykod` three times in a row.
Expected: all pass each time (PTY tests must not be flaky).

- [ ] **Step 5: Commit** `feat(kykod): output ring and PTY`.

### Task 4: `kykod` server: socket, spawn, input, output, exit

**Files:**
- Create: `apps/kykod/src/server.rs`, `apps/kykod/src/pane.rs`, `apps/kykod/tests/common/mod.rs`
- Modify: `apps/kykod/src/main.rs`, `apps/kykod/src/lib.rs`
- Test: `apps/kykod/tests/server.rs`

**Interfaces:**
- Consumes: `kyko_proto::*`, `Ring`, `Pty`.
- Produces:
  - Binary: `kykod [--socket PATH]` runs the server in the foreground and logs one line per event to stderr. Exit 1 if the socket path cannot be made or is already served by a live `kykod`; a stale socket file (nothing listening) is replaced.
  - `tests/common/mod.rs`: `struct Holder` (starts the built binary on a socket in a `tempfile::TempDir`, kills it on drop; `fn pid(&self)`, `fn socket(&self)`), and `struct Client` (`connect(&Holder) -> Client`; `send(ToHolder)`; `recv(timeout) -> Option<ToWindow>`; `hello() -> (u32, Vec<PaneInfo>, Vec<u8>)` which sends `Hello{version: VERSION}` and returns the reply; `output_until(pane, needle, timeout) -> Vec<u8>` collecting `Output` for that pane; `wait_for(pred, timeout) -> Option<ToWindow>`).
  - Behaviour in this task: `Hello`, `Spawn` (replies nothing on success, `SpawnFailed` on failure; a pane id already in use is `SpawnFailed` with "pane <n> already exists"), live `Output` for every pane to the connected window in chunks of at most 64 KiB, `Input`, `Resize`, `Close` (hangs the pane up and forgets it), `Exited` when a shell ends (the pane stays listed, with `exited`, until `Close`). Frames for an unknown pane are ignored. A protocol error closes the connection and nothing else.
  - Threads: one accept loop, one reader thread per connection, one reader thread per pane. The pane table and the connection's writer sit behind mutexes; a pane thread never holds the table lock while writing to the socket.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn hello_reports_the_version_and_no_panes()   { let h = Holder::start(); let mut c = Client::connect(&h); let (v, panes, layout) = c.hello(); assert_eq!((v, panes.len(), layout.len()), (VERSION, 0, 0)); }
#[test] fn socket_and_folder_are_private()            { /* mode of the socket & 0o777 == 0o600; of its folder == 0o700 */ }
#[test] fn spawn_then_output_arrives()                { /* Spawn sh -c "printf ready; sleep 30"; output_until(pane, b"ready") */ }
#[test] fn input_is_delivered()                       { /* Spawn `cat`; Input "ping\n"; output contains "ping" */ }
#[test] fn resize_reaches_the_shell()                 { /* Spawn sh; Resize 100x30; Input "stty size\n"; output contains "30 100" */ }
#[test] fn exit_status_is_reported_and_pane_stays_listed() { /* sh -c "exit 3" -> Exited{status: 3}; a fresh Hello on a new connection lists the pane with exited: Some(3) */ }
#[test] fn close_ends_the_shell_and_forgets_the_pane() { /* Spawn `sleep 300`; note its pid from /proc via Info or pgrep on the unique command; Close; pid gone within 2 s; new Hello lists no panes */ }
#[test] fn missing_shell_gives_spawn_failed()          { /* shell "/bin/nope" -> SpawnFailed whose message contains "/bin/nope"; then a good Spawn still works */ }
#[test] fn duplicate_pane_id_gives_spawn_failed()      { /* second Spawn with the same id -> "pane 1 already exists" */ }
#[test] fn too_many_panes_is_refused()                 { /* 256 spawns of `sleep 300` succeed; the 257th is SpawnFailed "too many panes" */ }
#[test] fn malformed_frame_closes_only_that_connection() { /* write raw garbage; the socket reaches EOF; a new connection's Hello still lists the running pane */ }
#[test] fn oversized_frame_closes_the_connection()     { /* a length of MAX_FRAME + 1 */ }
#[test] fn frames_before_hello_close_the_connection()  { /* Spawn as the first frame -> EOF, no pane created */ }
#[test] fn wrong_version_gets_hello_then_close()       { /* Hello{version: 999} -> ToWindow::Hello{version: 1, ..} then EOF */ }
#[test] fn stale_socket_file_is_replaced()             { /* create a plain file at the path; kykod starts and serves */ }
#[test] fn second_kykod_on_a_live_socket_exits_1()     { /* start a second binary on the same path: exit code 1, first still serves */ }
#[test] fn no_runtime_dir_and_no_socket_flag_exits_1() { /* env_remove("XDG_RUNTIME_DIR") */ }
```

A wrong-uid peer cannot be made in an unprivileged test; `server.rs` gets a unit test of the pure check `fn peer_allowed(peer_uid: u32, own_uid: u32) -> bool`, and the live check is listed under the plan's manual checks in Task 7.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p kykod --test server`
Expected: fails to compile.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p kykod` three times in a row; then `pgrep -x kykod; pgrep -f 'sleep 300'`.
Expected: all pass each time; both `pgrep`s print nothing (no process left behind by the tests).

- [ ] **Step 5: Commit** `feat(kykod): socket server with spawn, input, output and exit`.

### Task 5: Attach and replay, one window, layout, program and folder

**Files:**
- Modify: `apps/kykod/src/server.rs`, `apps/kykod/src/pane.rs`
- Test: `apps/kykod/tests/attach.rs`

**Interfaces:**
- Produces:
  - `Attach{pane}`: `kykod` sends the pane's ring as `Output` frames (at most 64 KiB each), then `Replayed{pane, truncated}`, then live output. No live byte is lost or duplicated across that boundary. A pane spawned on this connection is live from the start and needs no `Attach`; panes that existed before the connection send nothing until attached.
  - A second connection while one is live is closed at once, before any frame is read.
  - When the window's connection ends for any reason, every pane goes back to ring-only and keeps running.
  - `SaveLayout{blob}` replaces the stored blob (a blob over 256 KiB closes the connection); `Hello` returns the latest.
  - `Info{pane, program, cwd}` is sent when either value differs from the last one sent for that pane. It is checked after output on that pane has been quiet for 100 ms, and once right after `Spawn` and `Attach`. Nothing runs on a timer when no pane is producing output.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn output_before_any_window_is_replayed_on_attach() {
    // Window A spawns `sh -c 'echo one; sleep 1; echo two; sleep 300'` and disconnects at once.
    // After 2 s window B connects: Hello lists the pane; Attach -> Output containing "one" and "two" -> Replayed{truncated: false}.
}
#[test] fn live_output_follows_the_replay_without_loss() {
    // Pane prints an increasing counter every 5 ms. Attach mid-stream; collect 1 s.
    // The numbers seen across replay + live are consecutive: no gap, no repeat.
}
#[test] fn ring_wrap_sets_truncated_and_bounds_the_replay() {
    // Spawn with ring_bytes = 262144 running `yes` for 1 s, no window attached; Attach.
    // Total replayed bytes == 262144 and Replayed{truncated: true}.
}
#[test] fn writer_with_no_window_is_never_blocked_and_memory_is_bounded() {
    // `yes` with ring 262144, window disconnected, for 3 s: the program's CPU time keeps rising
    // (it is not blocked on a full PTY), and kykod's RssAnon stays under 32 MiB.
}
#[test] fn window_killed_mid_output_leaves_shells_running() {
    // Window connects, spawns a counter, then the test drops the socket abruptly. The shell pid is alive 1 s later; a new window attaches and sees later numbers.
}
#[test] fn second_window_is_refused_while_one_is_live() { /* second connect reaches EOF without a Hello; the first still works */ }
#[test] fn second_window_is_accepted_after_the_first_leaves() {}
#[test] fn layout_is_stored_and_returned() { /* SaveLayout b"layout-1", reconnect, Hello.layout == b"layout-1"; SaveLayout again replaces it */ }
#[test] fn oversized_layout_closes_the_connection() { /* 256 KiB + 1 */ }
#[test] fn info_reports_the_program_and_folder() {
    // Interactive sh. Input "cd /tmp\n" -> Info whose cwd is "/tmp". Input "sleep 30\n" -> Info whose program is "sleep".
}
#[test] fn info_is_not_repeated_when_nothing_changed() { /* three `echo x` in a row produce no further Info */ }
#[test] fn idle_holder_does_not_wake() {
    // One idle interactive shell, one attached window, nothing typed for 3 s:
    // kykod's voluntary + involuntary context switches (/proc/<pid>/status, all threads) rise by at most 2.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p kykod --test attach`
Expected: every test fails (no `Attach` handling, no `Replayed`, no `Info`).

- [ ] **Step 3: Implement.** Attach takes the pane's lock, sends the ring and `Replayed`, and flips the pane to live before releasing the lock, so the pane's reader thread cannot interleave. The 100 ms quiet check is done by the pane's reader thread using a read timeout that is armed only after output and disarmed once the check has run.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p kykod` three times in a row.
Expected: all pass each time.

- [ ] **Step 5: Commit** `feat(kykod): attach with replay, single window, layout and pane info`.

### Task 6: Ending sessions

**Files:**
- Modify: `apps/kykod/src/server.rs`, `apps/kykod/src/main.rs`
- Test: `apps/kykod/tests/ending.rs`

**Interfaces:**
- Produces:
  - `EndSessions{force: false}`: if any pane is busy (`Pty::is_busy`), reply `Busy{panes}` (ids ascending) and change nothing. Otherwise, and always with `force: true`: hang up every pane, wait up to 2 s, then exit 0. The socket file is removed on exit.
  - `SIGTERM` and `SIGINT`: the same shutdown as a forced `EndSessions`.
  - Exited panes do not count as busy.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn end_sessions_with_idle_shells_exits_0_and_ends_them() { /* two interactive sh; EndSessions{force:false}; kykod exits 0 within 3 s; both shell pids gone; socket file gone */ }
#[test] fn end_sessions_is_refused_while_a_program_runs() { /* pane 1 idle sh, pane 2 running `sleep 300`: Busy{panes: [2]}; kykod and both shells still alive */ }
#[test] fn forced_end_sessions_overrides_busy() { /* same setup, force: true: exits 0, the sleep is gone */ }
#[test] fn exited_panes_are_not_busy() { /* a pane whose shell already exited, plus an idle one: exits 0 */ }
#[test] fn sigterm_hangs_up_the_shells() { /* SIGTERM to kykod: exit 0 within 3 s, shells gone, socket gone */ }
#[test] fn a_shell_ignoring_hangup_does_not_delay_exit_past_2s() { /* `trap '' HUP; sleep 300`: kykod exits between 2 and 3 s after SIGTERM */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p kykod --test ending`
Expected: every test fails.

- [ ] **Step 3: Implement,** with `signal-hook`'s iterator on its own thread.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p kykod` three times in a row; then `pgrep -x kykod; pgrep -f 'sleep 300'`.
Expected: all pass; nothing left running.

- [ ] **Step 5: Commit** `feat(kykod): end sessions on request and on SIGTERM`.

### Task 7: The emulator through the holder, the speed check, and packaging

**Files:**
- Create: `apps/kykod/tests/end_to_end.rs`, `apps/kykod/tests/hop_bench.rs`, `packaging/kykod.service`, `docs/notes/2026-11-XX-m1b-checks.md` (dated the day it is written)
- Modify: `apps/kykod/Cargo.toml` (dev-dependency `kyko-term`), `packaging/kyko/PKGBUILD`, `packaging/kyko/.SRCINFO`, `CHANGELOG.md`, `README.md`, versions through `scripts/bump-version.sh 0.0.2`

**Interfaces:**
- Consumes: everything above.
- Produces: `packaging/kykod.service`, a systemd user unit: `Description=kyko session holder`, `ExecStart=/usr/bin/kykod`, `Restart=on-failure`, `KillMode=mixed`, `TimeoutStopSec=5`, `WantedBy=graphical-session.target`. It carries no sandboxing options (spec §2.11: the shells must be able to run `sudo`, `run0` and network tools). The package installs it to `/usr/lib/systemd/user/` and does not enable it.

- [ ] **Step 1: Write the failing end-to-end tests.** The test plays the window: every `Output` for a pane is fed to a `kyko_term::Emulator`, and the emulator's `take_replies()` are sent back as `Input`.

```rust
#[test] fn a_shell_session_renders_through_the_holder() {
    // Spawn sh; Input "printf '\\033[31mred\\033[0m plain\\n'\n". The emulator's screen has a row "red plain" whose first cell is 0xcc241d.
}
#[test] fn terminal_replies_reach_the_program() {
    // Run a script that sends ESC[6n and prints what it reads back; the output contains the cursor report, proving the reply went emulator -> Input -> PTY.
}
#[test] fn reattach_restores_a_shell_screen() {
    // Session: three commands with known output. Record snapshot S1. Drop the connection, connect again,
    // Attach, replay into a NEW emulator with Emulator::replay(bytes, truncated). Its snapshot equals S1 cell for cell.
}
#[test] fn reattach_restores_a_full_screen_program_after_the_resize_nudge() {
    // Run `less` on a 200-line file. Reconnect, Attach and replay, then send Resize with the same size
    // (the nudge from spec §2.8) and feed what follows. The screen's first row is the file's first line and the last row is less's prompt.
}
#[test] fn reattach_after_the_ring_wrapped_is_clean() {
    // ring_bytes = 262144; print 20,000 numbered lines, then `echo MARKER`. Reattach with replay(.., truncated = true).
    // No row contains escape-sequence debris (no row matches /^\d*;\d*m/ or contains U+FFFD), and a row reads "MARKER".
}
```

- [ ] **Step 2: Run to verify they fail, then make them pass.**

Run: `cargo test -p kykod --test end_to_end`
Expected first: fails to compile (the dev-dependency and helpers are missing). Add them; no change to `kykod` or `kyko-term` should be needed. If a test then fails, that is a real defect in an earlier task: find the cause with superpowers:systematic-debugging, fix it there with its own failing test first, and note it in the checks file.

- [ ] **Step 3: Write and run the speed check** `hop_bench.rs`, one `#[ignore]` test `holder_hop_is_within_15_percent`. It measures `seq 1 2000000` five times each way, to the point where the shell has exited and all output has been received, and discards the bytes: (a) `Pty::spawn` read directly through `Pty::reader()`; (b) the same through `kykod` and a `Client`. It prints both medians and the ratio and asserts ratio ≤ 1.15.

Run: `cargo test -p kykod --release --test hop_bench -- --ignored --nocapture` with the machine otherwise idle (`cut -d' ' -f1 /proc/loadavg` under 1.5; if not, wait or say so in the checks file).
Expected: a ratio at or under 1.15. If it is over, profile before changing anything (larger socket buffers and fewer, larger `Output` frames are the first things to try), and record the before and after.

- [ ] **Step 4: Package.** Add `packaging/kykod.service`, install it in `package()`, run `scripts/bump-version.sh 0.0.2`, write the `## [0.0.2]` CHANGELOG section, and update the README's status paragraph.

Run: `bash tests/shell/version_test.sh && scripts/build-local-pkg.sh && pacman -Qlp target/pkg/kyko-0.0.2-1-x86_64.pkg.tar.zst | grep -E 'kykod|systemd'`
Expected: lists `/usr/bin/kykod` and `/usr/lib/systemd/user/kykod.service`.

- [ ] **Step 5: Manual checks, recorded in the checks file** with the command and what happened for each:
  1. Run the unit for real: `systemd-run --user --unit=kykod-check target/release/kykod`, connect with the test client, spawn a shell, then `systemctl --user stop kykod-check`: the shell is gone within 5 s and the unit shows a clean exit.
  2. `systemctl --user kill -s KILL kykod-check` on a fresh run: confirms what the spec says happens when `kykod` itself dies (the shells end).
  3. The peer-uid check: as another user if one is available (`run0 -u nobody` connecting to the socket is refused by its 0600 mode before the uid check is reached; record that this is what was observed).
  4. The hop bench numbers and the load average at the time.
  5. Idle wake-ups: `kykod` with one idle shell for 60 s, context-switch counts before and after.

- [ ] **Step 6: Run everything.**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo deny check advisories licenses && for t in tests/shell/*.sh; do bash "$t" || exit 1; done`
Expected: all green.

- [ ] **Step 7: Commit** `feat: kykod 0.0.2, end-to-end tests, unit file and checks`. Merging to `main` and pushing (which releases 0.0.2) needs the user's go-ahead, as in slice 1a.
