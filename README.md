# kyko

kyko is a drop-down terminal for the uQ desktop, in the manner of Yakuake: one key slides a
terminal down from the top of the screen, and the same key hides it again with its sessions still
running. It has its own tabs and split panes, follows uQ's theme live, and can be sent commands
from uQ's Spotlight search.

Nothing is usable yet. Version 0.0.1 is the workspace, packaging and CI only; the binaries `kyko`
and `kykod` print their version and nothing else.

## Building

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

kyko is licensed under the MIT licence.
