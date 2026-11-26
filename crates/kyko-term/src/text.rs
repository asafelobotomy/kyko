//! Scrolling, selection, the screen as text, and searching the scrollback.

use super::*;

impl Emulator {
    /// Moves the view `lines` up into the history (down when negative).
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// Back to the live screen.
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// How many lines the view is above the live screen.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub(crate) fn point(&self, col: u16, row: u16) -> Point {
        let col = usize::from(col).min(self.term.columns().saturating_sub(1));
        let row = i32::from(row).min(self.term.screen_lines() as i32 - 1);
        Point::new(Line(row - self.display_offset() as i32), Column(col))
    }

    /// Starts a selection at a cell of the view; `right` is the cell's right half.
    pub fn select_start(&mut self, col: u16, row: u16, kind: SelectKind, right: bool) {
        let ty = match kind {
            SelectKind::Simple => SelectionType::Simple,
            SelectKind::Word => SelectionType::Semantic,
            SelectKind::Line => SelectionType::Lines,
            SelectKind::Block => SelectionType::Block,
        };
        let side = if right { Side::Right } else { Side::Left };
        self.term.selection = Some(Selection::new(ty, self.point(col, row), side));
    }

    /// Extends the selection to a cell of the view.
    pub fn select_update(&mut self, col: u16, row: u16, right: bool) {
        let side = if right { Side::Right } else { Side::Left };
        let point = self.point(col, row);
        if let Some(sel) = self.term.selection.as_mut() {
            sel.update(point, side);
        }
    }

    pub fn select_clear(&mut self) {
        self.term.selection = None;
    }

    /// Everything in the history and on the screen as text, one line per row, oldest first,
    /// with trailing blanks taken off each line and blank lines at the end dropped.
    pub fn all_text(&self) -> String {
        self.text_from(-(self.term.grid().history_size() as i32))
    }

    /// The live screen's text, as [`Emulator::all_text`] gives it.
    pub fn screen_text(&self) -> String {
        self.text_from(0)
    }

    pub(crate) fn text_from(&self, top: i32) -> String {
        let grid = self.term.grid();
        let bottom = grid.screen_lines() as i32 - 1;
        let cols = grid.columns();
        let mut out = String::new();
        for line in top..=bottom {
            let row = &grid[Line(line)];
            let mut text: String = (0..cols)
                .filter(|&c| !row[Column(c)].flags.contains(Flags::WIDE_CHAR_SPACER))
                .map(|c| row[Column(c)].c)
                .collect();
            let trimmed = text.trim_end().len();
            text.truncate(trimmed);
            out.push_str(&text);
            out.push('\n');
        }
        let keep = out.trim_end_matches('\n').len();
        out.truncate(keep);
        out.push('\n');
        out
    }

    /// The selected text, `None` when nothing (or only an empty range) is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|t| !t.is_empty())
    }

    /// Finds `query` in the history and the screen, one line at a time (a match does not cross a
    /// wrapped line). Smart case: a query with a capital letter is case-sensitive. Selects the
    /// match nearest the live screen and brings it into view; returns how many there are.
    pub fn search_find(&mut self, query: &str) -> usize {
        self.matches.clear();
        self.current = 0;
        if query.is_empty() {
            self.select_clear();
            return 0;
        }
        let sensitive = query.chars().any(char::is_uppercase);
        let fold = |c: char| {
            if sensitive {
                c
            } else {
                c.to_lowercase().next().unwrap_or(c)
            }
        };
        let needle: Vec<char> = query.chars().map(fold).collect();
        let grid = self.term.grid();
        let (cols, top, bottom) = (
            self.term.columns(),
            grid.topmost_line(),
            grid.bottommost_line(),
        );
        let mut line = top;
        while line <= bottom {
            let row = &grid[line];
            let mut text = Vec::new();
            let mut at = Vec::new();
            for c in 0..cols {
                let cell = &row[Column(c)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                text.push(fold(if cell.c == '\0' { ' ' } else { cell.c }));
                at.push(c);
            }
            let mut i = 0;
            while i + needle.len() <= text.len() {
                if text[i..i + needle.len()] == needle[..] {
                    self.matches.push((line.0, at[i], at[i + needle.len() - 1]));
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
            line += 1;
        }
        self.current = self.matches.len().saturating_sub(1);
        self.show_match();
        self.matches.len()
    }

    /// Moves to the next (or previous) match, wrapping round; `(position, total)`, from 0.
    pub fn search_step(&mut self, forward: bool) -> Option<(usize, usize)> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
        self.show_match();
        Some((self.current, n))
    }

    /// Where the search stands: `(position, total)`, from 0.
    pub fn search_state(&self) -> Option<(usize, usize)> {
        (!self.matches.is_empty()).then_some((self.current, self.matches.len()))
    }

    /// Ends the search; the match is no longer selected.
    pub fn search_clear(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.select_clear();
    }

    /// Selects the current match and scrolls so that it is in view.
    pub(crate) fn show_match(&mut self) {
        let Some(&(line, from, to)) = self.matches.get(self.current) else {
            return;
        };
        let rows = self.term.screen_lines() as i32;
        let offset = self.display_offset() as i32;
        // The lines in view are `-offset .. rows - offset`.
        if line < -offset || line >= rows - offset {
            let want = (rows / 2 - line).clamp(0, self.history_len() as i32);
            self.term.scroll_display(Scroll::Delta(want - offset));
        }
        let mut sel = Selection::new(
            SelectionType::Simple,
            Point::new(Line(line), Column(from)),
            Side::Left,
        );
        sel.update(Point::new(Line(line), Column(to)), Side::Right);
        self.term.selection = Some(sel);
    }
}
