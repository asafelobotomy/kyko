//! The layout blob `kykod` keeps for the window (`SaveLayout`, returned in `Hello`), and
//! reconcile: rebuilding the session from it and the panes `kykod` actually has.
//!
//! The blob is versioned JSON. Each tab's split tree is a flat list of nodes in preorder, a
//! split naming its children by index, because `serde_json` stops at a nesting depth of 128 and
//! `MAX_PANES` allows a tree 255 deep. Neither a pane's program nor its folder is stored:
//! `Hello` brings them fresh. `kykod` never reads the blob.

use std::collections::{BTreeMap, BTreeSet};

use kyko_proto::{MAX_LAYOUT, MAX_PANES, PaneId, PaneInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Warning;
use crate::session::{
    Effect, MAX_RATIO, MIN_RATIO, Node, Pane, PaneState, Session, SplitAxis, Tab, TabId,
    clean_rename,
};

/// The blob format this window writes.
pub const BLOB_VERSION: u32 = 1;

/// The most nodes a tab's tree can have: `MAX_PANES` leaves and the splits between them.
const MAX_NODES: usize = 2 * MAX_PANES - 1;

#[derive(Serialize, Deserialize)]
struct Blob {
    v: u32,
    active: TabId,
    next_pane: PaneId,
    next_tab: TabId,
    /// Decoded one by one, so a corrupt tab costs only that tab.
    tabs: Vec<Value>,
    #[serde(default)]
    panes: Vec<PaneBlob>,
}

#[derive(Serialize, Deserialize)]
struct TabBlob {
    id: TabId,
    #[serde(default)]
    rename: Option<String>,
    #[serde(default)]
    focus: Vec<PaneId>,
    nodes: Vec<NodeBlob>,
}

/// A leaf has `pane`; a split has `split`, `ratio`, `a` and `b`.
#[derive(Serialize, Deserialize)]
struct NodeBlob {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pane: Option<PaneId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    split: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    a: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct PaneBlob {
    id: PaneId,
    shell: String,
    hold: bool,
}

/// What reconcile starts from.
pub enum Base<'a> {
    /// A window starting: `Hello`'s layout.
    Blob(&'a [u8]),
    /// A window that lost `kykod` and found a new one: what it shows now.
    Current(Session),
}

fn flatten(node: &Node, out: &mut Vec<NodeBlob>) {
    match node {
        Node::Leaf(p) => out.push(NodeBlob {
            pane: Some(*p),
            split: None,
            ratio: None,
            a: None,
            b: None,
        }),
        Node::Split { axis, ratio, a, b } => {
            let at = out.len();
            out.push(NodeBlob {
                pane: None,
                split: Some(
                    match axis {
                        SplitAxis::Side => "side",
                        SplitAxis::Stacked => "stacked",
                    }
                    .into(),
                ),
                ratio: Some(*ratio),
                a: None,
                b: None,
            });
            out[at].a = Some(out.len());
            flatten(a, out);
            out[at].b = Some(out.len());
            flatten(b, out);
        }
    }
}

/// The tree whose root is `nodes[0]`, if the indices form exactly one tree.
fn unflatten(nodes: &[NodeBlob]) -> Result<Node, String> {
    if nodes.is_empty() {
        return Err("no nodes".into());
    }
    if nodes.len() > MAX_NODES {
        return Err(format!("{} nodes", nodes.len()));
    }
    let mut reached = vec![false; nodes.len()];
    let root = build(nodes, 0, &mut reached)?;
    if reached.iter().any(|r| !r) {
        return Err("a node no split refers to".into());
    }
    Ok(root)
}

/// Depth is bounded by `MAX_NODES`, since each node is reached once.
fn build(nodes: &[NodeBlob], i: usize, reached: &mut [bool]) -> Result<Node, String> {
    let Some(n) = nodes.get(i) else {
        return Err(format!("index {i} out of range"));
    };
    if std::mem::replace(&mut reached[i], true) {
        return Err(format!("node {i} reached twice"));
    }
    match (n.pane, &n.split, n.ratio, n.a, n.b) {
        (Some(p), None, None, None, None) => Ok(Node::Leaf(p)),
        (None, Some(axis), Some(ratio), Some(a), Some(b)) => {
            let axis = match axis.as_str() {
                "side" => SplitAxis::Side,
                "stacked" => SplitAxis::Stacked,
                other => return Err(format!("split {other:?}")),
            };
            if !ratio.is_finite() {
                return Err("a ratio that is not finite".into());
            }
            Ok(Node::Split {
                axis,
                ratio: ratio.clamp(MIN_RATIO, MAX_RATIO),
                a: Box::new(build(nodes, a, reached)?),
                b: Box::new(build(nodes, b, reached)?),
            })
        }
        _ => Err(format!("node {i} is neither a pane nor a split")),
    }
}

/// The session a blob describes, every pane `Running`; `None` for a blob to treat as empty.
fn decode(blob: &[u8], home: &str, shell: &str, w: &mut Vec<Warning>) -> Option<Session> {
    let warn = |w: &mut Vec<Warning>, m: String| w.push(Warning::new("layout", m));
    if blob.is_empty() {
        return None;
    }
    let value: Value = match serde_json::from_slice(blob) {
        Ok(v) => v,
        Err(e) => {
            warn(w, format!("not JSON, ignored: {e}"));
            return None;
        }
    };
    match value.get("v").and_then(Value::as_u64) {
        None => {
            warn(w, "no version, ignored".into());
            return None;
        }
        Some(v) if v > u64::from(BLOB_VERSION) => warn(
            w,
            format!("version {v} is from a newer kyko; reading what version 1 knows"),
        ),
        Some(_) => {}
    }
    let blob: Blob = match serde_json::from_value(value) {
        Ok(b) => b,
        Err(e) => {
            warn(w, format!("corrupt, ignored: {e}"));
            return None;
        }
    };
    let meta: BTreeMap<PaneId, &PaneBlob> = blob.panes.iter().map(|p| (p.id, p)).collect();
    let mut s = Session::empty(home);
    let mut seen_tabs = BTreeSet::new();
    for (i, t) in blob.tabs.into_iter().enumerate() {
        let t: TabBlob = match serde_json::from_value(t) {
            Ok(t) => t,
            Err(e) => {
                warn(w, format!("tab {i} is corrupt, dropped: {e}"));
                continue;
            }
        };
        if !seen_tabs.insert(t.id) {
            warn(w, format!("tab id {} twice; the second dropped", t.id));
            continue;
        }
        let root = match unflatten(&t.nodes) {
            Ok(r) => r,
            Err(e) => {
                warn(w, format!("tab {} is corrupt, dropped: {e}", t.id));
                continue;
            }
        };
        // A pane named twice, here or in an earlier tab: the later leaf goes.
        let named = root.pane_ids().len();
        let mut seen: BTreeSet<PaneId> = s.panes.keys().copied().collect();
        let Some(root) = drop_repeats(root, &mut seen) else {
            warn(
                w,
                format!("tab {} repeats panes of earlier tabs, dropped", t.id),
            );
            continue;
        };
        let leaves = root.pane_ids();
        if leaves.len() < named {
            warn(
                w,
                format!(
                    "tab {} names {} pane(s) twice; dropped",
                    t.id,
                    named - leaves.len()
                ),
            );
        }
        for &p in &leaves {
            let pane = match meta.get(&p) {
                Some(m) => Pane::new(&m.shell, m.hold),
                None => Pane::new(shell, false),
            };
            s.panes.insert(p, pane);
        }
        let mut focus: Vec<PaneId> = Vec::new();
        for p in t.focus.into_iter().chain(leaves.iter().copied()) {
            if leaves.contains(&p) && !focus.contains(&p) {
                focus.push(p);
            }
        }
        s.tabs.push(Tab {
            id: t.id,
            rename: t.rename.as_deref().and_then(clean_rename),
            root,
            focus,
            zoomed: false,
            broadcast: false,
        });
    }
    s.active = blob.active;
    // The counters: above every id the blob has, with an id still left to hand out.
    let max_pane = s.panes.keys().max().copied();
    let max_tab = s.tabs.iter().map(|t| t.id).max();
    let next = |stored: u32, max: Option<u32>| -> Option<u32> {
        let floor = match max {
            Some(m) => m.checked_add(1)?,
            None => 1,
        };
        Some(stored.max(floor)).filter(|n| *n < u32::MAX)
    };
    match (next(blob.next_pane, max_pane), next(blob.next_tab, max_tab)) {
        (Some(p), Some(t)) => {
            s.next_pane = p;
            s.next_tab = t;
        }
        _ => {
            warn(w, "its ids have run out, ignored".into());
            return None;
        }
    }
    Some(s)
}

/// The tree without the leaves whose pane is already in `seen`, adding the rest to it.
fn drop_repeats(node: Node, seen: &mut BTreeSet<PaneId>) -> Option<Node> {
    match node {
        Node::Leaf(p) => seen.insert(p).then_some(Node::Leaf(p)),
        Node::Split { axis, ratio, a, b } => {
            match (drop_repeats(*a, seen), drop_repeats(*b, seen)) {
                (Some(a), Some(b)) => Some(Node::Split {
                    axis,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            }
        }
    }
}

impl Session {
    /// The blob for `SaveLayout`, always at most `MAX_LAYOUT` bytes.
    pub fn to_blob(&self) -> Vec<u8> {
        let tabs = self
            .tabs
            .iter()
            .map(|t| {
                let mut nodes = Vec::new();
                flatten(&t.root, &mut nodes);
                serde_json::to_value(TabBlob {
                    id: t.id,
                    rename: t.rename.clone(),
                    focus: t.focus.clone(),
                    nodes,
                })
                .unwrap_or(Value::Null)
            })
            .collect();
        let panes = self
            .panes
            .iter()
            .map(|(id, p)| PaneBlob {
                id: *id,
                shell: p.shell.clone(),
                hold: p.hold,
            })
            .collect();
        let blob = Blob {
            v: BLOB_VERSION,
            active: self.active,
            next_pane: self.next_pane,
            next_tab: self.next_tab,
            tabs,
            panes,
        };
        let out = serde_json::to_vec(&blob).unwrap_or_default();
        debug_assert!(
            out.len() <= MAX_LAYOUT,
            "layout blob of {} bytes",
            out.len()
        );
        out
    }

    /// Rebuilds the session from `base` and `Hello`'s panes. Never fails: with no pane left it
    /// is a fresh session, and the effects hold its `Spawn`.
    pub fn reconcile(
        base: Base<'_>,
        panes: &[PaneInfo],
        home: &str,
        shell: &str,
    ) -> (Session, Vec<Effect>, Vec<Warning>) {
        let mut warnings = Vec::new();
        let (mut s, keep_missing) = match base {
            Base::Blob(blob) => (
                decode(blob, home, shell, &mut warnings).unwrap_or_else(|| Session::empty(home)),
                false,
            ),
            Base::Current(s) => (s, true),
        };
        let mut effects = Vec::new();
        let live: BTreeMap<PaneId, &PaneInfo> = panes.iter().map(|p| (p.pane, p)).collect();

        // The panes the session knows.
        let known: BTreeSet<PaneId> = s.pane_ids().into_iter().collect();
        for &id in &known {
            match live.get(&id) {
                None if keep_missing => {
                    if let Some(p) = s.panes.get_mut(&id)
                        && p.state == PaneState::Running
                    {
                        p.state = PaneState::Ended;
                    }
                }
                None => s.remove_quietly(id),
                Some(info) => {
                    if let Some(status) = info.exited {
                        effects.push(Effect::Close { pane: id });
                        let held = s.panes.get(&id).is_some_and(|p| p.hold);
                        if held {
                            if let Some(p) = s.panes.get_mut(&id) {
                                p.state = PaneState::Exited(status);
                            }
                        } else {
                            s.remove_quietly(id);
                            continue;
                        }
                    } else if let Some(p) = s.panes.get_mut(&id) {
                        p.state = PaneState::Running;
                    }
                    s.apply_info(id, &info.program, &info.cwd);
                }
            }
        }

        // The panes only `kykod` knows: each live one gets a tab at the end, in id order.
        for (&id, info) in &live {
            if known.contains(&id) {
                continue;
            }
            if info.exited.is_some() {
                effects.push(Effect::Close { pane: id });
                continue;
            }
            if id == u32::MAX || s.panes.len() >= MAX_PANES || s.next_tab == u32::MAX {
                // No id above it to hand out next, or no room: it cannot be shown.
                warnings.push(Warning::new(
                    "layout",
                    format!("pane {id} from kykod cannot be shown; closed"),
                ));
                effects.push(Effect::Close { pane: id });
                continue;
            }
            let tab = s.next_tab;
            s.next_tab += 1;
            s.panes.insert(id, Pane::new(shell, false));
            s.tabs.push(Tab {
                id: tab,
                rename: None,
                root: Node::Leaf(id),
                focus: vec![id],
                zoomed: false,
                broadcast: false,
            });
            s.apply_info(id, &info.program, &info.cwd);
        }

        // The pane counter goes above every id `kykod` has.
        if let Some(max) = live.keys().filter(|id| **id != u32::MAX).max() {
            s.next_pane = s.next_pane.max(max + 1);
        }

        if s.tab_ids().is_empty() {
            if s.next_pane == u32::MAX || s.next_tab == u32::MAX {
                // Only after billions of ids; start the counters again rather than show nothing.
                s.next_pane = live
                    .keys()
                    .filter(|id| **id != u32::MAX)
                    .max()
                    .map_or(1, |m| m + 1);
                s.next_tab = 1;
            }
            effects.extend(s.push_fresh_tab(shell).unwrap_or_default());
        } else if !s.tab_ids().contains(&s.active) {
            s.active = s.tab_ids()[0];
        }
        (s, effects, warnings)
    }
}

#[cfg(test)]
mod tests {
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
        let b = blob(
            r#"{"id":1,"nodes":[{"split":"side","ratio":7,"a":1,"b":2},{"pane":1},{"pane":2}]}"#,
        );
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
        let b = br#"{"v":1,"active":7,"next_pane":1,"next_tab":1,"tabs":[{"id":7,"nodes":[{"pane":5}]}]}"#;
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
}
