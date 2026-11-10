# SDD ledger — plan: docs/superpowers/plans/2026-11-10-kyko-m0-spike.md
Pre-flight: T1 Toggler.toggled()/root.shown -> T3,T4 consume: consistent. T2 Engine/Snapshot/Palette -> T3 bridge: consistent. T3 GridItem::paint -> T5: consistent.
Task 1: Ruling: slide animates the panel inside a fixed surface, not LayerShell margins — QMargins is not settable from QML — cost if wrong: milestone 1 picks a different slide mechanism
Task 1: complete (commits d9b6c8b..6556faf, tests: cargo build --manifest-path spike/Cargo.toml →     Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.21s)
Task 2: Ruling: tests live in src/engine_tests.rs and src/theme_tests.rs (via #[path]) instead of inline — lets the tests be written and seen failing before the implementation file exists — cost if wrong: none, spike code
Task 2: Ruling: added Engine::spawn_args(shell, args, ...) beside spawn — the tests need `/bin/sh -c script` — cost if wrong: none
Task 2: Ruling: the engine uses alacritty_terminal's own EventLoop and tty (not a hand-written reader thread); on_dirty is coalesced on top of its Wakeup events; exit is published on Event::Exit, after the last output is drained — the designed path in 0.26, far less code — cost if wrong: milestone 1's holder split cannot reuse EventLoop as is, since kykod owns the PTY
Task 2: Ruling: inverse cells arrive with fg/bg already swapped (INVERSE flag kept for information) — keeps the painter trivial — cost if wrong: none
Task 2: complete (commits 6556faf..40b0eb7, tests: cargo test --manifest-path spike/Cargo.toml → test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s)
Task 3: Ruling: GridItem is registered with qmlRegisterType as org.kyko.spike.native (not the cxx-qt QML module), and build.rs runs moc by hand — cxx-qt-build 0.10 has no moc for hand-written C++ headers — cost if wrong: none for a spike; milestone 1 should weigh writing the item as a cxx-qt QObject instead
Task 3: Ruling: added KYKO_SPIKE_NOFOCUS and KYKO_SPIKE_TYPE test aids, and claim SIGUSR1 before writing the pid file — automated runs were taking the user's keyboard, and an early toggle killed the process — cost if wrong: none
Task 3: Ruling: Step 2 (typing by hand: vim insert, Ctrl+C, arrow history) NOT done; rendering of the same programs verified by injected input — I cannot press real keys — cost if wrong: key mapping bugs surface only when the user tries it
Task 3: complete (commits 40b0eb7..701004c, tests: cargo test --manifest-path spike/Cargo.toml → test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s)
Task 4: Ruling: raw zbus (blocking) instead of ashpd — one less unknown API — cost if wrong: none
Task 4: Ruling: tested with LOGO+F12 not F12 — F12 is bound to Yakuake on this machine — cost if wrong: none
Task 4: Ruling: activation tested through KGlobalAccel invokeShortcut and a self-closing fullscreen QML window, with keyboard interactivity None; Steps 2(a,b,d), keyboard-focus-on-summon and the Exclusive/OnDemand/activateOnShow comparison in Step 3 NOT done — they need real key presses and would take the user's keyboard — cost if wrong: focus-on-summon stays an open risk into milestone 1
Task 4: Ruling: removed the stray `toggle` entry the first run left under [com.anthropic.Claude] in kglobalshortcutsrc, and the org.kyko.spike test entry and desktop file — my own test artefacts in the user's config — cost if wrong: none (Claude's own shortcut verified intact)
Task 4: complete (commits 701004c..81d0009, tests: cargo test --manifest-path spike/Cargo.toml → test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s)
Task 5: Ruling: paint_ms is printed every 300 paints as well as on exit — the harness stops the spike with SIGTERM, which skips destructors — cost if wrong: none
Task 5: Ruling: bench.sh injects everything instead of printing commands to paste — nothing needs the keyboard, and it is repeatable — cost if wrong: none
Task 5: Ruling: fallbacks (b) and (c) were done together as a per-row image cache keyed by row contents; dirty-rectangle repaint was not tried — one mechanism covers unchanged and scrolled rows — cost if wrong: the typical one-row update cost is unmeasured
Task 5: Ruling: Hyprland VM step NOT run — machine at load 16 with 11 GB swap in use and other sessions building; a 4 GB VM for ~40 min risked their work — cost if wrong: every Hyprland question stays open into milestone 1
Task 5: Ruling: compared the flood with Konsole, not kitty (not installed) — Konsole is what Yakuake embeds — cost if wrong: none
Task 5: Ruling: added two engine tests (ctrl_c_stops_a_flood, ..._while_snapshots_are_taken) after a harness probe wrongly showed Ctrl+C being ignored — they passed first run, so they pin behaviour that already held; the probe was the bug — cost if wrong: none
Task 5: complete (commits 81d0009..0ae8661, tests: cargo test --manifest-path spike/Cargo.toml → test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s)
Task 6: complete (commits 0ae8661..9b8bc73, tests: cargo test --manifest-path spike/Cargo.toml → test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s)
Final review: fresh reviewer (most capable model). Verdicts upheld. 1 Critical, 8 Important, 9 Minor.
Final: fixed (doc) memory row wrongly said the spec was unclear — now Fail, by about 2x; proposal 5 reworded
Final: fixed (doc) dense and plain benchmark screens labelled 55% and 25% full; paint-only caveat restored; 60 Hz paragraph rewritten
Final: fixed (doc) "never blocks input" narrowed to what was shown (PTY writer not starved; keyboard path untested)
Final: fixed (doc) rebind claim narrowed; proposals 2 and 3 reworded to what was tried; Keep/Drop lists corrected; three weak tests named
Final: Ruling: no code or test changes for findings about the spike's tests (coalescing test cannot fail, UTF-8 and resize tests assert little) — the code is throwaway; the note now says what each test does not prove — cost if wrong: none, milestone 1 writes its own tests
Final: minor (deferred): Ctrl+C probe, CPU and memory measurement methods are not saved in the repo
Final: minor (deferred): framebuffer fallback judged on paint time, which excludes the upload it would remove
Final: minor (deferred): Ctrl+C tests assert exit within 5 s, not 100 ms
