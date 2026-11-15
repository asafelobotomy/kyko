mod common;

use std::collections::BTreeMap;

use common::{HOME, Rng, SHELL, cases, random_session};
use kyko_core::layout::Base;
use kyko_core::session::{PaneState, Session, TabId};
use kyko_proto::{MAX_LAYOUT, PaneId, PaneInfo};

fn info(pane: PaneId, exited: Option<i32>) -> PaneInfo {
    PaneInfo {
        pane,
        program: "bash".into(),
        cwd: "/tmp".into(),
        cols: 80,
        rows: 24,
        exited,
    }
}

/// Each pane's tab, and the tabs' panes in leaf order.
fn layout(s: &Session) -> Vec<(TabId, Vec<PaneId>)> {
    s.tabs()
        .map(|t| (t.id, t.panes.iter().map(|p| p.id).collect()))
        .collect()
}

#[test]
fn reconcile_keeps_the_survivors_in_place() {
    for seed in 0..cases(2_000) as u64 {
        let mut rng = Rng::new(seed);
        let steps = 1 + rng.below(150);
        let s = random_session(&mut rng, steps);
        let blob = s.to_blob();
        assert!(blob.len() <= MAX_LAYOUT);
        // What kykod has: the running panes, some gone, some exited, and a few orphans.
        let mut panes = Vec::new();
        for id in s.pane_ids() {
            if s.pane_state(id) != Some(&PaneState::Running) || rng.chance(5) {
                continue;
            }
            panes.push(info(id, rng.chance(6).then_some(1)));
        }
        let mut orphan = s.pane_ids().into_iter().max().unwrap_or(0) + 1000;
        for _ in 0..rng.below(3) {
            panes.push(info(orphan, rng.chance(4).then_some(0)));
            orphan += 1 + rng.below(5) as u32;
        }
        let (r, effects, _) = Session::reconcile(Base::Blob(&blob), &panes, HOME, SHELL);
        if let Err(e) = r.check_invariants() {
            panic!("seed {seed}: {e}");
        }
        // Every survivor is in its old tab, in its old order, and the tabs keep their order.
        let before = layout(&s);
        let after = layout(&r);
        let tab_of: BTreeMap<PaneId, TabId> = before
            .iter()
            .flat_map(|(t, ps)| ps.iter().map(move |p| (*p, *t)))
            .collect();
        for (tab, ps) in &after {
            let old: Vec<PaneId> = ps
                .iter()
                .filter(|p| tab_of.contains_key(p))
                .copied()
                .collect();
            for p in &old {
                assert_eq!(tab_of[p], *tab, "seed {seed}: pane {p} moved tab");
            }
            let was = &before
                .iter()
                .find(|(t, _)| t == tab)
                .map(|(_, ps)| ps.clone());
            if let Some(was) = was {
                let kept: Vec<PaneId> = was.iter().filter(|p| old.contains(p)).copied().collect();
                assert_eq!(kept, old, "seed {seed}: tab {tab} reordered");
            }
        }
        let old_tabs: Vec<TabId> = before.iter().map(|(t, _)| *t).collect();
        let kept: Vec<TabId> = after
            .iter()
            .map(|(t, _)| *t)
            .filter(|t| old_tabs.contains(t))
            .collect();
        let want: Vec<TabId> = old_tabs
            .iter()
            .filter(|t| kept.contains(t))
            .copied()
            .collect();
        assert_eq!(kept, want, "seed {seed}: tabs reordered");
        // Every exited pane is closed exactly once.
        for p in panes.iter().filter(|p| p.exited.is_some()) {
            let n = effects
                .iter()
                .filter(|e| **e == kyko_core::session::Effect::Close { pane: p.pane })
                .count();
            assert_eq!(n, 1, "seed {seed}: pane {}", p.pane);
        }
        // And the round trip of the result is stable.
        let live: Vec<PaneInfo> = r
            .pane_ids()
            .into_iter()
            .filter(|p| r.pane_state(*p) == Some(&PaneState::Running))
            .map(|p| info(p, None))
            .collect();
        let (again, _, w) = Session::reconcile(Base::Blob(&r.to_blob()), &live, HOME, SHELL);
        assert!(w.is_empty(), "seed {seed}: {w:?}");
        again.check_invariants().unwrap();
    }
}

#[test]
fn random_bytes_as_a_blob_never_panic() {
    let mut rng = Rng::new(0xB10B);
    let s = random_session(&mut rng, 100);
    let good = s.to_blob();
    let panes: Vec<PaneInfo> = s.pane_ids().into_iter().map(|p| info(p, None)).collect();
    for i in 0..cases(5_000) {
        let bytes = if i % 2 == 0 {
            rng.bytes(300)
        } else {
            let mut b = good.clone();
            for _ in 0..=rng.below(6) {
                let j = rng.below(b.len());
                b[j] = *rng.pick(b"{}[]\":,0123456789-e.\\ ");
            }
            b
        };
        let (r, _, _) = Session::reconcile(Base::Blob(&bytes), &panes, HOME, SHELL);
        if let Err(e) = r.check_invariants() {
            panic!("case {i}: {e}\n{}", String::from_utf8_lossy(&bytes));
        }
    }
}
