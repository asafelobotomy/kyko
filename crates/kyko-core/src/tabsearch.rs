//! Finding open tabs for uQ's Spotlight (`org.uq.Launcher1.Provider`): by name or program.

/// The most rows one search returns.
pub const MAX_HITS: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabInfo {
    pub id: u32,
    pub name: String,
    /// The program in the tab's focused pane.
    pub program: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: u32,
    pub title: String,
    pub subtitle: String,
    pub score: f64,
}

/// Tabs whose name or program contains `query` (ignoring case), best first: an exact match, then
/// a prefix, then anywhere; the name before the program. At most [`MAX_HITS`]. An empty or
/// blank query matches nothing.
pub fn search(tabs: &[TabInfo], query: &str) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = tabs
        .iter()
        .filter_map(|t| {
            let score = rank(&t.name, &q).max(rank(&t.program, &q) * 0.8);
            (score > 0.0).then(|| Hit {
                id: t.id,
                title: if t.name.is_empty() {
                    t.program.clone()
                } else {
                    t.name.clone()
                },
                subtitle: if t.program.is_empty() || t.name.is_empty() || t.program == t.name {
                    "kyko tab".to_string()
                } else {
                    format!("kyko tab: {}", t.program)
                },
                score,
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
    hits.truncate(MAX_HITS);
    hits
}

fn rank(text: &str, q: &str) -> f64 {
    let t = text.to_lowercase();
    if t == q {
        1.0
    } else if t.starts_with(q) {
        0.8
    } else if t.contains(q) {
        0.5
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u32, name: &str, program: &str) -> TabInfo {
        TabInfo {
            id,
            name: name.into(),
            program: program.into(),
        }
    }

    #[test]
    fn matches_name_or_program_ignoring_case() {
        let tabs = [tab(1, "build", "cargo"), tab(2, "notes", "nvim")];
        assert_eq!(search(&tabs, "BUI")[0].id, 1);
        assert_eq!(search(&tabs, "nvim")[0].id, 2);
        assert!(search(&tabs, "zzz").is_empty());
    }

    #[test]
    fn exact_then_prefix_then_inside_and_name_before_program() {
        let tabs = [
            tab(1, "my vim", "sh"),
            tab(2, "vim", "sh"),
            tab(3, "vimrc", "sh"),
            tab(4, "other", "vim"),
        ];
        let ids: Vec<u32> = search(&tabs, "vim").iter().map(|h| h.id).collect();
        assert_eq!(ids, [2, 3, 4, 1]);
    }

    #[test]
    fn a_blank_query_matches_nothing() {
        assert!(search(&[tab(1, "a", "b")], "  ").is_empty());
    }

    #[test]
    fn at_most_twenty() {
        let tabs: Vec<_> = (0..30).map(|i| tab(i, "x", "sh")).collect();
        assert_eq!(search(&tabs, "x").len(), MAX_HITS);
    }

    #[test]
    fn an_unnamed_tab_shows_its_program() {
        let h = &search(&[tab(1, "", "htop")], "htop")[0];
        assert_eq!(h.title, "htop");
        assert_eq!(h.subtitle, "kyko tab");
    }
}
