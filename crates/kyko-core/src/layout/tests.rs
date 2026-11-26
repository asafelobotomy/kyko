use super::*;
use crate::session::{SpawnRequest, SplitAxis};

const SH: &str = "/bin/bash";
const HOME: &str = "/home/u";

fn info(pane: PaneId, exited: Option<i32>) -> PaneInfo {
    PaneInfo {
        pane,
        program: "bash".into(),
        cwd: format!("/srv/{pane}"),
        cols: 80,
        rows: 24,
        exited,
    }
}

/// Every pane given the `Info` that `info` would bring, so a reconcile matches exactly.
fn settled(mut s: Session) -> Session {
    for id in s.pane_ids() {
        let i = info(id, None);
        s.apply_info(id, &i.program, &i.cwd);
    }
    s
}

fn live(s: &Session) -> Vec<PaneInfo> {
    s.pane_ids().into_iter().map(|p| info(p, None)).collect()
}

/// Tabs 1 [1 | [2 / 3]], 2 [4] renamed, 3 [5] held; tab 2 active.
fn sample() -> Session {
    let (mut s, _) = Session::fresh(HOME, SH);
    s.split(SplitAxis::Side, SH).unwrap();
    s.split(SplitAxis::Stacked, SH).unwrap();
    s.set_ratio(1, &[], 0.3);
    s.set_ratio(1, &[true], 0.71);
    s.focus_pane(1);
    s.new_tab(SpawnRequest::default(), SH).unwrap();
    s.rename(2, Some("logs \"tail\""));
    s.new_tab(
        SpawnRequest {
            hold: true,
            ..Default::default()
        },
        "/usr/bin/zsh",
    )
    .unwrap();
    s.focus_tab(2);
    settled(s)
}

fn reconcile(blob: &[u8], panes: &[PaneInfo]) -> (Session, Vec<Effect>, Vec<Warning>) {
    let (s, e, w) = Session::reconcile(Base::Blob(blob), panes, HOME, SH);
    s.check_invariants().unwrap();
    (s, e, w)
}

#[test]
fn round_trip() {
    let s = sample();
    let (r, e, w) = reconcile(&s.to_blob(), &live(&s));
    assert!(e.is_empty() && w.is_empty(), "{e:?} {w:?}");
    assert_eq!(r, s);
    assert_eq!(r.panes[&5].shell, "zsh");
    assert!(r.panes[&5].hold);
}

#[test]
fn a_pane_gone() {
    let s = sample();
    let panes: Vec<_> = live(&s).into_iter().filter(|p| p.pane != 2).collect();
    let (r, e, _) = reconcile(&s.to_blob(), &panes);
    assert!(e.is_empty());
    let mut want = s.clone();
    want.close_pane(2, SH);
    assert_eq!(r, want);
}

#[test]
fn the_active_tab_gone() {
    let s = sample();
    let panes: Vec<_> = live(&s).into_iter().filter(|p| p.pane != 4).collect();
    let (r, _, _) = reconcile(&s.to_blob(), &panes);
    assert_eq!(r.tab_ids(), [1, 3]);
    assert_eq!(r.active_tab(), 3, "the tab to its right");
    // The last tab, active, gone: the one to its left.
    let mut s = sample();
    s.focus_tab(3);
    let panes: Vec<_> = live(&s).into_iter().filter(|p| p.pane != 5).collect();
    let (r, _, _) = reconcile(&s.to_blob(), &panes);
    assert_eq!(r.active_tab(), 2);
}

#[test]
fn exited_panes_are_closed() {
    let s = sample();
    let mut panes = live(&s);
    panes[4].exited = Some(3); // pane 5, held
    panes[1].exited = Some(0); // pane 2, not held
    let (r, e, _) = reconcile(&s.to_blob(), &panes);
    assert_eq!(e, [Effect::Close { pane: 2 }, Effect::Close { pane: 5 }]);
    assert_eq!(r.pane_state(5), Some(&PaneState::Exited(3)));
    assert_eq!(r.pane_state(2), None);
}

