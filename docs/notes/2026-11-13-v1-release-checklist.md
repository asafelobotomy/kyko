# Release checklist to v1.0.0

Written on 2026-11-13 from a review of `main` at `e7b325a` (v0.0.2) against the design spec
(`docs/superpowers/specs/2026-11-10-kyko-design.md`, §7 milestones). Tick items as they land; each
milestone ends in a version bump through `docs/releasing.md`.

## Where things stand

- **Done:** milestone 0 (spikes), slice 1a (workspace, packaging, CI, release on bump), slice 1b
  (`kyko-proto`, `kyko-term`, `kykod` with tests, unit file, hop bench). Releases v0.0.1 and v0.0.2
  are published as GitHub pre-releases with attestations.
- **Checked locally on 2026-11-13:** `cargo build`, `cargo fmt --check`,
  `cargo clippy --all-targets -D warnings`, `cargo test --workspace` (144 passed, 1 ignored bench),
  `tests/shell/*.sh` all pass. `cargo deny` was not run locally (not installed); CI runs it.
- **Not started:** the `kyko` window (it prints its version and exits 1), `kyko-core`'s logic
  (only `VERSION` exists), everything in milestones 2 to 5, and every uQ-side change.
- **Roughly 80% of the spec is still to build.** The remaining risk sits in the renderer, focus
  and hotkeys, not in the holder.

## 0. Housekeeping (before more code)

- [ ] Confirm or change the spec's **Assumptions** (§1): split panes in the first release, hide on
      focus loss, tab naming, `F12`, the tab provider in Spotlight, MIT. The spec still says
      "awaiting spec review".
- [ ] Settle the next version number. The spec says milestone 1 ends at `v0.1.0`; `apps/kyko/src/main.rs`
      says "the window arrives in 0.0.3". Make them agree.
- [ ] Delete the stale `origin/m1a-foundation` branch: it is the pre-rewrite history, has no work
      missing from `main`, and lacks the `--init` and `less` fixes `main` has.
- [ ] Decide whether `origin/archive/pre-public-main` and `origin/spike/m0` stay (the spec cites
      `spike/m0`; keep that one).
- [ ] Write the implementation plan for slice 1c and later milestones under `docs/superpowers/plans/`.

## Milestone 1 (rest): one pane, end to end, `v0.1.0`

### Slice 1c: holder follow-ups
- [ ] Open risk 8: terminal modes after the ring wraps (alternate screen, mouse reporting,
      application cursor keys, kitty keyboard). Design and test, e.g. `kykod` tracks mode switches
      and prefixes a truncated replay.
- [x] `Info{program, cwd}` and `SaveLayout` storage are in `kykod` already (slice 1b).
- [ ] Window side: reconcile the layout blob on attach (drop dead panes, give orphan panes a tab
      each) and handle a protocol version mismatch; both tested from the window.

### `kyko-core`
- [ ] `config`: load and validate `~/.config/kyko/config.toml`, per-field fallback, unknown keys
      warned, live reload on change (§2.6 table).
- [ ] `theme`: parse uQ `tokens.json`; built-in fallback; `builtin:<id>` for uQ's seven themes in
      dark and light, with credits; `custom`. Tests against all fourteen.
- [ ] `session`: tab and split tree, focus by direction, last-pane and last-tab rules, naming
      (milestone 2 uses most of it, but the data model lands here).

### The window (`apps/kyko`, cxx-qt + QML)
- [ ] cxx-qt 0.10 + Qt 6.11 build in `build.rs` (moc, Qt shader tool) and in the Arch CI container.
- [ ] Link to `kykod`: Hello, Attach, replay, reconnect every 2 s, "Sessions aren't available"
      notice, version-mismatch notice.
- [ ] Glyph-atlas grid item in C++ (§2.4): coverage atlas plus colour emoji atlas, 2 ms
      rasterisation budget per frame, page growth and 10-minute eviction, device-pixel snapping at
      fractional scale, only changed rows rebuilt.
- [ ] Styles: bold, italic, underline, strikethrough, 256 colour and true colour, wide characters,
      font fallback.
