# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.3] - 2026-11-25

### Fixed

- **The package builds from a kyko terminal.** The shell integration exported
  `KYKO_SHELL_INTEGRATION_LOADED`, so every bash started from a kyko shell inherited "already
  loaded" and skipped the integration. That included the bash integration tests that `makepkg`
  runs in `check()`, so an AUR upgrade run from a kyko terminal (`paru`, `bindleup`) failed all
  four of them. The guard is now a shell variable of its own (bash, zsh and fish), an inherited
  one from an older kyko is dropped, and the tests start bash with only the variables they set.
- **A flaky holder test.** The kyko-link tests connected as soon as kykod's socket file
  appeared, which is before kykod listens, so a slow start was refused and failed the release
  build's `check()`. They now retry for up to five seconds.

## [1.3.2] - 2026-11-23

### Fixed

- **Building the package from a terminal no longer stops it.** The bash integration tests,
  which `makepkg` runs in `check()`, started interactive bashes that took over the terminal the
  build ran in, so an AUR helper upgrade (`paru`, `bindleup`) was suspended as a stopped job
  partway through. Each test bash now runs in a session of its own, with no terminal to take.

## [1.3.1] - 2026-11-23

### Changed

- **uQ's paths and names come from uQ.** Where kyko finds uQ's `tokens.json` and the launcher provider's `/org/uq/Launcher1` path now come from uQ's shared uq-consts 1.0.0 (`crates/kyko-core/src/uq_consts.rs`, vendored unchanged). Like uQ, a relative `XDG_STATE_HOME` is now ignored. The theme watcher moves to uq-theme 1.1.0, and one `tests/shell/uq_native_pin.sh` (uQ's `scripts/check-uq-native.sh` with `uq/native.toml`) replaces the two pin tests and their three scripts.

## [1.3.0] - 2026-11-23

### Added

- **Click to move the cursor in the command line.** A plain click on the line being typed
  moves the shell's cursor there, in bash, zsh and fish (fish 4.1+ gets the click itself
  through `click_events`, the others get arrow keys).
- **Command blocks.** A click on a command's gutter selects its output; Ctrl+Alt+Shift+Up and
  Down jump to the previous and next failed command; Ctrl+Shift+K copies the last command.
- **The command stays pinned at the top** while you scroll through its long output.
- **Clickable file paths.** `path`, `path:line` and `path:line:col` (compiler, grep and test
  output) underline when the file exists, relative to the pane's folder, and open in
  `$EDITOR` at that line (vi, vim, nvim, nano, micro, kak, hx, emacs) or the default app.
- **Command palette** (Ctrl+Shift+P): fuzzy search over every action with its keys, and the
  last 50 commands run in any pane; picking one types it in the focused pane.

## [1.2.0] - 2026-11-23

### Added

- **The drop-down can come from any side.** `window.position` (Preferences › Window ›
  Position) is `top` (the default, below uQ's bar), `bottom`, `left`, `right` or `center`.
  From an edge it slides in; in the middle it fades in and grows from uQ's popup scale.
- **Drag the drop-down's edge to resize it.** Its free edges (the bottom edge at the top, the
  inner edge at a side, every edge in the middle) resize it, and the size is saved to
  `config.toml` for that position: top and bottom share `window.width_pct`/`height_pct`, the
  sides have `window.side_width_pct`/`side_height_pct` (40 by 100) and the middle
  `window.center_width_pct`/`center_height_pct` (60 by 60).
- **The screen behind dims while the drop-down shows.** `window.dim_pct` (40 by default, 0
  turns it off) darkens everything but uQ's bar; a click on the dimmed part hides the
  drop-down. Without the dim, clicks beside the drop-down reach the windows beneath.
- **The drop-down moves at uQ's speeds and follows Reduce motion.** The slide and fade use
  uQ's motion tokens (`tokens.json` `motion`, uQ 0.3.36): the panel comes in at the base speed,
  the dim at the slow one, and both leave at the fast one. With Reduce motion on, or
  `window.animations` off, it appears and goes at once.

- **A kyko page in uQ Settings** (Apps › kyko): position, shape, size, dim and when it hides,
  through the new `kyko config show --json` and `kyko config set KEY VALUE --json`.

### Changed

