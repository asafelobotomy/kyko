# kyko: design

Date: 2026-11-10 · Status: revised after the milestone 0 spikes; reviewed and assumptions
confirmed on 2026-11-13
Spike results: `docs/notes/2026-11-10-m0-spike-result.md` and `2026-11-10-renderer-spike-result.md`.
Related: uQ `docs/superpowers/specs/2026-11-08-uq-desktop-design.md` (§2.4 extension points) and
`2026-11-08-uq-phase2-shell-design.md` (§2.3 launcher providers and the `run` provider).

## 1. Intent

kyko is a drop-down terminal for the uQ desktop, in the manner of Yakuake: one key slides a
terminal down from the top of the screen, and the same key hides it again with its sessions still
running. It has its own tabs and split panes, follows uQ's theme live, and can be sent commands
from uQ's Spotlight search.

**What the user decided**

- Build it new in Rust. A Yakuake fork was ruled out: it would bring KDE Frameworks and Konsole
  into a Hyprland desktop and could not follow uQ's theme or extension points.
- The full app, with kyko's own tab bar and splits. A thin toggler around kitty was ruled out.
- Both looks from the 2026-11-10 mockup are options: **island** and **sheet**, with the tab bar on
  **top** or **bottom**, set independently.
- Spotlight link: the built-in `>` item gains a **Run in kyko** action. Enter keeps today's
  behaviour (run with no terminal).
- kyko stays running in the background whatever is closed in its window, and nothing in the
  window can quit it. Yakuake puts Close Tab and Quit one above the other, so a slip quits the
  whole app and its sessions; kyko has no quit control to hit (§2.7).

- **Packaged from the start.** kyko ships as an installable package, and CI builds and publishes
  it whenever the version is bumped (§8). This comes with the first real code, not at the end.
- **uQ native, but not uQ only.** It integrates fully with uQ and still works on other Wayland
  desktops (§2.10). It is tested with the other uQ apps in uQ's VM session (§5).
- **Its own Preferences**, where the user defines and customises the terminal (§2.6).
- **Its own hotkey**, changeable in Preferences. Whatever window is open or focused, the hotkey
  summons kyko above every app, in the foreground (§2.9).
- **Secure, efficient and modern** (§2.11).
- **Renderer:** a glyph atlas on Qt Quick's scene graph (§2.4), chosen after the renderer spike
  measured it against a painted item and Qt's text node.

Confirmed by the user on 2026-11-13 (until then listed as assumptions):

- "Pack" means an installable package: an Arch package on the AUR and on each GitHub Release.
- Split panes are in the first release.
- kyko hides when it loses focus unless pinned.
- Tabs are named after the foreground program or folder and can be renamed.
- The default hotkey is `F12`, as in Yakuake.
- kyko also lists open tabs in Spotlight ("Switch to: ssh nas"). This is the only use of the
  external-provider route; it can be cut without affecting anything else.
- License is MIT, as uQ.

**Success looks like**

- The hotkey shows kyko in under 100 ms with the previous sessions intact, over any window
  including a fullscreen one, with the keyboard in the terminal; the hotkey again hides it.
- Bumping the version on `main` produces a GitHub Release with an installable package and no
  further manual step.
- Switching uQ's theme recolours kyko, including running panes, without a restart.
- `> cargo build` in Spotlight, then **Run in kyko**, opens a new tab running the command and
  drops kyko down.
- Killing the window process with a build running in a tab loses nothing: the window returns
  with the same tabs and the build still going.
- uQ runs unchanged with kyko not installed. kyko runs, with a built-in theme, without `uqd`.

**Not in scope**

- X11. kyko is Wayland-only; §2.10 lists what each compositor gets.
- Packages other than Arch's (deb, rpm, Flatpak) in the first release. Other distributions build
  from source; a Flatpak suits a terminal poorly because of its sandbox.
- Session restore across logout or reboot, ligatures, inline images (sixel, kitty graphics),
  remote control of pane contents, scripting, and plugins.
- Being a general windowed terminal. `shell.terminal` in uQ stays kitty, foot or alacritty.

## 2. Architecture

Two long-lived processes per session, so that the shells outlive the window process (decided
2026-11-10, §2.7):

- **`kykod`, the session holder.** It owns every shell and its PTY and keeps a bounded copy of
  each pane's recent output. It has no Qt, no D-Bus and no rendering, and is kept small on
  purpose: the less it does, the less can crash it.
- **`kyko`, the window.** It draws, handles input, and serves D-Bus. It attaches to `kykod` over
  a Unix socket and can crash, restart or be upgraded without touching a shell. The same binary
  is the command-line client: with a subcommand it calls the running window over D-Bus and exits.

```
kyko (Cargo workspace, edition 2024, stable toolchain)
  crates/kyko-core   pure logic, no Qt: config, theme, session tree, command queue, provider index
  crates/kyko-proto  the kykod wire protocol: frame types, encode and decode, version
  crates/kyko-term   terminal emulation: feeds pane output to alacritty_terminal, exposes a grid snapshot
  apps/kykod         the session holder: PTYs, child processes, output rings, the socket
  apps/kyko          cxx-qt + QML: window, tab bar, panes, D-Bus service, CLI; the grid renderer in C++
  packaging/         PKGBUILD, two systemd user units, D-Bus service file, .provider file, settings page
```

