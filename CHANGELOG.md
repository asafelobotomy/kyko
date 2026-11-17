# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Prompt marks in the window (needs shell integration): a thin bar at the left edge of each
  command's prompt, green when it succeeded, red when it failed and amber while it runs;
  Ctrl+Shift+Up / Ctrl+Shift+Down jump to the previous / next prompt; Ctrl+Shift+O copies the
  last command's output. All three keys can be changed in Preferences > Keys.
- Shell integration, the same in bash, zsh and fish (`terminal.shell_integration`, on by default,
  in Preferences > Terminal): every new tab's shell reports its folder (OSC 7) and marks each
  prompt, command and exit status (OSC 133), and draws kyko's own prompt, rendered by the new
  `kyko-prompt` program (folder, git branch and state, virtualenv, slow-command time, exit
  status; terminal-palette colours, Nerd Font icons), unless your startup files set a prompt of
  your own or you use starship. Your rc files run first and are never replaced. Also loads
  zoxide, fzf and, when you have not set the alias yourself, eza and bat; zsh gets
  autosuggestions and syntax highlighting if the Arch packages are installed. Other shells start
  unchanged.
- `kyko run` with a command that begins `#!/bin/bash` (or zsh, fish, sh, dash, ksh, also via
  `env`) runs it in that shell instead of your login shell.
- Preferences: `kyko preferences`, or the ring button on the tab bar, opens a window with the
  Window, Terminal, Appearance, Keys and Advanced pages. A change is written to `config.toml` at once (the
  drop-down follows it), a value the config would refuse is shown with its reason instead of
  written, comments and other keys in a hand-edited file are kept, and "Reset this page" takes
  the page's keys out. Keys are set by clicking one and pressing the sequence (Esc cancels,
  Backspace clears); a sequence another action holds is refused. Sessions comes next.
- uQ's Spotlight lists kyko's open tabs: kyko serves `org.uq.Launcher1.Provider` at
  `/org/uq/Launcher1` and installs `/usr/share/uq/launcher/kyko.provider`. `Search` matches tab
  names and programs (at most 20); `Activate` only focuses a tab kyko returned and never runs a
  command. `ListTabs` and `FocusTab` are on `org.kyko.Terminal1` too.
- `Run` and `NewTab` on `org.kyko.Terminal1`, and `kyko run [-d DIR] COMMAND` and
  `kyko new-tab [-d DIR]`: a new tab in the drop-down that runs the command through the user's
  shell (1 to 4096 bytes, no NUL) and stays open after it ends, showing its exit status; the
  drop-down is shown. `advanced.allow_run = false` refuses `Run`.

## [0.1.0] - 2026-11-16

The first usable kyko: a windowed terminal and a drop-down, with tabs and splits. Not yet the
default in uQ; Preferences and the Run provider come in 0.2.0.

### Added

- The drop-down's toggle hotkey (`hotkey.toggle`, F12 by default): on Hyprland kyko binds it
  through the control socket, moves it when the config changes and binds it again after a
  Hyprland config reload; elsewhere it prints the line to bind by hand (`kyko toggle`).
- A desktop entry (`org.kyko.Kyko.desktop`, a terminal emulator that runs `kyko -e`) and an icon.
- Search in the scrollback (`search` key, Ctrl+Shift+F): a find bar over the focused pane with
  smart-case matching, a match count, Return / Shift+Return (or the arrow keys) for next and
  previous, and Escape to close; the current match is selected and scrolled into view. A match
  does not cross a wrapped line. Tabs rename on a double click; Return keeps the name, an empty
  name brings the automatic one back.
- Selection and the clipboard: drag, double click (word) and triple click (line) select, and
  `copy` / `paste` keys use the clipboard (bracketed paste when the program asked, escape
  characters stripped). The wheel scrolls the history, or sends arrow keys to a full-screen
  program, and programs that ask for mouse reports (click, drag, motion; SGR or legacy
  encoding) get them. `font_bigger`, `font_smaller` and `font_reset` change the size live.
  Closing a pane or tab that runs a program asks first. The multi-line paste prompt comes later.
- Tabs and splits in both the window and the drop-down: a tab bar (numbered pills, a close button
  on hover and on the active tab, middle-click to close, new-tab and split buttons, at the top
  or bottom per `window.tabs`), side-by-side and stacked splits with draggable dividers, and the
  `[keys]` actions (new tab, close pane, previous and next tab, Alt+1..9, split, focus by
  direction). The tabs, splits and focus are restored from `kykod` when the drop-down's window
  is started again. Closing the last pane of the last tab ends a windowed kyko, or hides the
  drop-down. Copy, paste, search and font-size keys come with later slices; closing a busy pane
  does not yet ask.
