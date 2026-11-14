# kyko Milestone 1c (terminal modes across a wrapped ring) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A window that attaches after a pane's ring has wrapped gets the pane back in the terminal
modes its program set before the cut: the alternate screen, cursor-key and keypad modes, mouse
reporting and its encoding, bracketed paste, focus reporting, the kitty keyboard flags, the
cursor style, and a few others alacritty implements. This closes open risk 8 of the spec (§6.2).

**Architecture:** `kykod` stays free of terminal emulation. It gains a small scanner, `Modes`,
that recognises only the sequences which set those modes and skips everything else. The ring
feeds it every byte it drops, so `Modes` always describes the terminal at the ring's first kept
byte. On `Attach` of a wrapped ring, `kykod` finds a clean place to start the replay (the scanner
in its ground state, not on a UTF-8 continuation byte, preferably just after a line break),
advances a copy of `Modes` to it, and sends a **preamble** that sets those modes, then the ring
from the clean place. The window feeds all of it to a fresh emulator. The cut moves from the
window (`kyko-term` skips to the first newline today) into `kykod`, the only side that knows
where the cut is.

**The reference is the window's emulator, not xterm.** Every rule in `Modes` is what
`alacritty_terminal` 0.26.0 and `vte` 0.15.0 do, cited by file and line in the code comments.
A differential test (Task 1) checks it: whatever a stream does to an `Emulator`, the preamble
`Modes` builds from the same stream must do to a fresh one.

**Tech Stack:** as slice 1b, plus `memchr` (already in the lock file through `alacritty_terminal`'s
`regex`) for the scanner's fast path.

**Spec:** `docs/superpowers/specs/2026-11-10-kyko-design.md` §2.8 (replay on attach), §2.4, §6.2
item 8.

**Depends on:** slice 1b (v0.0.2) on `main`.

## Global Constraints

