//! [`Session::check_invariants`]: what the random session tests hold every session to.

use super::*;

impl Session {
    #[doc(hidden)]
    pub fn check_invariants(&self) -> Result<(), String> {
        if self.tabs.is_empty() {
            return Err("no tab".into());
        }
        if self.tab_index(self.active).is_none() {
            return Err(format!("active tab {} does not exist", self.active));
        }
        if self.panes.len() > MAX_PANES {
            return Err(format!("{} panes", self.panes.len()));
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in &self.tabs {
            if t.id >= self.next_tab {
                return Err(format!("tab {} is not below next {}", t.id, self.next_tab));
            }
            let leaves = t.root.pane_ids();
            for p in &leaves {
                if !seen.insert(*p) {
                    return Err(format!("pane {p} in two leaves"));
                }
                if !self.panes.contains_key(p) {
                    return Err(format!("pane {p} has a leaf and no state"));
                }
                if *p >= self.next_pane {
                    return Err(format!("pane {p} is not below next {}", self.next_pane));
                }
            }
            let mut a = leaves.clone();
            let mut b = t.focus.clone();
            a.sort_unstable();
            b.sort_unstable();
            if a != b {
                return Err(format!(
                    "tab {}: focus {:?}, leaves {:?}",
                    t.id, t.focus, leaves
                ));
            }
            if !t.root.ratios_ok() {
                return Err(format!("tab {}: a ratio out of bounds", t.id));
            }
            let mut bounds = Vec::new();
            t.root.bounds(UNIT, &mut bounds);
            let area: f64 = bounds
                .iter()
                .map(|(_, b)| (b.x1 - b.x0) * (b.y1 - b.y0))
                .sum();
            if (area - 1.0).abs() > 1e-9 {
                return Err(format!("tab {}: panes cover {area}", t.id));
            }
            for (i, (p, x)) in bounds.iter().enumerate() {
                for (q, y) in &bounds[i + 1..] {
                    if overlap(x.x0, x.x1, y.x0, y.x1) > 0.0
                        && overlap(x.y0, x.y1, y.y0, y.y1) > 0.0
                    {
                        return Err(format!("tab {}: panes {p} and {q} overlap", t.id));
                    }
                }
            }
            if let Some(r) = &t.rename
                && (r.is_empty() || r.len() > MAX_RENAME || r.chars().any(char::is_control))
            {
                return Err(format!("tab {}: rename {r:?}", t.id));
            }
        }
        if seen.len() != self.panes.len() {
            return Err(format!(
                "{} panes have state, {} have leaves",
                self.panes.len(),
                seen.len()
            ));
        }
        Ok(())
    }
}
