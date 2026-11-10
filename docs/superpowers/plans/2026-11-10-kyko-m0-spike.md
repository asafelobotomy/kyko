# kyko Milestone 0 (spike) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Find out, with numbers, whether a terminal grid drawn in a Qt Quick item from Rust is fast and correct enough, and whether a `cxx-qt` app can be a well-behaved layer-shell drop-down, before any real kyko code is written.

**Architecture:** One throwaway binary, `kyko-spike`, in its own Cargo workspace under `spike/`. A Qt-free Rust module runs a shell on a PTY through `alacritty_terminal` and hands out grid snapshots; a C++ `QQuickPaintedItem` paints them; a QML `Window` uses the `org.kde.layershell` module to sit on the top layer. No `kykod`, no D-Bus, no tabs.

**Tech Stack:** Rust stable (edition 2024), `cxx-qt` 0.10 / `cxx-qt-build` 0.10 / `cxx-qt-lib` 0.10, `alacritty_terminal` 0.26.0, Qt 6.11 (Gui, Quick), `layer-shell-qt` 6.7.5 (QML module `org.kde.layershell`), `rustix` for signals and the PTY where needed.

**Spec:** `docs/superpowers/specs/2026-11-10-kyko-design.md` (§6 defines this milestone; §2.2, §2.4, §2.5 give the window, terminal and theme values used here).

## Global Constraints

- Everything lives under `spike/` and is throwaway: no later milestone imports from it, and its code is not reviewed for style, only for whether the measurements are honest.
- `spike/` is its own workspace (`spike/Cargo.toml` with `[workspace]`), so the future root workspace is unaffected.
- Wayland only; needs `wlr-layer-shell`. The development machine runs Plasma (KWin, Wayland); Hyprland checks are recorded as "not run" unless a Hyprland session is available.
- The spike reads the PTY directly. There is no holder process.
- Targets, copied from the spec: a full 200×50 repaint in under 4 ms; a 100,000-line `cat` that never blocks input.
- Window values from the spec: top layer, anchored to the top edge, keyboard interactivity on-demand, namespace (`scope`) `kyko`, island shape at 88% of monitor width, height 45% of the monitor, radius and border from the theme, slide over 150 ms.
- Shell: `$SHELL`, falling back to `/bin/sh`; `TERM=xterm-256color`, `COLORTERM=truecolor`.
- Theme: `~/.local/state/uq/theme/tokens.json` if present, otherwise the built-in palette in Task 2. No live reload in the spike.
- `alacritty_terminal` 0.26.0 is pinned exactly (`=0.26.0`). Read its docs.rs pages for that version before using a type; names in this plan describe roles, not guaranteed paths.
- License MIT. Commit messages end with the project's attribution line.
- Build and run in the foreground; leave nothing running when a task is reported done.

## Review Focus

1. **Bytes that are not valid UTF-8 from the shell** (`printf '\xff\xfe'`): the engine must keep running and show replacement characters, not panic. Test in Task 2.
2. **The shell exits** (`exit`, or the child is killed): the engine reports it and the window says so instead of freezing or spinning a core. Test in Task 2; visible check in Task 3.
3. **Resize to a tiny or zero size** (the surface is unmapped, or mid-animation): no panic, no 0-column PTY; the engine clamps to at least 2×1. Test in Task 2.
4. **A flood of output while the user types** (`yes`, a 100,000-line `cat`): the UI thread must not starve; `Ctrl+C` lands within about 100 ms. Measured in Task 4.
5. **The chosen font is missing or lacks a glyph** (CJK, emoji, box drawing): cells stay aligned to the grid whatever Qt substitutes. Visible check in Task 4.

---

### Task 1: A layer-shell window from `cxx-qt`

**Files:**
- Create: `spike/Cargo.toml`, `spike/build.rs`, `spike/src/main.rs`, `spike/src/toggle.rs`, `spike/qml/Main.qml`, `spike/.gitignore` (`target/`)

