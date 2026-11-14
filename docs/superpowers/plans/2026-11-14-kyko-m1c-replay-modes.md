# kyko Milestone 1c (terminal modes across a wrapped ring) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A window that attaches after a pane's ring has wrapped gets the pane back in the terminal
modes its program set before the cut: the alternate screen, cursor-key and keypad modes, mouse
reporting, bracketed paste, focus reporting, the kitty keyboard flags, the scroll region, the
cursor shape and the line-drawing character set. This closes open risk 8 of the spec (§6.2).

**Architecture:** `kykod` stays free of terminal emulation. It gains a small scanner, `Modes`,
that reads only the escape sequences which set those modes and ignores everything else. The ring
feeds it every byte it drops, so `Modes` always describes the terminal at the ring's first kept
byte. On `Attach` of a wrapped ring, `kykod` finds a clean place to start the replay (the
scanner in its ground state, on a character boundary, preferably just after a line break),
advances a copy of `Modes` over the bytes before it, and sends a **preamble** that sets those
modes, then the ring from the clean place. The window feeds all of it to a fresh emulator. The
cut moves from the window (`kyko-term` skips to the first newline today) into `kykod`, the only
side that knows where the cut is.

**Tech Stack:** as slice 1b. No new dependency unless Task 1's bench shows the scanner needs
`memchr` (already in the lock file through `alacritty_terminal`'s `regex`).

**Spec:** `docs/superpowers/specs/2026-11-10-kyko-design.md` §2.8 (replay on attach), §2.4 (the
modern protocols, kitty keyboard among them), §6.2 item 8.

**Depends on:** slice 1b (v0.0.2) on `main`.

## Global Constraints

- `kykod` does not grow an emulator. `Modes` recognises the sequences listed in Task 1 and skips
  the rest without interpreting it. It keeps no grid, no text, no SGR state.
- `Modes` is fed in arbitrary chunks. Its state after a stream must not depend on how the stream
  was split (tested at every split point of a corpus).
- Hop bench target unchanged: `seq 1 2000000` through `kykod` within 15% of the direct run,
  median of five (spec §6.2 item 6). The ring now scans what it drops, so the bench is re-run.
- No timers while idle; nothing here runs except on output and on `Attach`.
- Protocol version becomes `2`: the window no longer skips the head of a truncated replay, so a
  version-1 window with a version-2 holder (or the reverse) would cut twice or not at all. The
  frames themselves do not change. `Replayed.truncated` stays, for the window's "earlier output
  was not kept" notice.
- `kykod` keeps `unsafe` only in `pty.rs`; the new module is safe code.
- Version after this slice: `0.0.3`, released by CI. The window still arrives in `0.1.0`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A cut inside an escape sequence, an OSC or DCS string, or a UTF-8 character:** the replay
   starts at a clean place, so the emulator shows no debris and no `U+FFFD`. Tests in Tasks 1
   and 4.
2. **A mode set before the cut and cleared after it**, and the reverse: the window ends in the
   state the live terminal is in, not the state at the cut. Tests in Tasks 2 and 4.
3. **A full-screen program that never writes a newline** (it draws with cursor positioning):
   today the replay is empty; after this slice it starts at the first clean place. Test in
   Task 4.
4. **Kitty keyboard flags per screen:** the main and alternate screens keep separate flag
   stacks; the preamble rebuilds both in the right order. Test in Tasks 1 and 4.
5. **Throughput:** the scanner sees every byte the ring drops, which for a busy pane is nearly
   every byte. The hop bench must stay within its limit. Task 5.

---

### Task 1: `Modes`, the mode scanner

**Files:**
- Create: `apps/kykod/src/modes.rs`
- Modify: `apps/kykod/src/lib.rs` (module)
- Test: unit tests in `modes.rs`

**Interfaces:**

```rust
/// What the terminal's modes are after the bytes fed so far. Clone is cheap (a few dozen bytes
/// plus the kitty stacks, each at most KITTY_STACK entries).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Modes { /* private */ }

impl Modes {
    pub fn feed(&mut self, bytes: &[u8]);
    /// True when the scanner is between sequences and not inside a UTF-8 character: a replay
    /// may start at the next byte.
    pub fn at_clean_place(&self) -> bool;
    /// The bytes that bring a fresh terminal (xterm-256color defaults) to these modes. Empty
    /// when every mode is at its default.
    pub fn preamble(&self) -> Vec<u8>;
}
```

**Recognised** (everything else is skipped; a sequence is only acted on once complete):

| Sequence | Effect on `Modes` |
|---|---|
| `CSI ? Pm h` / `CSI ? Pm l` | set / reset each listed DEC private mode in: 1 (cursor keys), 7 (autowrap, default on), 12 (cursor blink), 25 (cursor visible, default on), 47, 1047, 1049 (alternate screen), 1000, 1002, 1003 (mouse tracking), 1004 (focus), 1005, 1006, 1015 (mouse encoding), 1007 (alternate scroll), 2004 (bracketed paste). Others, 2026 (synchronised output) among them, are ignored |
| `ESC =` / `ESC >` | keypad application / numeric |
| `CSI > f u` | push `f` on the current screen's kitty stack (at most `KITTY_STACK` entries, the oldest dropped beyond it; the same depth as the emulator's stack after Task 3, so the two cannot disagree) |
| `CSI < n u` | pop `n` (default 1) from the current screen's stack |
| `CSI = f ; m u` | set (`m` = 1, default), or (`m` = 2), or clear bits (`m` = 3) of the top entry |
| `CSI t ; b r` | scroll region (no parameters: the whole screen) |
| `CSI Ps SP q` | cursor shape |
| `ESC ( 0` / `ESC ( B` | G0 line drawing / ASCII |
| `ESC c` | everything back to its default (RIS) |
| `CSI ! p` | soft reset (DECSTR): cursor keys, keypad, autowrap on, cursor visible, scroll region, G0 back to their defaults |

