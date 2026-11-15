# kyko Milestone 1d (`kyko-core`: config, theme, session) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything the window needs to decide, and nothing it needs to draw. `kyko-core` loads
and validates `config.toml`, turns uQ's `tokens.json`, a built-in theme or custom colours into
one resolved theme, and holds the tabs, their split trees, focus, names, pane states and close
rules, saved to and restored from `kykod`'s layout blob. After this slice the window (slice 1e
onward) only wires these to Qt: it watches files, listens on D-Bus, draws, and calls into
`kyko-core`.

**Architecture:** five modules, each pure functions over plain data. No I/O except reading a
file whose path the caller gives, no threads, no clocks, no Qt, no D-Bus. Nothing logs: every
problem comes back as a `Warning` value, and the window logs it (once per distinct warning) and
shows it on the settings page (§4).

```
crates/kyko-core/src/
  keys.rs     KeySeq: parse, print and match "Ctrl+Shift+T"
  config.rs   Config: the §2.6 table, per-field fallback, warnings, what changed
  theme.rs    Theme: tokens.json, the 14 built-ins, custom; resolved fonts and palette
  session.rs  Session: tabs, split trees, focus, names, pane states, close rules
  layout.rs   the layout blob: encode, decode, reconcile with kykod's panes
themes/       the 14 built-in themes as uQ renders them, and their credits
```

**Tech Stack:** `toml` 1.x and `serde_json` 1.x for parsing (both MIT or Apache-2.0, already
allowed by `deny.toml`), `serde` with `derive` for the layout blob only, `kyko-proto` for
`PaneId`, `PaneInfo` and the holder's limits. No new test dependencies: random tests use a
seeded generator written in the tests, as slice 1c's differential test does.

**Spec:** `docs/superpowers/specs/2026-11-10-kyko-design.md` §2.1 (the `kyko-core` units), §2.3
(tabs and panes), §2.5 (theme), §2.6 (the settings table), §2.7 and §2.8 (holder, layout), §3.1
(what D-Bus will ask of the session), §4 (errors), §5 (unit tests for `kyko-core`).

**Depends on:** slice 1c (v0.0.3) on `main`.

## Global Constraints

- `#![forbid(unsafe_code)]` stays. No dependency on Qt, zbus, or anything that spawns a thread.
- **A bad value costs only that key** (§2.6). Config and tokens are read as untyped trees
  (`toml::Table`, `serde_json::Value`) and each field is taken out by hand, so one wrong field
  never fails a whole section the way `#[derive(Deserialize)]` would. Out-of-range values fall
  back to the default; they are not clamped.
- **A file that does not parse at all is not a file of defaults.** A TOML or JSON syntax error is
  an `Err`, and the window keeps the last config or theme that loaded. A half-saved
  `config.toml` during hand editing must not reset the hotkey, the keys and the fonts for a
  moment, nor a `tokens.json` caught mid-write flash the fallback theme.
- **Never panics on input.** Any bytes as `config.toml`, `tokens.json` or a layout blob give a
  value or an error, never a panic, including integer overflow (`checked_add` on every id
  counter read from a blob). Each module has a test that feeds it random bytes.
- **Defaults live in one place.** `Config::default()` is the §2.6 table; a test compares it
  with the table written out in the test, so a drift in either is caught.
- **The session's invariants** (Task 4) hold after every operation, checked by a random test.
- **The blob fits what `kykod` stores:** `MAX_LAYOUT` (256 KiB), not `MAX_FRAME`. A larger
  `SaveLayout` makes `kykod` close the connection (`server.rs` ~519), and the window would
  reconnect and resend it forever.
- Each implementation step ends with `cargo test -p kyko-core`, `cargo clippy --all-targets -D
  warnings` and `cargo fmt --check` before its commit. `cargo deny` is not installed on the
  development machine; CI runs it on the PR.
- Version after this slice: `0.0.4`, released by CI. Nothing becomes usable; the window still
  arrives in `0.1.0`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Exited panes are closed in `kykod`.** `kykod` keeps an exited pane, counted against
   `MAX_PANES`, until it gets `Close` (`server.rs` ~496), and lists it in `Hello` with
   `PaneInfo.exited` set. Every path that drops a pane from the session sends `Close`. Tasks 4
   and 5.
2. **Per-field fallback** in config and tokens: one bad key, one warning, every other key kept;
   a syntax error keeps the last good file. Tasks 2 and 3.
3. **The session invariants** under random operations, and the close rules (last pane closes the
   tab; last tab is replaced and kyko hides). Task 4.
4. **Reconcile** with `kykod`'s panes: a blob from an older or newer window, a corrupt blob,
   panes that exited while no window was attached, panes the blob does not know, a new empty
   `kykod` after the old one crashed. Task 5.
5. **The worst-case blob** stays under `MAX_LAYOUT`, and the deepest tree decodes. Task 5.

---

### Task 1: Key sequences; the holder's limits move to `kyko-proto`

