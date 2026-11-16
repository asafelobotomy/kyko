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

kyko is pre-1.0: Preferences, the Run provider and more hotkey backends are still to come.

## Building

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

kyko is licensed under the MIT licence.
