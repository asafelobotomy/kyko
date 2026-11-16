# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