**Files:**
- Create: `crates/kyko-core/src/keys.rs`
- Modify: `crates/kyko-core/src/lib.rs` (modules), `crates/kyko-core/Cargo.toml`,
  `crates/kyko-proto/src/lib.rs` (`MAX_PANES`, `MAX_LAYOUT`), `apps/kykod/src/server.rs` (uses
  them from there)
- Test: unit tests in `keys.rs`

**Interfaces:**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mods { pub ctrl: bool, pub alt: bool, pub shift: bool, pub super_: bool }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),        // printable; ASCII letters stored upper-case, everything else as given
    F(u8),             // 1..=24
    Up, Down, Left, Right, PgUp, PgDn, Home, End, Insert, Delete,
    Tab, Enter, Esc, Space, Backspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeySeq { pub mods: Mods, pub key: Key }

impl std::str::FromStr for KeySeq { type Err = KeyError; }
impl std::fmt::Display for KeySeq {}   // canonical: Ctrl+Alt+Shift+Super+Key

impl KeySeq {
    /// Whether a key press, as the window has translated it, is this sequence.
    pub fn matches(&self, mods: Mods, key: Key) -> bool;
}
```

- Grammar: modifiers then a key, joined by `+`, case-insensitive. A trailing empty segment is
  the `+` key, so `Ctrl++` is Ctrl and `+`. `PageUp`/`PageDown`, `Escape`, `Return`, `Del`,
  `Ins`, `Meta`/`Win`/`Logo` (as Super) and `Control` are accepted as aliases and printed in the
  canonical name.
- One key only (no chords), at most 64 bytes. A key without modifiers is allowed (`F12`, the
  default hotkey). Only ASCII letters change case; `ß` stays `ß`.
- **Matching:** for a `Char` that is not an ASCII letter, Shift is ignored on both sides,
  because the layout decides whether it needs Shift: on most layouts `+` does, so `Ctrl++` is
  pressed as Ctrl, Shift and `=`, which Qt reports as Ctrl+Shift with `+`. For letters and
  named keys Shift counts. The window maps the keypad's `+`, `-` and digits to the same `Char`
  as the main keys before calling `matches`; that mapping is the window's.
- `Display` then `parse` gives the same `KeySeq`; `parse` then `Display` gives the canonical
  text. This is what `config.toml` and the Keys page show.
- `MAX_PANES` (256) and `MAX_LAYOUT` (256 KiB) move from `kykod` to `kyko-proto`, unchanged,
  because the window side has to respect them too.

**Steps:**
- [ ] **Step 1:** Tests: every default in §2.3 and `F12` parse and print canonically; aliases;
      `Ctrl++`, `Ctrl+-`, `Ctrl+0`; case; `ß` and other non-ASCII characters; errors (empty,
      `Ctrl+`, unknown name, two keys `Ctrl+A+B`, a modifier twice, `F0`, `F25`, control
      characters, 65 bytes); round-trip over every `Key` with every modifier set; `matches`
      with and without Shift for `+`, `-`, `T` and `PgUp`.
- [ ] **Step 2:** Implement. Add `toml`, `serde_json`, `serde` (`derive`) and `kyko-proto` to
      `kyko-core`'s dependencies now, so later tasks do not each touch the lock file. Move the
      two constants. `cargo test --workspace`, clippy, fmt.
- [ ] **Step 3:** Commit `feat(core): key sequences`.

### Task 2: Config

**Files:**
- Create: `crates/kyko-core/src/config.rs`, `crates/kyko-core/src/paths.rs`
- Test: unit tests in `config.rs`; `crates/kyko-core/tests/config.rs`

**Interfaces:**

```rust
pub struct Config {
    pub hotkey: Option<KeySeq>,             // hotkey.toggle; None when set to ""
    pub window: WindowConfig,               // shape, tabs, height_pct, width_pct, monitor,
                                            // above_fullscreen, hide_on_focus_loss,
                                            // animations, opacity
    pub theme: ThemeSource,                 // Task 3; Auto when unset
    pub custom: CustomColors,               // theme.custom.*, Task 3
    pub terminal: TerminalConfig,           // font, font_size, line_height, padding, cursor,
                                            // cursor_blink, new_tab_dir, bell, copy_on_select,
                                            // confirm_multiline_paste, scrollback, shell
    pub sessions: SessionsConfig,           // ring_kib
    pub keys: Keys,                         // one Option<KeySeq> per Action
    pub advanced: AdvancedConfig,           // programs_write_clipboard, allow_run
}

pub enum Monitor { Focused, Primary, Output(String) }

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    NewTab, ClosePane, PrevTab, NextTab, Tab(u8 /* 1..=9 */),
    SplitSide, SplitStacked, FocusLeft, FocusRight, FocusUp, FocusDown,
    Copy, Paste, Search, FontBigger, FontSmaller, FontReset,
}

pub struct Warning { pub key: String, pub message: String }
pub struct Loaded { pub config: Config, pub warnings: Vec<Warning> }
pub struct SyntaxError { pub line: usize, pub column: usize, pub message: String }

pub enum LoadError {
    Syntax(SyntaxError),
    Unreadable { path: PathBuf, message: String },   // permission, not UTF-8, a directory
}