Stack, following uQ's convention for its own apps: Rust for logic, `cxx-qt` 0.10 and QML for the
window. Versions checked on 2026-11-10: `alacritty_terminal` 0.26.0 (crates.io), Qt 6.11,
`layer-shell-qt` 6.7.5 (Manjaro).

### 2.1 Units

| Unit | Does | Depends on |
|---|---|---|
| `kyko-core::config` | Loads and validates `~/.config/kyko/config.toml`; a bad field costs only that field | serde, toml |
| `kyko-core::theme` | Parses uQ's `tokens.json` into kyko's palette; holds the built-in fallback | serde_json |
| `kyko-core::session` | The tree of tabs, each a tree of splits with a pane at every leaf; focus, naming, close rules | — |
| `kyko-core::runq` | Maps `Run` requests to new tabs; validates and bounds commands | session |
| `kyko-proto` | Length-prefixed frames for the socket (§2.8); pure encode and decode | — |
| `kyko-term` | One `Term` per pane: feeds output bytes to `alacritty_terminal`, hands out damage-tracked grid snapshots, turns key and mouse input into bytes. It never touches a PTY | alacritty_terminal |
| `apps/kykod` | Spawns shells on PTYs, forwards their output, keeps a ring of recent output per pane, reports each pane's foreground program and folder, stores the window's layout blob | kyko-proto, rustix |
| `apps/kyko` link | The client side of the socket: attach, replay, reconnect | kyko-proto |
| `apps/kyko` grid item | A custom `QQuickItem` in C++ that draws one pane's grid snapshot with scene-graph nodes and a glyph atlas (§2.4), and forwards input. C++ because cxx-qt does not wrap the scene-graph classes; `build.rs` runs moc and Qt's shader tool for it | kyko-term, Qt Quick |
| `apps/kyko` window | The layer-shell surface, slide animation, tab bar and split layout in QML | layer-shell-qt, Qt Quick |
| `apps/kyko` service | `org.kyko.Terminal1` and `org.uq.Launcher1.Provider` on the session bus | zbus |
| `apps/kyko` hotkey | One trait, four backends (§2.9); picks, registers, re-registers, reports the bound key | zbus, ashpd or raw portal calls |
| `apps/kyko` preferences | The Preferences window (§2.6) in QML over the config properties | Qt Quick Controls |
| `apps/kyko` cli | `kyko toggle`, `show`, `hide`, `run`, `new-tab`, `preferences`, `quit`, `end-sessions` | zbus |

Everything except `apps/kyko` has no Qt dependency and is tested with plain `cargo test`.

### 2.2 Window

- A layer-shell surface on the `overlay` layer, so it is above every app including fullscreen
  ones, anchored to the top edge of the focused monitor, namespace `kyko`.
- Showing it takes the keyboard: interactivity is `exclusive` until the surface has focus, then
  `on-demand`, so a pinned kyko can lose focus to another window and stay open. The spike found
  that on-demand alone does not focus the surface when it is first mapped, and did not get as far
  as testing this scheme with real key presses; it is an open risk (§6).
- kyko is never shown while a desktop-portal dialog may be open: the overlay layer would cover it.
  Hotkey registration (§2.9) therefore happens while kyko is hidden.
- `window.above_fullscreen = false` uses the `top` layer instead.
- **Island:** 88% of monitor width, centred, offset below uQ's bar by `ui.gap_out`, corners
  `ui.radius`, border `ui.border_width` in `colors.accent`. The bar stays visible.
- **Sheet:** full monitor width from the top edge, square top corners, bottom corners `ui.radius`,
  a bottom border in `colors.accent`. It covers the bar while open.
- Height defaults to 45% of the monitor, width (island only) to 88%; both are settings.
- Show and hide slide over 150 ms. With `animations = false` they are instant.
- Hiding unmaps the surface; panes keep running and keep reading their PTYs.
- If focus leaves kyko and `pinned` is false, it hides. The pin button toggles `pinned` for the
  session; `hide_on_focus_loss` sets the default.

### 2.3 Tabs and panes

- Tab bar: numbered pills with the tab name, a new-tab button, then split, pin and settings
  buttons. Position `top` or `bottom`.
- The only close control is on a tab: a close button on the pill, shown on hover and on the
  active tab, and middle-click. There is no window close button and no quit entry in any menu.
- A tab's name is the foreground process name, or the folder name when the shell is idle; a
  user rename sticks until cleared.
- A tab holds a binary split tree. Splits are side by side or stacked; dividers drag to resize.
- A pane closes when its shell exits. Closing the last pane closes the tab; closing the last tab
  opens a fresh one and hides kyko.
- Closing a tab or pane whose foreground process is not the shell asks for confirmation.
- Scrollback is 10,000 lines per pane (setting, 0–100,000).

Default keys (all settable; shown with the kyko window focused):

| Key | Action |
|---|---|
| `Ctrl+Shift+T` / `Ctrl+Shift+W` | new tab / close pane |
| `Ctrl+PgUp` / `Ctrl+PgDn`, `Alt+1…9` | previous / next tab, tab by number |
| `Ctrl+Shift+D` / `Ctrl+Shift+E` | split side by side / stacked |
| `Alt+Arrow` | focus the pane in that direction |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | copy / paste |
| `Ctrl+Shift+F` | search scrollback |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | font size |

### 2.4 Terminal

