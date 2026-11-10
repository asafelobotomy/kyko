# kyko spike notes

Machine: Manjaro, Plasma (KWin, Wayland), Qt 6.11.2, layer-shell-qt 6.7.5, cxx-qt 0.10.0.

## Layer shell (KWin)

Checked 2026-11-10 by running the debug build and taking desktop screenshots.

- **Works from QML alone.** `import org.kde.layershell` with attached `LayerShell.Window.*` properties on a
  plain `Window`; no C++ and no extra link step. The panel is borderless, centred at the top (anchor top
  only), above normal windows, with nothing on stderr.
- **Slide:** `LayerShell.Window.margins` is a `QMargins`, which QML can't build or animate. The spike keeps
  the surface fixed and transparent and animates the panel's `y` inside it instead. That works; the cost is
  a transparent strip that still takes pointer input while shown (not investigated).
- **Keyboard focus on first map:** with `KeyboardInteractivityOnDemand`, the surface is NOT focused when
  first mapped (`Window.active` false).
- **Keyboard focus on re-show:** after `visible = true` plus `requestActivate()`, `Window.active` is true.
  Whether keys really arrive is tested in Task 3/4.
- **Unmap and remap keeps QML state:** a 1 s counter kept counting while hidden and showed the right value
  after re-show.
- **Toggle:** `kyko-spike toggle` (SIGUSR1) hid, showed and hid again; the process stayed alive; with no
  window process it prints "kyko-spike is not running" and exits 1.
- **Not checked yet:** which monitor it picks with several monitors (one monitor here), focus returning to
  the previous window on hide, behaviour over fullscreen windows (Task 4).

## Input and rendering (KWin, debug build)

Checked 2026-11-10 from screenshots, with input injected into the PTY (`KYKO_SPIKE_TYPE`), not typed.

- **Renders correctly:** a bash prompt; `ls --color`; bold, italic, underline, inverse, truecolor and
  background colours; all 16 ANSI colours; CJK (double width), box drawing, block elements, an emoji
  (drawn monochrome) and `é`, with the following `|` in the right column. `htop` and `vim` (with line
  numbers and a status line) look as they do in other terminals.
- **Grid alignment:** the font's letter spacing is set so every advance is one whole-pixel cell; runs of
  ASCII are drawn as one string and stay on the grid. Non-ASCII is drawn one cell at a time.
- **Shell exit:** `exit 3` shows "Shell exited (status 3)"; CPU after exit was 1 tick in 3 s (idle).
- **Hidden output:** `sleep 3; echo done-while-hidden`, hidden during the sleep, shows the line on re-show.
- **NOT verified: real keyboard input.** The key-to-bytes mapping (arrows, Ctrl, Alt, Home/End, input
  method) is written but has only been exercised by injection. It needs a person at the keyboard.
- **NOT verified: input method** (no fcitx5/ibus session was driven).

Things the spike got wrong on the way, worth remembering for milestone 1:

- A C++ `QQuickPaintedItem` needs moc; `cxx-qt-build` 0.10 only runs moc for its own QObjects, so
  `build.rs` runs moc by hand. `#pragma once` is not enough for a header that cxx-qt-build also copies
  into its include tree: use include guards.
- cxx-qt exposes Rust method names to QML as written (`no_focus`, not `noFocus`) unless renamed. A
  wrong name is a runtime TypeError that silently aborts the handler; the panel then never slid back.
- Qt logs to the journal, not stderr, in this session unless `QT_FORCE_STDERR_LOGGING=1`.
- SIGUSR1 before the handler is installed kills the process, and two quick signals merge into one.
  Signals are not a toggle transport; milestone 1 uses D-Bus as specified.
- **While shown with keyboard focus, the panel takes whatever the user types.** Automated runs set
  `KYKO_SPIKE_NOFOCUS=1` (keyboard interactivity none).

## Hotkey (KWin, xdg-desktop-portal 1.22.1, xdg-desktop-portal-kde 6.7.5)

Checked 2026-11-10. The key itself was never physically pressed: activation was triggered through
KGlobalAccel's `invokeShortcut`, which exercises portal → app but not keyboard → KWin.

- **The portal exists and works:** `org.freedesktop.portal.GlobalShortcuts` version 2. `CreateSession`,
  `BindShortcuts` and the `Activated` signal all work from raw zbus.
- **The portal needs an app id, and gets it from the systemd unit, not from the binary.**
  - Launched from another app's scope (here: the Claude desktop app), the shortcut was filed under
    *that app's* id (`[com.anthropic.Claude]` in `kglobalshortcutsrc`). Removed again afterwards.
  - Launched in a scope of its own with no desktop entry: `CreateSession` fails with
    `org.freedesktop.portal.Error.NotAllowed: An app id is required`.
  - Launched as `app-org.kyko.spike-<n>.scope` WITH `org.kyko.spike.desktop` installed: bound.
  - Consequence for kyko: it must ship a desktop entry whose name is its app id, and run in a unit the
    portal maps to that id (or register through `org.freedesktop.host.portal.Registry`; not tried).
- **Dialog:** under the wrong app id Plasma showed an OK/Cancel dialog and the bind waited on it. Under
  its own id the bind returned within 5 s with no dialog seen in the screenshot (not certain nobody
  clicked). The **overlay-layer panel covered that dialog**: kyko must not be showing while a portal
  dialog can appear.