- `kykod` does not grow an emulator. `Modes` keeps no grid, no text, no SGR, no cursor position.
- `Modes` is fed in arbitrary chunks; its state must not depend on how a stream was split.
- **Speed budget.** The hop bench's limit is 1.15 times the direct run. Slice 1b measured a
  direct median of 722 ms (limit about 830 ms) and 807 to 829 ms through `kykod`, so the margin is
  only a few tens of milliseconds at best. The scan of the bench's 16.9 MB must cost under 5 ms,
  which means at least 3.5 GB/s on output without escapes. Hence: the ground fast path is
  `memchr(0x1B)` (vte's own fast path looks only for ESC), `Modes` does not track UTF-8, and the
  ring hands it dropped bytes as slices (`as_slices`), not through the `drain` iterator. The scan
  runs on the reader thread under the pane lock, so it must be cheap.
- No timers while idle; nothing here runs except on output and on `Attach`.
- Protocol version becomes `2`: the window no longer skips the head of a truncated replay, so a
  version-1 window with a version-2 holder (or the reverse) would cut twice or not at all. The
  frames do not change. `Replayed.truncated` stays, for the window's "earlier output was not
  kept" notice.
- `kykod` keeps `unsafe` only in `pty.rs`; the new module is safe code.
- Version after this slice: `0.0.3`, released by CI. The window still arrives in `0.1.0`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **`Modes` agrees with the emulator.** The differential test is the evidence; a hand-written
   case that contradicts alacritty is wrong, not the test. Task 1.
2. **A cut inside an escape sequence, a string (OSC, DCS, APC, PM, SOS) or a UTF-8 character:**
   the replay starts at a clean place, so the emulator shows no debris and no `U+FFFD`. Tasks 2
   and 4.
3. **A mode set before the cut and cleared after it**, and the reverse: the window ends in the
   live terminal's state. Tasks 2 and 4.
4. **A full-screen program that never writes a newline:** today the replay is empty; after this
   slice it starts at the first clean place. Task 4.
5. **Throughput:** the hop bench within its limit. Task 5.

---

### Task 1: `Modes`, the mode scanner

**Files:**
- Create: `apps/kykod/src/modes.rs`
- Modify: `apps/kykod/src/lib.rs` (module), `apps/kykod/Cargo.toml` (`memchr`)
- Test: unit tests in `modes.rs`; `apps/kykod/tests/modes_differential.rs` (uses `kyko-term`, a
  dev-dependency already)

**Interfaces:**

```rust
/// The terminal's modes after the bytes fed so far. Cheap to clone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Modes { /* private */ }

impl Modes {
    pub fn feed(&mut self, bytes: &[u8]);
    /// Advances over `bytes` up to the first place a replay may start, and returns its index
    /// (`bytes.len()` if there is none). A place qualifies when the scanner is in ground state
    /// and the byte there is not a UTF-8 continuation byte (0x80..=0xBF): vte resynchronises at
    /// any other byte, so at most 3 bytes are skipped for that. The first such place just after
    /// a `\n` is preferred; with no line break in `bytes`, the first such place at all.
    pub fn find_start(&mut self, bytes: &[u8]) -> usize;
    /// The bytes that bring a fresh `Emulator` to these modes. Empty when all are default.
    pub fn preamble(&self) -> Vec<u8>;
}
```

**What it tracks**, each as alacritty applies it (`term/mod.rs` of 0.26.0; line numbers as of
that release):

| State | Set by | Notes |
|---|---|---|
| Alternate screen | `CSI ? 1049 h/l` | The only private mode vte maps to the alternate screen (`ansi.rs` `PrivateMode::new`); 47 and 1047 do nothing in this emulator and are ignored |
| Mouse tracking: none, clicks, drag, motion | `CSI ? 1000/1002/1003 h/l` | One value, not three bools: setting one clears the others (mod.rs ~1953) |
| Mouse encoding: default, UTF-8, SGR | `CSI ? 1005/1006 h/l` | One value, exclusive (mod.rs ~1970). 1015 is not implemented and is ignored |
| Cursor keys, origin, autowrap, cursor visible, focus, alternate scroll, bracketed paste | `CSI ? 1/6/7/25/1004/1007/2004 h/l` | Defaults from `TermMode::default` (mod.rs ~113): cursor visible, autowrap and **alternate scroll on** |
| Insert mode, line feed/new line | `CSI 4 h/l`, `CSI 20 h/l` | ANSI (non-private) modes alacritty implements |
| Keypad | `ESC =` / `ESC >` | |
| Cursor style: none, or shape and blinking | `CSI Ps SP q`; `CSI ? 12 h/l` | One value: alacritty writes both to `cursor_style` (mod.rs ~1988, ~2204); 12 sets blinking on the style, creating the default style if none. The preamble sends one `DECSCUSR` |
| Kitty keyboard: a stack per screen, and the active flags | `CSI > f u` push, `CSI < n u` pop, `CSI = f ; m u` | `=` changes only the **active flags**, never a stack entry (mod.rs ~1029); a push, a pop and a screen switch reload the active flags from the current stack's top (~1303, ~1314, ~726). Push depth capped at `KITTY_STACK` (4096, the oldest dropped: what alacritty intends; see Task 3 for the bug that stops it doing so) |
| Everything back to default | `ESC c` | RIS, which also clears both kitty stacks (mod.rs ~1849) |

Sequences alacritty does not act on are ignored, among them `CSI ! p` (vte has no DECSTR arm),
2026 (synchronised output, transient) and xterm's save and restore of private modes.

**Scanner rules, as vte's state machine (`vte` `lib.rs`):** ESC inside any CSI, OSC, DCS or
APC/PM/SOS string ends it and starts a new escape (so an unterminated OSC followed by
`ESC [ ? 1049 h` still switches the screen); BEL ends only an OSC (a DCS passes it through,
APC/PM/SOS ignore it); 0x9C ends a DCS; CAN and SUB abort a sequence and, in ground, are only
executed. A CSI with more than 32 parameters, or more than 2 intermediates, is ignored as a whole,
as alacritty drops it (`ansi.rs` ~1545). Other C0 controls inside a CSI are executed and do not
end it.

**Preamble order:** the main screen's kitty stack (one `CSI > f u` per entry, oldest first);
`CSI ? 1049 h` if the alternate screen is on, then its stack; `CSI = active ; 1 u` when the
active flags differ from the current stack's top; mouse tracking; mouse encoding; each other
mode that differs from its default, one `h` or `l`; keypad; one `DECSCUSR` if a cursor style is
set.

**Steps:**
- [ ] **Step 1:** The differential test, written first: a seeded random generator builds
      streams from a vocabulary of every sequence in the table, mode numbers that are ignored,
      CSI with 40 parameters, unterminated OSC, DCS and APC strings followed by an ESC sequence,
      CAN and SUB mid-sequence, C0 inside CSI, UTF-8 text including 4-byte characters, and plain
      text. For each stream: feed it to an `Emulator` with kitty keyboard enabled (Task 3's
      option; until then this test only builds); separately `Modes::feed`, then feed
      `preamble()` to a fresh `Emulator`; assert `modes()` equal, and the kitty state equal
      after a `CSI ? 1049 l` on both (to compare the main stack too). 10,000 streams in CI, more
      with an env var. A failure prints the seed and the shortest stream found by halving.
- [ ] **Step 2:** The chunking test: for every split point of a few long corpora, two feeds give
      the same `Modes` as one; every byte fed alone too.
- [ ] **Step 3:** `find_start` tests: inside each kind of sequence and string, at continuation
      bytes, with and without a line break, and with no clean place at all.
- [ ] **Step 4:** A few hand cases the random test is unlikely to hit: `KITTY_STACK + 1` pushes
      (`Modes` only; the emulator would panic, above);
      `CSI = 5 u` with an empty stack (fish does this); `=` then a screen switch (the `=` change
      is gone).
- [ ] **Step 5:** Implement.
- [ ] **Step 6:** A throughput bench (`#[ignore]`, like `hop_bench`): 64 MiB of `seq` output and
      64 MiB of coloured `ls -l`-like output; prints GB/s. Target: at least 3.5 GB/s on `seq`.
- [ ] **Step 7:** `cargo test -p kykod`, clippy, fmt. Commit `feat(kykod): scan the terminal modes`.

### Task 2: The ring keeps the modes at its first byte

**Files:**
- Modify: `apps/kykod/src/ring.rs`
- Test: unit tests in `ring.rs`

**Interfaces:**

```rust
impl Ring {
    /// What a window needs to rebuild the pane: the preamble, then the kept bytes from a clean
    /// place, and whether anything was dropped. Untruncated: no preamble, every byte.
    pub fn replay(&self) -> (Vec<u8>, bool);
}
```

- `push` feeds `Modes` every byte it drops, oldest first, as slices: the buffer's head (both
  halves of `as_slices` as needed), and, when one push is larger than the capacity, the whole old
  buffer then the head of the pushed bytes.