impl Config {
    pub fn parse(text: &str) -> Result<Loaded, SyntaxError>;
    /// A missing file is the defaults with no warning; that is not an error.
    pub fn load(path: &Path) -> Result<Loaded, LoadError>;
    /// What differs from `old`, so the window re-applies only that (a font change rebuilds
    /// the glyph atlas; a key change does not).
    pub fn changes(&self, old: &Config) -> Changes;   // bit set: Hotkey, Window, Theme,
                                                      // Font, Terminal, Keys, Sessions, Advanced
}

impl Action { pub fn key_name(self) -> &'static str; pub fn default_seq(self) -> KeySeq; }

// paths.rs. The environment is a function so tests do not touch the process's.
pub fn config_path(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf>;
pub fn uq_tokens_path(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf>;
```

- Keys and ranges as the §2.6 table, defaults as written there. Integers are TOML integers;
  `window.opacity` and `terminal.line_height` accept integers and floats; NaN and infinities
  are bad values.
- `terminal.shell` must be an absolute path of at most 4096 bytes; whether it exists is
  `kykod`'s concern (`SpawnFailed`). `terminal.font` must not be empty. `window.monitor` is
  `focused`, `primary`, or any other non-empty string, taken as an output name.
- `keys.*` names, one per `Action`: `new_tab`, `close_pane`, `prev_tab`, `next_tab`,
  `tab_1`…`tab_9`, `split_side`, `split_stacked`, `focus_left`, `focus_right`, `focus_up`,
  `focus_down`, `copy`, `paste`, `search`, `font_bigger`, `font_smaller`, `font_reset`. A value
  of `""` unbinds the action.
- **A sequence used twice:** among the kyko actions and `hotkey.toggle`, the first holder in the
  order above (hotkey first) keeps it; each later one is unbound with a warning. Two sequences
  count as the same when either `matches` the other. This is the file-side rule; the Keys page
  refuses the duplicate before it is written (§2.6).
- **Warnings:** unknown keys and sections (`unknown key window.hieght`), wrong types, values
  out of range, bad key sequences, duplicate sequences. `Warning.key` is the dotted key.
- **Paths, as uQ resolves them** (`uq-core/src/paths.rs`), so kyko finds the same
  `tokens.json` uQ writes: `XDG_CONFIG_HOME` or `XDG_STATE_HOME` as given when set, else under
  `HOME` (`.config`, `.local/state`); `None` with neither set. Config:
  `<config>/kyko/config.toml`; tokens: `<state>/uq/theme/tokens.json`.
- Writing `config.toml` (the Preferences window) is milestone 3, with `toml_edit` so comments
  survive. Watching the file is the window's (slice 1e onward); on `Err` it keeps the last
  `Config` it had and shows the error.

**Steps:**
- [ ] **Step 1:** Tests: the empty file gives `Config::default()`, which matches the §2.6 table
      written out in the test, field by field; every key set to a valid non-default value; each
      key out of range, of the wrong type, and (for enums) unknown, each costing only that key;
      NaN and infinity for the float keys; unknown keys and sections; a syntax error gives
      `Err` with its line and column; a missing file, a directory, a non-UTF-8 file; the
      duplicate rule, including a key bound to the hotkey and `Ctrl++` against `Ctrl+Shift++`;
      `""` for the hotkey and for an action; `changes` for one key in each group; both paths
      with each environment.
- [ ] **Step 2:** Robustness: 10,000 seeded random byte strings and 10,000 random byte
      mutations of a full valid file through `parse`, asserting only that it returns. Then
      structured mutations of the same file, each replacing one value with a wrong type, an
      out-of-range value or a bad sequence: `Ok`, one warning naming that key, every other key
      unchanged.
- [ ] **Step 3:** Implement. Tests, clippy, fmt. Commit `feat(core): config with per-field
      fallback`.

### Task 3: Theme

**Files:**
- Create: `crates/kyko-core/src/theme.rs`, `crates/kyko-core/themes/<id>-<variant>.json` (14
  files), `crates/kyko-core/themes/CREDITS.md`, `scripts/sync-uq-themes.sh`
- Modify: `crates/kyko-core/src/config.rs` (parse `theme.*`), `packaging/kyko/PKGBUILD`
  (install `CREDITS.md` to `/usr/share/licenses/kyko/THEMES.md`)
- Test: unit tests in `theme.rs`; `crates/kyko-core/tests/theme.rs`

**The built-ins are uQ's own rendering.** `scripts/sync-uq-themes.sh <uq checkout>` runs, in
that checkout,

```bash
cargo run -q -p uqctl -- render-themes --themes "$UQ/themes" --templates "$UQ/templates" "$tmp"
```

(the command uQ's PKGBUILD uses for its greeter; without the two flags it would render the
installed `/usr/share/uq` copies instead), copies each `$tmp/<id>-<variant>/tokens.json` to
`crates/kyko-core/themes/<id>-<variant>.json`, and writes `CREDITS.md` from uQ's
`themes/CREDITS.md` under a header saying the themes come from uQ
(`https://github.com/asafelobotomy/uq`), with uQ's MIT copyright line, and with its
"this repository" and source-file references rewritten to point at uQ. The files are committed
and compiled in with `include_str!`, so kyko needs neither uQ nor the script to build.
Re-running the script when uQ's themes change is a deliberate commit. Ids today: `dracula`,
`forest`, `graphite`, `gruvbox`, `matcha`, `rosewood`, `velvet`, each `dark` and `light`.

**Interfaces:**

```rust
pub type Rgb = u32;   // 0xRRGGBB, as kyko_term::Colors

pub enum Variant { Dark, Light }

pub struct Theme {
    pub id: String,             // uQ's id ("gruvbox"); with the variant it names a built-in
    pub name: String, pub variant: Variant,
    pub ui: UiColors,           // bg, surface, surface_alt, overlay, text, text_muted, accent,
                                // accent_text, border, success, warning, error
    pub metrics: UiMetrics,     // radius, gap_in, gap_out, border_width (px)
    pub fonts: ThemeFonts,      // ui, mono, size
    pub term: TermPalette,
}

pub struct TermPalette {
    pub foreground: Rgb, pub background: Rgb, pub cursor: Rgb,
    pub selection: Rgb, pub selection_text: Rgb, pub ansi: [Rgb; 16],
}

pub enum ThemeSource { Auto, Uq, Builtin(String /* "gruvbox-dark" */), Custom }

pub struct CustomColors {
    pub base: String,                       // theme.custom.base, a built-in key; FALLBACK
    pub foreground: Option<Rgb>, pub background: Option<Rgb>, pub cursor: Option<Rgb>,
    pub selection: Option<Rgb>, pub selection_text: Option<Rgb>,
    pub ansi: [Option<Rgb>; 16],
}

/// Built-ins are keyed `<id>-<variant>`, as `theme.source = "builtin:gruvbox-dark"` names them.
pub fn builtin(key: &str) -> Option<Theme>;
pub fn builtin_keys() -> impl Iterator<Item = &'static str>;   // sorted
pub const FALLBACK: &str = "gruvbox-dark";

pub struct TokensError { pub message: String }   // not JSON, or not a JSON object

impl Theme {
    /// uQ's tokens.json. Each missing or bad field takes the built-in gruvbox of the same
    /// variant (dark when the variant is missing or bad too) and adds a warning.
    pub fn from_tokens(json: &str) -> Result<(Theme, Vec<Warning>), TokensError>;
}

/// What the window draws with.
pub struct Appearance { pub theme: Theme, pub font: String, pub font_size: u32 }

/// `uq_present`: whether `org.uq.Desktop1` is on the bus. `uq_theme`: the last theme the
/// window loaded from uQ's tokens.json, if any (the window keeps it across a bad read).
pub fn resolve(config: &Config, uq_present: bool, uq_theme: Option<&Theme>)
    -> (Appearance, Vec<Warning>);
```

- **The terminal palette from uQ's tokens** is what uQ already gives its own terminals
  (`templates/kitty.conf.j2`): foreground `colors.text`, background `colors.bg`, cursor
  `colors.accent`, selection `colors.accent` with text `colors.accent_text`, and
  `terminal.ansi`. kyko does not invent a mapping uQ's other terminals do not use.
- **Source:**
  - `Auto` is `Uq` when `uq_present`, else `Builtin(FALLBACK)` (§2.5).
  - `Uq` uses `uq_theme`. With none (uQ absent, file missing, or never loaded), it is the
    built-in `FALLBACK` and one warning (§4: built-in palette, retried on file change, which
    the window does). An explicit `Uq` without `uqd` on the bus still uses the file if the
    window has one: the user asked for it.
  - `Builtin` with an unknown key: warning, and `FALLBACK`.
  - `Custom`: the built-in `custom.base` (unknown: warning, `FALLBACK`), with each colour set
    under `[theme.custom]` laid over its `term` palette; the window chrome comes from the base.
- **`theme.custom.base` and `theme.custom.selection_text` are new keys** (§2.6 lists
  foreground, background, cursor, selection and the ANSI colours only). The chrome needs
  colours the custom table does not give, and the selection needs a text colour. Task 6 adds
  both to the spec. `theme.custom.ansi` is an array of `"#rrggbb"` strings: a bad entry costs
  that entry; fewer than 16 leaves the rest to the base; more than 16 is a warning and the
  extra entries are ignored.
- **Fonts:** `terminal.font` else the theme's `fonts.mono`; `terminal.font_size` else the
  theme's `fonts.size`.
- Colours are `#rrggbb`, either case. Anything else (`#rgb`, names, alpha) is a bad value.
- `config.rs` parses `theme.source` and `theme.custom.*` into `ThemeSource` and `CustomColors`,
  so all config warnings still come from one `Config::parse`.

**Steps:**
- [ ] **Step 1:** Write `scripts/sync-uq-themes.sh` and run it against `~/Projects/uq`. Commit
      the 14 files and the credits on their own (`build(core): uQ's themes, rendered by uQ`),
      so the code commit's diff stays readable.
- [ ] **Step 2:** Tests: each of the 14 built-ins loads through `from_tokens` with **no**
      warnings, its `id` and `variant` together match its file name, and its terminal palette
      matches the file's `terminal.ansi` and the kitty mapping above; the user's `tokens.json`
      shape with each field removed in turn, and with each set to a bad value, costs that field
      only, from the light fallback when the variant is light; `{}` (all fields fallen back,
      warnings), `[]` and invalid JSON (`Err`); `resolve` for each source with uQ present and
      absent, with and without a uQ theme; custom over a light base, an unknown base, and
      `ansi` arrays of 3 and 20 entries; fonts from config and from the theme; seeded random
      bytes through `from_tokens` without a panic.
- [ ] **Step 3:** Implement. PKGBUILD: install the credits; `bash tests/shell/version_test.sh`.
      Tests, clippy, fmt. Commit `feat(core): themes from uQ, built in, or custom`.

### Task 4: Session

**Files:**
- Create: `crates/kyko-core/src/session.rs`
- Test: unit tests in `session.rs`; `crates/kyko-core/tests/common/mod.rs` (the seeded
  operation generator, shared with Task 5); `crates/kyko-core/tests/session_random.rs`

**The model.** A `Session` is an ordered list of tabs and the active one. A tab is a binary
tree: a leaf is a pane; an inner node splits its area side by side or stacked at a ratio, first
child left or top. Each pane carries its state, what `Info` last said (program, folder), the
name of the shell it was spawned with, and whether it is held open after it ends. Each tab keeps
its panes in most-recently-focused order. The session knows the home folder (for names and for
new tabs), given when it is created.

**Interfaces:**

```rust
pub type TabId = u32;       // stable for the window's life and across a restart (layout blob);
                            // used by ListTabs, FocusTab and the Spotlight provider (§3)

pub enum Dir { Left, Right, Up, Down }
pub enum SplitAxis { Side, Stacked }

pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }  // in the tab's unit square

pub enum PaneState {
    Running,
    Exited(i32),        // shown with its status until dismissed; only for held panes
    Failed(String),     // SpawnFailed: shows the error and the command (§4)
    Ended,              // kykod went away: "Session ended" (§2.7)
}

pub struct PaneView<'a> {
    pub id: PaneId, pub rect: Rect, pub state: &'a PaneState,
    pub program: &'a str, pub cwd: &'a str, pub busy: bool, pub focused: bool,
}
pub struct DividerView { pub path: Vec<bool>, pub axis: SplitAxis, pub rect: Rect, pub ratio: f64 }
pub struct TabView<'a> {
    pub id: TabId, pub name: String, pub program: &'a str,   // ListTabs' a(uss)
    pub panes: Vec<PaneView<'a>>, pub dividers: Vec<DividerView>,
}

pub struct SpawnRequest { pub cwd: Option<String>, pub args: Vec<String>, pub hold: bool }
                                    // cwd None: home. args empty: an interactive shell; the
                                    // window builds the Spawn's shell and args (Run, milestone 4,
                                    // passes ["-c", command] with hold = true)

/// What a change asks the window to do beyond redrawing the tab bar and layout.
pub enum Effect {
    Spawn { pane: PaneId, cwd: Option<String>, args: Vec<String> },   // send Spawn
    Close { pane: PaneId },                                           // send Close
    Hide,                                                             // the last tab was replaced
}

pub struct Full;   // MAX_PANES reached: nothing changed

impl Session {
    /// One tab with one pane, to spawn.
    pub fn fresh(home: &str, shell: &str) -> (Session, Vec<Effect>);

    pub fn tabs(&self) -> impl Iterator<Item = TabView<'_>>;
    pub fn active_tab(&self) -> TabId;
    pub fn focused_pane(&self) -> PaneId;

    pub fn new_tab(&mut self, req: SpawnRequest, shell: &str) -> Result<Vec<Effect>, Full>;
    pub fn new_tab_dir(&self, dir: NewTabDir) -> Option<String>;  // §2.6 new_tab_dir, resolved
    pub fn split(&mut self, axis: SplitAxis, shell: &str) -> Result<Vec<Effect>, Full>;
    pub fn close_pane(&mut self, pane: PaneId, shell: &str) -> Vec<Effect>;
    pub fn close_tab(&mut self, tab: TabId, shell: &str) -> Vec<Effect>;

    pub fn pane_exited(&mut self, pane: PaneId, status: i32, shell: &str) -> Vec<Effect>;
    pub fn spawn_failed(&mut self, pane: PaneId, message: &str);
    pub fn dismiss(&mut self, pane: PaneId, shell: &str) -> Vec<Effect>;  // a key on an ended pane
    pub fn holder_lost(&mut self);                                       // every pane Ended

    pub fn focus_dir(&mut self, dir: Dir) -> bool;           // false when nothing is there
    pub fn focus_pane(&mut self, pane: PaneId) -> bool;      // a click; also switches tab
    pub fn focus_tab(&mut self, tab: TabId) -> bool;
    pub fn focus_tab_index(&mut self, n: usize) -> bool;     // Alt+1..9, 0-based here
    pub fn next_tab(&mut self); pub fn prev_tab(&mut self);  // wrap around

    pub fn set_ratio(&mut self, tab: TabId, path: &[bool], ratio: f64) -> bool;
    pub fn rename(&mut self, tab: TabId, name: Option<&str>) -> bool;   // None clears

    /// True when a tab's name or program changed (the window then emits TabsChanged).
    pub fn apply_info(&mut self, pane: PaneId, program: &str, cwd: &str) -> bool;
    pub fn busy_panes(&self, tab: TabId) -> Vec<PaneId>;     // for the close confirmation

    #[doc(hidden)]
    pub fn check_invariants(&self) -> Result<(), String>;    // for the random tests
}
```

**Rules**, each from §2.3 unless marked as decided here:

- **Ids.** The session hands out pane ids and tab ids itself, each from a counter that only
  goes up, starting above any id it has seen, including ids reconciled from `kykod` (Task 5).
  An id is never reused within a session: frames for a closed pane (`Output`, `Info`,
  `Exited`) may still be in flight when the window would reuse its id, and would land on the
  new pane. A counter at `u32::MAX` is `Full`. (Decided here.)
- **At most `MAX_PANES` panes**, counting held and ended ones, since `kykod` counts its exited
  panes too. `new_tab` and `split` return `Full` beyond that and change nothing.
- **Busy** means "the foreground program is not the shell". `program` differs from the shell's
  name as `/proc/<pid>/comm` gives it (the base name, cut to 15 bytes), which is what `kykod`
  sends in `Info`. Both sides are cut to 15 bytes before comparing, because `kykod`'s first
  `Info` after `Spawn` may carry the uncut base name of the shell path. An empty `program`
  (`kykod` found no member of the foreground group) is idle. Limits, accepted:
  - A shell inside the shell (`bash` in `bash`) counts as idle.
  - A pane the session first meets in `Hello`, with no blob entry, is given today's shell
    name, which may not be the one it was started with.

  The `EndSessions` busy check in `kykod`, which works by process group, has neither limit.
  (Decided here. A `busy` flag in `Info` would need protocol version 3, and these cases are
  not worth it.)
- **Tab name:** the rename if set; else, from the tab's focused pane, the program if busy, else
  the last component of its folder, `~` for the home folder, `/` for the root. Before any
  `Info`: the shell's name. `TabView.program` is the focused pane's program.
- **Renames** have control characters removed and are cut to 256 bytes at a character boundary.
  An empty result clears the rename.
- **New tab** goes right after the active tab and becomes active. `new_tab_dir(Current)` is the
  focused pane's folder if it has had an `Info`, else `None` (home).
- **Split** puts the new pane right of or below the focused one, at ratio 0.5, focuses it, and
  spawns it in the focused pane's folder. (Folder decided here: it matches `current`.)
- **A pane's shell exits:** a held pane becomes `Exited(status)` and stays until `dismiss`;
  any other pane closes, as below. Either way the session sends `Close`, because only `Close`
  frees `kykod`'s slot. A held pane sends it at once (the pane shows its status from the
  session, not from `kykod`).
- **`spawn_failed`** marks the pane `Failed`; `dismiss` closes it. No `Close` is sent: `kykod`
  never added the pane.
- **`holder_lost`** marks every pane `Ended`. They stay until dismissed or until reconcile
  with the new `kykod` (Task 5). No `Close` is sent for them; the old `kykod` is gone.
- **Close pane:** its sibling takes the parent's place, keeping the sibling's own subtree;
  focus goes to the most recently focused pane left in the tab. The last pane closes the tab.
  Sends `Close` for a `Running` pane.
- **Close tab:** sends `Close` for each `Running` pane in it. If it was the active tab, the tab
  to its right becomes active, else the one to its left. (Decided here, as browsers do.)
  Closing the last tab opens a fresh one in the home folder and returns `Hide`.
- **Focus by direction** works from rects computed in `f64`. A split's dividing line is computed
  once and given to both children, so two panes that share an edge have bitwise-equal
  coordinates there, and "touches" is exact equality, at any depth. The candidates are the panes
  whose rect touches the focused one's edge on that side and overlaps it (strictly) on the other
  axis. The one with the largest overlap wins, ties to the most recently focused. Nothing there:
  no change. (Decided here; it is what tmux and kitty do, and it needs no pixel sizes.)
- **Ratios** must be finite; `set_ratio` refuses NaN and infinities and keeps the rest within
  0.05–0.95. The window also enforces a minimum pane size in cells, which it can and the session
  cannot.

**Invariants**, checked by `check_invariants` after every operation in the random test:
- Every pane id appears in exactly one leaf of exactly one tab, and there are at most
  `MAX_PANES` panes.
- There is at least one tab, and the active tab exists.
- Each tab's focused pane is in it, and its focus order holds exactly its panes.
- Every ratio is finite and within bounds.
- The pane rects in a tab cover the unit square: their areas sum to 1 within a relative 1e-9,
  and no two overlap, with exact comparisons.
- Every id is below the next id to hand out.
- Every `Spawn` effect names a new pane.
- Every `Close` effect names a pane that is no longer `Running` in the session.

**Steps:**
- [ ] **Step 1:** Unit tests for each rule above:
      - focus by direction in an L-shaped layout, a three-way stack, a grid of four, and a
        chain of 255 nested splits;
      - closing the middle of three splits, the last pane, the last tab, a non-active tab;
      - a held pane exiting, a failed spawn, `holder_lost`, then `dismiss`;
      - `Full` at 256 panes;
      - renaming, clearing, a rename with control characters and one of 300 multi-byte
        characters;
      - naming in the home folder and at `/`, a 15-byte comm, an empty `program`;
      - `set_ratio` with NaN and infinities;
      - `apply_info`'s return value.
- [ ] **Step 2:** The random test, with its generator in `tests/common/mod.rs`: seeded
      sequences of 200 operations (every public mutator, with valid and invalid ids, and ratios
      including NaN, infinities and the bounds). 5,000 sequences run in CI, more with
      `KYKO_RANDOM_CASES`. It calls `check_invariants` after each step, and a failure prints the
      seed and the shortest failing prefix.
- [ ] **Step 3:** Implement. Tests, clippy, fmt. Commit `feat(core): the session tree`.

### Task 5: The layout blob and reconcile

**Files:**
- Create: `crates/kyko-core/src/layout.rs`
- Test: unit tests in `layout.rs`; `crates/kyko-core/tests/layout_random.rs` (uses
  `tests/common`)

**Interfaces:**

```rust
pub enum Base<'a> {
    Blob(&'a [u8]),         // a window starting: Hello's layout
    Current(Session),       // a window that lost kykod and found a new one: what it shows now
}

impl Session {
    /// The blob for `SaveLayout`, always at most MAX_LAYOUT bytes.
    pub fn to_blob(&self) -> Vec<u8>;

    /// Rebuilds the session from `base` and `Hello`'s panes. Never fails: with no pane left,
    /// it is a fresh session and the effects hold its `Spawn`.
    pub fn reconcile(base: Base<'_>, panes: &[PaneInfo], home: &str, shell: &str)
        -> (Session, Vec<Effect>, Vec<Warning>);
}
```

- **Format.** JSON, because it is small, easy to read when debugging, and `serde_json` is here
  already. Versioned: `v` is 1. Fields: tabs (id, rename, focus order, tree), the active tab,
  the next pane and tab ids, and per pane its shell's comm name (at most 15 bytes) and its
  `hold` flag. Neither `program` nor `cwd` is stored, because `Hello` brings them fresh.
- **The tree is a flat list, not nested JSON.** Each tab's nodes are stored in preorder, and a
  split refers to its children by index. `serde_json` stops at a nesting depth of 128, which a
  nested encoding reaches at about 60 splits, and `MAX_PANES` allows a depth of 255. Decoding
  checks that the indices form exactly one tree: each node is reached once, and there are no
  cycles and no out-of-range indices. Anything else is a corrupt tab, dropped with a warning,
  and its panes become orphans.
- **Versions.** A blob with a higher `v` comes from a newer window. This version reads only
  the fields it knows and adds a warning. An unparseable blob, or one with no `v`, is treated
  as empty, with a warning. `kykod` never reads it (§2.8).
- **Size.** Renames are at most 256 bytes with no control characters, so no JSON escapes
  longer than the character; shells are 15 bytes; panes at most 256. The worst case is about
  256 tabs × (256 + ~120) bytes, under 100 KB. A test builds it (256 tabs, each with a
  256-byte rename of 4-byte characters and quotes, and the deepest tree in one of them) and
  asserts it encodes under `MAX_LAYOUT`. `to_blob` also asserts the limit with
  `debug_assert!`, so a later field that breaks it fails the tests.
- **Reconcile with a blob,** as §2.8 says:
  - Panes in the blob that `panes` lacks are dropped. Their splits collapse as `close_pane`
    does, without effects. Tabs left with no pane are dropped.
  - **A pane whose `PaneInfo.exited` is set:**
    - If the blob has it with `hold`, it is kept as `Exited(status)`.
    - Otherwise it is dropped like a pane `kykod` lacks.
    - In both cases the effects carry its `Close`, which frees `kykod`'s slot.
    - This covers shells that ended while no window was attached.
  - Each live pane in `panes` that the blob lacks gets its own tab at the end, in id order,
    with `shell` as its shell.
  - The `Info` in `PaneInfo` is applied to every pane.
  - The next ids are the larger of the blob's and one above the largest id in `panes`. A
    counter that would overflow means the blob is corrupt: it is treated as empty.
  - With no pane left at all, the result is a fresh session whose pane id comes from that same
    counter, and the effects carry its `Spawn`.
  - The active tab and each tab's focus survive when their pane does. Otherwise the rules of
    `close_pane` and `close_tab` pick the replacement.
- **Reconcile with `Current`:** the window lost `kykod`, called `holder_lost`, and has now
  connected to a new one, usually empty after a crash (§2.7). The current session is kept as it
  is, its `Ended` panes included. `panes` is merged into it by the same rules as a blob: orphans
  get tabs, exited panes are closed. The pane counter is raised above `panes`' ids. If no tab is
  left, the result is fresh, as above.

**Steps:**
- [ ] **Step 1:** Tests:
      - round-trip: `reconcile(Blob(to_blob(s)), panes of s)` gives `s`, compared by tabs,
        names, trees, ratios, focus, hold flags and next ids;
      - a pane gone, a whole tab gone, the active tab gone;
      - an exited pane, held and not held, with its `Close`;
      - an orphan pane, and nothing left at all;
      - an empty blob with panes: the first window after `kykod` was restarted, one tab per
        pane;
      - a corrupt blob, `v: 2` with an extra field, a blob naming a pane twice (the second leaf
        dropped, with a warning), a tree with a cycle and one with an out-of-range index;
      - `next_pane` at `u32::MAX`, and ids beyond `u32` in JSON;
      - the depth-255 tree round-trip, and the worst-case size;
      - `Current` after `holder_lost` with an empty `panes`, and with one new pane.
- [ ] **Step 2:** The random test: random sessions from the shared generator are encoded, then
      reconciled after removing random panes, marking some exited, and adding orphans. The
      invariants must hold, and every surviving pane keeps its tab and its relative order.
      Random bytes as the blob must not panic.
- [ ] **Step 3:** Implement. Tests, clippy, fmt. Commit `feat(core): save and restore the layout`.

### Task 6: Docs, spec, release

**Files:**
- Modify: `docs/superpowers/specs/2026-11-10-kyko-design.md`, `CHANGELOG.md`,
  `docs/notes/2026-11-13-v1-release-checklist.md`, `README.md` if it lists the crates

- [ ] **Step 1:** Spec.
      - §2.5: the terminal palette from uQ's tokens (the kitty mapping); the built-ins are uQ's
        rendered `tokens.json` files, synced by script and keyed `<id>-<variant>`; a
        `tokens.json` that does not parse keeps the previous theme.
      - §2.6: add `theme.custom.base` and `theme.custom.selection_text`; name the `keys.*`
        actions; the duplicate-sequence rule for a hand-edited file; a `config.toml` that does
        not parse keeps the previous settings and shows the error.
      - §2.3: the rules decided in Task 4: new tab position, split folder, the tab that becomes
        active after a close, focus by direction, at most `MAX_PANES` panes.
      - §2.8: the layout blob is versioned JSON with a flat tree, capped at `MAX_LAYOUT`. Pane
        ids are never reused, because frames for a closed pane may still be in flight. The
        window sends `Close` for every pane that ends, since `kykod` keeps exited panes until
        then.
- [ ] **Step 2:** CHANGELOG `[Unreleased]`, Added: the logic the window will use (config with
      per-field fallback, themes from uQ or built in or custom, tabs and splits, the saved
      layout); still nothing usable. Then the release steps in `docs/releasing.md` for `0.0.4`.
- [ ] **Step 3:** Release checklist.
      - Reword the `config` item to "load and validate, per-field fallback, unknown keys
        warned", and tick it.
      - Move "live reload on change" for `config.toml` and `tokens.json`, and listening for
        `ThemeChanged`, to the window section as new items.
      - Tick `theme` and `session`.
      - Split the "Window side" item into "reconcile the layout blob (kyko-core, slice 1d)",
        ticked, and "wire reconcile on attach and handle a protocol version mismatch, tested
        from the window", left open.
      - Tick "plans for slice 1d"; the item stays open for 1e onward.
- [ ] **Step 4:** Full `cargo test --workspace`, clippy, fmt, and the shell tests. `cargo deny`
      runs in CI.

## Not in this slice

- **Watching files and the bus:** `config.toml` and `tokens.json` changes, `ThemeChanged` on
  `org.uq.Desktop1`. These need the window's event loop (`QFileSystemWatcher`, zbus). From
  slice 1e onward the window calls `Config::load`, `Theme::from_tokens`, `resolve` and
  `Config::changes` from them, keeping the last good value on an `Err`.
- **Translating Qt key events into `Mods` and `Key`,** keypad included: the window's.
- **Writing `config.toml`:** the Preferences window, milestone 3.
- **`runq` and the provider index** (§2.1): milestone 4, with `Run` and Spotlight. The
  session's `SpawnRequest { args, hold }` and `PaneState::Exited` are what `Run` will use, so
  the blob format does not change for it.
- **The holder follow-ups** in the release checklist:
  - The hop bench misses its limit on the development machine (1.43 to 1.49 against 1.15).
  - `htop` does not repaint after the resize nudge.

  Both are `kykod` work and neither blocks `kyko-core`. Each needs its own look before the
  window relies on it, and the window slice's plan takes them up first.
- **The kitty keyboard protocol:** turned on with the window's key encoder, as the checklist
  says.
