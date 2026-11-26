//! Panes: broadcast, zoom, focus, closing, new tabs and the pane limit.

use super::*;

#[test]
fn broadcast_types_into_every_running_pane_of_the_tab() {
    let mut s = fresh();
    let first = s.focused_pane();
    assert!(!s.toggle_broadcast(), "one pane: nothing to broadcast to");
    assert_eq!(s.typing_targets(first), vec![first]);
    s.split(SplitAxis::Side, SH).unwrap();
    let second = s.focused_pane();
    assert!(s.toggle_broadcast());
    assert!(tab(&s).broadcast);
    let mut got = s.typing_targets(second);
    got.sort();
    assert_eq!(got, vec![first, second]);
    s.pane_exited(first, 0, SH);
    assert_eq!(s.typing_targets(second), vec![second]);
    assert!(!tab(&s).broadcast, "a lone pane does not broadcast");
    // A new split starts without it.
    s.split(SplitAxis::Side, SH).unwrap();
    assert!(!tab(&s).broadcast);
}

#[test]
fn zoom_fills_the_tab_with_the_focused_pane() {
    let mut s = fresh();
    assert!(!s.toggle_zoom(), "one pane: nothing to zoom");
    s.split(SplitAxis::Side, SH).unwrap();
    let focused = s.focused_pane();
    assert!(s.toggle_zoom());
    let t = tab(&s);
    assert!(t.zoomed);
    assert!(t.dividers.is_empty());
    for p in &t.panes {
        assert_eq!(p.rect, UNIT.rect());
        assert_eq!(p.hidden, p.id != focused);
    }
    // Moving the focus shows the other pane, still zoomed.
    assert!(s.focus_dir(Dir::Left));
    let t = tab(&s);
    assert!(t.panes.iter().any(|p| p.id != focused && !p.hidden));
    // A split brings the panes back.
    s.split(SplitAxis::Stacked, SH).unwrap();
    let t = tab(&s);
    assert!(!t.zoomed);
    assert!(t.panes.iter().all(|p| !p.hidden));
    assert!(s.toggle_zoom());
    assert!(s.toggle_zoom());
    assert!(!tab(&s).zoomed);
}

#[test]
fn fresh_spawns_one_pane() {
    let (s, e) = Session::fresh("/home/u", SH);
    assert_eq!(
        e,
        [Effect::Spawn {
            pane: 1,
            cwd: None,
            args: vec![]
        }]
    );
    assert_eq!(s.focused_pane(), 1);
    assert_eq!(tab(&s).name, "bash");
    s.check_invariants().unwrap();
}

#[test]
fn focus_in_an_l_shape() {
    // [1 | [2 / 3]]
    let mut s = fresh();
    s.split(SplitAxis::Side, SH).unwrap();
    s.split(SplitAxis::Stacked, SH).unwrap();
    assert_eq!(s.focused_pane(), 3);
    assert!(s.focus_dir(Dir::Up));
    assert_eq!(s.focused_pane(), 2);
    assert!(s.focus_dir(Dir::Left));
    assert_eq!(s.focused_pane(), 1);
    // From 1, right touches both 2 and 3 equally: the most recently focused, 2.
    assert!(s.focus_dir(Dir::Right));
    assert_eq!(s.focused_pane(), 2);
    assert!(s.focus_dir(Dir::Down));
    assert!(s.focus_dir(Dir::Left));
    assert!(s.focus_dir(Dir::Right));
    assert_eq!(s.focused_pane(), 3, "3 was more recent than 2");
    assert!(!s.focus_dir(Dir::Right));
    assert!(!s.focus_dir(Dir::Down));
    s.check_invariants().unwrap();
}