#[test]
fn orphans_get_tabs_and_nothing_left_is_fresh() {
    let s = sample();
    let mut panes = live(&s);
    panes.push(info(70, None));
    panes.push(info(60, None));
    panes.push(info(65, Some(1)));
    let (r, e, _) = reconcile(&s.to_blob(), &panes);
    assert_eq!(e, [Effect::Close { pane: 65 }]);
    let ids = r.tab_ids();
    assert_eq!(ids.len(), 5);
    let last: Vec<PaneId> = r.tabs().skip(3).map(|t| t.panes[0].id).collect();
    assert_eq!(last, [60, 70], "in id order, at the end");
    assert_eq!(r.next_pane, 71);
    // Nothing left at all: fresh, from the counter.
    let (r, e, _) = reconcile(&s.to_blob(), &[info(80, Some(0))]);
    assert_eq!(
        e,
        [
            Effect::Close { pane: 80 },
            Effect::Spawn {
                pane: 81,
                cwd: None,
                args: vec![]
            }
        ]
    );
    assert_eq!(r.tab_ids().len(), 1);
}

#[test]
fn an_empty_blob_gives_a_tab_per_pane() {
    let (r, e, w) = reconcile(b"", &[info(3, None), info(1, None)]);
    assert!(e.is_empty() && w.is_empty());
    let panes: Vec<PaneId> = r.tabs().map(|t| t.panes[0].id).collect();
    assert_eq!(panes, [1, 3]);
    assert_eq!(r.next_pane, 4);
    // And nothing at all: one fresh tab.
    let (r, e, _) = reconcile(b"", &[]);
    assert_eq!(r.tab_ids().len(), 1);
    assert!(matches!(e[..], [Effect::Spawn { pane: 1, .. }]));
}

#[test]
fn corrupt_blobs() {
    let panes = [info(1, None)];
    for bad in [
        &b"garbage"[..],
        b"[]",
        b"{}",
        b"{\"v\":1}",
        b"{\"v\":\"1\",\"active\":1,\"next_pane\":2,\"next_tab\":2,\"tabs\":[]}",
        b"{\"v\":1,\"active\":1,\"next_pane\":4294967296,\"next_tab\":2,\"tabs\":[]}",
    ] {
        let (r, _, w) = reconcile(bad, &panes);
        assert_eq!(w.len(), 1, "{}", String::from_utf8_lossy(bad));
        assert_eq!(r.tab_ids().len(), 1);
    }
}

fn blob(tabs: &str) -> Vec<u8> {
    format!("{{\"v\":1,\"active\":1,\"next_pane\":10,\"next_tab\":10,\"tabs\":[{tabs}]}}")
        .into_bytes()
}

#[test]
fn a_newer_blob_is_read_for_what_it_shares() {
    let b = br#"{"v":2,"active":1,"next_pane":10,"next_tab":10,"colour":"red",
        "tabs":[{"id":1,"nodes":[{"pane":1}],"pinned":true}]}"#;
    let (r, _, w) = reconcile(b, &[info(1, None)]);
    assert_eq!(w.len(), 1);
    assert!(w[0].message.contains("newer"));
    assert_eq!(r.tab_ids(), [1]);
    assert_eq!(r.next_pane, 10);
}

