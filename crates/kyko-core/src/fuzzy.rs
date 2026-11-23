//! Fuzzy matching for the command palette: the query's characters in order, anywhere in the
//! text, ignoring case.

/// How well `text` matches `query`, higher is better; `None` when it does not. An empty query
/// matches everything equally. A match at the start of the text or of a word, and characters
/// that follow each other, score more; a shorter text breaks ties.
pub fn score(query: &str, text: &str) -> Option<i64> {
    let q: Vec<char> = query.trim().to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut qi = 0;
    let mut last: Option<usize> = None;
    for (i, &c) in t.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        if c != q[qi] {
            continue;
        }
        let word_start = i == 0 || !t[i - 1].is_alphanumeric();
        score += 1;
        if word_start {
            score += 8;
        }
        if i == 0 {
            score += 4;
        }
        if last.is_some_and(|l| l + 1 == i) {
            score += 10;
        }
        last = Some(i);
        qi += 1;
    }
    (qi == q.len()).then(|| score * 100 - t.len() as i64)
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn in_order_anywhere_ignoring_case() {
        assert!(score("cpo", "Copy last output").is_some());
        assert!(score("opc", "Copy last output").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn word_starts_and_runs_rank_higher() {
        let run = score("copy", "Copy last output").unwrap();
        let scattered = score("copy", "Close pane or yank").unwrap();
        assert!(run > scattered);
        let prefix = score("new", "New tab").unwrap();
        let inside = score("new", "Renew window").unwrap();
        assert!(prefix > inside);
    }
}
