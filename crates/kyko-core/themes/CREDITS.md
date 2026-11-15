# Theme credits

kyko's built-in themes are uQ's (https://github.com/asafelobotomy/uq), rendered by uQ
and copied by `scripts/sync-uq-themes.sh`. uQ is under the MIT licence:
Copyright (c) 2026 asafelobotomy.

| Theme | Source | Licence |
|---|---|---|
| Gruvbox | Palette by Pavel Pertsev (morhetz), https://github.com/morhetz/gruvbox | MIT |
| Dracula (dark) | Dracula Theme, https://draculatheme.com | MIT |
| Dracula (light) | "Alucard", the Dracula Theme light palette | MIT |
| Matcha, Forest, Graphite, Rosewood, Velvet | Original palettes authored for uQ | MIT (uQ, https://github.com/asafelobotomy/uq) |

Where a published accent failed WCAG AA (4.5:1) against its own background, uQ darkens it
slightly: Gruvbox light uses `#946110` instead of `#b57614`, Matcha light `#197c69` instead of
`#1a7f6b`.

Each theme file holds semantic tokens; see uQ's `crates/uq-core/src/theme.rs` for the schema.