- The drop-down: `kyko dropdown` runs a layer-shell surface on the top edge (island or sheet,
  overlay or top layer, from `config.toml`) that slides in and out; `kyko toggle`, `show` and
  `hide` drive it over D-Bus (`org.kyko.Terminal1`, with a `Visible` property). Its shell lives in
  the shared `kykod`, so stopping and starting the window keeps it; a lost connection is retried.
  Focus loss hides it. A second drop-down refuses to start. `Esc` is passed to the program, not
  taken to hide (a deliberate change from the spec: it would break vim); the toggle key and focus
  loss hide it.
- Live reload: `config.toml` and uQ's `tokens.json` are watched, and the font, colours, shape and
  size follow a change without a restart.
- `kyko.service`, `kykod.service` and a D-Bus activation file for `org.kyko.Terminal1`.
- The window, windowed mode first: `kyko` opens a terminal window; `kyko -e CMD ARGS` runs a
  program in it (the window closes with the program's exit status), with `--title`, `--class`
  and `--working-directory`. It draws with a glyph atlas on Qt Quick's scene graph (one textured
  quad per cell, colours, bold, italic, underline, wide characters, the cursor), takes real
  keyboard and input-method input, and follows the config and theme. Each window runs its shells
  in a `kykod` of its own that ends with it. The drop-down, tabs and the hotkey come next.
- `kyko-term::keys`: key presses to xterm's bytes (arrows and application cursor mode, modifiers,
  function keys, Alt and Ctrl), with tests.
- `kyko-link`: the window's side of the `kykod` socket without Qt. It connects and exchanges
  Hellos (a version mismatch is an error), spawns and attaches panes, keeps one emulator per pane
  fed from the holder's output, rebuilds it from a replay, and answers the terminal's own replies.
  Tested against a real `kykod`, including a second window finding the first one's pane and screen.

## [0.0.4] - 2026-11-15

Nothing is usable yet: this is the logic the window will use, with no window around it.

### Added

- `kyko-core` reads `config.toml`: every setting of the Preferences table, with a bad value
  costing only its own key, unknown keys warned, and a file that does not parse at all kept
  from replacing the last good settings.
- Themes: uQ's live `tokens.json`, uQ's fourteen themes built in (with their credits, installed
  as `/usr/share/licenses/kyko/THEMES.md`), or custom terminal colours over a built-in.
- Tabs and split panes as data: focus by direction, tab names from the running program or
  folder, held panes that show their exit status, and the close rules.
- The saved layout: tabs and splits come back after the window restarts, reconciled with the
  shells `kykod` still has.

### Changed

- `kykod` refuses to start when the socket path holds a file that is not a socket, and leaves
  the file alone; before, it removed any such file as stale.
- `kykod` checks the socket path's length before creating anything, so a path too long to bind
  no longer leaves a folder and lock file behind.
- While a window is connected, `kykod` logs only the first connection it refuses and counts the
  rest in the line logged when the window leaves.

## [0.0.3] - 2026-11-14

### Added

- A window that reattaches after a pane's output ring has wrapped gets the pane back in the
  terminal modes its program had set: the alternate screen, mouse reporting, cursor keys and
  keypad, bracketed paste, focus reporting, the cursor style, the kitty keyboard flags and
  others. `kykod` reads them from the output it drops, without emulating the terminal.

### Changed

- The replay of a wrapped ring is cut by `kykod`, at a clean place (never inside an escape
  sequence or a character, and after a line break when there is one nearby), instead of by the
  window at the first line break. A full-screen program that draws without line breaks no longer
  replays as a blank screen.
- Protocol version 2. After upgrading from 0.0.2, a `kykod` 0.0.2 that is still running refuses
  the new version's window, and `kyko end-sessions` does not exist yet: restart it with
  `systemctl --user restart kykod`, which ends its shells.

## [0.0.2] - 2026-11-10

### Added

- `kykod`, the session holder: it owns the shells, each on its own pseudo-terminal, and keeps
  them running while no window is connected. It serves one window at a time over a private
  socket in `$XDG_RUNTIME_DIR/kyko/`, keeps the last 2 MiB of each pane's output and replays it
  when a window attaches, and ends the shells on `SIGTERM` or when asked.
- A systemd user unit for `kykod`, installed to `/usr/lib/systemd/user/kykod.service` and not
  enabled.
- The wire protocol between the window and `kykod`, and the terminal emulator the window will
  draw from. The `kyko` window itself is still to come, so nothing is usable yet.

## [0.0.1] - 2026-11-10

### Added

- Workspace, packaging and CI; no features yet.