- `replay` when truncated: clone `Modes`, call `find_start` on the kept bytes, and return the
  clone's preamble followed by the kept bytes from the start.
- `contents()` stays for the tests that use it.

**Steps:**
- [ ] **Step 1:** Tests: untruncated replay equals `contents()`; a mode set in dropped bytes
      appears in the preamble; a mode set in dropped bytes and cleared in kept bytes ends cleared
      in an `Emulator` fed the replay; cuts mid-CSI, mid-OSC and mid-UTF-8; the large-push path.
- [ ] **Step 2:** Implement. Commit `feat(kykod): the ring replays from a clean place with the modes`.

### Task 3: Attach sends the replay; protocol version 2; `kyko-term` changes

**Files:**
- Modify: `apps/kykod/src/pane.rs` (`attach`), `crates/kyko-proto/src/lib.rs` (`VERSION = 2`),
  `crates/kyko-term/src/lib.rs`, `crates/kyko-term/tests/emulator.rs`,
  `apps/kykod/tests/common/mod.rs` (the test window's reattach), `apps/kykod/tests/attach.rs`
  (`assert_eq!(bytes.len(), 262_144)` at about line 152 becomes "at most the ring, starting at a
  clean place", and the `numbers(.., cut)` helper no longer drops a first partial line, since
  the holder already has), every other caller of `Emulator::replay`
- Test: `crates/kyko-proto/tests/roundtrip.rs`, `crates/kyko-term/tests/emulator.rs`

**Interfaces:**
- `Emulator::replay(&mut self, bytes: &[u8])`: reset, feed everything, drop the replies. The
  `truncated` parameter goes.
- `Pane::attach` sends `Ring::replay`'s bytes in `OUTPUT_CHUNK` frames, then
  `Replayed { truncated }`, then `Exited` if ended, as today, all under the one hold of the pane
  lock it already takes, so no byte is lost or sent twice.
- `Emulator::modes() -> TermModes`: a small `Copy` struct read from alacritty's `TermMode` and
  cursor style: alternate screen, mouse tracking and encoding, cursor keys, origin, autowrap,
  cursor visible, focus, alternate scroll, bracketed paste, insert, line feed/new line, keypad,
  cursor style, the active kitty flags. For tests now and for the window's input mapping later.
- **Kitty keyboard stays off by default.** With alacritty's `kitty_keyboard` on, the emulator
  answers the `CSI ? u` query (mod.rs ~1275), so nvim, fish and helix would switch to kitty key
  encoding, and the window has no key encoder until the input slice. So `kyko-term` gets an
  option, `Emulator::set_kitty_keyboard(bool)` (off by default): the differential test turns it
  on now, and the window turns it on in the same slice as its key encoder. The release
  checklist carries that as a hard item.
- **A crash to fix before the option is ever on outside tests.** In `alacritty_terminal` 0.26.0
  (the newest release), `Term::push_keyboard_mode` (mod.rs ~1295) is meant to drop the oldest
  entry once the stack holds 4096 (`KEYBOARD_MODE_STACK_MAX_DEPTH`), but calls
  `self.title_stack.remove(0)`. With no titles pushed that panics; with titles pushed it
  silently drops one and the keyboard stack grows without limit. This slice does not fix it: the
  option is off, and the differential test keeps its pushes far below 4096. It reports the bug
  upstream with a reproduction, and the release checklist makes the fix (a fixed release, or a
  one-line `[patch.crates-io]`) a condition of turning the option on. `Modes`' `KITTY_STACK` is
  4096 with the oldest entry dropped, which is what alacritty intends.

**Steps:**
- [ ] **Step 0:** Report the `push_keyboard_mode` bug to alacritty with a minimal reproduction
      (a `Term` with `kitty_keyboard: true`, 4097 `CSI > 1 u`). Record the issue link in the m1c
      checks note.
- [ ] **Step 1:** Tests: `modes()` reflects each sequence fed; with the option on, `CSI > 1 u`
      sets the disambiguate flag; with it off, nothing; `replay` no longer drops the head of its
      input; the version round-trip expects 2, and a version-1 `Hello` still parses leniently
      (slice 1b's rule).
- [ ] **Step 2:** Implement; update every caller of `replay(.., truncated)`.
- [ ] **Step 3:** Full `cargo test --workspace`. Commit
      `feat: replay is cut by the holder; protocol version 2; kitty keyboard option`.

### Task 4: End-to-end: modes survive a wrapped ring

**Files:**
- Modify: `apps/kykod/tests/end_to_end.rs`

Each test spawns a pane with a 256 KiB ring, has it set modes, writes well over 256 KiB, then a
marker, reattaches with the test window (kitty keyboard on), and asserts on `Emulator::modes()`
and the screen.

- [ ] **Step 1:** `modes_set_before_the_cut_survive_it`: `printf` the alternate screen, cursor
      keys, keypad, mouse 1002 with SGR encoding, focus, bracketed paste, a blinking beam cursor,
      kitty `CSI > 1 u` on the main screen and `CSI > 5 u` on the alternate; then 400 KB of
      `CSI row;col H` and text with **no line break**; then the marker at a fixed position.
      Assert every mode, both kitty stacks, and the marker on screen.
- [ ] **Step 2:** `a_mode_cleared_after_the_cut_stays_cleared`: set bracketed paste and mouse,
      flood, clear both, write less than the ring. Assert both off.
- [ ] **Step 3:** Keep `reattach_after_the_ring_wrapped_is_clean` passing unchanged in what it
      asserts: line-oriented output still starts on a whole line.
- [ ] **Step 4:** Commit `test(kykod): terminal modes across a wrapped ring`.

### Task 5: Speed, live checks, spec, release

**Files:**
- Modify: `docs/superpowers/specs/2026-11-10-kyko-design.md`, `CHANGELOG.md`,
  `docs/notes/2026-11-13-v1-release-checklist.md`
- Create: `docs/notes/2026-11-14-m1c-checks.md`

- [ ] **Step 1:** The hop bench, five runs each way, recorded as slice 1b's note does, with the
      load average. Within 1.15, or stop and profile.
- [ ] **Step 2:** The scanner bench figures.
- [ ] **Step 3:** Live checks on the development machine, through a small test client into
      `kyko-term`, each in a pane with a 256 KiB ring left until it has wrapped: `less` scrolled
      page by page, `htop`, and `vim` redrawing. Record what was right and what was not.
- [ ] **Step 4:** Spec: §2.8 "Replay on attach" describes the preamble and the holder's cut;
      protocol version 2; §6.2 item 8 closed, naming the note; §2.4 notes that the kitty keyboard
      protocol is switched on with the key encoder.
- [ ] **Step 5:** CHANGELOG `[Unreleased]`: Added (modes kept across a wrapped ring), Changed
      (protocol version 2; the replay is cut by the holder), and a note for anyone running 0.0.2:
      after upgrading, a `kykod` 0.0.2 still running refuses the new version's connection (it
      closes on a version mismatch) and no `kyko end-sessions` exists yet, so restart it with
      `systemctl --user restart kykod`, which ends its shells. Then the release steps in
      `docs/releasing.md` for `0.0.3`.
- [ ] **Step 6:** Tick open risk 8 and the slice 1c items in the release checklist.

## Not in this slice

- **SGR at the cut:** a replay may start with the wrong colours until the program next sets them.
  The spec accepts this (§2.8).
- **The scroll region (`DECSTBM`):** alacritty resets it on every resize (mod.rs ~700) and
  ignores a region with top ≥ bottom, and `Modes` knows neither the size nor the resizes. Left to
  the redraw nudge after attach (§2.8), which makes full-screen programs set it again. The nudge
  does not reach programs behind `ssh` or `tmux`, which the spec already lists as a limit.
- **Character sets (`ESC ( 0`, G1, SO/SI):** they live in each screen's cursor, are copied into
  the alternate screen on entry, and are saved and restored with the cursor. Tracking them
  needs the saved cursor too. Left to the nudge, like the scroll region.
- **Window title (`OSC 0`/`2`):** tab names come from `Info`, not the title.
- **Anything the window needs:** its link, its replay buffering, its key encoder. Those are
  slice 1d onward.
