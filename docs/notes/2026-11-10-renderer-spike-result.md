# Renderer spike: result

Date: 2026-11-10 · Branch `spike/m0` · Follows `2026-11-10-m0-spike-result.md` (proposed change 1).
Not independently reviewed.

## Verdict

**Go with the glyph atlas on Qt Quick scene-graph nodes.** It rebuilds a completely full 211×51
screen in about 1.3 ms of CPU per frame and holds 60 frames per second, using only Qt's public
scene-graph classes. Qt's text node and the painted item are both ruled out by the same test.

## What was built

Two more renderers in the throwaway app, selected with `KYKO_SPIKE_RENDERER`:

- **`atlas`:** each glyph (per character and style) is drawn once into a 2048×2048 image with
  QPainter; every non-blank cell is a textured quad coloured per vertex; backgrounds are coloured
  quads. One geometry node for backgrounds, one for glyphs, one small custom shader baked with
  `qsb`. Public API only: `QSGGeometryNode`, `QSGGeometry`, `QSGMaterial`, `QSGMaterialShader`,
  `QSGTexture`, `QSGVertexColorMaterial`. No private Qt headers and no `Qt::GuiPrivate`.
- **`text`:** one `QSGTextNode` per row, laid out again when the row changes.
- **`painted`:** the previous `QQuickPaintedItem`, unchanged, as the baseline.

## How it was measured

- Screens were **completely full** this time (`spike/gen.py`): every cell printable. "Dense" changes
  colour every 2 to 7 cells and gives a third of the runs a background; "plain" is one colour.
- **Whole-frame timing** from `QQuickWindow`'s own signals, not paint time: `sync` (building the
  nodes, with the GUI thread blocked; for the painted item this includes painting and the upload),
  `render` (the scene graph submitting the frame), and `interval` (time between presented frames;
  16.7 ms means 60 per second was held). 300 frames after 60 warm-up frames.
- "Forced" runs rebuild everything on every frame from a static screen, which is the cost of a
  full-screen change. "Changing" runs had a program write a different full screen every 12 ms.
- Release build, KWin on Wayland, Qt 6.11.2 scene graph on OpenGL, Intel i5-8300H with its integrated
  UHD 630 graphics (Mesa), 1920×1080 at
  scale 1. Load average 1.4 to 2.4 during the runs (the earlier spike ran at 6 to 16).
- GPU time is not measured directly. It is inferred to be small where the interval stays at 16.7 ms.

## Numbers

Milliseconds, p50 / p95. Grid 211×51 unless noted.

| Renderer | Screen | sync | render | interval | Holds 60 fps |
|---|---|---|---|---|---|
| atlas | dense, forced | 0.62 / 0.94 | 0.66 / 1.34 | 16.68 / 17.60 | Yes |
| atlas | plain, forced | 0.58 / 0.87 | 0.95 / 1.89 | 16.68 / 17.66 | Yes |
| atlas | dense, forced, island size 185×23 | 0.29 / 0.43 | 0.34 / 0.47 | 16.67 / 17.65 | Yes |
| atlas | dense, changing | 0.56 / 0.75 | 0.56 / 0.95 | 19.50 / 23.90 | Limited by the program producing frames, not the renderer |
| text node | plain, forced | 9.43 / 10.07 | 1.08 / 1.20 | 16.70 / 17.46 | Yes, with 10 ms of the GUI thread blocked |
| text node | plain, forced, native rendering | 9.26 / 10.87 | 0.37 / 0.49 | 16.70 / 17.84 | As above |
| text node | dense, forced | 29.54 / 32.75 | 30.70 / 35.83 | 63.44 / 70.49 | No (16 fps) |
| painted | plain, forced | 17.25 / 19.17 | 0.02 / 0.06 | 18.21 / 20.77 | No |
| painted | dense, forced | 26.43 / 29.17 | 0.02 / 0.04 | 27.38 / 30.32 | No (37 fps) |
| painted | dense, changing | 24.00 / 28.87 | 0.03 / 0.05 | 24.99 / 29.76 | No |
| painted, row cache, all hits | dense, forced | 4.71 / 5.45 | 0.02 / 0.05 | 16.68 / 17.79 | Yes, static content only |

Other measurements:

| What | atlas | painted |
|---|---|---|
| CPU with a static full screen, 5 s | 1 tick | 0 ticks |
| Memory, full-size grid: resident / the process's own | 177 MB / 64 MB | 172 MB / 59 MB |
| Slowest single build | 22.7 ms, the first frame, when 96 glyphs were rasterised | — |

Corrections to the earlier note, now that the screens are full and the machine is quiet: the
painted item costs 17 ms (plain) to 26 ms (dense) per full repaint, not 4 to 16 ms, and misses a
60 Hz frame for both.

## What the atlas renderer got right, by screenshot

Bold, italic, underline, inverse, 256 and true colour, backgrounds, CJK double-width, box drawing
and block elements match the painted renderer. It draws no ligatures (`->`, `!=` stay separate
characters), which is what the spec asks for; the painted renderer was drawing them.

## What it does not do yet, and what is unmeasured

- **Colour emoji** come out as a single-colour silhouette: the atlas keeps only alpha. A second,
  colour atlas is needed.
- **Atlas misses.** Only 96 distinct glyphs were on screen. Rasterising is about 0.24 ms a glyph
  (22.7 ms for 96, first frame), so a screen of hundreds of new CJK characters would stall one
  frame. Rasterising off the render thread, or a few per frame, would fix it; not tried.
- **Atlas capacity.** One fixed 2048×2048 image, no eviction, no growth.
- **Fractional and high-DPI scaling.** Tested at scale 1 only. The atlas must be rasterised at
  device pixels and quads snapped to them; not tried.
- **Cursor, selection, underline styles, hyperlinks, the exit message.** Not drawn in this mode.
- **Keyboard input** is not wired to the scene-graph item (the painted item has it).
- **One GPU, one driver, OpenGL only.** Not run on Vulkan, on another vendor, or in software
  rendering (a VM), where the result could differ.
- **Memory is no better.** Resident memory is about 175 MB either way; the spec's 80 MB target still
  needs restating.
- Everything unverified in the first spike still is: Hyprland, real key presses, keyboard focus.

## What this means for the spec

- Proposed change 1 is settled as **option A**: the terminal is a custom `QQuickItem` with scene-graph
  nodes and its own glyph atlas, written in C++ (cxx-qt does not wrap these classes; `build.rs` runs
  moc and `qsb`, which adds `qt6-shadertools` as a build dependency).
- The speed target can be restated and is already met with room: a completely full screen changes
  within one frame at 60 Hz, measured from Qt's frame signals. At about 1.3 ms of CPU it also fits
  120 Hz and 144 Hz.
- New work the spec should name: a colour atlas for emoji, glyph rasterisation that cannot stall a
  frame, atlas growth, and rasterising at device pixels.
- One optimisation is available but not needed at these numbers: rebuilding only changed rows.