- **Preferred trigger honoured the first time:** asked for `LOGO+F12`, got `Meta+F12`.
- **Rebinding does NOT work through `BindShortcuts`:** asking for `CTRL+LOGO+F11` in a later session
  kept `Meta+F12`. Once registered, Plasma keeps its own stored key. Changing it needs the portal's
  `ConfigureShortcuts` (v2; opens Plasma's own UI, not tried) or KGlobalAccel's D-Bus API directly.
  **"Change the hotkey in kyko's Preferences" is therefore not possible through the portal alone on
  Plasma.**
- **Activation path works:** `invokeShortcut toggle` on the KGlobalAccel component showed the panel.

## Foreground (KWin)

- **Over a fullscreen window:** `LayerOverlay` shows above it; `LayerTop` stays hidden behind it.
  Screenshot evidence, both layers, same fullscreen test window. Overlay is the right layer.
- **Keyboard focus when summoned:** NOT verified. All automated runs used keyboard interactivity
  `None` so the spike could not swallow the user's typing. `Window.active` proved unreliable as a signal
  (it read "focused" in a run where the surface could not take keys).
- **Not checked:** maximised window, another virtual desktop, focus returning on hide, a real key press.

`spike/try.sh` sets up the scope and desktop entry for a hands-on check and cleans up on exit.

## Measurements (KWin, release build)

2026-11-10. Intel i5-8300H (4 cores / 8 threads, 2018 laptop), 1920×1080 at scale 1, Qt scene graph:
threaded render loop, QRhi on OpenGL. **The machine was heavily loaded throughout** (load average 6 to
16 on 8 threads, 11 GB of swap in use, other sessions compiling Rust), so these are pessimistic and
noisy. `paint_ms` is the time inside `GridItem::paint()` only; it does not include Qt uploading the
painted image as a texture. Grid 211×51 unless noted (the target was written for 200×50).

| What | p50 | p95 | max | 4 ms target |
|---|---|---|---|---|
| Dense colour screen (colour change every 2–7 cells), first version | 26.8 ms | 35.9 ms | 55.8 ms | FAIL |
| Same, after not re-setting font and pen per run | 15.7 ms | 23.9 ms | 35.7 ms | FAIL |
| Same, 185×23 (the default island size), first version | 11.5 ms | 14.4 ms | 18.9 ms | FAIL |
| Plain text screen (`ls -la /usr/bin`), after the font/pen fix | 4.3 ms | 6.9 ms | 11.3 ms | FAIL (close) |
| Plain text, `FramebufferObject` render target (fallback a) | 5.2 ms | 6.2 ms | 14.8 ms | FAIL |
| Dense, `FramebufferObject` (fallback a) | slower than 23 ms: fewer than 300 paints in 7 s | | | FAIL |
| Dense, per-row image cache, every row a cache hit (fallbacks b+c) | 4.8 ms | 5.9 ms | 30.2 ms | FAIL (close) |

- **The 4 ms full-repaint target is not met by a `QQuickPaintedItem`, by any of the three fallbacks.**
  With every row cached the cost is one blit per row, about 2 megapixels in all, and that alone is
  4.8 ms here. Text drawn through QPainter costs 4 to 27 ms a screen depending on how often the colour
  changes.
- **The 16.7 ms frame budget (60 Hz) is met** for plain text and for the cached path; an uncached dense
  screen at full size misses it (15.7 ms p50, 23.9 ms p95) on this loaded machine.
- Not tried: repainting only the dirty rectangle (typing changes one row), and a scene-graph node with
  a glyph atlas (the usual way terminals reach sub-millisecond frames).

| What | Result | Target |
|---|---|---|
| `time seq 1 100000` in the spike | 0.119 s | never blocks input: PASS |
| Same in Konsole 6 (what Yakuake embeds) | 0.239 s | — |
| Same to `/dev/null` | 0.003 s | — |
| Ctrl+C during `yes`, engine test (includes exit detection) | 61 ms and 73 ms | about 100 ms: PASS |
| Ctrl+C during `yes` in the window, 5 runs (10 ms poll) | 10, 23, 27, 58, 67 ms | about 100 ms: PASS |
| CPU while shown at an idle prompt | 1 tick in 5 s | idle: PASS |
| CPU after the shell exited | 1 tick in 3 s | idle: PASS |
| Memory, one pane, idle | 157 MB resident, of which 42 MB anonymous | spec target 80 MB: see below |

- The flood is fast because output is coalesced: the engine wakes the painter once per pending
  snapshot, so most of the 100,000 lines are never drawn.
- kitty is not installed here, so the comparison is with Konsole.
- Memory: 157 MB resident is mostly shared Qt and graphics libraries; 42 MB is the process's own. The
  spec's "under 80 MB with three tabs" needs to say which number it means.
- **Glyph fallback:** CJK, box drawing, blocks and an emoji stayed on the grid (see Input and rendering).
- **Input method: not tested.**

## Hyprland (uQ VM)

**Not run.** The VM needs 4 GB and 4 vCPUs for about 40 minutes; the machine was at load 16 with 11 GB
of swap in use and other sessions building, and an earlier note on this machine says earlyoom has been
killing large builds. Unverified on Hyprland: layer shell from QML, the overlay layer over fullscreen,
keyboard focus, whether xdg-desktop-portal-hyprland binds a global shortcut at all, and paint times.

## Renderer comparison (second spike)

See `docs/notes/2026-11-10-renderer-spike-result.md` for the numbers. Lessons from building it:

- A `QSGMaterialShader`'s vertex shader must be baked with `qsb -b` (batchable). Without it the scene
  graph prints "No rewriter-inserted attribute found" and draws nothing.
- `KYKO_SPIKE_RENDERER=atlas|text|painted`; `spike/gen.py dense|plain [frames]` fills the screen.
- Frame timing comes from `QQuickWindow` signals connected directly on the render thread.
- The scene-graph item takes the snapshot inside `updatePaintNode`, where the GUI thread is blocked.
