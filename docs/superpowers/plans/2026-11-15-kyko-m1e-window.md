# kyko Milestone 1e (the window: one tab end to end) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `kyko` becomes a window. It drops down from the top of the screen on KWin, attaches
to `kykod`, restores the saved layout, and shows the active tab's panes drawn by the glyph-atlas
renderer, with real keyboard input and input methods. `kyko toggle`, `show`, `hide` and `quit`
work over D-Bus. Config and theme changes apply live. The two `kykod` follow-ups that slice 1d
left open are dealt with first, because the window relies on them.

**After this slice:** you can install kyko from a local build on Plasma, bind `kyko toggle` to a
key by hand, and use one tab as a terminal. Tab and split *controls* (tab bar, keys, dividers)
are milestone 2. The renderer's unbuilt parts, mouse input, the fallbacks, the systemd and D-Bus
packaging, and `v0.1.0` are slice 1f.

**Architecture:** the window is `apps/kyko`, a cxx-qt 0.10 app with QML for the window and one
C++ `QQuickItem` for the terminal grid (§2.1; the renderer spike's option A). Everything that
can be plain Rust is plain Rust and tested with `cargo test` without Qt: the link to `kykod`, the
key encoder, the cell metrics and row diffing, the file watcher. Qt is a thin layer that calls
into it.

```
crates/kyko-term/src/
  input.rs        key events to bytes (xterm encoding; kitty stays off), paste wrapping
apps/kyko/
  build.rs        cxx-qt build, moc for the C++ item, qsb for its shaders
  src/main.rs     CLI: no subcommand starts the window; toggle/show/hide/quit call it over D-Bus
  src/link.rs     the client side of the socket: Hello, reconcile, Attach, replay, reconnect (no Qt)
  src/panes.rs    one Emulator per pane, fed by the link thread; snapshots for the grid item
  src/app.rs      the cxx-qt QObject the QML talks to: session, appearance, visibility
  src/dbus.rs     org.kyko.Terminal1 (Toggle, Show, Hide, Quit, Visible), single instance (zbus)
  src/watch.rs    config.toml and tokens.json watched with inotify; ThemeChanged on the bus
  cpp/grid.h/.cpp the grid item: scene-graph nodes, coverage glyph atlas, input
  shaders/        glyph.vert, glyph.frag
  qml/Main.qml    the layer-shell surface, shapes, slide, the panes at their rects, notices
```

**Tech Stack:** `cxx` 1, `cxx-qt`, `cxx-qt-lib` and `cxx-qt-build` 0.10 (as the spike used),
Qt 6 (6.12 on the development machine; the spike ran 6.11.2), `layer-shell-qt` (QML module
`org.kde.layershell`), `qt6-shadertools` (`qsb`) at build time, `zbus` (blocking API) for D-Bus,
`rustix` (`fs` feature, already a `kykod` dependency) for inotify. Each new crate is checked with
`cargo deny` in CI before its task is merged.

**Spec:** `docs/superpowers/specs/2026-11-10-kyko-design.md` §2.1 (units), §2.2 (window), §2.4
(terminal and renderer), §2.5 and §2.6 (theme, config, live apply), §2.7 and §2.8 (holder, attach,
replay, reconnect, version), §2.10 (environments), §3.1 (the D-Bus members this slice serves), §4
(errors), §5 (testing).

**Depends on:** slice 1d (v0.0.4) on `main`. Release checklist:
`docs/notes/2026-11-13-v1-release-checklist.md`.

## Global Constraints

- **`kyko-core`, `kyko-proto` and `kyko-term` stay free of Qt** and keep
  `#![forbid(unsafe_code)]`. `apps/kyko` uses `unsafe` only at the cxx boundary, each block
  commented with its reason (§2.11).
- **The GUI thread never blocks on the socket or on a lock held across I/O.** The link reads on
  its own thread. Emulators are fed on that thread, in chunks of at most 64 KiB, taking a pane's
  lock once per chunk, so the grid item's snapshot (taken under the same lock) waits at most one
  chunk.
- **Hidden and idle means asleep** (§2.11): no timers run while connected and hidden. The only
  timers are the 2 s reconnect while `kykod` is unreachable and the slide animation.
- **Repaints are coalesced:** the link thread marks a pane dirty with an atomic flag and queues
  one update to the GUI thread only when the flag was clear.
- **Nothing in the window ends a shell** (§2.7). `Quit` stops the window process only.
- **The window follows slice 1d's rules for `kykod`:** it sends `Close` for every pane that ends,
  keeps pane ids rising, and keeps the layout blob under `MAX_LAYOUT` (all done by `kyko-core`;
  this slice only carries out the effects).
- **The kitty keyboard protocol stays off** until `alacritty_terminal` ships the stack fix
  (checklist). The encoder is written so that adding it later is a new branch, not a rewrite.
- Each step ends with `cargo test --workspace`, `cargo clippy --all-targets -D warnings` and
  `cargo fmt --check` before its commit; tasks that touch C++ or QML also run the window's smoke
  test (Task 9).
- Version after this slice: `0.0.5`, released by CI.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **The link's state machine** (Task 4): Hello, version mismatch, reconcile, the order of
   `Spawn`, `Close`, `Attach`, buffered replay then `Replayed`, `Resize`; reconnect after `kykod`
   dies, with `holder_lost` and then reconcile from `Base::Current`. No frame for a pane is lost
   or applied to the wrong pane.
2. **Threads and locks** between the link thread, the GUI thread and Qt's render thread (Tasks
   4, 5, 6): no lock held across I/O, no GUI-thread wait on the socket.