**Interfaces:**
- Produces: binary `kyko-spike`. `kyko-spike` (no arguments) starts the window process and writes its pid to `$XDG_RUNTIME_DIR/kyko-spike.pid`; `kyko-spike toggle` sends `SIGUSR1` to that pid and exits 0, or exits 1 with "kyko-spike is not running".
- Produces: QML root `Window` with `id: root` and a `property bool shown`; `toggle.rs` exposes a `QObject` named `Toggler` (QML element, module `org.kyko.spike`) with signal `toggled()` emitted on the Qt thread for each `SIGUSR1`.

Build setup follows `~/Projects/uq/apps/uq-settings/build.rs`: `CxxQtBuilder::new_qml_module(QmlModule::new("org.kyko.spike").qml_files([...]))`, `.qt_module("Gui")`, `.qt_module("Quick")`, the same `-Wno-sfinae-incomplete` flag.

- [ ] **Step 1: Scaffold the crate and a plain `Window`** with a solid rectangle in `#282828`, and confirm it builds and shows.

Run: `cargo run --manifest-path spike/Cargo.toml`
Expected: an ordinary window appears; closing it exits 0.

- [ ] **Step 2: Make it a layer surface.** In `Main.qml`, `import org.kde.layershell as LayerShell` and set on the `Window`: `LayerShell.Window.layer: LayerShell.Window.LayerTop`, `LayerShell.Window.anchors: LayerShell.Window.AnchorTop`, `LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityOnDemand`, `LayerShell.Window.scope: "kyko"`, `LayerShell.Window.exclusionZone: -1`. Width `Screen.width * 0.88`, height `Screen.height * 0.45`, `color: "transparent"`, the rectangle with radius 10 and a 2 px `#d79921` border. The exact enum spellings are in `/usr/lib/qt6/qml/org/kde/layershell/LayerShellQtQml.qmltypes`.

Run: `cargo run --manifest-path spike/Cargo.toml`
Expected: a borderless panel centred at the top of the active monitor, above normal windows, not in the task switcher.

- [ ] **Step 3: Toggle.** Implement the pid file, the `toggle` subcommand and `Toggler` (a signal-handling thread that queues `toggled()` to the Qt thread). In QML, `toggled` flips `shown`; `shown` drives `visible` and a 150 ms `y`/margin slide (`LayerShell.Window.margins.top` from `-height` to `12`).

Run, in two terminals: `cargo run --manifest-path spike/Cargo.toml` then `spike/target/debug/kyko-spike toggle` three times.
Expected: hide, show, hide, each sliding; the process stays alive; `kyko-spike toggle` with no window process prints the error and exits 1.

- [ ] **Step 4: Record the layer-shell observations** in `spike/NOTES.md` under "Layer shell (KWin)": does it take keyboard focus when shown, does focus return to the previous window when hidden, does it follow the active monitor, does unmap and remap keep QML state (add a counter `Text` that increments on a `Timer` to see), any warnings on stderr. Write what happened, including failures.

- [ ] **Step 5: Commit**

```bash
git add spike && git commit -m "spike: layer-shell window with toggle"
```

### Task 2: Terminal engine without Qt

**Files:**
- Create: `spike/src/engine.rs`, `spike/src/theme.rs`
- Test: inline `#[cfg(test)]` modules in both files

**Interfaces:**
- Produces (`engine.rs`):
  - `pub struct Cell { pub ch: char, pub fg: u32, pub bg: u32, pub flags: u8 }` with flag bits `BOLD = 1`, `ITALIC = 2`, `UNDERLINE = 4`, `INVERSE = 8`, `WIDE = 16`, `WIDE_SPACER = 32`; colours are `0xRRGGBB`, already resolved through the palette.
  - `pub struct Snapshot { pub cols: u16, pub rows: u16, pub cells: Vec<Cell>, pub cursor: Option<(u16, u16)>, pub seq: u64 }` (`cells` is row-major, `cols * rows` long; `seq` increases whenever the screen changed).
  - `pub struct Engine`; `Engine::spawn(shell: &str, cols: u16, rows: u16, palette: Palette, on_dirty: Box<dyn Fn() + Send>) -> io::Result<Engine>`; `fn snapshot(&self) -> Snapshot`; `fn write(&self, bytes: &[u8])`; `fn resize(&self, cols: u16, rows: u16)` (clamps to at least 2×1); `fn exited(&self) -> Option<i32>`.
  - A reader thread owns the PTY read side, feeds `alacritty_terminal`, and calls `on_dirty` at most once per pending snapshot (coalesced), never per byte.
