//! The split tree's geometry: each pane's bounds in a tab, the dividers between them, and the
//! edits that remove or replace a leaf.

use super::*;

/// Edges as coordinates, not sizes: a dividing line is computed once and given to both sides,
/// so two panes that share an edge have bitwise-equal coordinates there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub(crate) x0: f64,
    pub(crate) y0: f64,
    pub(crate) x1: f64,
    pub(crate) y1: f64,
}

pub(crate) const UNIT: Bounds = Bounds {
    x0: 0.0,
    y0: 0.0,
    x1: 1.0,
    y1: 1.0,
};

impl Bounds {
    pub(crate) fn rect(self) -> Rect {
        Rect {
            x: self.x0,
            y: self.y0,
            w: self.x1 - self.x0,
            h: self.y1 - self.y0,
        }
    }

    pub(crate) fn split(self, axis: SplitAxis, ratio: f64) -> (Bounds, Bounds) {
        match axis {
            SplitAxis::Side => {
                let mid = self.x0 + (self.x1 - self.x0) * ratio;
                (Bounds { x1: mid, ..self }, Bounds { x0: mid, ..self })
            }
            SplitAxis::Stacked => {
                let mid = self.y0 + (self.y1 - self.y0) * ratio;
                (Bounds { y1: mid, ..self }, Bounds { y0: mid, ..self })
            }
        }
    }
}

impl Node {
    pub(crate) fn leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(p) => out.push(*p),
            Node::Split { a, b, .. } => {
                a.leaves(out);
                b.leaves(out);
            }
        }
    }

    pub(crate) fn pane_ids(&self) -> Vec<PaneId> {
        let mut v = Vec::new();
        self.leaves(&mut v);
        v
    }

    pub(crate) fn contains(&self, pane: PaneId) -> bool {
        match self {
            Node::Leaf(p) => *p == pane,
            Node::Split { a, b, .. } => a.contains(pane) || b.contains(pane),
        }
    }

    pub(crate) fn bounds(&self, at: Bounds, out: &mut Vec<(PaneId, Bounds)>) {
        match self {
            Node::Leaf(p) => out.push((*p, at)),
            Node::Split { axis, ratio, a, b } => {
                let (ba, bb) = at.split(*axis, *ratio);
                a.bounds(ba, out);
                b.bounds(bb, out);
            }
        }
    }

    pub(crate) fn dividers(&self, at: Bounds, path: &mut Vec<bool>, out: &mut Vec<DividerView>) {
        if let Node::Split { axis, ratio, a, b } = self {
            out.push(DividerView {
                path: path.clone(),
                axis: *axis,
                rect: at.rect(),
                ratio: *ratio,
            });
            let (ba, bb) = at.split(*axis, *ratio);
            path.push(false);
            a.dividers(ba, path, out);
            path.pop();
            path.push(true);
            b.dividers(bb, path, out);
            path.pop();
        }
    }

    /// The tree without `pane`: its sibling takes the parent's place. `None` when the tree was
    /// that one leaf.
    pub(crate) fn without(self, pane: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(p) if p == pane => None,
            leaf @ Node::Leaf(_) => Some(leaf),
            Node::Split { axis, ratio, a, b } => match (a.without(pane), b.without(pane)) {
                (Some(a), Some(b)) => Some(Node::Split {
                    axis,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    /// Replaces the leaf `pane` with `with`. False when `pane` is not here.
    pub(crate) fn replace_leaf(&mut self, pane: PaneId, with: Node) -> bool {
        match self {
            Node::Leaf(p) if *p == pane => {
                *self = with;
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => {
                if a.contains(pane) {
                    a.replace_leaf(pane, with)
                } else {
                    b.replace_leaf(pane, with)
                }
            }
        }
    }

    pub(crate) fn at_path(&mut self, path: &[bool]) -> Option<&mut Node> {
        match (path.split_first(), self) {
            (None, node) => Some(node),
            (Some((side, rest)), Node::Split { a, b, .. }) => {
                if *side { b } else { a }.at_path(rest)
            }
            (Some(_), Node::Leaf(_)) => None,
        }
    }

    pub(crate) fn ratios_ok(&self) -> bool {
        match self {
            Node::Leaf(_) => true,
            Node::Split { ratio, a, b, .. } => {
                ratio.is_finite()
                    && (MIN_RATIO..=MAX_RATIO).contains(ratio)
                    && a.ratios_ok()
                    && b.ratios_ok()
            }
        }
    }
}

/// The overlap of two intervals; 0 or less when they do not overlap.
pub(crate) fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    a1.min(b1) - a0.max(b0)
}
