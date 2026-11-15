mod common;

use common::{HOME, Rng, SHELL, apply, cases};
use kyko_core::session::{Effect, PaneState, Session};

/// Seeded sequences of 200 operations; the invariants hold after every one. A failure names the
/// seed and the step, which is the shortest failing prefix.
#[test]
fn invariants_hold_under_random_operations() {
    for seed in 0..cases(5_000) as u64 {
        let mut rng = Rng::new(seed);
        let (mut s, _) = Session::fresh(HOME, SHELL);
        let mut ops = Vec::new();
        for step in 0..200 {
            let op = rng.op(&s);
            ops.push(op.clone());
            let before = s.pane_ids();
            let effects = apply(&mut s, &op);
            if step % 50 == 49 {
                // The views too, now and then: they must not panic.
                let _ = s.tabs().count();
            }
            let fail = |why: String| -> ! {
                panic!("seed {seed}, step {step}: {why}\nops: {ops:#?}");
            };
            if let Err(e) = s.check_invariants() {
                fail(e);
            }
            for e in &effects {
                match e {
                    Effect::Spawn { pane, .. } => {
                        if before.contains(pane) {
                            fail(format!("Spawn of existing pane {pane}"));
                        }
                        if s.pane_state(*pane) != Some(&PaneState::Running) {
                            fail(format!("Spawn of pane {pane} that is not running"));
                        }
                    }
                    Effect::Close { pane } => {
                        if s.pane_state(*pane) == Some(&PaneState::Running) {
                            fail(format!("Close of pane {pane} that is still running"));
                        }
                        if !before.contains(pane) {
                            fail(format!("Close of unknown pane {pane}"));
                        }
                    }
                    Effect::Hide => {}
                }
            }
        }
    }
}