- Produces (`theme.rs`): `pub struct Palette { pub fg: u32, pub bg: u32, pub cursor: u32, pub accent: u32, pub ansi: [u32; 16], pub font: String, pub font_size: u16, pub radius: u16, pub border: u16 }`; `Palette::builtin()` (Gruvbox dark: bg `#282828`, fg `#ebdbb2`, accent `#d79921`, the 16 ANSI values from `~/Projects/uq/themes/gruvbox/dark.toml`, font `JetBrains Mono` 11, radius 10, border 2); `Palette::from_tokens(json: &str) -> Option<Palette>` mapping `colors.bg`, `colors.text`, `colors.accent`, `terminal.ansi`, `fonts.mono`, `fonts.size`, `ui.radius`, `ui.border_width`; `Palette::load() -> Palette` reads the tokens file and falls back to `builtin()`.

- [ ] **Step 1: Write the failing tests**

```rust
// theme.rs
#[test] fn builtin_is_gruvbox_dark() {
    let p = Palette::builtin();
    assert_eq!((p.bg, p.fg, p.accent), (0x282828, 0xebdbb2, 0xd79921));
    assert_eq!(p.ansi[1], 0xcc241d);
}
#[test] fn tokens_parse_and_bad_json_is_none() {
    let j = r##"{"colors":{"bg":"#101010","text":"#eeeeee","accent":"#ff0000"},
      "ui":{"radius":4,"border_width":1},"fonts":{"mono":"Hack","size":12},
      "terminal":{"ansi":["#000000","#111111","#222222","#333333","#444444","#555555","#666666","#777777",
                          "#888888","#999999","#aaaaaa","#bbbbbb","#cccccc","#dddddd","#eeeeee","#ffffff"]}}"##;
    let p = Palette::from_tokens(j).unwrap();
    assert_eq!((p.bg, p.font.as_str(), p.font_size, p.ansi[15]), (0x101010, "Hack", 12, 0xffffff));
    assert!(Palette::from_tokens("{").is_none());
}

// engine.rs — helper: run(script) spawns `/bin/sh -c script` at 80x24 with Palette::builtin(),
// waits (up to 5 s) for exited().is_some(), returns the final Snapshot. row_text(&snap, r) joins a row, trimmed.
#[test] fn prints_text() { assert_eq!(row_text(&run("printf 'hi there\\n'"), 0), "hi there"); }
#[test] fn ansi_red_resolves_through_palette() {
    let s = run("printf '\\033[31mX\\033[0m'");
    assert_eq!(s.cells[0].ch, 'X'); assert_eq!(s.cells[0].fg, 0xcc241d);
}
#[test] fn truecolor_passes_through() {
    assert_eq!(run("printf '\\033[38;2;1;2;3mX'").cells[0].fg, 0x010203);
}
#[test] fn wide_char_takes_two_cells() {
    let s = run("printf '日x'");
    assert!(s.cells[0].flags & WIDE != 0 && s.cells[1].flags & WIDE_SPACER != 0);
    assert_eq!(s.cells[2].ch, 'x');
}
#[test] fn invalid_utf8_does_not_kill_the_engine() {
    let s = run("printf '\\377\\376ok'");
    assert!(row_text(&s, 0).ends_with("ok"));
}
#[test] fn exit_status_is_reported() {
    // run() returns only after exit; spawn directly here and poll exited()
    assert_eq!(spawn_and_wait("exit 7"), Some(7));
}
#[test] fn resize_clamps_and_survives_zero() {
    let e = spawn_cat();               // `/bin/sh -c cat`, stays alive
    e.resize(0, 0); let s = e.snapshot();
    assert_eq!((s.cols, s.rows), (2, 1));
    e.resize(120, 40); assert_eq!(e.snapshot().cols, 120);
}
#[test] fn input_reaches_the_shell() {
    let e = spawn_cat(); e.write(b"echo-me\n");
    assert!(wait_for(&e, |s| row_text(s, 1) == "echo-me"));   // cat echoes the line back on row 1
}
#[test] fn on_dirty_is_coalesced() {
    // count on_dirty calls while running `seq 1 20000`; snapshot() is taken only after each call.
    assert!(dirty_calls_for("seq 1 20000") < 2000);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path spike/Cargo.toml`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement `theme.rs` and `engine.rs`** to the interfaces above. The engine holds the `alacritty_terminal` terminal behind a mutex; the reader thread reads up to 64 KiB at a time, advances the parser, sets a dirty flag and calls `on_dirty` only on the flag's false-to-true edge; `snapshot()` clears the flag. Set `TERM` and `COLORTERM` as in Global Constraints.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --manifest-path spike/Cargo.toml`
