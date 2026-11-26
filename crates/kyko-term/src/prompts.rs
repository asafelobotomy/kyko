//! Prompt marks: rows the shell marked with OSC 133, and what the window does with them (the
//! gutter, jumping between prompts, selecting a command's output, the sticky header).

use super::*;

/// Prompt marks (see [`marks`]): what the shell's OSC 133 sequences let the window do.
impl Emulator {
    /// The number of the row at screen line `line` (negative lines are the history): it stays the
    /// same line's number as the screen scrolls, and counts up from the oldest line ever lost.
    pub(crate) fn row_id(&self, line: i32) -> u64 {
        (self.gone as i64 + self.history_len() as i64 + i64::from(line)).max(0) as u64
    }

    /// The screen line of row number `id`.
    pub(crate) fn line_of(&self, id: u64) -> i32 {
        (id as i64 - self.gone as i64 - self.history_len() as i64) as i32
    }

    pub(crate) fn in_alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Records a mark the shell sent, on the row the cursor is on.
    pub(crate) fn mark(&mut self, event: marks::Event) {
        if self.in_alt_screen() {
            // A full-screen program has the screen; the shell's marks are for the main one.
            return;
        }
        let cursor = self.term.grid().cursor.point;
        let id = self.row_id(cursor.line.0);
        match event {
            marks::Event::Prompt { click_events } => {
                let row = self.marks.entry(id).or_default();
                row.prompt = true;
                row.click_events |= click_events;
                self.input = None;
            }
            marks::Event::Input => self.input = Some((id, cursor.column.0)),
            marks::Event::Output { command } => {
                self.input = None;
                self.marks.entry(id).or_default().output = true;
                // A shell that marks twice (fish 4 and kyko's script) sends C twice for one
                // command: the first start, and any command line, stand.
                if self.started.is_none() {
                    self.started = Some(std::time::Instant::now());
                }
                if let Some(c) = command {
                    if let Some(p) = self.prompt_before(id + 1) {
                        self.commands.entry(p).or_insert_with(|| c.clone());
                    }
                    self.running.get_or_insert(c);
                }
            }
            marks::Event::Notify(..)
            | marks::Event::Graphics(_)
            | marks::Event::CellSizeRequest
            | marks::Event::Sixel(_)
            | marks::Event::Termcap(_)
            | marks::Event::VersionRequest => unreachable!("handled in feed"),
            marks::Event::Done(code) => {
                let command = self.running.take();
                if let Some(t) = self.started.take() {
                    self.finished.push(Finished {
                        ran: t.elapsed(),
                        status: code.unwrap_or(0),
                        command,
                    });
                }
                self.marks.entry(id).or_default().end = Some(cursor.column.0 > 0);
                // The status belongs to the prompt the command began at: the nearest above.
                let prompt = self
                    .marks
                    .range(..=id)
                    .rev()
                    .find(|(_, m)| m.prompt)
                    .map(|(&p, _)| p);
                if let Some(p) = prompt {
                    let row = self.marks.entry(p).or_default();
                    row.status.get_or_insert(code.unwrap_or(0));
                }
            }
        }
        self.forget_lost_marks();
    }

    /// Marks on lines that have scrolled out of the history are of no use.
    pub(crate) fn forget_lost_marks(&mut self) {
        let keep = self.gone;
        self.marks = self.marks.split_off(&keep);
        self.commands = self.commands.split_off(&keep);
    }

    /// The newest command line the shell sent.
    pub fn last_command(&self) -> Option<&str> {
        self.commands.values().next_back().map(String::as_str)
    }

    /// The command line run at the prompt on screen line `line` (or the nearest prompt above
    /// it), when the shell sent one.
    pub fn command_at(&self, line: i32) -> Option<&str> {
        let p = self.prompt_before(self.row_id(line) + 1)?;
        self.commands.get(&p).map(String::as_str)
    }

    /// The nearest prompt row number before `id`.
    pub(crate) fn prompt_before(&self, id: u64) -> Option<u64> {
        self.marks
            .range(..id)
            .rev()
            .find(|(_, m)| m.prompt)
            .map(|(&p, _)| p)
    }

    /// The nearest prompt row number after `id`.
    pub(crate) fn prompt_after(&self, id: u64) -> Option<u64> {
        self.marks
            .range(id + 1..)
            .find(|(_, m)| m.prompt)
            .map(|(&p, _)| p)
    }

    /// What the gutter shows for the command whose prompt is on row `prompt`.
    pub(crate) fn gutter_of(&self, prompt: u64) -> u8 {
        let row = self.marks.get(&prompt);
        match row.and_then(|r| r.status) {
            Some(0) => GUTTER_OK,
            Some(_) => GUTTER_FAILED,
            // No status yet: running if its output began and no later prompt came.
            None if self.prompt_after(prompt).is_none()
                && self.marks.range(prompt + 1..).any(|(_, m)| m.output) =>
            {
                GUTTER_RUNNING
            }
            None => GUTTER_NONE,
        }
    }