#[test]
fn focus_in_a_stack_of_three_and_a_grid_of_four() {
    let mut s = fresh();
    s.split(SplitAxis::Stacked, SH).unwrap();
    s.split(SplitAxis::Stacked, SH).unwrap();
    assert!(s.focus_dir(Dir::Up));
    assert!(s.focus_dir(Dir::Up));
    assert_eq!(s.focused_pane(), 1);
    assert!(!s.focus_dir(Dir::Up));
    assert!(!s.focus_dir(Dir::Left));
    // Grid: [[1 / 3] | [2 / 4]]
    let mut g = fresh();
    g.split(SplitAxis::Side, SH).unwrap(); // 2 right of 1
    g.focus_pane(1);
    g.split(SplitAxis::Stacked, SH).unwrap(); // 3 below 1
    g.focus_pane(2);
    g.split(SplitAxis::Stacked, SH).unwrap(); // 4 below 2
    assert_eq!(g.focused_pane(), 4);
    assert!(g.focus_dir(Dir::Left));
    assert_eq!(g.focused_pane(), 3);
    assert!(g.focus_dir(Dir::Up));
    assert_eq!(g.focused_pane(), 1);
    assert!(g.focus_dir(Dir::Right));
    assert_eq!(g.focused_pane(), 2);
    g.check_invariants().unwrap();
}

#[test]
fn focus_through_255_nested_splits() {
    // Each split is of pane 1, which shrinks towards the origin, where f64 keeps its
    // precision at every depth.
    let mut s = fresh();
    for i in 0..255 {
        let axis = if i % 2 == 0 {
            SplitAxis::Side
        } else {
            SplitAxis::Stacked
        };
        s.split(axis, SH).unwrap();
        s.focus_pane(1);
    }
    assert_eq!(s.panes.len(), 256);
    s.check_invariants().unwrap();
    assert!(rect_of(&s, 1).w > 0.0 && rect_of(&s, 1).h > 0.0);
    // The deepest pane walks out to the first split's other half.
    let mut moves = 0;
    while s.focus_dir(Dir::Right) || s.focus_dir(Dir::Down) {
        moves += 1;
        assert!(moves < 1000);
    }
    assert_eq!(s.focused_pane(), 2);
    assert!(moves >= 1);
    // And back in: left from 2 lands in the chain.
    assert!(s.focus_dir(Dir::Left));
    assert_ne!(s.focused_pane(), 2);
    assert_eq!(s.split(SplitAxis::Side, SH), Err(Full));
    assert_eq!(s.new_tab(SpawnRequest::default(), SH), Err(Full));
    s.check_invariants().unwrap();
}

#[test]
fn closing_the_middle_of_three() {
    let mut s = fresh();
    s.split(SplitAxis::Side, SH).unwrap();
    s.split(SplitAxis::Side, SH).unwrap();
    s.focus_pane(1);
    let e = s.close_pane(2, SH);
    assert_eq!(e, [Effect::Close { pane: 2 }]);
    s.check_invariants().unwrap();
    assert_eq!(s.focused_pane(), 1);
    // 3 keeps its own half; 1 takes its half.
    assert_eq!(rect_of(&s, 3).x, 0.5);
    assert_eq!(rect_of(&s, 1).w, 0.5);
    // Closing the focused one: focus goes to the most recent left, 3.
    s.focus_pane(3);
    s.focus_pane(1);
    let e = s.close_pane(1, SH);
    assert_eq!(e, [Effect::Close { pane: 1 }]);
    assert_eq!(s.focused_pane(), 3);
    assert_eq!(
        rect_of(&s, 3),
        Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0
        }
    );
    s.check_invariants().unwrap();
}

#[test]
fn closing_the_last_pane_and_the_last_tab() {
    let mut s = fresh();
    s.new_tab(SpawnRequest::default(), SH).unwrap();
    let second = s.active_tab();
    assert_eq!(s.close_pane(2, SH), [Effect::Close { pane: 2 }]);
    assert!(s.tabs().all(|t| t.id != second));
    s.check_invariants().unwrap();
    let e = s.close_pane(1, SH);
    assert_eq!(
        e,
        [
            Effect::Close { pane: 1 },
            Effect::Spawn {
                pane: 3,
                cwd: None,
                args: vec![]
            },
            Effect::Hide
        ]
    );
    assert_eq!(s.tabs().count(), 1);
    s.check_invariants().unwrap();
}