Expected: all tests pass, twice in a row (the PTY tests must not be flaky).

- [ ] **Step 5: Commit**

```bash
git add spike && git commit -m "spike: PTY and emulation engine with grid snapshots"
```

### Task 3: Draw the grid and type into it

**Files:**
- Create: `spike/cpp/griditem.h`, `spike/cpp/griditem.cpp`, `spike/src/bridge.rs`
- Modify: `spike/build.rs` (add `.cpp_file("cpp/griditem.cpp")`, `bridge.rs`), `spike/qml/Main.qml`, `spike/src/main.rs`

**Interfaces:**
- Consumes: `Engine`, `Snapshot`, `Cell`, flag bits, `Palette` from Task 2; `root.shown` from Task 1.
- Produces (`bridge.rs`, a `#[cxx::bridge]`): shared struct `FfiCell { ch: u32, fg: u32, bg: u32, flags: u8 }`; `fn spike_start(cols: u16, rows: u16)`, `fn spike_snapshot(out: &mut Vec<FfiCell>, cols: &mut u16, rows: &mut u16, cur_x: &mut i32, cur_y: &mut i32) -> u64` (returns `seq`; cursor is -1 when hidden), `fn spike_write(bytes: &[u8])`, `fn spike_resize(cols: u16, rows: u16)`, `fn spike_exit_status() -> i32` (-1 while running), `fn spike_palette_*()` getters for bg, fg, cursor, font and size. One global engine; `on_dirty` posts a queued call to `GridItem::requestRepaint()`.
- Produces (C++): `class GridItem : public QQuickPaintedItem`, registered as QML element `GridItem` in `org.kyko.spike`, with read-only properties `cellWidth`, `cellHeight`, `cols`, `rows`.

- [ ] **Step 1: Paint.** `GridItem::paint(QPainter*)` fetches the snapshot, fills the background, then draws row by row: one `fillRect` per run of equal background, one `drawText` per run of equal foreground and style, each cell placed at `col * cellWidth` (never by text advance, so substituted glyphs stay on the grid), wide cells spanning two. Draw the cursor as an inverse block. Cell size comes from `QFontMetricsF` of the palette font at the palette size. `geometryChange` computes `cols`/`rows` and calls `spike_resize`. Put a `GridItem` inside the rounded rectangle in `Main.qml` with 8 px padding.

Run: `cargo run --manifest-path spike/Cargo.toml`
Expected: the panel shows a shell prompt in the palette colours.

- [ ] **Step 2: Type.** `keyPressEvent` turns keys into bytes and calls `spike_write`: printable text as UTF-8, Enter `\r`, Backspace `0x7f`, Tab, Esc, arrows and Home/End/PgUp/PgDn/Delete as xterm sequences, `Ctrl+letter` as the control byte, `Alt+key` as `ESC` prefix. `inputMethodEvent` writes the commit string and `inputMethodQuery` reports the cursor rectangle; set `ItemAcceptsInputMethod`. The item takes focus when `root.shown` becomes true.

Run the binary and, in it: `ls --color`, `vim` (insert text, `:q!`), `htop` (`q`), `Ctrl+C` on `sleep 100`, arrow-key history.
Expected: each behaves as in kitty. Note every difference in `spike/NOTES.md` under "Input and rendering".

- [ ] **Step 3: Shell exit.** When `spike_exit_status() != -1`, paint a centred line `Shell exited (status N)` and stop requesting repaints.

Run the binary, type `exit 3`.
Expected: the line appears with status 3; CPU use drops to idle (`top` shows the process near 0%).

- [ ] **Step 4: Toggle keeps the session.** Start `sleep 5; echo done`, toggle to hide, wait, toggle to show.

