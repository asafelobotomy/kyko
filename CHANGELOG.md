# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `kykod` refuses to start when the socket path holds a file that is not a socket, and leaves
  the file alone; before, it removed any such file as stale.

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
