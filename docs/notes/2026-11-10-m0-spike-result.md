# Milestone 0 spike: result

Date: 2026-11-10 · Branch `spike/m0` · Raw notes: `spike/NOTES.md` · Plan:
`docs/superpowers/plans/2026-11-10-kyko-m0-spike.md`

## Verdict

| Risk | Verdict | In one sentence |
|---|---|---|
| Layer shell from `cxx-qt` | **Go**, on KWin | It works from QML alone, toggles, keeps state while hidden, and shows above a fullscreen window on the overlay layer; Hyprland is unverified. |
| Terminal grid in a Qt Quick item | **Revise** | It is correct and never blocks input, but a `QQuickPaintedItem` misses the 4 ms full-repaint target by every route tried (best 4.8 ms, typical 4 to 27 ms). |
| Hotkey and foreground focus | **Revise** | The portal binds and fires on Plasma, but only for an app with its own desktop entry and unit, and it cannot change the key afterwards; keyboard focus on summon was not tested. |

The spec needs changes before milestone 1 (see Proposed spec changes). Nothing found says kyko
cannot be built as designed; two parts of the design were wrong in detail.

## Numbers

Release build, KWin on Wayland, Intel i5-8300H, 1920×1080 at scale 1. The machine was heavily
loaded during every run (load average 6 to 16 on 8 threads), so paint times are pessimistic.

| Measure | Target | Measured | Result |
|---|---|---|---|
| Full repaint, dense colour screen, 211×51 | under 4 ms | 15.7 ms p50, 23.9 ms p95 | Fail |
| Full repaint, plain text, 211×51 | under 4 ms | 4.3 ms p50, 6.9 ms p95 | Fail (close) |
| Full repaint, all rows from the row cache, 211×51 | under 4 ms | 4.8 ms p50, 5.9 ms p95 | Fail (close) |
| Full repaint, dense, default island size 185×23 (before any tuning) | under 4 ms | 11.5 ms p50, 14.4 ms p95 | Fail |
| `seq 1 100000` | never blocks input | 0.119 s (Konsole: 0.239 s) | Pass |
| Ctrl+C during `yes`, in the window, 5 runs | about 100 ms | 10 to 67 ms | Pass |
| CPU, shown and idle | idle | 1 tick in 5 s | Pass |
| CPU, after the shell exits | idle | 1 tick in 3 s | Pass |
| Memory, one pane | spec: 80 MB with three tabs | 157 MB resident, 42 MB of it the process's own | Unclear: the spec doesn't say which |

Against the 60 Hz frame budget (16.7 ms) rather than 4 ms: plain text and the cached path fit
comfortably; an uncached dense screen at full size does not fit at p95 on this loaded machine.

## What worked

- **Layer shell:** `import org.kde.layershell` and attached properties on a plain QML `Window`. No
  C++ and no extra linking. Borderless, centred at the top, nothing on stderr.
- **Hide and show:** the process and the shell keep running while hidden; output produced while
  hidden is on screen at re-show; QML state survives unmap and remap.
- **Above fullscreen:** on `LayerOverlay` the panel shows over a fullscreen window; on `LayerTop` it
  does not. Screenshot evidence for both.
- **The engine:** `alacritty_terminal` 0.26 with its own event loop and PTY code. 13 tests pass,
  repeatedly: text, ANSI and true colour, wide characters, invalid UTF-8, exit status, resize
  clamping, input, coalesced wake-ups, Ctrl+C during a flood.
- **Rendering correctness:** bash, `ls --color`, `htop` and `vim` look right. Bold, italic, underline,
  inverse, 16 and 256 colours, true colour, CJK, box drawing and an emoji stay on the grid.
- **Flood handling:** wake-ups are coalesced, so a 100,000-line flood finishes in 0.12 s and Ctrl+C
  lands in tens of milliseconds while it runs.
- **Portal hotkey:** `GlobalShortcuts` version 2 is present on Plasma; create, bind and the
  activation signal work over raw zbus; the preferred key was honoured on first registration.