Expected: `done` is on screen; record in `spike/NOTES.md` whether output during the hidden period was drawn correctly on remap.

- [ ] **Step 5: Commit**

```bash
git add spike && git commit -m "spike: painted grid item with keyboard and input-method input"
```

### Task 4: Measure

**Files:**
- Modify: `spike/cpp/griditem.cpp` (timing), `spike/src/bridge.rs` (`fn spike_stats_enabled() -> bool`)
- Create: `spike/bench.sh`

**Interfaces:**
- Consumes: `GridItem::paint` from Task 3.
- Produces: with `KYKO_SPIKE_STATS=1`, the process prints one line on exit: `paint_ms n=<count> p50=<x> p95=<x> max=<x> cols=<c> rows=<r>`, timing each `paint()` with `QElapsedTimer`; and with `KYKO_SPIKE_FULL=1` every paint redraws all cells (no reliance on unchanged regions).

- [ ] **Step 1: Add the timing and the two environment switches**, build in release.

Run: `cargo build --release --manifest-path spike/Cargo.toml`
Expected: builds; running with `KYKO_SPIKE_STATS=1` and exiting the shell prints the `paint_ms` line.

- [ ] **Step 2: Write `spike/bench.sh`**, which prints the commands to paste into the spike window and what to record:
  1. Full repaint: resize so the stats line shows 200×50 (or note the largest grid the monitor allows), run `KYKO_SPIKE_FULL=1`, then `cat` a prepared file of 50 rows × 200 mixed-colour columns 200 times. Record p50, p95, max.
  2. Flood: `time seq 1 100000`, and the same in kitty for comparison. Record both wall times.
  3. Input under flood: run `yes`, press `Ctrl+C`, estimate the delay (under or over about 100 ms); repeat five times.
  4. Glyph fallback: `printf '日本語 ▛▜ ├─┤ 😀 é\n'` and a `tree` listing; check columns stay aligned.
  5. Input method, if `fcitx5` or `ibus` is running: compose one CJK word. Otherwise record "not tested: no input method running".

- [ ] **Step 3: Run the bench on KWin** and write every number and observation into `spike/NOTES.md` under "Measurements (KWin, release build)", with the machine's CPU, monitor resolution and Qt scene-graph backend (`QSG_INFO=1` output).

- [ ] **Step 4: If the full-repaint p95 is 4 ms or more**, try, in this order, and re-measure after each: (a) `setRenderTarget(QQuickPaintedItem::FramebufferObject)`; (b) repaint only rows whose cells changed since the last `seq` (keep the previous snapshot); (c) cache each row as a `QStaticText`. Record each result. Stop at the first that meets the target, or after (c).

- [ ] **Step 5: Hyprland.** If a Hyprland session is available, repeat Task 1 Step 4 and Task 4 Steps 2–3 there under "Hyprland" headings. If not, write "Hyprland: not run" and list exactly what remains unverified.

- [ ] **Step 6: Commit**

```bash
git add spike && git commit -m "spike: paint timing and bench results"
```

### Task 5: Report and decision

**Files:**
- Create: `docs/notes/2026-11-10-m0-spike-result.md`

- [ ] **Step 1: Write the result**, from `spike/NOTES.md`, with these headings: **Verdict** (go, or revise the spec, one sentence each for the grid item and for layer shell); **Numbers** (a table: target, measured, pass or fail, for the 4 ms repaint and the flood; plus the kitty comparison); **What worked**; **What didn't** (every difference from kitty, every stderr warning, every Review Focus item's outcome); **Not verified** (Hyprland, input method, anything skipped, said plainly); **What milestone 1 should keep and drop** (which painting approach, what the bridge taught, nothing from the spike's code is carried over).

- [ ] **Step 2: If either risk failed**, add a **Proposed spec change** section: for the grid item, the spec's stated fallback (`alacritty_terminal` with `wgpu` in a plain layer surface, QML out of the terminal path); for layer shell, what went wrong and the alternative tried. Do not edit the spec; the user decides.

- [ ] **Step 3: Commit**

```bash
git add docs/notes && git commit -m "docs: milestone 0 spike result"
```

- [ ] **Step 4: Stop and hand the result to the user** for the go or revise decision. Milestone 1 is planned only after that.