    /// The gutter of every row in view.
    pub(crate) fn gutter(&self) -> Vec<u8> {
        let rows = self.term.screen_lines();
        let mut out = vec![GUTTER_NONE; rows];
        if self.marks.is_empty() || self.in_alt_screen() {
            return out;
        }
        let first = -(self.display_offset() as i32);
        let mut code = self
            .prompt_before(self.row_id(first))
            .map_or(GUTTER_NONE, |p| self.gutter_of(p));
        for (i, slot) in out.iter_mut().enumerate() {
            let id = self.row_id(first + i as i32);
            if self.marks.get(&id).is_some_and(|m| m.prompt) {
                code = self.gutter_of(id);
            }
            *slot = code;
        }
        out
    }

    /// Scrolls to the previous (`dir < 0`) or next (`dir > 0`) prompt: the one before or after the
    /// line at the top of the view. False when there is none that way.
    pub fn jump_prompt(&mut self, dir: i32) -> bool {
        if self.in_alt_screen() {
            return false;
        }
        let top = self.row_id(-(self.display_offset() as i32));
        let target = if dir < 0 {
            self.prompt_before(top)
        } else {
            // The prompt at the top of the view is where the view already is.
            self.prompt_after(top)
        };
        let Some(id) = target else { return false };
        let want = (-self.line_of(id)).clamp(0, self.history_len() as i32);
        let now = self.display_offset() as i32;
        if want == now {
            // Already as far as the view goes: the prompt is on the live screen.
            return false;
        }
        self.term.scroll_display(Scroll::Delta(want - now));
        true
    }

    /// The view row of row number `id`.
    pub(crate) fn view_row(&self, id: u64) -> i32 {
        self.line_of(id) + self.display_offset() as i32
    }

    /// The row numbers a command's output covers: from its output mark to its end, for the
    /// command whose prompt is on row `prompt`. `None` when it printed nothing or is lost.
    pub(crate) fn output_rows(&self, prompt: u64) -> Option<(u64, u64)> {
        let next = self.prompt_after(prompt);
        let within = |id: u64| next.is_none_or(|n| id < n);
        let begin = self
            .marks
            .range(prompt..)
            .take_while(|(id, _)| within(**id))
            .find(|(_, m)| m.output)
            .map(|(&b, _)| b)?;
        let end = self
            .marks
            .range(begin..)
            .take_while(|(id, _)| within(**id))
            .find_map(|(&id, m)| m.end.map(|inc| if inc { id } else { id.saturating_sub(1) }));
        // Still running: its output so far, to the cursor.
        let last = match end {
            Some(e) => e,
            None if next.is_none() => self.row_id(self.term.grid().cursor.point.line.0),
            None => next?.saturating_sub(1),
        };
        (last >= begin).then_some((begin, last))
    }

    /// The command at view row `row`: the one whose prompt is on it or the nearest above.
    pub fn block_at(&self, row: u16) -> Option<Block> {
        if self.in_alt_screen() {
            return None;
        }
        let id = self.row_id(i32::from(row) - self.display_offset() as i32);
        let p = self.prompt_before(id + 1)?;
        let m = self.marks.get(&p).copied().unwrap_or_default();
        Some(Block {
            prompt: self.view_row(p),
            output: self
                .output_rows(p)
                .map(|(a, b)| (self.view_row(a), self.view_row(b))),
            status: m.status,
            command: self.commands.get(&p).cloned(),
        })
    }

    /// Selects the output of the command at view row `row`. False when it printed nothing.
    pub fn select_output_at(&mut self, row: u16) -> bool {
        if self.in_alt_screen() {
            return false;
        }
        let id = self.row_id(i32::from(row) - self.display_offset() as i32);
        let Some((begin, last)) = self.prompt_before(id + 1).and_then(|p| self.output_rows(p))
        else {
            return false;
        };
        self.select_rows(begin, last);
        true
    }

    pub(crate) fn select_rows(&mut self, begin: u64, last: u64) {
        let cols = self.term.columns();
        let mut sel = Selection::new(
            SelectionType::Lines,
            Point::new(Line(self.line_of(begin)), Column(0)),
            Side::Left,
        );
        sel.update(
            Point::new(Line(self.line_of(last)), Column(cols.saturating_sub(1))),
            Side::Right,
        );
        self.term.selection = Some(sel);
    }

    /// Scrolls to the previous (`dir < 0`) or next failed command's prompt. False when none.
    pub fn jump_failed(&mut self, dir: i32) -> bool {
        if self.in_alt_screen() {
            return false;
        }
        let top = self.row_id(-(self.display_offset() as i32));
        let failed = |m: &marks::Row| m.prompt && m.status.is_some_and(|s| s != 0);
        let target = if dir < 0 {
            self.marks.range(..top).rev().find(|(_, m)| failed(m))
        } else {
            self.marks.range(top + 1..).find(|(_, m)| failed(m))
        }
        .map(|(&id, _)| id);
        let Some(id) = target else { return false };
        let want = (-self.line_of(id)).clamp(0, self.history_len() as i32);
        let now = self.display_offset() as i32;
        if want == now {
            return false;
        }
        self.term.scroll_display(Scroll::Delta(want - now));
        true
    }