#[test]
fn bad_trees() {
    let panes = [info(1, None), info(2, None)];
    // A pane twice: the second leaf goes.
    let b = blob(
        r#"{"id":1,"nodes":[{"split":"side","ratio":0.5,"a":1,"b":2},{"pane":1},{"pane":1}]},
                    {"id":2,"nodes":[{"pane":2}]}"#,
    );
    let (r, _, w) = reconcile(&b, &panes);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(r.tab_ids(), [1, 2]);
    assert_eq!(r.tabs().next().unwrap().panes.len(), 1);
    // A pane in two tabs: the later tab loses it, and goes.
    let b = blob(r#"{"id":1,"nodes":[{"pane":1}]},{"id":2,"nodes":[{"pane":1}]}"#);
    let (r, _, w) = reconcile(&b, &panes);
    assert_eq!(w.len(), 1);
    // tab 1, then pane 2 as an orphan in a new tab
    assert_eq!(r.tab_ids(), [1, 10]);
    // A cycle, an index out of range, a node nobody reaches, a bad axis, a NaN-like ratio.
    for nodes in [
        r#"[{"split":"side","ratio":0.5,"a":0,"b":1},{"pane":1}]"#,
        r#"[{"split":"side","ratio":0.5,"a":1,"b":5},{"pane":1}]"#,
        r#"[{"pane":1},{"pane":2}]"#,
        r#"[{"split":"diagonal","ratio":0.5,"a":1,"b":2},{"pane":1},{"pane":2}]"#,
        r#"[{"split":"side","ratio":null,"a":1,"b":2},{"pane":1},{"pane":2}]"#,
        r#"[{"pane":1,"split":"side"}]"#,
        r#"[]"#,
        r#"[{"pane":4294967296}]"#,
    ] {
        let b = blob(&format!(r#"{{"id":1,"nodes":{nodes}}}"#));
        let (r, _, w) = reconcile(&b, &panes);
        assert_eq!(w.len(), 1, "{nodes}: {w:?}");
        assert_eq!(r.tab_ids().len(), 2, "{nodes}: both panes as orphans");
    }
    // A ratio out of bounds is kept within them.
    let b =
        blob(r#"{"id":1,"nodes":[{"split":"side","ratio":7,"a":1,"b":2},{"pane":1},{"pane":2}]}"#);
    let (r, _, w) = reconcile(&b, &panes);
    assert!(w.is_empty());
    assert_eq!(r.tabs().next().unwrap().dividers[0].ratio, MAX_RATIO);
}

#[test]
fn counters() {
    // next_pane at u32::MAX: no id left, so the blob is ignored.
    let b = br#"{"v":1,"active":1,"next_pane":4294967295,"next_tab":2,"tabs":[{"id":1,"nodes":[{"pane":1}]}]}"#;
    let (r, _, w) = reconcile(b, &[info(1, None)]);
    assert_eq!(w.len(), 1);
    assert_eq!(r.tab_ids().len(), 1);
    assert_eq!(r.next_pane, 2);
    // A stored counter below the ids in the blob is raised above them.
    let b =
        br#"{"v":1,"active":7,"next_pane":1,"next_tab":1,"tabs":[{"id":7,"nodes":[{"pane":5}]}]}"#;
    let (r, _, w) = reconcile(b, &[info(5, None)]);
    assert!(w.is_empty());
    assert_eq!((r.next_pane, r.next_tab), (6, 8));
    // A pane id of u32::MAX from kykod cannot be shown.
    let (r, e, w) = reconcile(b"", &[info(u32::MAX, None)]);
    assert_eq!(w.len(), 1);
    assert_eq!(e[0], Effect::Close { pane: u32::MAX });
    assert!(r.pane_ids().iter().all(|p| *p != u32::MAX));
}

#[test]
fn the_deepest_tree_round_trips() {
    let (mut s, _) = Session::fresh(HOME, SH);
    for i in 0..255 {
        let axis = if i % 2 == 0 {
            SplitAxis::Side
        } else {
            SplitAxis::Stacked
        };
        s.split(axis, SH).unwrap();
    }
    let s = settled(s);
    let b = s.to_blob();
    let (r, e, w) = reconcile(&b, &live(&s));
    assert!(e.is_empty() && w.is_empty());
    assert_eq!(r, s);
}

#[test]
fn the_worst_case_fits() {
    let worst = "\"".repeat(MAX_RENAME_FOR_TEST);
    // 256 tabs, each with the longest rename escaping worst.
    let (mut s, _) = Session::fresh(HOME, "/usr/bin/fifteen-bytes-x");
    for _ in 1..MAX_PANES {
        s.new_tab(SpawnRequest::default(), "/usr/bin/fifteen-bytes-x")
            .unwrap();
    }
    for t in s.tab_ids() {
        s.rename(t, Some(&worst));
    }
    let wide = s.to_blob().len();
    // One tab with the deepest tree.
    let (mut d, _) = Session::fresh(HOME, SH);
    for _ in 1..MAX_PANES {
        d.split(SplitAxis::Side, SH).unwrap();
    }
    d.rename(1, Some(&worst));
    let deep = d.to_blob().len();
    assert!(wide < MAX_LAYOUT && deep < MAX_LAYOUT, "{wide} {deep}");
    assert!(wide < 200_000, "{wide}");
}

const MAX_RENAME_FOR_TEST: usize = crate::session::MAX_RENAME;

#[test]
fn current_after_the_holder_was_lost() {
    let mut s = sample();
    s.holder_lost();
    let before = s.clone();
    let (r, e, w) = Session::reconcile(Base::Current(s.clone()), &[], HOME, SH);
    r.check_invariants().unwrap();
    assert!(e.is_empty() && w.is_empty());
    assert_eq!(r, before, "kept as it is, Ended panes included");
    // A new kykod with one pane: it gets a tab; the counter goes above it.
    let (r, _, _) = Session::reconcile(Base::Current(s), &[info(40, None)], HOME, SH);
    r.check_invariants().unwrap();
    assert_eq!(r.tab_ids().len(), 4);
    assert_eq!(r.pane_state(40), Some(&PaneState::Running));
    assert_eq!(r.pane_state(1), Some(&PaneState::Ended));
    assert_eq!(r.next_pane, 41);
}