The scanner's states: ground, escape, escape-intermediate, CSI (parameters and intermediates
collected, bounded: at most 32 parameters, each saturating at 65535), and string (OSC, DCS, APC,
PM, SOS: skipped until BEL or `ESC \`). CAN and SUB abort a sequence. Other C0 controls inside
a CSI are ignored, as VT parsers do, and do not end it. In ground it also tracks UTF-8: a lead
byte opens a character of 2 to 4 bytes, and the place is clean again after its last
continuation byte or at any byte that cannot continue it.

**Preamble order:** the main screen's kitty stack (one `CSI > f u` per entry, oldest first);
`CSI ? 1049 h` if the alternate screen is on, then its kitty stack; the scroll region; the DEC
modes that differ from their defaults; keypad; cursor shape; G0. One `h` or `l` sequence per mode
that differs.

**Steps:**
- [ ] **Step 1:** Tests first, each feeding a stream and comparing `preamble()` with the expected
      bytes: every row of the table; a mode set then reset (empty preamble); RIS after modes
      (empty); DECSTR resets only its subset; kitty push, pop, set/or/clear, per screen, stack
      limit; an OSC containing `ESC [ ? 1049 h` inside its text (ignored); an unterminated OSC
      followed by CAN (aborted, scanner clean again); a CSI with an embedded LF still parsed;
      42 parameters (no panic, bounded); `KITTY_STACK + 1` pushes (the oldest dropped).
- [ ] **Step 2:** The chunking test: a corpus of all the above plus UTF-8 text with 2-, 3- and
      4-byte characters and an emoji; for every split point `i`, `feed(&c[..i]); feed(&c[i..])`
      gives the same `Modes` as one `feed(&c)`. Also every byte fed alone.
- [ ] **Step 3:** `at_clean_place` tests: false inside each kind of sequence and inside a UTF-8
      character; true after each ends; true after a stray continuation byte.
- [ ] **Step 4:** Implement. Ground fast path: find the next ESC, CAN, SUB or byte ≥ 0x80 and skip
      the plain run in one step.
- [ ] **Step 5:** A throughput bench (`#[ignore]`, like `hop_bench`): 64 MiB of `seq` output and
      64 MiB of coloured `ls -l`-like output; print MB/s. Target well above the hop bench's
      ~20 MB/s so the scan is noise there; if under 500 MB/s, use `memchr` for the fast path.
- [ ] **Step 6:** `cargo test -p kykod`, clippy, fmt. Commit `feat(kykod): scan the terminal modes`.

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

- `push` feeds `Modes` every byte it drops, oldest first: the buffer's head, and, when one push
  is larger than the capacity, the head of the pushed bytes after the whole old buffer.
- `replay` when truncated: clone `Modes`; find the start, which is the first byte after a line
  break where the clone is clean, else the first clean place, else the end (nothing kept is
  safe to show); feed the clone the bytes before the start; return its preamble followed by the
  bytes from the start.
- `contents()` stays for the tests that use it.

**Steps:**
- [ ] **Step 1:** Tests: untruncated replay equals `contents()`; a mode set in bytes that were
      dropped appears in the preamble; a mode set in dropped bytes and cleared in kept bytes ends
      cleared once the emulator has the replay (test with `kyko-term`, a dev-dependency); a cut
      mid-CSI, mid-OSC and mid-UTF-8 starts after it; line-oriented output starts after a line
      break; output with no line break starts at the first clean place.
- [ ] **Step 2:** Implement. Commit `feat(kykod): the ring replays from a clean place with the modes`.

### Task 3: Attach sends the replay; protocol version 2; `kyko-term` stops cutting

**Files:**
- Modify: `apps/kykod/src/pane.rs` (`attach`), `crates/kyko-proto/src/lib.rs` (`VERSION = 2`),
  `crates/kyko-term/src/lib.rs` (`replay`), `crates/kyko-term/tests/emulator.rs`,
  `apps/kykod/tests/common/mod.rs` (the test window's reattach), the tests that call
  `Emulator::replay`
- Test: `crates/kyko-proto/tests/roundtrip.rs` (version), `crates/kyko-term/tests/emulator.rs`

**Interfaces:**
- `Emulator::replay(&mut self, bytes: &[u8])`: reset, feed everything, drop the replies. The
  `truncated` parameter goes: the holder has already made the bytes safe to feed.
- `Pane::attach` sends `Ring::replay`'s bytes in `OUTPUT_CHUNK` frames, then
  `Replayed { truncated }`, then `Exited` if ended, as today.
- `kyko-term`'s `Config` sets `kitty_keyboard: true`, which alacritty leaves off by default, so
  the kitty keyboard protocol the spec promises (§2.4) is actually on. **Not before the stack
  bug below is handled.**
- **A crash in `alacritty_terminal` 0.26.0 that this switch exposes.** `Term::push_keyboard_mode`
  (`src/term/mod.rs`, about line 1295) is meant to drop the oldest entry once the kitty stack
  holds `KEYBOARD_MODE_STACK_MAX_DEPTH` (4096), but it calls `self.title_stack.remove(0)`, the
  wrong stack, which panics when the title stack is empty, and the keyboard stack then grows
  without limit. With `kitty_keyboard` on, 4097 `CSI > u` from any program (or `cat` of a file)
  would crash the window. Handle it in this order: look for a released `alacritty_terminal` that
  fixes it and take it if the rest of slice 1b's tests pass on it; otherwise carry the one-line
  fix (`self.keyboard_mode_stack.remove(0)`) as a `[patch.crates-io]` of a vendored copy, or, if
  that is unwanted, cap pushes in `kyko-term` before they reach the `Term`. Either way, report it
  upstream with the reproduction. `KITTY_STACK` in `Modes` equals whatever depth the emulator
  ends up enforcing.
- `Emulator::modes() -> TermModes`, a small `Copy` struct of bools and the kitty flags, read from
  alacritty's `TermMode`: alternate screen, app cursor, app keypad, mouse reporting (any),
  SGR mouse, focus reporting, bracketed paste, cursor visible, line wrap, kitty flags. For tests
  now and for the window's input mapping later.

**Steps:**
- [ ] **Step 0:** A test that feeds 4097 `CSI > 1 u` with `kitty_keyboard: true` and an empty
      title stack, then 4097 `CSI < u`: it must not panic and must end with no kitty flags. It
      panics on 0.26.0 as it is. Then the fix chosen above.
- [ ] **Step 1:** Tests: `modes()` reflects each sequence fed; `CSI > 1 u` sets
      `DISAMBIGUATE_ESC_CODES` (fails before `kitty_keyboard: true`); `replay` no longer drops the
      head of its input; the version round-trip test expects 2, and a version-1 `Hello` still
      parses leniently (slice 1b's rule).
- [ ] **Step 2:** Implement; update every caller of `replay(.., truncated)`.
- [ ] **Step 3:** Full `cargo test --workspace`. Commit
      `feat: replay is cut by the holder; protocol version 2; kitty keyboard on`.

### Task 4: End-to-end: modes survive a wrapped ring

**Files:**
- Modify: `apps/kykod/tests/end_to_end.rs`

Each test spawns a pane with a 256 KiB ring, has it set modes, then writes well over 256 KiB, then
a marker, reattaches with the test window, and asserts on `Emulator::modes()` and the screen.

- [ ] **Step 1:** `modes_set_before_the_cut_survive_it`: `printf` the alternate screen, cursor
      keys, keypad, SGR mouse 1000+1006, focus, bracketed paste, kitty `CSI > 5 u` on the
      alternate screen and `CSI > 1 u` on the main one; then 400 KB of `CSI row;col H` + text with
      **no line break**; then the marker at a fixed position. Assert every mode, both kitty
      stacks (leave the alternate screen in the emulator and check the main screen's flags), and
      the marker on screen.
- [ ] **Step 2:** `a_mode_cleared_after_the_cut_stays_cleared`: set bracketed paste and mouse,
      flood, clear both, flood less than the ring. Assert both off.
- [ ] **Step 3:** `less_after_the_wrap`: `less` on a long file, scrolled with input until the ring
      has wrapped (less redraws each page); reattach and nudge as in slice 1b; assert the
      alternate screen is on and the paged screen is whole, with no debris.
- [ ] **Step 4:** Keep `reattach_after_the_ring_wrapped_is_clean` passing unchanged in what it
      asserts: line-oriented output still starts on a whole line.
- [ ] **Step 5:** Commit `test(kykod): terminal modes across a wrapped ring`.

### Task 5: Speed, spec, release

**Files:**
- Modify: `docs/superpowers/specs/2026-11-10-kyko-design.md`, `CHANGELOG.md`,
  `docs/notes/2026-11-13-v1-release-checklist.md`
- Create: `docs/notes/2026-11-14-m1c-checks.md`

- [ ] **Step 1:** The hop bench, five runs each way, as slice 1b's note records it, with the load
      average. Within 1.15, or stop and profile.
- [ ] **Step 2:** The scanner bench figures.
- [ ] **Step 3:** Live check on the development machine: `htop` and `vim` each left running in a
      pane with a 256 KiB ring until it has wrapped (`htop` redraws once a second; `vim` with a
      `:terminal`-free macro that redraws), then a reattach through a small test client into
      `kyko-term`; record what was right and what was not. These go in the m1c checks note.
- [ ] **Step 4:** Spec: §2.8 "Replay on attach" describes the preamble and the holder's cut;
      §6.2 item 8 marked closed with the note's name; protocol version 2 in §2.8.
- [ ] **Step 5:** CHANGELOG `[Unreleased]` (Added: modes kept across a wrapped ring; Changed:
      protocol version 2, the replay is cut by the holder; Fixed: kitty keyboard protocol was off),
      then the release steps in `docs/releasing.md` for `0.0.3`.
- [ ] **Step 6:** Tick open risk 8 and the slice 1c items in the release checklist.

## Not in this slice

- SGR state at the cut: a replay may still start with the wrong colours until the program next
  sets them. The spec accepts this (§2.8).
- xterm's save and restore of private modes (`CSI ? Pm s` / `r`): rare; ignored by `Modes`.
- Window title (`OSC 0`/`2`): tab names come from `Info`, not the title.
- Anything the window needs: its link, its replay buffering. That is slice 1d onward.