#[test]
fn closing_a_tab_picks_the_right_then_the_left() {
    let mut s = fresh(); // tab 1
    for _ in 0..3 {
        s.new_tab(SpawnRequest::default(), SH).unwrap(); // tabs 2, 3, 4 in order
    }
    let ids: Vec<TabId> = s.tabs().map(|t| t.id).collect();
    assert_eq!(ids, [1, 2, 3, 4]);
    s.focus_tab(2);
    s.close_tab(2, SH);
    assert_eq!(s.active_tab(), 3);
    s.focus_tab(4);
    s.close_tab(4, SH);
    assert_eq!(s.active_tab(), 3);
    // A non-active tab: the active one stays.
    s.close_tab(1, SH);
    assert_eq!(s.active_tab(), 3);
    // A new tab goes right after the active one.
    s.focus_tab(3);
    s.new_tab(SpawnRequest::default(), SH).unwrap();
    s.check_invariants().unwrap();
    assert!(s.close_tab(99, SH).is_empty());
}

#[test]
fn new_tab_goes_after_the_active_one() {
    let mut s = fresh();
    s.new_tab(SpawnRequest::default(), SH).unwrap();
    s.focus_tab(1);
    s.new_tab(SpawnRequest::default(), SH).unwrap();
    let ids: Vec<TabId> = s.tabs().map(|t| t.id).collect();
    assert_eq!(ids, [1, 3, 2]);
    assert_eq!(s.active_tab(), 3);
}

#[test]
fn held_failed_and_ended_panes() {
    let mut s = fresh();
    let e = s
        .new_tab(
            SpawnRequest {
                cwd: Some("/tmp".into()),
                args: vec!["-c".into(), "make".into()],
                hold: true,
            },
            SH,
        )
        .unwrap();
    assert_eq!(
        e,
        [Effect::Spawn {
            pane: 2,
            cwd: Some("/tmp".into()),
            args: vec!["-c".into(), "make".into()]
        }]
    );
    // Held: Exited, Close at once, stays.
    assert_eq!(s.pane_exited(2, 2, SH), [Effect::Close { pane: 2 }]);
    assert_eq!(s.pane_state(2), Some(&PaneState::Exited(2)));
    assert!(s.pane_exited(2, 2, SH).is_empty(), "exits once");
    // Dismiss: closes it, no second Close.
    assert!(s.dismiss(2, SH).is_empty());
    assert_eq!(s.pane_state(2), None);
    // Not held: closes with Close.
    s.split(SplitAxis::Side, SH).unwrap();
    assert_eq!(s.pane_exited(3, 0, SH), [Effect::Close { pane: 3 }]);
    assert_eq!(s.pane_state(3), None);
    // Failed: no Close, dismiss closes it.
    s.split(SplitAxis::Side, SH).unwrap();
    s.spawn_failed(4, "no such file");
    assert_eq!(
        s.pane_state(4),
        Some(&PaneState::Failed("no such file".into()))
    );
    assert!(s.dismiss(4, SH).is_empty());
    assert!(
        s.dismiss(1, SH).is_empty(),
        "a running pane is not dismissed"
    );
    assert_eq!(s.pane_state(1), Some(&PaneState::Running));
    // Holder lost: Ended, no Close on dismiss or close.
    s.split(SplitAxis::Side, SH).unwrap();
    s.holder_lost();
    assert_eq!(s.pane_state(1), Some(&PaneState::Ended));
    assert!(s.dismiss(5, SH).is_empty());
    let e = s.close_tab(s.active_tab(), SH);
    assert_eq!(
        e,
        [
            Effect::Spawn {
                pane: 6,
                cwd: None,
                args: vec![]
            },
            Effect::Hide
        ]
    );
    s.check_invariants().unwrap();
}

#[test]
fn full_at_256_panes() {
    let mut s = fresh();
    for _ in 0..255 {
        s.new_tab(SpawnRequest::default(), SH).unwrap();
    }
    let before = s.clone();
    assert_eq!(s.new_tab(SpawnRequest::default(), SH), Err(Full));
    assert_eq!(s.split(SplitAxis::Stacked, SH), Err(Full));
    assert_eq!(s, before);
    // An exited held pane still counts.
    s.close_tab(s.active_tab(), SH);
    s.new_tab(
        SpawnRequest {
            hold: true,
            ..Default::default()
        },
        SH,
    )
    .unwrap();
    s.pane_exited(s.focused_pane(), 0, SH);
    assert_eq!(s.split(SplitAxis::Side, SH), Err(Full));
}