- The shell is `$SHELL`, falling back to `/bin/sh`; `TERM=xterm-256color`, `COLORTERM=truecolor`.
- **Renderer:** a glyph atlas on Qt Quick's scene graph, chosen after the spikes measured three
  options. Each glyph, per character and style, is rasterised once into a texture; every
  non-blank cell is a textured quad coloured per vertex, and backgrounds are coloured quads. It
  uses only Qt's public scene-graph classes (`QSGGeometryNode`, `QSGMaterial`,
  `QSGMaterialShader`, `QSGTexture`), never Qt's private GPU layer, so a packaged build keeps
  working across Qt updates.
- A monospace grid from `fonts.mono` at `fonts.size`. Every cell is placed by its grid position,
  never by text advance, so substituted glyphs cannot push later columns off the grid. Wide
  characters span two cells. No ligatures.
- Bold, italic, underline and strikethrough, 256 and true colour. Glyphs missing from the font
  come from Qt's font fallback when they are rasterised.
- **Colour emoji** use a second, colour atlas; the first holds coverage only and is tinted.
- **Rasterising must not stall a frame:** at most 2 ms of new glyphs are rasterised per frame;
  cells still waiting are drawn blank for that frame and filled on the next.
- **The atlas grows** by adding pages when one fills; a page unused for 10 minutes is dropped.
- **Scale:** glyphs are rasterised at device pixels and quads snapped to them, so text is crisp at
  fractional scales; the atlas is rebuilt when the scale changes.
- Only rows whose contents changed are rebuilt.
- The 16 ANSI colours, foreground, background, cursor and selection come from the theme.
- Mouse: selection, word and line selection, mouse reporting to programs, wheel scrollback,
  Ctrl+click on a URL opens it with `xdg-open`.
- Input methods work through Qt's input-method support.
- Bracketed paste is on; a multi-line paste into a shell that has not enabled it asks first.
- Modern protocol support, as `alacritty_terminal` 0.26 provides it: the kitty keyboard protocol,
  OSC 8 hyperlinks, synchronised output, styled and coloured underlines, focus reporting. Each is
  confirmed by a test in milestone 1 or removed from this list.
- The window follows the monitor's scale, including fractional scales, when it moves between
  monitors.

### 2.5 Theme

- Source: `~/.local/state/uq/theme/tokens.json`, which is uQ's theme serialised as
  `{id, name, variant, colors, ui, fonts, icons, terminal: {ansi: [16]}}`.
- kyko subscribes to `ThemeChanged` on `org.uq.Desktop1` and re-reads the file; it also watches
  the file, so it follows the theme when `uqd` restarts.
- If the file is missing or does not parse, kyko uses a built-in dark palette and logs once.
  A missing field falls back for that field only.
- `theme.source` chooses: `uq` (follow uQ; the default when `uqd` is present), `builtin:<id>`
  (uQ's seven themes in dark and light ship inside kyko, with their credits, so they are
  available on any desktop; the default elsewhere is `builtin:gruvbox-dark`), or `custom` (the
  colours under `[theme.custom]`).

### 2.6 Preferences

kyko has its own Preferences window, opened from the settings button on the tab bar or with
`kyko preferences`. It is an ordinary window, not part of the drop-down, with these pages:
**Appearance**, **Window**, **Terminal**, **Keys**, **Sessions**, **Advanced**. Changes apply at
once and are written to `~/.config/kyko/config.toml`, which can also be edited by hand; kyko
re-reads it on change. Unknown keys are ignored with a warning, and a bad value costs only that
key. Every page has "Reset this page".

