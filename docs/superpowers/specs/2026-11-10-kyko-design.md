# kyko: design

Date: 2026-11-10 · Status: awaiting spec review
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

**Assumptions** (not yet confirmed by the user)

- Split panes are in the first release.
- kyko hides when it loses focus unless pinned.
- Tabs are named after the foreground program or folder and can be renamed.
- The toggle key is `F12`, as in Yakuake.
- kyko also lists open tabs in Spotlight ("Switch to: ssh nas"). This is the only use of the
  external-provider route; it can be cut without affecting anything else.
- License is MIT, as uQ.

**Success looks like**

- `F12` shows kyko in under 100 ms with the previous sessions intact; `F12` again hides it.
- Switching uQ's theme recolours kyko, including running panes, without a restart.
- `> cargo build` in Spotlight, then **Run in kyko**, opens a new tab running the command and
  drops kyko down.
- uQ runs unchanged with kyko not installed. kyko runs, with a built-in theme, without `uqd`.

**Not in scope**

- X11. kyko is Wayland-only and needs a compositor with `wlr-layer-shell`.
- Session restore across logout or reboot, ligatures, inline images (sixel, kitty graphics),
  remote control of pane contents, scripting, and plugins.
- Being a general windowed terminal. `shell.terminal` in uQ stays kitty, foot or alacritty.

## 2. Architecture

One long-lived process per session, `kyko`, started with the graphical session and D-Bus
activatable. The same binary is the command-line client: with a subcommand it calls the running
instance over D-Bus and exits.

```
kyko (Cargo workspace, edition 2024, stable toolchain)
  crates/kyko-core   pure logic, no Qt: config, theme, session tree, command queue, provider index
  crates/kyko-term   terminal engine: PTY + alacritty_terminal, exposes a cell-grid snapshot
  apps/kyko          cxx-qt + QML: window, tab bar, panes, D-Bus service, CLI
  packaging/         PKGBUILD, systemd user unit, D-Bus service file, .provider file, settings page
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
| `kyko-term` | One `Term` per pane: spawns the shell on a PTY, feeds bytes to `alacritty_terminal`, hands out damage-tracked grid snapshots, takes key and mouse input | alacritty_terminal |
| `apps/kyko` grid item | A custom `QQuickItem` that draws one pane's grid snapshot and forwards input | kyko-term, Qt |
| `apps/kyko` window | The layer-shell surface, slide animation, tab bar and split layout in QML | layer-shell-qt, Qt Quick |
| `apps/kyko` service | `org.kyko.Terminal1` and `org.uq.Launcher1.Provider` on the session bus | zbus |
| `apps/kyko` cli | `kyko toggle`, `show`, `hide`, `run`, `new-tab`, `quit` | zbus |

`kyko-core` and `kyko-term` have no Qt dependency, so they are tested with plain `cargo test`.

### 2.2 Window

- A layer-shell surface on the `top` layer, anchored to the top edge of the focused monitor,
  keyboard interactivity `on-demand`, namespace `kyko`.
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
- Rendering: a monospace grid from `fonts.mono` at `fonts.size`, with Qt's font fallback for
  missing glyphs, wide characters, bold, italic, underline, strikethrough, 256 and true colour.
- The 16 ANSI colours, foreground, background, cursor and selection come from the theme.
- Mouse: selection, word and line selection, mouse reporting to programs, wheel scrollback,
  Ctrl+click on a URL opens it with `xdg-open`.
- Input methods work through Qt's input-method support.
- Bracketed paste is on; a multi-line paste into a shell that has not enabled it asks first.

### 2.5 Theme

- Source: `~/.local/state/uq/theme/tokens.json`, which is uQ's theme serialised as
  `{id, name, variant, colors, ui, fonts, icons, terminal: {ansi: [16]}}`.
- kyko subscribes to `ThemeChanged` on `org.uq.Desktop1` and re-reads the file; it also watches
  the file, so it follows the theme when `uqd` restarts.
- If the file is missing or does not parse, kyko uses a built-in dark palette and logs once.
  A missing field falls back for that field only.
- `[theme] follow_uq = false` in the config pins the built-in palette or a user palette.

### 2.6 Configuration

`~/.config/kyko/config.toml`, read at start and on change. Unknown keys are ignored with a warning.

| Key | Values | Default |
|---|---|---|
| `window.shape` | `island`, `sheet` | `island` |
| `window.tabs` | `top`, `bottom` | `top` |
| `window.height_pct` | 20–100 | 45 |
| `window.width_pct` | 40–100 (island only) | 88 |
| `window.hide_on_focus_loss` | bool | true |
| `window.animations` | bool | true |
| `terminal.scrollback` | 0–100000 | 10000 |
| `terminal.font_size` | 6–48, or unset to follow the theme | unset |
| `terminal.shell` | path, or unset for `$SHELL` | unset |
| `theme.follow_uq` | bool | true |
| `keys.*` | key sequences for the actions in §2.3 | as §2.3 |

The same settings are properties on `org.kyko.Terminal1`; setting one writes the file. The uQ
Settings page (§3.3) uses those properties.

## 3. Integration with uQ

### 3.1 D-Bus: `org.kyko.Terminal1` at `/org/kyko/Terminal1`

| Member | Arguments | Effect |
|---|---|---|
| `Toggle`, `Show`, `Hide` | — | as named |
| `Run` | `command: s`, `cwd: s` (may be empty) | new tab, runs `command` through the user's shell, shows kyko |
| `NewTab` | `cwd: s` | new tab, shows kyko |
| `ListTabs` | — | `a(uss)`: id, name, foreground program |
| `FocusTab` | `id: u` | focuses the tab, shows kyko |
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

- **Toggle key (a change in the uQ repo):** `base.lua` binds `F12` to `kyko toggle`, guarded so it
  does nothing when kyko is not installed.
- **Settings page:** `/usr/share/uq/settings/pages/kyko/` with a `page.toml`
  (`category = "apps"`, `dbus_name = "org.kyko.Terminal1"`) and a QML page for §2.6.
- **Start-up:** a systemd user unit bound to `graphical-session.target`, and a D-Bus service
  file so the first `Toggle` or `Run` starts kyko if it is not running.

## 4. Errors

| Situation | Behaviour |
|---|---|
| No layer-shell support | Exit 1 with a one-line message naming the missing protocol |
| `uqd` absent, or `tokens.json` missing or bad | Built-in palette; logged once; retried on file change |
| Bad `config.toml` field | That field's default; a warning in the log and on the settings page |
| Shell fails to start | The pane shows the error and the command tried; any key closes it |
| `Run` with an invalid command | D-Bus error `org.kyko.Error.InvalidCommand`; no tab opens |
| A second `kyko` daemon starts | It finds the bus name taken, calls `Show` on the owner and exits 0 |
| CLI with no daemon and no activation | Exit 1: "kyko is not running" |
| The focused monitor is unplugged while shown | kyko moves to the monitor that now has focus |

## 5. Testing

- **Unit (`kyko-core`):** config parsing and per-field fallback; theme parsing against all seven
  uQ themes in both variants; session tree operations (split, close, focus by direction, last
  pane, last tab); tab naming; `Run` validation; provider matching and id checks.
- **Unit (`kyko-term`):** a real PTY running `printf` and `sh` scripts, asserting on grid
  snapshots: colours, wide characters, wrapping, resize, scrollback limits, alternate screen.
- **Integration:** the D-Bus interface against a private `dbus-daemon`, with the window replaced
  by a fake: `Toggle`, `Run`, `ListTabs`, `FocusTab`, the provider's `Search` and `Activate`, and
  the single-instance rule.
- **Live checklist** (`docs/notes/`): slide in both shapes and tab positions, focus-loss hiding,
  theme switch with panes open, input method, a 100,000-line `cat` staying responsive, `vim`,
  `htop`, `tmux`, the Spotlight action, multi-monitor.
- **CI:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`; the Qt app builds in an
  Arch container.