## What didn't

- **Paint speed.** Three fallbacks from the plan, none reached 4 ms:
  1. `FramebufferObject` render target: no better for plain text, worse for dense colour.
  2. and 3. together, a per-row image cache keyed by row contents: 4.8 ms with every row cached,
     because blitting a full screen of row images is itself that slow.
- **The portal identifies the app by its systemd unit and desktop entry, not by the binary.**
  - Started from another app's scope, the shortcut was filed under that app (here, Claude's own
    entry in Plasma's shortcuts file; I removed it and checked Claude's own shortcut was intact).
  - Started in its own scope with no desktop entry: "An app id is required".
  - Started as `app-org.kyko.spike-<n>.scope` with a matching desktop entry: bound.
- **The key cannot be changed through the portal's bind call.** A second session asking for a
  different key kept the first one. Plasma stores the key and ignores later preferences.
- **The overlay panel covered Plasma's shortcut dialog** in the one run where a dialog appeared.
- **A wrong method name from QML fails silently.** cxx-qt exposes Rust names as written
  (`no_focus`); calling `noFocus()` threw inside the show handler and the panel never came back.
- **Signals are not a toggle transport:** an early SIGUSR1 killed the process and two quick ones
  merged. Only the spike used signals; the spec already says D-Bus.
- **A visible panel with keyboard focus swallows whatever the user is typing.** Automated runs had
  to switch keyboard focus off entirely.

Review Focus outcomes: invalid UTF-8, shell exit, tiny and zero resize, and a flood while typing
all pass with tests or measurements; glyph fallback passes by screenshot.

## Not verified

- **Anything on Hyprland.** The uQ VM run was skipped: the machine was at load 16 with 11 GB of swap
  in use and other sessions building. Open: layer shell from QML, overlay over fullscreen, keyboard
  focus, and whether Hyprland's portal binds a global shortcut at all.
- **Real key presses.** Every input was injected into the PTY, and hotkey activation was triggered
  through Plasma's shortcut service, not the keyboard. The key-to-bytes mapping is unexercised.
- **Keyboard focus when summoned**, focus returning to the previous window on hide, and the
  exclusive against on-demand comparison. Testing them needs a person at the keyboard.
- **Input methods.**
- **Summoning over a maximised window and from another virtual desktop**; multiple monitors.
- **Whether Plasma shows a dialog on first bind** under kyko's own app id (none was seen within 5 s,
  but I can't rule out that it was answered).
- **Paint times on an idle machine**, and the cost of a one-row update (typing).

`spike/try.sh` runs the spike in its own scope with a temporary desktop entry, binds Meta+F12, and
cleans up on exit; it is the quickest way to check the keyboard items by hand.

## What milestone 1 should keep and drop

- **Keep:** layer shell through the QML module; the overlay layer; `alacritty_terminal`; coalescing
  wake-ups on a dirty flag; drawing by cell position with whole-pixel cells; claiming resources
  before advertising the process.
- **Drop:** `QQuickPaintedItem` as the renderer; signals; a global engine; hand-run moc.
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
     at the monitor's refresh rate", adding dirty-rectangle repaint. Least work; dense full-screen
     output would drop frames on slower machines.
2. **Hotkey on Plasma (§2.9, §2.10).** Add a `kde` backend that sets the key through KGlobalAccel's
   D-Bus API, since the portal cannot change a key after first registration. Keep `portal` for
   desktops without it.
3. **App identity (§3.3, §8).** State that kyko ships a desktop entry named for its app id and that
   `kyko.service` is a unit the portal maps to that id; add a test that a bind lands under kyko's id.
4. **Dialogs (§2.2).** kyko must not be shown while a portal dialog may be open (bind while hidden).
5. **Memory target (§2.11).** Say whether 80 MB means resident or the process's own memory.
6. **Risk list (§6).** Carry forward as open: keyboard focus on summon, and everything on Hyprland.