| Key | Values | Default |
|---|---|---|
| `hotkey.toggle` | a key sequence, or empty for none (§2.9) | `F12` |
| `window.shape` | `island`, `sheet` | `island` |
| `window.tabs` | `top`, `bottom` | `top` |
| `window.height_pct` | 20–100 | 45 |
| `window.width_pct` | 40–100 (island only) | 88 |
| `window.monitor` | `focused`, `primary`, or an output name | `focused` |
| `window.above_fullscreen` | bool | true |
| `window.hide_on_focus_loss` | bool | true |
| `window.animations` | bool | true |
| `window.opacity` | 0.5–1.0 | 1.0 |
| `theme.source` | `uq`, `builtin:<id>`, `custom` | see §2.5 |
| `theme.custom.*` | foreground, background, cursor, selection, 16 ANSI colours | — |
| `terminal.font` | a font family, or unset to follow the theme | unset |
| `terminal.line_height` | 0.8–2.0 | 1.0 |
| `terminal.padding` | 0–32 px | 8 |
| `terminal.cursor` | `block`, `beam`, `underline`; `terminal.cursor_blink` bool | `block`, false |
| `terminal.new_tab_dir` | `home`, `current` (the focused pane's folder) | `current` |
| `terminal.bell` | `none`, `visual`, `sound` | `visual` |
| `terminal.copy_on_select` | bool | false |
| `terminal.confirm_multiline_paste` | bool | true |
| `terminal.scrollback` | 0–100000 | 10000 |
| `sessions.ring_kib` | 256–16384, output kept per pane for replay (§2.8); applies to new panes | 2048 |
| `terminal.font_size` | 6–48, or unset to follow the theme | unset |
| `terminal.shell` | path, or unset for `$SHELL` | unset |
| `keys.*` | key sequences for the actions in §2.3 | as §2.3 |
| `advanced.programs_write_clipboard` | bool: programs in the terminal may set the clipboard | false |
| `advanced.allow_run` | bool: other apps may call `Run` (§3.1); off disables **Run in kyko** | true |

The Keys page captures a key press for each action and for the hotkey, and refuses a sequence
already used by another kyko action. The same settings are properties on `org.kyko.Terminal1`.

### 2.7 Staying in the background

- **Nothing in the window ends the process.** Closing a pane or tab closes only that; closing
  the last tab opens a fresh one and hides kyko. `Esc`, focus loss and the toggle key only hide.
- **The shells are not in the window process.** `kykod` holds them, so a crash, restart or
  upgrade of `kyko` ends nothing. The restarted window attaches, gets its tab and split layout
  back, and each pane replays its recent output (§2.8).
- **Stopping the window is harmless.** `kyko quit` stops only the window process; every shell
  keeps running, and the next `kyko toggle` or **Run in kyko** brings the window back through
  D-Bus activation with the same tabs. The hotkey does too on the `uq`, `hyprland` and `manual`
  backends, which run `kyko toggle`; the `portal` backend needs the window process alive, which
  `Restart=on-failure` and session start-up cover for everything except a deliberate quit.
- **Ending the sessions is deliberate and off-screen.** `kyko end-sessions` stops `kykod` and
  with it every shell. It refuses with exit 3, listing the tabs concerned, while any pane has a
  foreground program other than its shell; `--force` overrides. It is not bound to a key, shown
  in the window or offered in Spotlight.
- **Both come back.** Each user unit has `Restart=on-failure`. `kyko.service` wants and starts
  after `kykod.service`, and stopping or restarting `kyko.service` leaves `kykod.service` alone.
- **Limit: `kykod` itself.** If `kykod` crashes, or at logout, the shells end; panes then show
  "Session ended" and a restarted `kykod` starts empty. That is why `kykod` is kept minimal.
- **Session end:** on `SIGTERM`, or `SIGINT` (handled the same way), `kykod` sends `SIGHUP` to
  every shell, waits up to 2 s, then exits. Panes the user had closed whose shells are still
  running at the 2 s deadline are killed (`SIGKILL` to their process group), as closing them
  promised; every other shell gets the `SIGHUP` and the wait only.

### 2.8 The holder and its socket

- **Socket:** `$XDG_RUNTIME_DIR/kyko/hold.sock`, in a directory of mode 0700, the socket 0600.
  `kykod` checks the peer's uid with `SO_PEERCRED` and drops any other user. Whoever attaches can
  read pane output and type into shells, so the socket is as sensitive as the terminal itself.
- **One window at a time.** A second attach is refused while one is live.
- **Frames:** a `u32` length, a `u8` type, then the body. Window to holder: `Hello{version}`,
  `Spawn{pane, shell, args, cwd, env, cols, rows, ring_bytes}`, `Input{pane, bytes}`,
  `Resize{pane, cols, rows}`, `Close{pane}`, `Attach{pane}`, `SaveLayout{blob}`,
  `EndSessions{force}`. Holder to window: `Hello{version, panes, layout}`, `Output{pane, bytes}`,
  `Replayed{pane, truncated}`, `Info{pane, program, cwd}`, `Exited{pane, status}`,
  `SpawnFailed{pane, message}`, `Busy{panes}`. `Attach` is answered with the pane's ring as
  `Output`, then `Replayed`, then `Exited` if the pane's shell has already ended.
- **Version:** `Hello` carries one protocol number. On a mismatch (the window was upgraded and
  `kykod` is still the old one) the window does not attach; it shows a notice that sessions from
  before the update keep running in the old holder and names `kyko end-sessions` as the way to
  move over.
- **Output ring:** `kykod` always reads every PTY, so a program never blocks because the window
  is away. It keeps the last 2 MiB of each pane's output (setting, 256 KiB–16 MiB).
- **Replay on attach:** the window resets the pane's emulator and feeds it the ring, from the
  first line break only when `Replayed.truncated` says the ring has wrapped (it then starts
  mid-stream). It then sends `Resize` with the pane's current size. The kernel signals a program
  only when its size changes, so `kykod` answers the first same-size `Resize` after an `Attach`
  by sending `SIGWINCH` to the terminal's foreground process group itself, which makes
  full-screen programs such as `vim`, `less` and `top` redraw themselves; a later same-size
  `Resize` sends nothing, so a window that sends its size on every show does not make programs
  repaint. Limits, measured: a shell at its prompt redraws nothing; a setuid foreground program
  (`sudo vim`) cannot be signalled; programs behind `ssh` or `tmux` do not redraw, because an
  unchanged size is not forwarded. Output older than the ring is lost, and a shell prompt's
  colours can be wrong until its next line; this is the accepted cost of keeping emulation out of
  the holder.
- **Input:** `kykod` queues each pane's input for its program, at most 4 MiB per pane. Input
  beyond that is dropped, and so is all input after it until the queue has emptied, so a paste
  too large is cut short, never typed with a piece missing.
- **Layout:** the window sends its tab and split tree, names and focus as an opaque blob whenever
  it changes; `kykod` stores the latest and returns it in `Hello`. Panes in the blob that no
  longer exist are dropped; panes `kykod` has that the blob lacks get a tab each.
- **Foreground program and folder:** `kykod` reads them from the PTY's foreground process group
  and `/proc`, and sends `Info` when they change. It checks once after `Spawn` and after
  `Attach`, then whenever a pane's output has been quiet for 100 ms; never on a timer. Tab names
  and the busy check use these.

### 2.9 The hotkey

On Wayland no app can grab a key by itself: the compositor owns the keyboard. kyko "owning" its
hotkey therefore means kyko registers it with the compositor, keeps it registered, and changes
it from Preferences, without the user editing a compositor config. It does this through the
first backend that works:

| Backend | Used when | How |
|---|---|---|
| `uq` | `org.uq.Desktop1` is on the bus and offers app hotkeys | kyko sets `hotkeys.kyko` through `uqd`, which writes the bind into its generated Hyprland config (a uQ change, §3.3) |
| `kde` | Plasma: `org.kde.kglobalaccel` is on the bus | kyko registers `toggle` with KGlobalAccel over D-Bus and sets or changes its key there |
| `portal` | the desktop portal offers `org.freedesktop.portal.GlobalShortcuts` and no backend above applies | kyko binds `toggle` with `hotkey.toggle` as the preferred trigger and listens for `Activated` |
| `hyprland` | Hyprland without uQ | a runtime bind through Hyprland's control socket, re-applied after each config reload |
| `manual` | none of the above | Preferences shows `kyko toggle` and a ready-to-paste bind line for the compositor in use |

- Preferences shows which backend is active and the key that is actually bound. With `portal`
  the compositor has the final say on the key and may show its own dialog; kyko displays what
  the portal reports.
- Changing the key unbinds the old one first. If the new key cannot be bound, the old one is
  restored and Preferences says why.
- The hotkey always calls the same `Toggle` as the command line, so `kyko toggle` bound by hand
  works everywhere regardless of backend.
- **What the spike found on Plasma (portal 1.22, version 2 of the interface):**
  - The portal binds and its activation reaches the app, and the preferred key is honoured the
    first time.
  - The portal identifies the app by its desktop entry and systemd unit, not its binary. With no
    desktop entry it refuses ("An app id is required"); started from another app's scope, the
    shortcut is filed under that app. §3.3 covers what kyko ships because of this.
  - Binding again with a different preferred key does not change the key. That is why Plasma has
    its own `kde` backend. The `kde` backend itself, and the portal's `ConfigureShortcuts` call,
    are untried; milestone 3 starts by trying both.
- On desktops where only `portal` is available, Preferences may be unable to change the key
  itself; it then offers a button that opens the desktop's own shortcut settings through
  `ConfigureShortcuts`.
- Nothing about hotkeys is verified on Hyprland (§6).

### 2.10 Environments

| Environment | Window | Hotkey | Theme | Extras |
|---|---|---|---|---|
| uQ | layer shell | `uq` | follows uQ live | Spotlight, Settings link |
| Plasma (KWin) | layer shell | `kde`, else `portal` | built-in or custom | — |
| Hyprland without uQ | layer shell | `portal` or `hyprland` | built-in or custom | — |
| Sway, niri, river, other layer-shell compositors | layer shell | `portal` if offered, else `manual` | built-in or custom | — |
| GNOME (no layer shell) | an undecorated window, placed by the compositor; best effort | `portal` | built-in or custom | — |

kyko never requires `uqd`, Quickshell or Hyprland to be installed; the package depends only on
Qt 6, `layer-shell-qt` and a D-Bus session. uQ features switch on when uQ is detected.

### 2.11 Secure, efficient, modern

**Secure**

- The holder socket is private to the user and checks the peer (§2.8).
- kyko makes no network connections. A URL opens only on `Ctrl`+click, through the desktop
  portal's OpenURI, so the window process never spawns a browser itself.
- Escape sequences cannot read the clipboard, cannot report the window title back, and can set
  the clipboard only when `advanced.programs_write_clipboard` is on.
- Multi-line paste asks first (§2.4); `Run` can be switched off (`advanced.allow_run`).
- `kyko-core`, `kyko-proto` and `kyko-term` carry `#![forbid(unsafe_code)]`; `kykod` and the
  window use `unsafe` only at the PTY and Qt boundaries, each block commented with its reason.
- `kyko.service` (the window) runs with `NoNewPrivileges=yes` and
  `RestrictAddressFamilies=AF_UNIX`. `kykod.service` is deliberately not restricted: the shells
  it holds must be able to run `sudo`, `run0` and network tools.
- Supply chain: `Cargo.lock` is committed, CI runs `cargo deny` (advisories and licences),
  GitHub Actions are pinned by commit, and releases carry checksums and build attestations (§8).

**Efficient** (targets, confirmed or revised with measurements in milestone 1)

- Hidden with idle shells: no repaints and no timers; both processes sleep in `poll`.
- Shown: only changed rows are rebuilt. A completely full screen (every cell printable, colour
  changing every few cells) changes within one frame at 60 Hz, measured from Qt's frame signals
  on integrated graphics. The spike measured about 1.3 ms of CPU per such frame at 211×51.
- Summon in under 100 ms.
- Memory is budgeted as the process's own (anonymous) memory, since resident memory is mostly
  shared Qt and graphics libraries (the spike measured 172 to 177 MB resident with one pane, of
  which 59 to 64 MB was its own). Targets: the window under 100 MB of its own memory with three
  tabs and empty scrollback; `kykod` under 10 MB plus the output rings.
- Scrollback has its own budget: a full 10,000-line pane is estimated at tens of megabytes (not
  measured). Milestone 1 measures it and, if it exceeds 40 MB a pane, lowers the default.

**Modern**

- Wayland-native, fractional scaling, the protocol support in §2.4, live theming, and
  configuration that applies without a restart.

## 3. Integration with uQ

### 3.1 D-Bus: `org.kyko.Terminal1` at `/org/kyko/Terminal1`

| Member | Arguments | Effect |
|---|---|---|
| `Toggle`, `Show`, `Hide` | — | as named |
| `Run` | `command: s`, `cwd: s` (may be empty) | new tab, runs `command` through the user's shell, shows kyko |
| `NewTab` | `cwd: s` | new tab, shows kyko |
| `ListTabs` | — | `a(uss)`: id, name, foreground program |
| `FocusTab` | `id: u` | focuses the tab, shows kyko |
| `Quit` | — | stops the window process only; shells keep running (§2.7) |
| `EndSessions` | `force: b` | as `kyko end-sessions` (§2.7); error `org.kyko.Error.Busy` when refused |
| properties | the settings in §2.6, plus `Visible: b` | read and write (`Visible` read-only) |
| signals | `VisibleChanged(b)`, `TabsChanged()` | — |

`Run` accepts commands of 1–4096 bytes with no NUL. The tab stays open after the command exits,
showing its exit status, until the user presses a key. Any same-user process can call `Run`; this
matches uQ's threat model, in which same-user processes are trusted.

### 3.2 Spotlight

**Run in kyko (a change in the uQ repo).** The bridge's built-in `run` provider returns its item
with actions `["run", "run-in-kyko"]` when `org.kyko.Terminal1` is owned or activatable, and
`["run"]` otherwise. The second action calls `Run` with the command mapped to that generation's
`run-<generation>` token. The first action, and Enter, are unchanged. Spotlight shows the second
action as "Run in kyko" (`Shift+Enter`).

**Open tabs (no uQ change).** kyko owns `/org/uq/Launcher1` with `org.uq.Launcher1.Provider` and
installs `/usr/share/uq/launcher/kyko.provider` (`BusName=org.kyko.Terminal1`, `Name=kyko`, no
`Prefix`). `Search` returns tabs whose name or foreground program matches, at most 20, answered
from memory well inside uQ's 300 ms deadline. `Activate` accepts only ids kyko returned for a tab
that still exists, and only focuses that tab. This route never runs a command.

### 3.3 Other pieces

- **App hotkeys (a change in the uQ repo):** a new extension point. An app installs
  `/usr/share/uq/hotkeys/<id>.toml` naming its command (`kyko toggle`) and default key; only
  `/usr` is searched, so the command comes from a package, never from user-writable data. `uqd`
  stores the user's key as `hotkeys.<id>`, validates it, refuses one that collides with a uQ
  bind, and writes the bind into its generated Hyprland config. This is specified in uQ's own
  Phase 4 spec; until it exists, kyko on uQ falls through to the next backend in §2.9.
- **Settings page:** `/usr/share/uq/settings/pages/kyko/` with a `page.toml`
  (`category = "apps"`, `dbus_name = "org.kyko.Terminal1"`). It shows the hotkey, shape and theme
  source, and a button that opens kyko's own Preferences for everything else.
- **App identity:** kyko's app id is `org.kyko.Kyko`. The package ships `org.kyko.Kyko.desktop`,
  and the window process runs in a unit the desktop portal maps to that id. The one arrangement
  shown to work is a unit named `app-<id>-<n>.scope`; a unit simply named `kyko.service` probably
  does not map. Milestone 3 settles this by trying, in order: naming the service unit
  `app-org.kyko.Kyko.service`, and declaring the id through the portal's host Registry. A test
  asserts that a bind lands under `org.kyko.Kyko`.
- **Start-up:** two systemd user units bound to `graphical-session.target` (`kykod.service`, and
  the window's unit, called `kyko.service` in this document, which wants it), and a D-Bus service file so the first `Toggle` or `Run` starts
  the window if it is not running.

## 4. Errors

| Situation | Behaviour |
|---|---|
| No layer-shell support | The best-effort undecorated window of §2.10; logged once and noted in Preferences |
| No Wayland display | Exit 1 with a one-line message |
| A screen needs more new glyphs than fit in one frame's budget | The rest appear over the next frames; nothing blocks |
| The GPU or driver cannot run the scene graph | Qt's software backend is used; logged once and noted in Preferences |
| No hotkey backend works | `manual`: Preferences shows the bind line; `kyko toggle` still works |
| The chosen hotkey is taken or refused | The previous key stays bound; Preferences shows the backend's reason |
| The portal or `uqd` restarts | kyko re-registers the hotkey when the service returns |
| `uqd` absent, or `tokens.json` missing or bad | Built-in palette; logged once; retried on file change |
| Bad `config.toml` field | That field's default; a warning in the log and on the settings page |
| Shell fails to start | The pane shows the error and the command tried; any key closes it |
| `Run` with an invalid command | D-Bus error `org.kyko.Error.InvalidCommand`; no tab opens |
| A second `kyko` daemon starts | It finds the bus name taken, calls `Show` on the owner and exits 0 |
| The window process crashes | systemd restarts it hidden; it attaches and the tabs return with replayed output (§2.8) |
| `kykod` crashes | The shells end; panes show "Session ended"; systemd restarts `kykod` empty (§2.7) |
| `kykod` not reachable when the window starts | The window shows "Sessions aren't available" with the socket error and retries every 2 s |
| Protocol version mismatch | No attach; the notice in §2.8 |
| `kyko end-sessions` while programs are running | Exit 3 and a list of the busy tabs; nothing closes |
| CLI with no daemon and no activation | Exit 1: "kyko is not running" |
| The focused monitor is unplugged while shown | kyko moves to the monitor that now has focus |

## 5. Testing

- **Unit (`kyko-core`):** config parsing and per-field fallback; theme parsing against all seven
  uQ themes in both variants; session tree operations (split, close, focus by direction, last
  pane, last tab); tab naming; `Run` validation; provider matching and id checks.
- **Unit (`kyko-term`):** recorded byte streams, asserting on grid snapshots: colours, wide
  characters, wrapping, resize, scrollback limits, alternate screen.
- **Unit (`kyko-proto`):** every frame round-trips; truncated, oversized and unknown frames are
  rejected without a panic.
- **Integration (`kykod`):** a real `kykod` on a temporary socket, driven by a test client: spawn
  and output, input, resize, exit status, ring limits, a program that keeps writing with no
  window attached, a second attach refused, a wrong-uid peer dropped (where the test can create
  one), version mismatch, the busy check, `SIGTERM` hanging up the shells.
- **Integration (reattach):** kill the client mid-output, attach again, and assert the replayed
  screen matches for a shell session and for a full-screen program after the resize nudge.
- **Integration:** the D-Bus interface against a private `dbus-daemon`, with the window replaced
  by a fake: `Toggle`, `Run`, `ListTabs`, `FocusTab`, the provider's `Search` and `Activate`, the
  single-instance rule, `Quit` leaving the shells running, `EndSessions` refused while a program
  runs and allowed with `force`, and the last tab closing without either process exiting.
- **Live checklist** (`docs/notes/`): slide in both shapes and tab positions, focus-loss hiding,
  theme switch with panes open, input method, a 100,000-line `cat` staying responsive, `vim`,
  `htop`, `tmux`, the Spotlight action, multi-monitor.
- **Renderer:** the grid item rendered offscreen (Qt's software backend, so it runs in CI) and
  compared with reference images: styles, colours, wide characters, a colour emoji, a glyph
  missing from the font, and the same at scale 1.5. A frame-time bench (`kyko bench`, not in CI)
  fills the screen as the spike's `gen.py` did and reports sync, render and frame interval from
  Qt's frame signals; it is run on real hardware before each release.
- **Unit (hotkey):** backend selection from a faked environment; rebind, refusal and rollback
  against a fake backend; the `kde`, `portal` and `uq` backends against fake services on a private bus. On Plasma, a live
  check that a bind lands under `org.kyko.Kyko` and that changing the key in Preferences changes it.
- **uQ VM session:** uQ's harness (`~/Projects/uq/tests/vm/`) boots an Arch guest running uQ on
  Hyprland and already builds a sibling app, noti-fi, from its checkout. kyko joins it the same
  way: the harness builds kyko's package from `KYKO_SRC` (default `~/Projects/kyko`), installs it
  in the guest, and runs a `40-kyko.sh` check file: hotkey summons over a fullscreen window and
  takes the keyboard, hide, sessions survive `systemctl --user restart kyko`, theme switch
  recolours, **Run in kyko** from Spotlight, uQ unaffected with kyko removed. The harness change
  is made in the uQ repo.
- **CI:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `cargo deny`; the Qt app
  and the package build in an Arch container (§8).

The development machine runs Plasma, not Hyprland. KWin implements layer-shell, so the window
and the `portal` hotkey are exercised there day to day; everything uQ- and Hyprland-specific
runs in the VM session.

## 6. Risks

### 6.1 What milestone 0 settled

Two throwaway spikes ran on Plasma (KWin); their code is on the `spike/m0` branch and is not
carried over.

| Question | Answer |
|---|---|
| Layer shell from a `cxx-qt` app | Works from QML alone (`org.kde.layershell`): toggles, keeps state while hidden, and on the overlay layer shows above a fullscreen window |
| A terminal grid in Qt Quick, fast enough | Yes with a glyph atlas on scene-graph nodes: about 1.3 ms of CPU per full-screen change, 60 frames per second held. A `QQuickPaintedItem` (17 to 26 ms) and `QSGTextNode` rows (10 to 60 ms) are ruled out |
| `alacritty_terminal` as the engine | Works; a 100,000-line flood took 0.12 s and Ctrl+C written to the PTY during `yes` landed in 10 to 67 ms |
| A global hotkey through the portal | Binds and fires on Plasma, with the conditions in §2.9 and §3.3 |

### 6.2 Still open

Each is checked in the milestone named, before work is built on it.

1. **Everything on Hyprland** (milestone 1, in the uQ VM): layer shell from QML, the overlay layer
   over fullscreen, keyboard focus, paint speed on its renderer, and whether its portal binds a
   global shortcut. The VM run was skipped during the spikes because the machine was overloaded.
2. **Keyboard focus when summoned** (milestone 1): §2.2's "exclusive, then on-demand" scheme, focus
   returning to the previous window on hide, and `activateOnShow`. Needs real key presses.
3. **Real keyboard input and input methods** (milestone 1): every input in the spikes was injected
   into the PTY. The key-to-bytes mapping, and the window staying responsive to keys during a
   flood, are unexercised.
4. **The renderer's unbuilt parts** (milestone 1): the colour atlas for emoji, rasterising without
   stalling a frame, atlas growth, and crisp output at fractional scales. The spike ran at scale
   1, on one integrated GPU, on OpenGL only, with 96 distinct glyphs.
5. **Feeding the emulator from `kykod`** (milestone 1): the spike used `alacritty_terminal`'s own
   event loop, which owns the PTY. With `kykod` owning it, the window feeds bytes to the emulator
   itself; that path is unexercised. **Closed by slice 1b:** the end-to-end tests drive the
   emulator through `kykod` (`docs/notes/2026-11-10-m1b-checks.md`).
6. **The holder hop** (milestone 1): every output byte crosses a socket before it is drawn. Target:
   `seq 1 2000000` (about 14 MB of output) through `kykod` finishes within 15% of the same run
   with the PTY read directly, each taken as the median of five runs. **Closed by slice 1b:**
   1.10 to 1.13 times the direct run (`docs/notes/2026-11-10-m1b-checks.md`).
7. **Changing the hotkey on Plasma, and app identity** (milestone 3): the `kde` backend, the
   portal's `ConfigureShortcuts`, and which unit name or registration makes the portal see
   `org.kyko.Kyko`.
8. **Terminal modes after the ring has wrapped** (milestone 1, slice 1c): a reattach then loses
   not only old output but the terminal modes set before the cut: the alternate screen, mouse
   reporting, application cursor keys, the kitty keyboard protocol. A long-running `htop` or
   `vim` outlives a 2 MiB ring. To be designed in slice 1c, for example by `kykod` tracking those
   mode switches and putting them before a truncated replay.

If the renderer's unbuilt parts cannot be made to work on Hyprland or at fractional scales, the
fallback remains `alacritty_terminal` with `wgpu` in a plain layer surface, with QML out of the
terminal path; the spec is revised before going further.

## 7. Milestones

| # | Delivers |
|---|---|
| 0 | Done: two spikes (§6.1), written up in `docs/notes/` |
| 1 | The real workspace with **packaging, CI and release-on-bump first** (§8), then `kyko-proto`, `kykod`, `kyko-core`, `kyko-term` and the glyph-atlas renderer: one pane through the holder, real keyboard input, the open risks 1 to 6 of §6.2 checked, reattach after a window restart, both shapes, `kyko toggle`, theme sources, config file. Ends with `v0.1.0` published by CI |
| 2 | Tabs, splits, keys, scrollback search, copy and paste, close confirmations |
| 3 | App identity; the hotkey backends `kde`, `portal`, `hyprland` and `manual` (open risk 7 of §6.2 first); the Preferences window |
| 4 | uQ integration: `org.kyko.Terminal1` `Run`, the tab provider and the settings page here; **Run in kyko**, app hotkeys and the VM checks in the uQ repo; the `uq` hotkey backend |
| 5 | Hardening against §2.11's targets, the live checklist, README, `v1.0.0` |

Each milestone gets its own implementation plan and ends in a version bump, so each one is
installable. The uQ changes in milestone 4 are made in the uQ repo and follow its review rules.

## 8. Packaging and release

Modelled on uQ's (`~/Projects/uq/docs/releasing.md`), with the tagging automated.

- **Version:** the workspace `version` in `Cargo.toml` is the version. `packaging/kyko/PKGBUILD`'s
  `pkgver` follows it; a CI test fails when they differ or `.SRCINFO` is stale.
  `scripts/bump-version.sh X.Y.Z` updates all of them.
- **CI (`ci.yml`, every push and pull request):** in an `archlinux:latest` container: fmt, clippy,
  tests, `cargo deny`, the version test, and a full `makepkg` build of the package, uploaded as a
  workflow artifact so every commit has an installable build.
- **Release on version bump (`release.yml`):** on a push to `main`, if `Cargo.toml`'s version has
  no matching `vX.Y.Z` tag, the workflow runs the full CI, builds `kyko-X.Y.Z.tar.gz` with
  `git archive`, builds the package from that tarball in a clean Arch container, installs it there
  and checks that `kyko --version` and `kykod --version` report the version. Only then, in a
  separate job that builds nothing, are the tarball and the package attested (the attestations
  live in GitHub's attestation store and are checked with `gh attestation verify`), the tag created
  and the GitHub Release published, with the CHANGELOG section as notes and the tarball, its
  `.sha256` and the `.pkg.tar.zst` as assets. Versions below 1.0.0 and release candidates are
  pre-releases. A push that does not change the version releases nothing.
- **AUR:** the package is `kyko`, building from the release tarball. The workflow prepares the
  updated `PKGBUILD` and `.SRCINFO` with the published checksum and, when an `AUR_SSH_KEY`
  repository secret exists, pushes them to the AUR; without the secret it leaves them as an
  artifact for a manual push.
- **Package contents:** `/usr/bin/kyko`, `/usr/bin/kykod`, the two systemd user units, the D-Bus
  service file, `org.kyko.Kyko.desktop` (the app's identity, §3.3, and the launcher entry for Preferences), the icon, licences, and the uQ files that
  are inert elsewhere: `/usr/share/uq/launcher/kyko.provider`,
  `/usr/share/uq/settings/pages/kyko/`, `/usr/share/uq/hotkeys/kyko.toml`. Dependencies:
  `qt6-base`, `qt6-declarative`, `qt6-wayland`, `layer-shell-qt`, `dbus`; build only:
  `qt6-shadertools` (bakes the renderer's shader); optional:
  `xdg-desktop-portal` (hotkey and opening links). uQ is not a dependency.
- **Needs from the user:** a GitHub repository for kyko (`asafelobotomy/kyko`, to match uQ) before
  milestone 1's CI can run, and an AUR SSH key as a secret if AUR pushes are to be automatic.