3. **The key encoder** (Task 3) against xterm's tables, in each cursor and keypad mode.
4. **Focus on show** (Task 7): `exclusive` until focused, then `on-demand` (open risk 2), with
   real key presses.
5. **The `htop` repaint** (Task 1) without adding a timer to `kykod`.

## Decided here

- **`Esc` goes to the terminal.** The spec lists `Esc` among the ways to hide (§2.7) and the
  checklist repeats it, but `Esc` is a key every full-screen program needs (`vim`, `less`,
  `htop`). It goes to the pane; hiding is the toggle key and focus loss. Confirmed by the user
  on 2026-11-15; Task 10 changes §2.7 and the checklist to match.
- **What the window shows before milestone 2:** the active tab's panes at the rects `Session`
  gives, with no tab bar and no keys to switch tabs. A layout restored from an older window may
  hold several tabs; they keep running and come back with milestone 2's tab bar.
- **Live reload watches the folders, not the files,** because editors and uQ replace a file by
  renaming a new one over it, which ends an inotify watch on the old file.

---

### Task 1: `kykod` follow-ups: the `htop` repaint and the hop bench

**Files:**
- Modify: `apps/kykod/src/pane.rs`, `apps/kykod/src/pty.rs`
- Test: `apps/kykod/tests/attach.rs`
- Create: `docs/notes/2026-11-1x-m1e-checks.md` (the hop bench findings; date it on the day)

**The repaint.** A same-size `SIGWINCH` makes `vim`, `less`, `nano` and `top` redraw but not
`htop`, whose ncurses repaints only changed cells (m1c checks note). A real size change makes
ncurses repaint everything. Two ways to get one without a timer in `kykod`:

- **A (preferred): shrink on `Attach`, restore on the window's `Resize`.** When a window attaches
  a pane, `kykod` sets the terminal one row shorter (the kernel signals the change). The window's
  `Resize`, which always follows the replay, sets the real size (signalled again). The program
  repaints twice; the second is at the right size. The gap between the two is the window's own
  replay time, so there is no race to tune.
- **B: a row less and back, at once,** on the first same-size `Resize`. Two signals in a row may
  be merged into one by the kernel, after which ncurses sees no change.

