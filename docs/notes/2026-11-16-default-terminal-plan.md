# kyko as uQ's default terminal: gap assessment and phased plan

Date: 2026-11-16. Based on kyko `main` at 86f5724 (v0.0.4, CI green), uQ and Bindle `main`.

## Where kyko stands

- Done: `kykod` (session holder with PTYs, emulator via alacritty_terminal, reattach, terminal-mode replay),
  `kyko-proto`, `kyko-term`, `kyko-core` (config, themes incl. uQ's 14 built in, tabs/splits data model, saved layout).
  Packaging, CI and release-on-bump work. Dependencies are current (only `errno` 0.3.14 -> 0.3.15 behind).
- Not started: **the window itself** (`apps/kyko` prints its version). No renderer, no keyboard/mouse input,
  no layer-shell surface, no D-Bus API, no hotkeys, no Preferences. The project's own checklist puts ~80% of the
  work still ahead, and the risk is in the renderer and focus/hotkeys.
- Releases are pre-releases until 1.0.0, so Bindle cannot pin kyko (retired in bindle#35) until then.

## The scope conflict

The kyko spec (section 1, "Not in scope") says kyko is a Yakuake-style drop-down and *not* a general windowed
terminal; `shell.terminal` was to stay kitty/foot/alacritty. Replacing kitty as the default needs a normal
windowed mode, so the spec has to change. (Decided 2026-11-16: drop-down plus general terminal; spec 2.12.)

## Daily-driver gaps vs kitty

Everything in the window layer, plus these not in the current spec:
1. Normal windows: `kyko -e CMD`, `--class/--title/--working-directory`, new-window per launch (SUPER+Return),
   `Terminal=true` .desktop launches, `xdg-terminal-exec` compatibility, an `org.kyko.Kyko.desktop`
   with `TerminalEmulator` category.
2. Window mode and drop-down mode sharing one session holder (kykod already supports many panes).
3. kitty keyboard protocol (blocked on alacritty_terminal release after 0.26.0, fix alacritty#9073).
4. OSC 8 links, synchronised output, styled underlines, focus reporting, bracketed paste, clipboard rules.
5. Scrollback search, selection, URL open, IME, fractional scale, fonts/fallback/emoji.
6. Not planned and probably acceptable to lose: kitty graphics protocol/sixel, ligatures, kitten, remote control.
   Need an owner call on whether any of these is a daily need.

## Phases

0. Safe now (this thread): dependency refresh PR; spec amendment draft; keep uQ/Bindle unchanged.
1. kyko v0.1.0: the window (milestone 1 of the checklist): Qt/QML + glyph-atlas renderer, one pane, input,
   layer-shell drop-down, `kyko toggle`, live theme. Test headless on Hyprland in uQ's CI job.
2. kyko v0.2.0: tabs, splits, copy/paste, search, scrollback, config.
3. kyko v0.3.0: windowed mode (`kyko -e`, `--class`, per-window sessions) + app identity/hotkeys + Preferences.
4. Paired uQ + Bindle PRs, no default change yet: uQ `shell.terminal` choice includes kyko; keybind and
   `kdeglobals` terminal derive from it (kitty stays default); Bindle catalog lists kyko once a stable tag exists.
   uQ settings page for kyko; Spotlight "Run in kyko"; theme sync via live tokens.json (already in kyko-core).
5. kyko v1.0.0 (stable, CI green, hardening per checklist) -> Bindle pins it, default install.
6. Flip the default: uQ `TERMINAL` command, `kdeglobals`, `FALLBACK_TERMINAL`, essentials list -> kyko,
   with kitty as fallback when kyko is missing. Bindle ships kyko by default, keeps kitty as optional fallback.

## Where uQ and Bindle hardcode kitty today (for phase 4 and 6)

- uQ: `crates/uq-core/src/keybinds/defaults.rs` (TERMINAL), `kdeglobals.rs`, `settings/shell.rs`,
  `uqctl/src/bridge/defaultapps.rs` (FALLBACK_TERMINAL), `config/hypr/base.lua`, kitty theme template and stub,
  `catalog/desktops.toml` essentials, `packaging/uq/PKGBUILD` optdepends, goldens in `tests/golden/generated`.
- Bindle: `catalog/arch/desktop.toml` kyko entry (pinned 0.0.2 earlier, retired), `packaging/appimage/AppRun`.
