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
