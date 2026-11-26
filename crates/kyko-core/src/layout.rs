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
mod tests;
