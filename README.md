# kyko

kyko is a drop-down terminal for the uQ desktop, in the manner of Yakuake: one key slides a
terminal down from the top of the screen, and the same key hides it again with its sessions still
running. It has its own tabs and split panes, follows uQ's theme live, and can be sent commands
from uQ's Spotlight search.

kyko runs as a window (`kyko`, `kyko -e CMD`) or as the drop-down (`kyko dropdown`, toggled with
`kyko toggle`). Both have tabs, split panes, selection and the clipboard, scrollback with search, and
mouse reporting. Shells live in `kykod`, so the drop-down keeps them while hidden. On Hyprland the
toggle key (`hotkey.toggle`, F12 by default) is bound for you; elsewhere bind `kyko toggle` by hand.
The package installs a desktop entry and icon, so kyko shows up as a terminal emulator.

## What it does

- **Text:** ligatures, colour emoji, wide and combining characters, box drawing and Powerline
  shapes drawn to the pixel, five underline styles in colour, and true colour.
- **Images:** kitty's graphics protocol and sixel, so `chafa`, `timg`, `yazi`, `ranger` and
  editor image plugins show pictures. `kyko icat FILE` shows one.
- **Keyboard:** kitty's keyboard protocol, so editors and shells see every key and modifier.
  Every action has a key you can change in Preferences.
- **Tabs and splits:** split side by side or stacked, drag the dividers, zoom a pane
  (Ctrl+Shift+Z), and type into every pane of a tab at once (Ctrl+Shift+B).
- **Links:** URLs and OSC 8 links open on Ctrl+click; Ctrl+Shift+L labels every link on screen
  to open or copy it from the keyboard.
- **Shell integration:** jump between prompts, copy the last command's output, and get a
  desktop notification when a long command finishes while you look elsewhere.
- **Scrollback:** search it (Ctrl+Shift+F), open it in a pager (Ctrl+Shift+H), or drag the
  scrollbar.
- **Remote control:** `kyko @ ls`, `kyko @ launch`, `kyko @ send-text` and more, like
  `kitty @`, over a socket only you can open (`kyko --listen-on unix:PATH` names it, for
  `kyko @ --to unix:PATH` from outside); `kyko --session FILE` opens a set of tabs.
- **Also:** Unicode input by name (Ctrl+Shift+U), a see-through background, a bell that
  flashes, sounds or marks the window, colour queries, and a guard on multi-line pastes.
- **On uQ:** follows uQ's theme live, lists its tabs in Spotlight, and runs commands that
  Spotlight sends it.

See [the changelog](CHANGELOG.md) for each release and `kyko --help` for the command line.

## Install

On Arch, from the AUR:

```sh
paru -S kyko        # or yay -S kyko
```

The drop-down starts on its own the first time you run `kyko toggle` (or press its key). To have
it ready from login instead, enable the two user units:

```sh
systemctl --user enable --now kykod.service kyko.service
```

The toggle key is `hotkey.toggle` in Preferences (F12 by default). On Hyprland kyko binds it for
you; on other desktops bind a key to `kyko toggle` in the desktop's own shortcut settings.

For a normal window, run `kyko` or `kyko -e COMMAND`. Settings live in
`~/.config/kyko/config.toml`, and Preferences (the gear on the tab bar, or `kyko preferences`)
edits them; changes apply at once.

To remove it: `systemctl --user disable --now kyko.service kykod.service`, then
`paru -R kyko`.

## Building

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

kyko is licensed under the MIT licence.
