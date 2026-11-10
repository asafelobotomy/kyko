# Milestone 0 spike: result

Date: 2026-11-10 · Branch `spike/m0` · Raw notes: `spike/NOTES.md` · Plan:
`docs/superpowers/plans/2026-11-10-kyko-m0-spike.md`

## Verdict

| Risk | Verdict | In one sentence |
|---|---|---|
| Layer shell from `cxx-qt` | **Go**, on KWin, keyboard focus aside | It works from QML alone, toggles, keeps state while hidden, and shows above a fullscreen window on the overlay layer; keyboard focus and Hyprland are unverified. |
| Terminal grid in a Qt Quick item | **Revise** | It draws correctly and a flood does not starve the shell's input, but a `QQuickPaintedItem` misses the 4 ms full-repaint target by every route tried (best 4.8 ms, typical 4 to 27 ms), on screens that were only partly full. |
| Hotkey and foreground focus | **Revise** | The portal binds and fires on Plasma, but only for an app with its own desktop entry and app scope, and asking again with a different key did not change it; keyboard focus on summon was not tested. |

The spec needs changes before milestone 1 (see Proposed spec changes). Nothing found says kyko
cannot be built as designed; two parts of the design were wrong in detail.

## Numbers

Release build, KWin on Wayland, Intel i5-8300H, 1920×1080 at scale 1. The machine was heavily
loaded during every run (load average 6 to 16 on 8 threads), so paint times are pessimistic.

Three limits on the paint numbers, all of which make the real picture worse, not better:

- **They time `paint()` only.** That includes taking the snapshot and copying it, but not Qt
  uploading the painted image (about 2 megapixels) to the GPU, which every frame also pays.
- **The test screens were not full.** The "dense colour" screen had about 55% of its cells filled
  (lines wrapped, leaving every second row nearly empty) and the "plain text" screen about 25%. A
  truly full screen costs more.
- **The grid was 211×51**, 7.6% more cells than the 200×50 the target names.

| Measure | Target | Measured | Result |
|---|---|---|---|
| Full repaint, dense colour screen (55% full), 211×51 | under 4 ms | 15.7 ms p50, 23.9 ms p95 | Fail |
| Full repaint, plain text (25% full), 211×51 | under 4 ms | 4.3 ms p50, 6.9 ms p95 | Fail (close) |
| Full repaint with the row cache, static screen, 211×51 | under 4 ms | 4.8 ms p50, 5.9 ms p95 | Fail (close) |
| Full repaint, dense, default island size 185×23 (before any tuning) | under 4 ms | 11.5 ms p50, 14.4 ms p95 | Fail |
| `seq 1 100000` wall time | — | 0.119 s (Konsole: 0.239 s) | Throughput only |
| Ctrl+C during `yes`, injected into the PTY, 5 runs | about 100 ms | 10 to 67 ms | Pass for the shell side; the keyboard path is untested |
| CPU, shown and idle | idle | 1 tick in 5 s | Pass |
| CPU, after the shell exits | idle | 1 tick in 3 s | Pass |
| Memory, one pane | spec: resident under 80 MB with three tabs | 157 MB resident (42 MB of it the process's own) | Fail, by about 2× with one pane |

Against the 60 Hz frame budget (16.7 ms) rather than 4 ms: the quarter-full plain-text screen and
the cached path were inside it for paint time alone; the half-full dense screen, uncached, was not
at p95, and a full dense screen would probably miss it at p50 too. With the upload added, none of
these has been shown to fit a frame.

## What worked

- **Layer shell:** `import org.kde.layershell` and attached properties on a plain QML `Window`. No
  C++ and no extra linking. Borderless and centred at the top. (No warnings were seen, but Qt was
  logging to the journal at that point, not stderr, so that says little.)
- **Hide and show:** the process and the shell keep running while hidden; output produced while
  hidden is on screen at re-show; QML state survives unmap and remap.
- **Keyboard focus is not automatic:** with on-demand keyboard interactivity the surface was not
  focused when first mapped.
- **Above fullscreen:** on `LayerOverlay` the panel shows over a fullscreen window; on `LayerTop` it
  does not. Screenshot evidence for both.
- **The engine:** `alacritty_terminal` 0.26 with its own event loop and PTY code. 13 tests pass,
  repeatedly: text, ANSI and true colour, wide characters, exit status, input, and Ctrl+C during a
  flood. Three of them prove less than their names suggest: the invalid-UTF-8 test shows no crash
  but does not check what is drawn; the resize test checks the emulator's size, not the PTY's; and
  the wake-up test would pass without the spike's own coalescing, because `alacritty_terminal`
  already batches.
- **Rendering correctness:** bash, `ls --color`, `htop` and `vim` look right. Bold, italic, underline,
  inverse, 16 and 256 colours, true colour, CJK, box drawing and an emoji stay on the grid.
- **Flood handling:** a 100,000-line flood finishes in 0.12 s, and a Ctrl+C written to the PTY
  while `yes` runs stops it in tens of milliseconds. This shows the shell's input is not starved by
  its output. It does not show the window stays responsive to the keyboard: the Ctrl+C was
  injected from a background thread and never went through Qt's event loop.
- **Portal hotkey:** `GlobalShortcuts` version 2 is present on Plasma; create, bind and the
  activation signal work over raw zbus; the preferred key was honoured on first registration.

## What didn't

- **Paint speed.** Three fallbacks from the plan, none reached 4 ms:
  1. `FramebufferObject` render target: no better for plain text, worse for dense colour.
  2. and 3. together, a per-row image cache keyed by row contents: 4.8 ms on a static screen. My
     reading is that blitting a full screen of row images is itself that slow, but the timer also
     covers the snapshot, a full background fill and row hashing, so that is an inference. Cache
     hits were not recorded. The cached path was not measured at the default island size, where it
     covers under half the pixels and might meet 4 ms.
- **The portal identifies the app by its systemd unit and desktop entry, not by the binary.**
  - Started from another app's scope, the shortcut was filed under that app (here, Claude's own
    entry in Plasma's shortcuts file; I removed it and checked Claude's own shortcut was intact).
  - Started in its own scope with no desktop entry: "An app id is required".
  - Started as `app-org.kyko.spike-<n>.scope` with a matching desktop entry: bound.