**Steps:**
- [ ] **Step 1:** Live experiment with a throwaway harness (as slice 1c's), 80×24, 256 KiB ring:
      `htop -d 1`, `vim -u NONE`, `less`, `top`, `nano`, `tmux` and a shell at its prompt, each
      with A and with B. Record which repaint fully, and what the shell at a prompt does with
      the extra row (bash redraws its prompt line; check nothing is left behind). Choose A
      unless the experiment shows a problem with it.
- [ ] **Step 2:** Tests in `attach.rs`: after `Attach`, the PTY's size is one row less until the
      window's `Resize`; after it, the size is the window's. A second `Resize` of the same size
      sends nothing (as today). A pane whose shell has exited is not resized. An `Attach` without
      a following `Resize` (the window died) leaves the shorter size until the next window's
      `Resize`, which is harmless; the test documents it. A pane of 1 row is not shrunk.
- [ ] **Step 3:** Implement; replace the same-size nudge (`Pty::nudge`) if A makes it redundant,
      keeping its doc comment's reasoning in the new code. Update §2.8's replay paragraph in
      Task 10. Commit `fix(kykod): a real size change after attach, so ncurses repaints`.

**The hop bench** measured 1.10 to 1.13 on 2026-11-10 and 1.43 to 1.49 on 2026-11-14, on the
same code. It is a gate in the release checklist, so it must either be trusted again or
replaced.

- [ ] **Step 4:** At most half a day. On a quiet machine (load under 1, the CPU governor and
      energy preference written down): run the bench five times on `v0.0.2`, `v0.0.3` and
      `main`. Then profile one through-`kykod` run (`perf stat` for context switches, `perf
      record` on `kykod`) against the 2026-11-10 note's numbers. Look at the kernel version
      (`uname -r` was 7.3.x on 2026-11-15; the 11-10 note may name another) and PTY or
      scheduler changes.
- [ ] **Step 5:** Write the note. Either the cause is found and fixed (or is outside kyko and the
      bench is re-baselined with the reason), or the gate becomes relative: the bench compares
      `main` with the branch under test on the same machine and fails on a regression of more
      than 5%, instead of an absolute 15% over the direct read. Update the checklist item.
      Commit `docs: hop bench on the current machine`.

### Task 2: The Qt build, locally and in CI

**Files:**
- Modify: `apps/kyko/Cargo.toml`, `Cargo.toml` (workspace dependencies), `.github/workflows/ci.yml`,
  `packaging/kyko/PKGBUILD` (`makedepends`), `deny.toml` if a new licence appears
- Create: `apps/kyko/build.rs`, `apps/kyko/qml/Main.qml` (an empty window for now),
  `apps/kyko/cpp/grid.h`, `apps/kyko/cpp/grid.cpp` (an empty `QQuickItem`), `apps/kyko/shaders/`

**Steps:**
- [ ] **Step 1:** Development machine: `layer-shell-qt` is not installed (it was during the
      spike). Install it with `run0 pacman -S layer-shell-qt`. Record Qt's version.
- [ ] **Step 2:** `build.rs` after the spike's (`spike/m0:spike/build.rs`), cleaned up: find
      `moc` and `qsb` through `qmake6 -query` (`QT_HOST_LIBEXECS`, `QT_HOST_BINS`) rather than a
      list of paths, fail with one clear line naming the missing package, and mark every input
      with `rerun-if-changed`. The QML module is `org.kyko`. The spike's GCC 16 warning flag
      stays, with its comment.
- [ ] **Step 3:** `kyko` with no subcommand starts a `QGuiApplication` and loads `Main.qml`;
      `--version` and `--help` still work without a display (they run before Qt starts). Check
      that cxx-qt 0.10 builds against Qt 6.12; if not, find the newest cxx-qt that does and note
      it.
- [ ] **Step 4:** CI: the Arch container also installs `qt6-base qt6-declarative qt6-wayland
      qt6-shadertools layer-shell-qt` (and `gcc`, part of `base-devel`). The PKGBUILD's
      `makedepends` gains `qt6-shadertools`; its `depends` change is slice 1f's. Check the CI
      time: if the Qt build makes it slow, cache `~/.cargo` and `target` keyed by `Cargo.lock`.
- [ ] **Step 5:** Commit `build(kyko): cxx-qt, moc and qsb in build.rs and CI`.

### Task 3: Keys to bytes (`kyko-term::input`)

**Files:**
- Create: `crates/kyko-term/src/input.rs`
- Modify: `crates/kyko-term/src/lib.rs`, `crates/kyko-term/Cargo.toml` (`kyko-core` for `Key`
  and `Mods`, so there is one key type)
- Test: unit tests in `input.rs`

**Interfaces:**

```rust
/// A key press as the window has translated it: the key, its modifiers, and the text the
/// keyboard layout produced for it (empty for named keys and most Ctrl combinations).
pub struct KeyPress<'a> { pub key: Key, pub mods: Mods, pub text: &'a str }

/// The bytes a key press sends to the program, given the modes it has set. Empty when the
/// press sends nothing (a lone modifier, an F-key above F20 in xterm's tables).
pub fn encode_key(press: &KeyPress, modes: &TermModes) -> Vec<u8>;

/// Pasted text as the program should get it: in `ESC[200~ … ESC[201~` when it set bracketed
/// paste, with any `ESC[201~` inside the text removed so a paste cannot end the bracket early.
pub fn encode_paste(text: &str, modes: &TermModes) -> Vec<u8>;
```

- xterm's encoding (what `TERM=xterm-256color` promises): printable text as UTF-8; `Enter`
  `\r`; `Backspace` `\x7f` (`\x08` with Ctrl); `Tab` `\t`, `Shift+Tab` `ESC[Z`; `Esc` `\x1b`;
  arrows, `Home` and `End` as `ESC[A`… or `ESCOA`… in application cursor mode, and with
  modifiers as `ESC[1;<m>A` (m = 1 + Shift 1 + Alt 2 + Ctrl 4 + Super 8); `Insert`, `Delete`,
  `PgUp`, `PgDn` as `ESC[2~`, `ESC[3~`, `ESC[5~`, `ESC[6~` with `;<m>`; F1–F4 as `ESCOP`…,
  F5–F12 as `ESC[15~`… with modifiers; F13–F20 as xterm's shifted codes.
- `Ctrl` with a letter sends the control character; with `@ [ \ ] ^ _` and `Space` the
  corresponding C0 code; `Ctrl+?` `\x7f`. `Alt` prefixes `ESC` to what the key would send.
- The keypad in application keypad mode is the window's concern to report as keypad keys; in
  this slice the window maps keypad keys to their main-key equivalents (as `kyko-core::keys`
  says), so application keypad sequences are not sent. Recorded as a known gap for 1f.

**Steps:**
- [ ] **Step 1:** Tests from xterm's tables: every named key with each modifier combination, in
      normal and application cursor mode; Ctrl with each letter and each of `@[\]^_ ?` and
      Space; Alt with text and with Ctrl; non-ASCII text (`é`, `ß`, a CJK character, an emoji)
      unchanged; a lone modifier sends nothing; paste with and without bracketed paste, and a
      paste containing `ESC[201~`.
- [ ] **Step 2:** Implement. Commit `feat(term): keys and paste to bytes`.

### Task 4: The link to `kykod` (no Qt)

**Files:**
- Create: `apps/kyko/src/link.rs`, `apps/kyko/src/panes.rs`
- Test: `apps/kyko/tests/link.rs`, against a real `kykod` (reusing `apps/kykod/tests/common`'s
  holder by moving it to a shared `tests/support` path or a small dev-only crate, decided in
  Step 1)

**Interfaces:**

```rust
/// What the link tells the window, on the GUI thread (through cxx-qt's thread queue).
pub enum Event {
    Attached,                         // session reconciled and every pane attached
    Unavailable(String),              // "Sessions aren't available": the socket error
    VersionMismatch { holder: u32 },  // §2.8's notice
    PaneDirty(PaneId),                // new output was fed; coalesced
    SessionChanged,                   // Info, Exited, SpawnFailed changed the session
}

pub struct Link { /* the writer half, the session behind a mutex, the pane table */ }

impl Link {
    /// Starts the reader thread. `socket` is `$XDG_RUNTIME_DIR/kyko/hold.sock`.
    pub fn start(socket: PathBuf, settings: LinkSettings, notify: impl Fn(Event) + Send + 'static)
        -> Link;
    pub fn input(&self, pane: PaneId, bytes: &[u8]);
    pub fn resize(&self, pane: PaneId, cols: u16, rows: u16);
    pub fn new_tab(&self, req: SpawnRequest);        // for D-Bus later; used by tests now
    pub fn session(&self) -> MutexGuard<'_, Session>;
    pub fn pane(&self, pane: PaneId) -> Option<Arc<PaneHandle>>; // emulator + dirty flag
}
```

- **Connect:** `Hello{VERSION}`. On a different version: `VersionMismatch`, no attach, retry
  only when the window restarts (the old `kykod` keeps running; §2.8).
- **Reconcile:** `Session::reconcile(Base::Blob(layout), panes, home, shell)` on the first
  connection; `Base::Current(session)` after a reconnect, having called `holder_lost` when the
  connection dropped. Carry out the effects in order (`Close`, then `Spawn`, then nothing for
  `Hide` at start-up), then `Attach` every running pane.
- **Replay:** `Output` after `Attach` and before `Replayed` is buffered per pane, then given to
  `Emulator::replay` whole; then `Resize` with the pane's current size (which, with Task 1's
  A, is the second of the two size changes). Live `Output` after that is fed directly.
- **Spawn:** shell `terminal.shell`, else `$SHELL`, else `/bin/sh`; args from the effect; cwd
  from the effect or home; env `TERM=xterm-256color`, `COLORTERM=truecolor`; size from the
  pane's grid; `ring_bytes` from `sessions.ring_kib`.
- **Exited / SpawnFailed / Info:** into the session (`pane_exited`, `spawn_failed`,
  `apply_info`); carry out the effects; `SessionChanged`.
- **SaveLayout** after every change to the session, at most once per GUI event (the link sends
  the latest blob when the GUI thread asks, after handling a batch).
- **Reconnect:** when the socket closes, `holder_lost`, `Unavailable`, and a retry every 2 s (a
  timer only while disconnected). The panes show "Session ended" (§2.7).
- **Replies:** after each feed, `Emulator::take_replies` goes back to `kykod` as `Input`.

**Steps:**
- [ ] **Step 1:** Decide where the test holder lives (move `apps/kykod/tests/common` to a
      `kyko-testsupport` dev-only crate, or start `kykod` from `apps/kyko/tests` by path). Tests:
      a fresh `kykod` gives one tab and its shell's prompt; restart the link (a new window) and
      the tab returns with its output replayed; kill `kykod` and the panes become `Ended`, the
      link reports `Unavailable` and reconnects to a new `kykod`, reconciling from `Current`; a
      held pane whose command exits is closed in `kykod` (`Hello` no longer lists it); a
      version mismatch (a fake holder answering `Hello{1}`) attaches nothing; replies to a
      `CSI 6n` query reach the shell; 64 KiB chunks: a 20 MB flood does not hold a pane's lock
      for more than one chunk (measured by a second thread taking snapshots).
- [ ] **Step 2:** Implement. Commit `feat(kyko): the link to kykod`.

### Task 5: The grid item, first part

**Files:**
- Create: `apps/kyko/cpp/grid.h`, `apps/kyko/cpp/grid.cpp`, `apps/kyko/shaders/glyph.vert`,
  `apps/kyko/shaders/glyph.frag`, `apps/kyko/src/grid.rs` (the cxx bridge: snapshot rows,
  cell metrics), `apps/kyko/src/metrics.rs` (no Qt: cell size, grid size, row diffing)
- Test: unit tests in `metrics.rs`

What this slice builds of §2.4, and what it leaves to 1f:

| Built here | Slice 1f |
|---|---|
| Coverage atlas, one page, rasterised at the device pixel ratio, quads snapped to device pixels | Colour emoji atlas |
| Bold, italic, underline, strikethrough, inverse, 256 and true colour, wide characters | The 2 ms rasterisation budget per frame |
| Font fallback through Qt when a glyph is rasterised | Page growth and 10-minute eviction |
| Only changed rows rebuilt (per-row geometry nodes, rows compared with the last frame) | Reference-image tests offscreen |
| The cursor in the shape the program set (block, beam, underline, hollow when unfocused) | Selection, search highlights, hyperlinks |
| Padding and line height from config | Mouse |

- **Cell metrics** come from the resolved font (`Appearance`) through `QFontMetricsF`: cell width
  is the advance of `M`, height is the line spacing times `terminal.line_height`, both rounded
  to device pixels. `metrics.rs` turns an item size and padding into columns and rows and back,
  and is unit tested.
- **A full atlas** in this slice: when the one page is full, it is cleared and rebuilt from the
  visible screen, logged once. Growth replaces this in 1f.
- **The snapshot** is taken in `updatePaintNode` (render thread, GUI blocked) under the pane's
  lock, as the spike did. Rows equal to last frame's are not rebuilt.

**Steps:**
- [ ] **Step 1:** `metrics.rs` tests: grid size from item size, padding and cell size at scales
      1, 1.25, 1.5 and 2; a size below one cell gives 2×1 (the emulator's minimum); row diffing.
- [ ] **Step 2:** The item, from the spike's `sgitem.cpp` (`spike/m0`), rewritten to the table
      above. A `kyko --render-test <file>` mode (hidden) feeds a recorded byte stream into one
      pane with no `kykod` and saves a screenshot after it settles; used by Task 9's smoke test
      and, in 1f, by the reference-image tests.
- [ ] **Step 3:** Check by eye against the spike's screenshots: `ls --color`, `htop`, `vim`, the
      spike's `gen.py` dense screen, CJK, box drawing; at scale 1 and 1.5 (KWin's per-output
      scale). Record in the checks note.
- [ ] **Step 4:** Commit `feat(kyko): the grid item draws a pane`.

### Task 6: Input in the grid item

**Files:**
- Modify: `apps/kyko/cpp/grid.cpp`, `apps/kyko/src/grid.rs`

- `keyPressEvent`: Qt's key and modifiers to `kyko_core::keys::{Key, Mods}` (keypad keys mapped
  to the main keys; see Task 3), the event's text, then `encode_key` with the pane's modes, then
  `Link::input`. Keys the window keeps for itself in this slice: none (milestone 2 adds the
  `[keys]` actions; the toggle hotkey is the compositor's).
- Input methods: `inputMethodQuery` (cursor rectangle at the cursor cell, so the candidate
  window sits there) and `inputMethodEvent` (the commit string is sent as text; the preedit
  string is drawn at the cursor, underlined, without being sent).
- Paste with `Ctrl+Shift+V` is milestone 2's, so in this slice no paste exists; `encode_paste`
  is wired when it comes.
- Focus: the item takes active focus when the window shows; focus in and out send
  `ESC[I` / `ESC[O` when the program enabled focus reporting.

**Steps:**
- [ ] **Step 1:** Live checks with real key presses on KWin (open risk 3): bash line editing,
      `vim` (insert, `Esc`, arrows in both cursor modes), `htop` (F-keys), `less`, `nano`;
      Ctrl+C during `yes` stops it promptly with the window drawing the flood (responsive during
      a flood); a dead-key accent and an IBus or fcitx5 input method if one is installed.
      Record in the checks note.
- [ ] **Step 2:** Commit `feat(kyko): keyboard and input methods`.

### Task 7: The drop-down window

**Files:**
- Modify: `apps/kyko/qml/Main.qml`, `apps/kyko/src/app.rs`

- A `Window` with `org.kde.layershell` attached properties (as the spike): layer `overlay`
  (`top` when `window.above_fullscreen = false`), anchored to the top, namespace `kyko`,
  on the focused output (`window.monitor`: `focused` uses the compositor's choice by leaving the
  output unset; `primary` and a named output select a `QScreen`).
- **Shapes** (§2.2): island and sheet from `window.shape`, sizes from `height_pct` and
  `width_pct`, corners, border and gap from the theme's metrics and accent, opacity from
  `window.opacity`.
- **Slide:** 150 ms, the content moving down from above the top edge; instant with
  `animations = false`. Hiding unmaps the surface after the slide.
- **Focus:** keyboard interactivity `exclusive` while showing, switched to `on-demand` once the
  window has active focus; focus loss hides unless pinned (`hide_on_focus_loss`; the pin button
  is milestone 2's, so in this slice the setting alone decides).
- **No layer shell** (the `offscreen` platform in tests, GNOME): a plain undecorated window,
  logged once. The full GNOME behaviour is slice 1f's.
- **Panes:** one grid item per pane of the active tab, placed at its `Rect` in the content area;
  "Session ended", a held pane's exit status, and a failed spawn's message are drawn over the
  pane; the `Unavailable` and version-mismatch notices cover the content area.

**Steps:**
- [ ] **Step 1:** Live checks on KWin (open risk 2): show over a fullscreen window and type at
      once with no click; hide returns focus to the window that had it; island and sheet; top
      and `above_fullscreen = false`; 1.5 scale; `animations = false`; focus loss hides.
      Record in the checks note, with screenshots.
- [ ] **Step 2:** Commit `feat(kyko): the drop-down window`.

### Task 8: D-Bus, the command line, and live config

**Files:**
- Create: `apps/kyko/src/dbus.rs`, `apps/kyko/src/watch.rs`
- Modify: `apps/kyko/src/main.rs`, `apps/kyko/src/app.rs`
- Test: `apps/kyko/tests/dbus.rs` (a private `dbus-daemon --session`, the window side replaced by
  a fake that records calls), unit tests in `watch.rs`

- **`org.kyko.Terminal1` at `/org/kyko/Terminal1`**, this slice's members: `Toggle`, `Show`,
  `Hide`, `Quit`, property `Visible` and signal `VisibleChanged`. The rest of §3.1 (`Run`,
  `NewTab`, `ListTabs`, `FocusTab`, `EndSessions`, the settings properties) is milestone 2 to 4.
  zbus's blocking connection on its own thread; calls reach the GUI thread through the thread
  queue.
- **Single instance** (§4): the window requests the name; when it is taken, it calls `Show` on
  the owner and exits 0.
- **CLI:** `kyko toggle | show | hide | quit` call the running window and exit 0; with no owner
  (activation files are slice 1f's) they print "kyko is not running" and exit 1.
- **Live config** (§2.6): `watch.rs` watches `config.toml`'s and `tokens.json`'s folders with
  inotify (created on demand if missing; a missing folder is watched from its nearest existing
  parent) and reports a change to either file by name. On a config change: `Config::load`; on
  `Err` keep the last config and log the error once; apply `Config::changes` (font changes
  rebuild the atlas and re-grid every pane, sending `Resize`). On a tokens change or uQ's
  `ThemeChanged` signal on `org.uq.Desktop1`: `Theme::from_tokens`, keeping the last on `Err`,
  then `resolve`. Running panes recolour on the next frame (the snapshot resolves colours).
  Each distinct warning is logged once.

**Steps:**
- [ ] **Step 1:** Tests: `watch.rs` against a temporary folder (write, rename over, delete and
      recreate, the folder created after the watch starts); D-Bus: `Toggle` twice, `Show` while
      shown, `Quit`, the second instance calling `Show` and exiting 0, the CLI with no window.
- [ ] **Step 2:** Implement. Live check: switching uQ's theme (or rewriting a `tokens.json` by
      hand) recolours a running `htop`. Commit `feat(kyko): D-Bus, the command line and live
      config`.

### Task 9: The window's smoke test in CI

**Files:**
- Create: `apps/kyko/tests/smoke.rs`

- With `QT_QPA_PLATFORM=offscreen` and a private bus: start `kykod` on a temporary socket and
  `kyko` against it; `kyko show`; write `echo kyko-smoke` to the pane through a test hook
  (a hidden `--test-input` D-Bus method compiled only with a `test-hooks` feature, so release
  builds do not carry it); wait for the text in the pane's emulator (read back through the same
  hook); `kyko quit`; the shell is still running in `kykod`; start `kyko` again and the output
  is replayed.
- `kyko --render-test` on a recorded stream produces a non-empty image (its contents are 1f's
  reference tests).

**Steps:**
- [ ] **Step 1:** Write the test; run it in CI's container. Commit
      `test(kyko): the window starts, attaches and survives a restart`.

### Task 10: Docs, spec, release

**Files:**
- Modify: `docs/superpowers/specs/2026-11-10-kyko-design.md`, `CHANGELOG.md`, `README.md`,
  `docs/notes/2026-11-13-v1-release-checklist.md`, the checks note

- [ ] **Step 1:** Spec: §2.8 the attach resize (Task 1); §2.7 `Esc` goes to the terminal, not
      a way to hide; §2.2 the undecorated fallback as built; §2.6 live reload watching folders;
      the hop bench gate if Task 1 changed it.
- [ ] **Step 2:** Checklist: reword "Hide on focus loss unless pinned; `Esc` hides" to drop
      `Esc`. Tick the cxx-qt build, link to `kykod`, live reload, the grid
      item's first part (split the checklist item into this slice's half and 1f's), keyboard and
      input methods, the drop-down window's KWin half, `kyko toggle`/`show`/`hide` and the single
      instance rule, the window side of reconcile and the version mismatch, the `htop` item and
      the hop bench item. Open risks 2 and 3 move to "checked on KWin", still open on Hyprland
      (risk 1 stays open for the VM).
- [ ] **Step 3:** CHANGELOG `[Unreleased]` → `0.0.5`: the window, how to try it (build, run
      `kykod`, run `kyko`, bind `kyko toggle`); README the same in two lines. Then
      `docs/releasing.md`'s steps for `0.0.5`.
- [ ] **Step 4:** Full `cargo test --workspace`, clippy, fmt, the shell tests, the smoke test.

## Not in this slice (slice 1f, then `v0.1.0`)

- The renderer's unbuilt parts: the colour emoji atlas, the 2 ms rasterisation budget, atlas
  growth and eviction; reference-image tests offscreen at scale 1 and 1.5 (open risk 4).
- Mouse: selection, word and line selection, reporting to programs, wheel scrollback, Ctrl+click
  on URLs through the portal.
- Application keypad sequences.
- The GNOME fallback in full and Qt's software-backend fallback, each noted in Preferences later.
- Packaging: `depends`, `kyko.service` (hardened), the D-Bus service file for activation,
  `org.kyko.Kyko.desktop`, the icon.
- The uQ VM run on Hyprland (open risk 1), with the window as built here.
- Each modern protocol in §2.4 confirmed by a test or removed from the spec.
- `v0.1.0`.

Milestone 2 and later, unchanged: the tab bar, splits and their keys, copy and paste, search,
close confirmations, the Preferences window, the hotkey backends, `Run` and Spotlight.