    /// What to pin at the top of the view while it shows the middle of a command's output whose
    /// prompt has scrolled above it: the command line, else the prompt row's text.
    pub fn sticky_header(&self) -> Option<String> {
        if self.in_alt_screen() || self.marks.is_empty() {
            return None;
        }
        let top = self.row_id(-(self.display_offset() as i32));
        let p = self.prompt_before(top + 1)?;
        if p >= top {
            return None;
        }
        let (begin, last) = self.output_rows(p)?;
        if top < begin || top > last {
            return None;
        }
        if let Some(c) = self.commands.get(&p) {
            return Some(c.clone());
        }
        let grid = self.term.grid();
        let row = &grid[Line(self.line_of(p))];
        let text: String = (0..grid.columns())
            .filter(|&c| !row[Column(c)].flags.contains(Flags::WIDE_CHAR_SPACER))
            .map(|c| row[Column(c)].c)
            .collect();
        let text = text.trim().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// What to send the shell so its cursor goes to a click at view `col`, `row` on the command
    /// line being typed: an SGR mouse click when the prompt asked for clicks (`click_events=1`),
    /// else Left or Right arrows. `None` when the click is not on that command line (a command
    /// runs, the view is scrolled back, a full-screen program has the screen).
    pub fn click_to_cursor(&self, col: u16, row: u16) -> Option<Vec<u8>> {
        if self.in_alt_screen() || self.display_offset() != 0 {
            return None;
        }
        let (start_id, start_col) = self.input?;
        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        let cols = grid.columns();
        let col = usize::from(col).min(cols - 1);
        let line = i32::from(row);
        let click_id = self.row_id(line);
        // The command line runs from its start to the last row with text at or below the cursor.
        let screen_last = grid.screen_lines() as i32 - 1;
        let blank = |l: i32| {
            let r = &grid[Line(l)];
            (0..cols).all(|c| r[Column(c)].c == ' ')
        };
        let mut last_line = cursor.line.0;
        while last_line < screen_last && !blank(last_line + 1) {
            last_line += 1;
        }
        let last_id = self.row_id(last_line);
        if click_id < start_id || click_id > last_id || (click_id == start_id && col < start_col) {
            return None;
        }
        let p = self.prompt_before(start_id + 1)?;
        if self.marks.get(&p).is_some_and(|m| m.click_events) {
            let (x, y) = (col + 1, line + 1);
            return Some(format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").into_bytes());
        }
        // A click past the end of a row's text goes to the end of the text, for the last row.
        let end_of_text = |l: i32| {
            let r = &grid[Line(l)];
            (0..cols)
                .rev()
                .find(|&c| r[Column(c)].c != ' ')
                .map_or(0, |c| c + 1)
        };
        let mut target = (line, col);
        if line == last_line {
            let end = end_of_text(line).max(if line == cursor.line.0 {
                cursor.column.0
            } else {
                0
            });
            target.1 = target.1.min(end);
        }
        let here = (cursor.line.0, cursor.column.0);
        if target == here {
            return Some(Vec::new());
        }
        let (from, to, right) = if target > here {
            (here, target, true)
        } else {
            (target, here, false)
        };
        // One arrow per character between them; the second half of a wide one is no character.
        let mut n = 0usize;
        let (mut l, mut c) = from;
        while (l, c) < to {
            if !grid[Line(l)][Column(c)]
                .flags
                .contains(Flags::WIDE_CHAR_SPACER)
            {
                n += 1;
            }
            c += 1;
            if c == cols {
                c = 0;
                l += 1;
            }
        }
        let app = self.term.mode().contains(TermMode::APP_CURSOR);
        let arrow = keys::arrow(right, app);
        Some(arrow.repeat(n))
    }

    /// Selects the output of the last command that finished, from the line its output began on to
    /// the line it ended on. False when there is none, or it printed nothing.
    pub fn select_last_output(&mut self) -> bool {
        if self.in_alt_screen() {
            return false;
        }
        let Some((&end, inclusive)) = self
            .marks
            .iter()
            .rev()
            .find_map(|(id, m)| m.end.map(|inc| (id, inc)))
        else {
            return false;
        };
        let previous_end = self
            .marks
            .range(..end)
            .rev()
            .find(|(_, m)| m.end.is_some())
            .map(|(&p, _)| p);
        // Its output began at the last output mark before the end, and after the previous end. If
        // that mark has scrolled out of the history, what is left starts at the oldest line.
        let begin = match self.marks.range(..=end).rev().find(|(_, m)| m.output) {
            Some((&b, _)) if previous_end.is_none_or(|p| p < b) => b,
            None if previous_end.is_none() => self.gone,
            _ => return false,
        };
        let last = if inclusive {
            end
        } else {
            end.saturating_sub(1)
        };
        if last < begin {
            return false;
        }
        self.select_rows(begin, last);
        true
    }
}