- **Asking again with a different key did not change it.** A second session that bound with a
  different preferred key kept the first one. Two other routes were not tried: the portal's own
  `ConfigureShortcuts` call (version 2, which opens Plasma's UI) and KGlobalAccel's D-Bus setter.
- **The overlay panel covered Plasma's shortcut dialog** in the one run where a dialog appeared.
- **A wrong method name from QML fails silently.** cxx-qt exposes Rust names as written
  (`no_focus`); calling `noFocus()` threw inside the show handler and the panel never came back.
- **Signals are not a toggle transport:** an early SIGUSR1 killed the process and two quick ones
  merged. Only the spike used signals; the spec already says D-Bus.
- **A visible panel with keyboard focus swallows whatever the user is typing.** Automated runs had
  to switch keyboard focus off entirely.

Review Focus outcomes:

| Failure mode | Outcome |
|---|---|
| Invalid UTF-8 from the shell | Partly: no crash or hang (test); what is drawn was not checked |
| The shell exits | Covered for `exit N`: test, on-screen message, idle CPU |
| Tiny or zero resize | Partly: the emulator clamps (test); the PTY side and the window were not exercised |
| A flood while typing | Shell side only (see Flood handling) |
| Missing font or glyph | Missing glyph: fine by screenshot. Missing font: not tested, and runs of text rely on the font being monospace |

## Not verified

- **Anything on Hyprland.** The uQ VM run was skipped: the machine was at load 16 with 11 GB of swap
  in use and other sessions building. Open: layer shell from QML, overlay over fullscreen, keyboard
  focus, and whether Hyprland's portal binds a global shortcut at all.
- **Real key presses.** Every input was injected into the PTY, and hotkey activation was triggered
  through Plasma's shortcut service, not the keyboard. The key-to-bytes mapping is unexercised.
- **Keyboard focus when summoned**, focus returning to the previous window on hide, and the
  exclusive against on-demand comparison. Testing them needs a person at the keyboard.
- **Input methods.**
- **Window responsiveness to real keys during a flood.**
- **Changing the hotkey** through `ConfigureShortcuts` or KGlobalAccel; the portal's host Registry
  as a way to declare the app id; whether a plain `kyko.service` unit name maps to an app id.
- **`activateOnShow`**, the focused-monitor rule, hiding by hotkey, and pointer input on the
  transparent strip above the panel.
- **A missing font**, and a real full screen of text or colour.
- **Summoning over a maximised window and from another virtual desktop**; multiple monitors.
- **Whether Plasma shows a dialog on first bind** under kyko's own app id (none was seen within 5 s,
  but I can't rule out that it was answered).
- **Paint times on an idle machine**, and the cost of a one-row update (typing).

`spike/try.sh` runs the spike in its own scope with a temporary desktop entry, binds Meta+F12, and
cleans up on exit; it is the quickest way to check the keyboard items by hand.

## What milestone 1 should keep and drop

- **Keep:** layer shell through the QML module; the overlay layer; `alacritty_terminal`; a dirty
  flag between engine and painter; whole-pixel cells; claiming resources before advertising the
  process.
- **Change:** the spike draws runs of ASCII as one string and relies on letter spacing to stay on
  the grid, which only holds for a monospace font. The real renderer must place every cell by
  position.
- **Drop:** signals; a global engine; hand-run moc. Whether `QQuickPaintedItem` goes depends on the
  renderer decision below.
- **Note for the holder split:** the spike used `alacritty_terminal`'s `EventLoop`, which owns the
  PTY. In the real design `kykod` owns the PTY, so the window must feed bytes to the emulator
  itself. That path is not exercised here.
- None of the spike's code is carried over.

## Proposed spec changes

For the user to decide; the spec is not edited.

1. **Renderer (§2.1, §2.11, §6).** Choose one:
   - **A. A scene-graph node with a glyph atlas**, still inside Qt Quick: glyphs are rasterised once
     into a texture and each cell is a textured quad. This is how GPU terminals get sub-millisecond
     frames, and it keeps QML for tabs and Preferences. It is unproven here and needs its own short
     spike. Recommended.
   - **B. The spec's stated fallback:** `alacritty_terminal` with `wgpu` in a plain layer surface,
     QML out of the terminal path. Proven by other terminals, but tabs, Preferences and theming
     then need a second toolkit or hand-built widgets.
   - **C. Keep the painted item and restate the target** as "a full-screen change within one frame
     at the monitor's refresh rate", adding dirty-rectangle repaint. Least work, and the weakest
     case: the numbers above are paint-only on part-full screens and already miss a frame for
     dense output.
2. **Changing the hotkey on Plasma (§2.9, §2.10).** Re-binding with a new preferred key does not
   work. Before choosing a fix, try the two untried routes: the portal's `ConfigureShortcuts`, and a
   `kde` backend that sets the key through KGlobalAccel's D-Bus API. The second is the likely
   answer if Preferences is to change the key without opening Plasma's own dialog.
3. **App identity (§3.3, §8).** kyko must ship a desktop entry named for its app id and run where
   the portal can map it to that id. The only arrangement shown to work is an
   `app-<id>-<n>.scope` unit; a unit simply named `kyko.service` is unlikely to. Decide between
   naming the unit to match, or declaring the id through the portal's host Registry (untried), and
   add a test that a bind lands under kyko's id.
4. **Dialogs (§2.2).** kyko must not be shown while a portal dialog may be open (bind while hidden).
5. **Memory target (§2.11).** The spec says resident under 80 MB with three tabs; one pane already
   measured 157 MB resident, most of it shared Qt and graphics libraries. Either restate the target
   as the process's own memory and set a number from measurement, or accept the miss. Scrollback
   also needs a budget: 10,000 lines per pane is tens of megabytes when full (estimated, not
   measured).
6. **Risk list (§6).** Carry forward as open: keyboard focus on summon, including §2.2's
   "exclusive, then on-demand" scheme, and everything on Hyprland.
7. **Holder speed target (§6).** "Within 15% of the spike's direct-PTY time" is 18 ms of a 0.119 s
   run, inside the noise on this machine. Use a larger workload or an absolute bound.