- [ ] Keyboard to bytes, input methods, mouse selection and reporting, wheel scrollback.
- [ ] Layer-shell surface on `overlay`, `exclusive` then `on-demand` focus, `island` and `sheet`,
      tabs top or bottom, 150 ms slide, `animations = false`.
- [ ] Hide on focus loss unless pinned; `Esc` hides.
- [ ] `kyko toggle`, `show`, `hide` over D-Bus (`org.kyko.Terminal1`), single-instance rule.
- [ ] Theme sources wired to the renderer; live recolour of running panes.
- [ ] Undecorated-window fallback where layer shell is missing (GNOME), logged once.
- [ ] Qt software-backend fallback, logged once.

### Open risks to close in milestone 1 (§6.2)
- [ ] 1. Everything on Hyprland, in the uQ VM: layer shell, overlay over fullscreen, focus, paint
      speed, portal shortcut.
- [ ] 2. Focus when summoned and on hide (real key presses).
- [ ] 3. Real keyboard and input-method input; responsive during a flood.
- [ ] 4. Renderer's unbuilt parts: emoji atlas, frame budget, atlas growth, fractional scale.
- [x] 5. Feeding the emulator from `kykod` (slice 1b).
- [x] 6. The holder hop within 15% (slice 1b: 1.10 to 1.14).

### Tests and packaging for v0.1.0
- [ ] Renderer reference-image tests offscreen (software backend) in CI, incl. scale 1.5.
- [ ] Reattach test: kill the window mid-output, reattach, replayed screen matches for a shell and
      for a full-screen program.
- [ ] Each modern protocol in §2.4 (kitty keyboard, OSC 8, synchronised output, styled underlines,
      focus reporting) confirmed by a test or removed from the spec.
- [ ] PKGBUILD: `depends=(qt6-base qt6-declarative qt6-wayland layer-shell-qt dbus)`,
      `makedepends+=(qt6-shadertools)`, `optdepends=(xdg-desktop-portal)`.
- [ ] Package `kyko.service` (`NoNewPrivileges=yes`, `RestrictAddressFamilies=AF_UNIX`,
      `Wants=`/`After=kykod.service`), the D-Bus service file, `org.kyko.Kyko.desktop`, the icon.
- [ ] CHANGELOG, README updated; bump to `0.1.0`.

## Milestone 2: tabs and panes

- [ ] Tab bar: numbered pills, new-tab, split, pin, settings buttons; close on hover/active pill and
      middle-click; no quit control anywhere (§2.3, §2.7).
- [ ] Binary splits, draggable dividers, focus by `Alt+Arrow`.
- [ ] Tab names from foreground program or folder; rename that sticks until cleared.
- [ ] Closing last pane closes the tab; last tab opens a fresh one and hides.
- [ ] Close confirmation when the foreground program is not the shell.
- [ ] Default keys in §2.3, all settable.
- [ ] Scrollback (10,000 default, 0–100,000) and `Ctrl+Shift+F` search.
- [ ] Copy and paste; bracketed paste; multi-line paste confirmation; `copy_on_select`.
- [ ] Clipboard rules: escape sequences cannot read it; can write only with
      `advanced.programs_write_clipboard`; no title report-back.
- [ ] Ctrl+click URLs through the portal's OpenURI.
- [ ] Bell modes, cursor shapes and blink, padding, line height, font size keys.
- [ ] `kyko end-sessions` (exit 3 with busy tab list, `--force`) and `kyko quit` from the CLI.
- [ ] Measure scrollback memory per full pane; lower the default if over 40 MB (§2.11).
- [ ] Version bump.

## Milestone 3: identity, hotkeys, Preferences

- [ ] Open risk 7 first: which unit name or Registry call makes the portal see `org.kyko.Kyko`; a
      test asserts the bind lands there.
- [ ] Hotkey trait with backends `kde`, `portal`, `hyprland`, `manual` (`uq` waits for milestone 4);
      selection, rebind, refusal and rollback, re-register when the service restarts.
- [ ] Hotkey registration only while kyko is hidden (overlay would cover a portal dialog).
- [ ] `ConfigureShortcuts` button where the portal cannot change the key.
- [ ] Preferences window: Appearance, Window, Terminal, Keys, Sessions, Advanced; changes apply at
      once, written to `config.toml`; "Reset this page"; key capture refuses duplicates; shows the
      active hotkey backend and bound key; shows config warnings.
