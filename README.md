# kyko

kyko is a drop-down terminal for the uQ desktop, in the manner of Yakuake: one key slides a
terminal down from the top of the screen, and the same key hides it again with its sessions still
running. It has its own tabs and split panes, follows uQ's theme live, and can be sent commands
from uQ's Spotlight search.

Nothing is usable yet. Version 0.0.2 adds `kykod`, the session holder that owns the shells and
keeps them running while no window is open, with a systemd user unit (installed, not enabled).
The window, `kyko`, still prints its version and nothing else.

## Building

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

kyko is licensed under the MIT licence.