- **On uQ, the toggle key is uQ's.** When Hyprland's config already runs `kyko toggle` on a
  key (uQ's Settings › Shortcuts binds F12), kyko binds nothing itself, so one press no longer
  toggles twice. Elsewhere kyko still binds `hotkey.toggle` at runtime.
- `window.width_pct` now goes down to 20.

## [1.1.0] - 2026-11-22

### Added

- **Shell extras** (Preferences › Terminal › Shell extras, `terminal.shell_extras`, on by
  default): where your own shell files set none, kyko's bash and zsh keep a long history shared
  between tabs, complete in a menu (case-insensitive), and bind Home, End, Delete and
  Ctrl+arrows. Up and Down search history by what is typed (bash; zsh with
  `zsh-history-substring-search`). zsh gets fzf-tab when it is installed, and a missing command
  names the package it is in when `pkgfile` is installed (fish does this on its own). Plugins
  your own files already loaded are not loaded twice.
- **Command lines in the marks.** bash, zsh and fish send the command line with each command
  (`OSC 133;C;cmdline_url=`), and kyko keeps it: a long command's finish notification is now
  titled with the command instead of the tab. Groundwork for command blocks.
- **XTGETTCAP and XTVERSION answers**, so fish 4's Ctrl+L keeps the screen in the scrollback
  instead of erasing it.

### Fixed

- **fish 4 no longer gets every mark twice.** fish 4 marks prompts and commands itself; kyko's
  fish script now leaves that to it, and kyko reads fish's own marks, command lines and
  `click_events`.
- **bash marks were wrong on Arch.** Arch's `bash.bashrc` (and zoxide) add commands to
  `PROMPT_COMMAND` as an array; those ran under kyko's command hook, so every prompt was marked
  as a command that started and never ended. The gutter, "copy last output" and finish
  notifications were off by one command.
- **zsh printed `__kyko_precmd:4: unknown function: int`** after every command and skipped the
  folder report and prompt mark.
- **fish without `hostname` installed** (it is not in Arch's `base`) printed an error at every
  prompt.
- A command line longer than 48 bytes no longer drops the whole command-start mark.

## [1.0.0] - 2026-11-22

kyko's first stable release: the 1.0.0 release candidates (rc.1 to rc.3) with nothing added.
kyko is uQ's default terminal from here, in place of kitty. See the release candidates below for
what changed since 0.6.0.

## [1.0.0-rc.3] - 2026-11-22

### Fixed

- **Images can no longer reach outside the temporary folders.** A graphics-protocol temporary
  file named through `..` or a symlink (`/tmp/x-tty-graphics-protocol/../../home/…`) could have
  any of your files that had `tty-graphics-protocol` in its path read and then deleted by
  printing an escape sequence (a `cat` of a crafted file was enough). The checks now run on the
  real path: a temporary file must sit directly in `/tmp`, `/dev/shm` or `$TMPDIR` with the
  marker in its own name, and `/proc`, `/sys` and `/dev` stay out of reach for named files.
- **A long paste no longer cuts the window off from its shells.** Input of more than 1 MiB
  (a big paste, or `kyko @ send-text --stdin`) went to kykod in one frame, which kykod refuses,
  and it dropped the connection. Input now goes in 256 KiB pieces.
- **A notification flood is cut short.** A program printing OSC 9/777/99 in a loop started a
  thread and a D-Bus connection per notification; now at most 5 are shown in 10 seconds.
- **Huge images are refused before memory is taken.** A few bytes of PNG claiming 10000×10000
  made kyko allocate 400 MB before finding the data missing; images are now capped at 25
  megapixels up front, as sixel already was. Very large `c=`/`r=`/`X=`/`Y=` values and sixel
  HLS hues no longer overflow.

### Added

- **`kyko --listen-on unix:PATH`** names the window's `kyko @` socket, so the program that
  opened it can open tabs in it later with `kyko @ --to unix:PATH launch --type tab …` (what
  uQ's docked look=see>do_ terminal needs, as kitty's `--listen-on` does).

## [1.0.0-rc.2] - 2026-11-22

### Changed

- **Corners follow uQ's radius.** The tab pills, buttons, pane borders, the Unicode picker, the
  close dialog and the Preferences sidebar take their corners from the theme's `ui.radius`
  instead of fixed sizes, so a square or extra-round uQ theme reaches them too (at kyko's own
  default of 10 they look as before).
- **No polling for live reload.** uQ's theme is followed with uQ's shared watcher (`uq_theme`,
  vendored at 1.0.0 and pinned by `scripts/check-uq-theme.sh`), and `config.toml` with an
  inotify watch, in place of a check twice a second.

### Added

- **Problem codes and crash records for uQ.** kyko's and kykod's warnings and errors start with
  a code, `[KYKO-003] hotkey …`, listed with what each means and how to fix it in
  `/usr/share/uq/diag/kyko.toml`, so `uqctl log`, `uqctl explain KYKO-003` and uQ's
  Settings › Problems can say what went wrong. Under systemd the lines carry their level, so the
  journal files them as warnings and errors. A panic in kyko or kykod leaves a crash record that
  uQ notices and lists. This uses uQ's `uq-diag` (1.0.1, vendored in `crates/kyko-diag` and
  pinned by `tests/shell/uq_diag_pin.sh`).
- AppStream metainfo (`/usr/share/metainfo/org.kyko.Kyko.metainfo.xml`), so software centres
  list kyko with its description.

### Fixed

- The desktop entry no longer repeats its generic name ("Terminal") as a keyword.

## [1.0.0-rc.1] - 2026-11-20

The release candidate for 1.0.0: kyko now matches kitty where a daily terminal needs it, and goes
past it with sixel images and a drop-down that keeps its sessions. 1.0.0 follows once it has been
checked on real hardware (`docs/notes/2026-11-13-v1-release-checklist.md`).

### Changed

- The README says what kyko does, and how to install, start, bind and remove it.

## [0.6.0] - 2026-11-20

### Added

- **Images in the terminal: kitty's graphics protocol.** PNG and raw RGB/RGBA, zlib-compressed
  or not, sent in pieces or as a file; placed, cropped, scaled to cells, drawn under or over the
  text, deleted and queried. Images scroll with their lines, and programs such as `chafa`,
  `timg`, `yazi`, `ranger` and `neovim`'s image plugins can show pictures in kyko as in kitty.
- **Sixel images** too, which kitty does not draw: `img2sixel`, `chafa -f sixels`, `lsix` and
  the like work in kyko.
- **`CSI 16 t`** answers with the cell size in pixels, which image programs ask for.
- **Ctrl+Shift+U types a character by its code point or name** (`1f600`, `smiling face`),
  as kitty's Unicode input does; names come from the Unicode character database when it is
  installed.
- **Programs can ask for the colours** (OSC 4, 10, 11, 12 with `?`), as editors do to tell a
  dark background from a light one.
- **OSC 52 sets the clipboard** when `advanced.programs_write_clipboard` is on (off by
  default). The setting was there before but did nothing. Programs still cannot read the
  clipboard.
- **`kyko icat FILE...`** shows images: PNG as it is, other formats through ImageMagick.

## [0.5.0] - 2026-11-20

### Added

- **`kyko @`: control a kyko window from a shell or a script**, like `kitty @`:
  `ls`, `send-text`, `get-text`, `launch` (a tab or a split, with a command), `focus-pane`,
  `focus-tab`, `close-pane`, `set-tab-title`, `zoom` and `broadcast`. Commands typed inside kyko
  reach their own window and pane (`$KYKO_LISTEN_ON`, `$KYKO_PANE_ID`); from elsewhere, give
  `--to unix:PATH`. The socket is in `$XDG_RUNTIME_DIR/kyko/` and only its user can open it.
  `kyko @ help` lists it all.
- **`kyko --session FILE`** opens the tabs and splits a file describes: one `kyko @` command per
  line (`launch --cwd ~/src`, `launch --type split-side htop`, `set-tab-title work`).
- **Ctrl+Shift+Z zooms the focused pane** to fill its tab; the other panes keep their size out of
  sight, and moving the focus brings the next one up.
- **Ctrl+Shift+B types into every pane of the tab** at once (a red border says so), and back.
- **A scrollbar** while the view is scrolled back: drag it to move through the history.
- **`window.opacity_pct`** (Preferences > Window): a see-through background, so Hyprland's blur
  shows behind the text. Cells a program coloured stay opaque.

### Fixed

- **A full-screen program could come back one row short after kyko reattached** (`less`
  until a key was pressed): the row kykod takes off to make it redraw went back too soon
  after, and the program could miss the second resize.

## [0.4.0] - 2026-11-20

### Added

- **The kitty keyboard protocol** (`CSI > flags u`, all five flags): programs that ask for it
  (Neovim, Helix, fish, kakoune) can tell Ctrl+I from Tab and Ctrl+[ from Escape, and see key
  releases, repeats and the Super key. kyko answers the protocol's queries and keeps a stack of
  flags per screen.
- **Links.** URLs in the text and OSC 8 hyperlinks underline under the pointer and open with a
  click (`xdg-open`; only http, https, ftp, file, mailto and gemini). In a program that takes
  the mouse, Ctrl+Shift+click opens them. **Ctrl+Shift+L labels every link on screen**: type a
  label to open its link, or its capital to copy it.
- **Desktop notifications from programs**: OSC 9, OSC 777 (`notify;title;body`) and kitty's
  OSC 99, shown through the desktop's notification service.
- **A notification when a long command finishes** (`terminal.notify_on_finish`: `never`,
  `unfocused` or `always`, for commands that ran `terminal.notify_after` seconds or more; off by
  default). It says whether the command failed and how long it took.
- **The bell** follows `terminal.bell`: `visual` flashes the pane, `sound` plays the desktop's
  bell (libcanberra's `canberra-gtk-play`), and either marks the window urgent when it is in
  the background. Preferences > Terminal now has the bell and the notification settings.
- **Primary selection**: selecting text sets it and a middle click pastes it, as on any Linux
  desktop. **Ctrl+Alt+drag selects a rectangle.**
- **Ctrl+Shift+H opens the scrollback in `less`** in a new tab, colours kept.
- **Paste guard**: pasting several lines into a shell that did not turn on bracketed paste asks
  first, so a pasted command cannot run by itself (`terminal.confirm_multiline_paste`).
- Focus reporting (`CSI ? 1004 h`), for programs that redraw when the window gains focus.

## [0.3.0] - 2026-11-20

### Added

- **A new text renderer.** Each run of text is shaped with HarfBuzz (through Qt), so:
  - **Ligatures** in fonts that have them (`=>`, `!=`, `->` in JetBrains Mono, Fira Code,
    Cascadia Code). `terminal.ligatures` (on by default, in Preferences > Terminal) turns them
    off; the character under the cursor is never joined.
  - **Colour emoji**, in their own colours, and **grapheme clusters**: combining accents,
    flags and joined emoji such as families draw as one character.
  - Fallback fonts for anything the terminal font lacks, placed glyph by glyph.
- **All five underline styles** (SGR 4:1 to 4:5: single, double, curly, dotted, dashed) and
  **underline colours** (SGR 58), as used by Neovim's diagnostics; **strikethrough** and **dim**
  text are drawn too.
- **Box drawing, block elements and Powerline separators drawn by kyko**, not the font, so lines
  meet across cells at any line height: light, heavy, double, dashed, rounded and diagonal
  lines, the block and shade characters, and U+E0B0 to U+E0BF.
- The window now follows `terminal.cursor` (block, beam, underline), `terminal.cursor_blink`,
  `terminal.line_height` and `terminal.padding`. An unfocused window shows the cursor as an
  outline.
- The desktop entry carries the `X-TerminalArg*` keys, so `xdg-terminal-exec` and launchers
  that read them pass the command, title, app id and folder the right way.

## [0.2.2] - 2026-11-20

### Changed

- **New icon:** two mahjong tiles with red glyphs.
- Preferences: the 'New tabs start in' choices say what each one means.

### Fixed

- **Full-screen programs such as htop repaint everything after you reattach a tab.** Attaching now takes a row off the terminal and the window gives it back, a real size change both ways, instead of a same-size nudge that some programs answered by redrawing only what had changed.

## [0.2.1] - 2026-11-17

### Changed

- **Preferences: an empty Font field now shows the theme's font** as its hint, so you can see what
  you get when you leave it blank.

## [0.2.0] - 2026-11-17

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