- [ ] Settings as properties on `org.kyko.Terminal1`.
- [ ] Unit tests with fake backends on a private bus; live check on Plasma.
- [ ] Version bump.

## Milestone 4: uQ integration

In kyko:
- [ ] `org.kyko.Terminal1` complete: `Run` (1–4096 bytes, no NUL, `InvalidCommand`, tab stays with
      exit status), `NewTab`, `ListTabs`, `FocusTab`, `Quit`, `EndSessions` (`org.kyko.Error.Busy`),
      `Visible`, `VisibleChanged`, `TabsChanged`; `advanced.allow_run`.
- [ ] Tab provider on `/org/uq/Launcher1`: `Search` (at most 20, well under 300 ms), `Activate`
      only for ids it returned.
- [ ] `uq` hotkey backend.
- [ ] Ship `/usr/share/uq/launcher/kyko.provider`, `/usr/share/uq/settings/pages/kyko/`,
      `/usr/share/uq/hotkeys/kyko.toml`.
- [ ] D-Bus integration tests against a private `dbus-daemon` with a fake window (§5).

In the uQ repo (its own review rules):
- [ ] **Run in kyko** action on the `run` provider (`Shift+Enter`), only when kyko is owned or
      activatable.
- [ ] App-hotkey extension point (`/usr/share/uq/hotkeys/*.toml`, `hotkeys.<id>` in `uqd`).
- [ ] VM harness builds kyko from `KYKO_SRC` and runs `40-kyko.sh`: summon over fullscreen with
      keyboard, hide, sessions survive `systemctl --user restart kyko`, theme switch recolours,
      Run in kyko, uQ unaffected with kyko removed.
- [ ] Version bump.

## Milestone 5: hardening and v1.0.0

### Targets (§2.11), measured on real hardware
- [ ] Summon in under 100 ms with sessions intact, over a fullscreen window.
- [ ] Hidden and idle: no repaints, no timers, both processes asleep in `poll`.
- [ ] Full-screen change within one 60 Hz frame on integrated graphics (`kyko bench`).
- [ ] Window under 100 MB own memory with three tabs; `kykod` under 10 MB plus rings.

### Live checklist (`docs/notes/`)
- [ ] Slide in both shapes and both tab positions.
- [ ] Focus-loss hiding and pinning.
- [ ] Theme switch with panes open.
- [ ] Input method.
- [ ] 100,000-line `cat` stays responsive.
- [ ] `vim`, `htop`, `tmux`, including after a window restart.
- [ ] Spotlight **Run in kyko** and tab switching.
- [ ] Multi-monitor, monitor unplugged while shown, fractional scale.
- [ ] Each environment in §2.10: uQ, Plasma, Hyprland, one other layer-shell compositor, GNOME
      best effort.

### Success criteria (§1), each demonstrated
- [ ] Hotkey shows kyko in under 100 ms over any window, keyboard in the terminal; again hides it.
- [ ] Version bump on `main` produces a release with an installable package and no manual step.
- [ ] uQ theme switch recolours kyko, running panes included, without a restart.
- [ ] `> cargo build`, Run in kyko, opens a new tab running it and drops kyko down.
- [ ] Killing the window with a build running loses nothing.
- [ ] uQ unchanged without kyko; kyko works with a built-in theme without `uqd`.

### Release gate
- [ ] No open Critical or Important issues.
- [ ] Security pass: socket and peer check, no network, clipboard rules, `unsafe` only at PTY and
      Qt boundaries with reasons, `kyko.service` hardening, `cargo deny` clean.
- [ ] README rewritten for users: install, enable the units, hotkey per desktop, config reference,
      uninstall.
- [ ] Spec status moved from "awaiting review" to final; assumptions confirmed.
- [ ] AUR: add the `AUR_SSH_KEY` secret (none exists today) or plan the manual push; the `kyko`
      AUR package does not exist yet, so the first push creates it.
- [ ] Optionally one `1.0.0-rc.N` through the workflow first.
- [ ] Bump to `1.0.0`: the first release published as a full release, not a pre-release.
- [ ] After release: copy the published checksum back into `packaging/kyko/` and check
      `yay -S kyko` on a clean system.
