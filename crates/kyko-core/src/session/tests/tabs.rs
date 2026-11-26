//! Tabs: ids, renames, names from Info, ratios, the split's folder and switching.

use super::*;

#[test]
fn no_ids_left() {
    let mut s = fresh();
    s.next_pane = u32::MAX;
    assert_eq!(s.split(SplitAxis::Side, SH), Err(Full));
    assert_eq!(s.new_tab(SpawnRequest::default(), SH), Err(Full));
    // The last tab cannot be replaced: it stays, and kyko hides.
    assert_eq!(s.close_tab(1, SH), [Effect::Hide]);
    s.check_invariants().unwrap();
}

#[test]
fn renames() {
    let mut s = fresh();
    assert!(s.rename(1, Some("build")));
    assert_eq!(tab(&s).name, "build");
    assert!(s.rename(1, None));
    assert_eq!(tab(&s).name, "bash");
    s.rename(1, Some("a\u{1b}[31mb\nc\u{7}"));
    assert_eq!(tab(&s).name, "a[31mbc");
    s.rename(1, Some("\n\t"));
    assert_eq!(tab(&s).name, "bash", "an empty result clears");
    let long = "€".repeat(300);
    s.rename(1, Some(&long));
    let name = tab(&s).name;
    assert!(name.len() <= MAX_RENAME && name.len() > MAX_RENAME - 3);
    assert!(name.chars().all(|c| c == '€'));
    assert!(!s.rename(42, Some("x")));
    s.check_invariants().unwrap();
}

#[test]
fn names_from_info() {
    let mut s = fresh();
    assert_eq!(tab(&s).name, "bash");
    assert!(s.apply_info(1, "bash", "/home/u"));
    assert_eq!(tab(&s).name, "~");
    assert!(s.apply_info(1, "bash", "/"));
    assert_eq!(tab(&s).name, "/");
    assert!(s.apply_info(1, "bash", "/home/u/src/kyko"));
    assert_eq!(tab(&s).name, "kyko");
    assert!(
        !s.apply_info(1, "bash", "/home/u/src/kyko"),
        "nothing changed"
    );
    assert!(s.apply_info(1, "vim", "/home/u/src/kyko"));
    assert_eq!(tab(&s).name, "vim");
    assert_eq!(tab(&s).program, "vim");
    assert_eq!(s.busy_panes(1), [1]);
    // An empty program is idle.
    assert!(s.apply_info(1, "", "/home/u/src/kyko"));
    assert_eq!(tab(&s).name, "kyko");
    assert!(s.busy_panes(1).is_empty());
    // A pane that is not focused changes neither the name nor the program.
    s.split(SplitAxis::Side, SH).unwrap();
    s.focus_pane(1);
    assert!(!s.apply_info(2, "", "/home/u"));
    assert!(!s.apply_info(99, "x", "/"));
}

#[test]
fn a_15_byte_comm_is_the_shell() {
    let shell = "/usr/local/bin/a-very-long-shell-name";
    let (mut s, _) = Session::fresh("/home/u", shell);
    assert_eq!(tab(&s).name, "a-very-long-she");
    // kykod's first Info may carry the uncut name, later ones the cut one: both idle.
    s.apply_info(1, "a-very-long-shell-name", "/home/u");
    assert_eq!(tab(&s).name, "~");
    s.apply_info(1, "a-very-long-she", "/home/u");
    assert_eq!(tab(&s).name, "~");
    assert!(s.busy_panes(1).is_empty());
    s.apply_info(1, "a-very-long-sh", "/home/u");
    assert_eq!(s.busy_panes(1), [1]);
}

#[test]
fn ratios() {
    let mut s = fresh();
    s.split(SplitAxis::Side, SH).unwrap();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!s.set_ratio(1, &[], bad));
    }
    assert!(s.set_ratio(1, &[], 0.0));
    assert_eq!(tab(&s).dividers[0].ratio, MIN_RATIO);
    assert!(s.set_ratio(1, &[], 7.0));
    assert_eq!(tab(&s).dividers[0].ratio, MAX_RATIO);
    assert!(s.set_ratio(1, &[], 0.3));
    assert_eq!(rect_of(&s, 1).w, 0.3);
    assert!(!s.set_ratio(1, &[false], 0.3), "a leaf has no ratio");
    assert!(!s.set_ratio(9, &[], 0.3));
    s.check_invariants().unwrap();
}

#[test]
fn split_spawns_in_the_focused_folder() {
    let mut s = fresh();
    let e = s.split(SplitAxis::Side, SH).unwrap();
    assert_eq!(
        e,
        [Effect::Spawn {
            pane: 2,
            cwd: None,
            args: vec![]
        }]
    );
    s.apply_info(2, "bash", "/srv");
    let e = s.split(SplitAxis::Stacked, SH).unwrap();
    assert_eq!(
        e,
        [Effect::Spawn {
            pane: 3,
            cwd: Some("/srv".into()),
            args: vec![]
        }]
    );
    assert_eq!(s.new_tab_dir(NewTabDir::Home), None);
    s.apply_info(3, "bash", "/var");
    assert_eq!(s.new_tab_dir(NewTabDir::Current), Some("/var".into()));
}

#[test]
fn tab_switching_wraps() {
    let mut s = fresh();
    s.new_tab(SpawnRequest::default(), SH).unwrap();
    s.new_tab(SpawnRequest::default(), SH).unwrap();
    s.next_tab();
    assert_eq!(s.active_tab(), 1);
    s.prev_tab();
    assert_eq!(s.active_tab(), 3);
    assert!(s.focus_tab_index(1));
    assert_eq!(s.active_tab(), 2);
    assert!(!s.focus_tab_index(3));
    assert!(!s.focus_tab(7));
    // A click on a pane in another tab switches to it.
    assert!(s.focus_pane(1));
    assert_eq!(s.active_tab(), 1);
}