The development machine runs Plasma, not Hyprland. KWin implements layer-shell, so the window is
exercised there day to day; the uQ-specific rows of the live checklist need a Hyprland session.

## 6. Risks and the first milestone

Two things are unproven and everything else rests on them:

1. **Drawing the grid in a `QQuickItem` from Rust** fast enough, with correct font fallback and
   input-method behaviour. Target: a full 200×50 repaint in under 4 ms, and a 100,000-line `cat`
   that never blocks input.
2. **Layer-shell from a `cxx-qt` app** through `layer-shell-qt`: anchoring, on-demand keyboard
   focus, and unmapping without losing state, on both KWin and Hyprland. Not yet verified.

**Milestone 0 is a spike** that answers both: one themed pane in a layer-shell window, toggled
from the command line, measured against the targets above. Its code is throwaway. If the grid
item misses the targets, the fallback is to render with `alacritty_terminal` and `wgpu` in a
plain Wayland layer surface and keep QML out of the terminal path; the spec is revised before
any further milestone starts.

## 7. Milestones

| # | Delivers |
|---|---|
| 0 | Spike (§6); a written result and a go or revise decision |
| 1 | `kyko-core`, `kyko-term`, one pane, both shapes, toggle, theme following, config file |
| 2 | Tabs, splits, keys, scrollback search, copy and paste, close confirmations |
| 3 | `org.kyko.Terminal1`, CLI, `Run`, the tab provider; in uQ: **Run in kyko** and the `F12` bind |
| 4 | Settings page, packaging (PKGBUILD, units), README, live checklist |

Each milestone gets its own implementation plan. The two uQ changes in milestone 3 are made in
the uQ repo and follow its own review rules.
