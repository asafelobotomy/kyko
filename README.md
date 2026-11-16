# kyko

kyko is a drop-down terminal for the uQ desktop, in the manner of Yakuake: one key slides a
terminal down from the top of the screen, and the same key hides it again with its sessions still
running. It has its own tabs and split panes, follows uQ's theme live, and can be sent commands
from uQ's Spotlight search.

Windowed mode works from the build tree (`kyko`, `kyko -e CMD`); the drop-down, tabs and splits,
and the hotkey do not exist yet. Version 0.0.2 added `kykod`, the session holder that owns the shells and
keeps them running while no window is open, with a systemd user unit (installed, not enabled).
Version 0.0.3 has `kykod` restore a pane's terminal modes when a window reattaches after the
pane's output has outrun what `kykod` keeps.
Version 0.0.4 adds the logic the window will use, in `kyko-core`: settings, themes, tabs and
splits, and the saved layout.
The window, `kyko`, still prints its version and nothing else.

## Building

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

kyko is licensed under the MIT licence.
