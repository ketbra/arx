//! Interactive buffer search — swiper / telescope-style line filtering.
//!
//! The user presses `C-s` (Emacs) or `/` (Vim) to open a bottom overlay
//! where they type a query. Matching lines from the active buffer appear
//! in a scrolling list; navigating between them jumps the cursor and
//! scrolls the buffer in real time. Enter accepts (cursor stays at the
//! match); Escape cancels (cursor returns to original position).
//!
//! Three match modes:
//!
//! * **Fuzzy** (default) — case-insensitive subsequence match on the
//!   line text, scored so substring and prefix hits sort first.
//! * **Literal** — case-insensitive substring match, results in line
//!   order.
//! * **Regex** — `regex::Regex` match, results in line order. Invalid
//!   regex patterns produce empty results (no crash).
//!
//! # State lifecycle
//!
//! ```text
//!     closed ──open()──▶ open(lines, query="", matches=<all>, selected=0)
//!        ▲                         │
//!        │ execute / cancel        │ append_char / backspace / next / prev
//!        │                         ▼
//!        └──────────────── refresh matches
//! ```

/// Maximum number of candidate lines kept. Buffers larger than this
/// have their lines capped to avoid stalling the UI on open.
const MAX_CANDIDATE_LINES: usize = 50_000;

/// Maximum number of results shown after filtering. Keeps the overlay
/// responsive even when a short query matches thousands of lines.
const MAX_FILTERED_RESULTS: usize = 5_000;

/// A single line that matched the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// 0-based line number in the buffer.
    pub line_number: usize,
    /// Full text of the line (no trailing newline).
    pub line_text: String,
    /// Byte offset of the line start in the buffer.
    pub byte_start: usize,
    /// Byte offset of the match start *within the line*.
    pub match_offset: usize,
    /// Length in bytes of the matched span within the line.
    pub match_len: usize,
    /// Fuzzy score (lower = better). Only meaningful in `Fuzzy` mode;
    /// `Literal` and `Regex` modes use 0.
    pub score: i32,
}

/// Which matching algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Case-insensitive fuzzy subsequence matching, scored like the
    /// command palette. Best matches sort first.
    #[default]
    Fuzzy,
    /// Case-insensitive literal substring matching. Results in line order.
    Literal,
    /// Regex matching. Invalid patterns produce empty results. Results
    /// in line order.
    Regex,
}

impl SearchMode {
    /// Cycle to the next mode: Fuzzy → Literal → Regex → Fuzzy.
    pub fn next(self) -> Self {
        match self {
            Self::Fuzzy => Self::Literal,
            Self::Literal => Self::Regex,
            Self::Regex => Self::Fuzzy,
        }
    }

    /// Human-readable label for the prompt line.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fuzzy => "fuzzy",
            Self::Literal => "literal",
            Self::Regex => "regex",
        }
    }
}

/// A cached line from the buffer snapshot, captured at open time.
#[derive(Debug, Clone)]
struct CandidateLine {
    line_number: usize,
    text: String,
    byte_start: usize,
}

/// Editor-side state for interactive buffer search.
///
/// Always present on [`crate::Editor`] but normally closed (zero
/// allocations). [`open`](Self::open) switches it into the
/// query-accepting state and [`close`](Self::close) resets it.
#[derive(Debug, Default)]
pub struct BufferSearch {
    /// Whether the search overlay is currently visible.
    open: bool,
    /// Current matching mode.
    mode: SearchMode,
    /// The user's query string.
    query: String,
    /// Snapshot of buffer lines, captured at open time.
    candidates: Vec<CandidateLine>,
    /// Filtered + scored + sorted match list.
    matches: Vec<SearchMatch>,
    /// Index into `matches` for the highlighted row.
    selected: usize,
    /// Saved cursor byte offset to restore on cancel.
    saved_cursor: usize,
    /// Saved scroll-top line to restore on cancel.
    saved_scroll: usize,
    /// Search history (most recent last).
    history: Vec<String>,
    /// Current position in history during M-p/M-n browsing.
    /// `None` = fresh query; `Some(0)` = most recent entry.
    history_index: Option<usize>,
    /// Query text saved before entering history browsing.
    saved_query: String,
}

impl BufferSearch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected_match(&self) -> Option<&SearchMatch> {
        self.matches.get(self.selected)
    }

    pub fn saved_cursor(&self) -> usize {
        self.saved_cursor
    }

    pub fn saved_scroll(&self) -> usize {
        self.saved_scroll
    }

    /// Open the search overlay. `buffer_text` is the full buffer
    /// content; it's split into lines and snapshotted as the candidate
    /// pool. `cursor` and `scroll` are the current window state,
    /// saved for cancel-restore.
    pub fn open(&mut self, buffer_text: &str, cursor: usize, scroll: usize) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.history_index = None;
        self.saved_query.clear();
        self.saved_cursor = cursor;
        self.saved_scroll = scroll;

        // Snapshot lines.
        self.candidates.clear();
        let mut byte_offset = 0;
        for (i, line) in buffer_text.split('\n').enumerate() {
            if i >= MAX_CANDIDATE_LINES {
                break;
            }
            self.candidates.push(CandidateLine {
                line_number: i,
                text: line.to_owned(),
                byte_start: byte_offset,
            });
            byte_offset += line.len() + 1; // +1 for the '\n'
        }
        self.refresh();
    }

    /// Close the search overlay and drop cached state.
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
        self.history_index = None;
        self.saved_query.clear();
        self.candidates.clear();
        self.matches.clear();
    }

    /// Append one character to the query and refilter.
    pub fn append_char(&mut self, c: char) {
        self.history_index = None;
        self.query.push(c);
        self.refresh();
    }

    /// Remove the last character from the query and refilter.
    pub fn backspace(&mut self) {
        self.history_index = None;
        if self.query.pop().is_some() {
            self.refresh();
        }
    }

    /// Move the selection down one row (saturates at last match).
    pub fn select_next(&mut self) {
        if !self.matches.is_empty() && self.selected + 1 < self.matches.len() {
            self.selected += 1;
        }
    }

    /// Move the selection up one row (saturates at 0).
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move the selection down by `n` rows.
    pub fn select_next_n(&mut self, n: usize) {
        if self.matches.is_empty() {
            return;
        }
        self.selected = (self.selected + n).min(self.matches.len() - 1);
    }

    /// Move the selection up by `n` rows.
    pub fn select_prev_n(&mut self, n: usize) {
        self.selected = self.selected.saturating_sub(n);
    }

    /// Toggle to the next search mode and refilter.
    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.next();
        self.refresh();
    }

    /// Record a query in the search history.
    pub fn push_history(&mut self, entry: String) {
        self.history.retain(|h| h != &entry);
        self.history.push(entry);
    }

    /// Navigate to the previous (older) history entry.
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next_idx = match self.history_index {
            None => {
                self.saved_query = self.query.clone();
                0
            }
            Some(i) if i + 1 < self.history.len() => i + 1,
            Some(_) => return,
        };
        self.history_index = Some(next_idx);
        let entry = &self.history[self.history.len() - 1 - next_idx];
        self.query = entry.clone();
        self.refresh();
    }

    /// Navigate to the next (newer) history entry. Past the newest
    /// restores the saved query.
    pub fn history_next(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx == 0 {
            self.history_index = None;
            self.query = std::mem::take(&mut self.saved_query);
        } else {
            self.history_index = Some(idx - 1);
            let entry = &self.history[self.history.len() - idx];
            self.query = entry.clone();
        }
        self.refresh();
    }

    /// Recompute `matches` from `candidates` under the current query.
    fn refresh(&mut self) {
        self.matches.clear();

        if self.query.is_empty() {
            // Empty query: show all lines in order (capped).
            for c in self.candidates.iter().take(MAX_FILTERED_RESULTS) {
                self.matches.push(SearchMatch {
                    line_number: c.line_number,
                    line_text: c.text.clone(),
                    byte_start: c.byte_start,
                    match_offset: 0,
                    match_len: 0,
                    score: c.line_number as i32,
                });
            }
        } else {
            match self.mode {
                SearchMode::Fuzzy => self.refresh_fuzzy(),
                SearchMode::Literal => self.refresh_literal(),
                SearchMode::Regex => self.refresh_regex(),
            }
        }

        if self.selected >= self.matches.len() {
            self.selected = self.matches.len().saturating_sub(1);
        }
    }

    fn refresh_fuzzy(&mut self) {
        let query_lc = self.query.to_lowercase();
        for c in &self.candidates {
            if let Some((score, offset, len)) = score_line(&query_lc, &c.text) {
                self.matches.push(SearchMatch {
                    line_number: c.line_number,
                    line_text: c.text.clone(),
                    byte_start: c.byte_start,
                    match_offset: offset,
                    match_len: len,
                    score,
                });
                if self.matches.len() >= MAX_FILTERED_RESULTS {
                    break;
                }
            }
        }
        self.matches.sort_by(|a, b| {
            a.score
                .cmp(&b.score)
                .then_with(|| a.line_number.cmp(&b.line_number))
        });
    }

    fn refresh_literal(&mut self) {
        let query_lc = self.query.to_lowercase();
        for c in &self.candidates {
            let line_lc = c.text.to_lowercase();
            if let Some(pos) = line_lc.find(&query_lc) {
                self.matches.push(SearchMatch {
                    line_number: c.line_number,
                    line_text: c.text.clone(),
                    byte_start: c.byte_start,
                    match_offset: pos,
                    match_len: query_lc.len(),
                    score: 0,
                });
                if self.matches.len() >= MAX_FILTERED_RESULTS {
                    break;
                }
            }
        }
    }

    fn refresh_regex(&mut self) {
        let Ok(re) = regex::Regex::new(&self.query) else {
            return; // invalid regex → empty results
        };
        for c in &self.candidates {
            if let Some(m) = re.find(&c.text) {
                self.matches.push(SearchMatch {
                    line_number: c.line_number,
                    line_text: c.text.clone(),
                    byte_start: c.byte_start,
                    match_offset: m.start(),
                    match_len: m.len(),
                    score: 0,
                });
                if self.matches.len() >= MAX_FILTERED_RESULTS {
                    break;
                }
            }
        }
    }
}

/// Score a line against a lowercased query for fuzzy mode. Returns
/// `(score, match_offset, match_len)` or `None` if the query doesn't
/// match.
///
/// Scoring tiers (lower = better):
/// 1. Exact prefix on the line → large bonus.
/// 2. Substring match → medium bonus, earlier position better.
/// 3. Subsequence match → position-based score.
fn score_line(query_lc: &str, line: &str) -> Option<(i32, usize, usize)> {
    let line_lc = line.to_lowercase();

    // (1) Exact prefix.
    if line_lc.trim_start().starts_with(query_lc) {
        let offset = line.len() - line.trim_start().len();
        return Some((-10_000 + line.len() as i32, offset, query_lc.len()));
    }

    // (2) Substring.
    if let Some(pos) = line_lc.find(query_lc) {
        return Some((-1_000 + pos as i32 + line.len() as i32, pos, query_lc.len()));
    }

    // (3) Subsequence.
    if let Some(first_hit) = subsequence_position(query_lc, &line_lc) {
        return Some((first_hit as i32 + line.len() as i32, first_hit, 1));
    }

    None
}

/// If `query` appears as a subsequence in `text`, return the index of
/// the first matched character. Both inputs assumed already lowercased.
fn subsequence_position(query: &str, text: &str) -> Option<usize> {
    let mut q = query.chars();
    let mut next = q.next()?;
    let mut first_hit: Option<usize> = None;
    for (idx, ch) in text.char_indices() {
        if ch == next {
            if first_hit.is_none() {
                first_hit = Some(idx);
            }
            match q.next() {
                Some(c) => next = c,
                None => return first_hit,
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_text() -> &'static str {
        "fn main() {\n    println!(\"hello\");\n    let x = 42;\n}\n"
    }

    #[test]
    fn open_seeds_all_lines() {
        let mut s = BufferSearch::new();
        s.open(sample_text(), 0, 0);
        assert!(s.is_open());
        assert_eq!(s.query(), "");
        // 5 lines: "fn main() {", "    println!(...)", "    let x = 42;", "}", ""
        assert_eq!(s.matches().len(), 5);
        assert_eq!(s.matches()[0].line_number, 0);
    }

    #[test]
    fn fuzzy_query_filters_lines() {
        let mut s = BufferSearch::new();
        s.open(sample_text(), 0, 0);
        s.append_char('m');
        s.append_char('a');
        s.append_char('i');
        s.append_char('n');
        // "main" matches line 0: "fn main() {"
        assert!(!s.matches().is_empty());
        assert!(s.matches().iter().any(|m| m.line_text.contains("main")));
    }

    #[test]
    fn literal_mode_case_insensitive() {
        let mut s = BufferSearch::new();
        s.open("Hello World\nhello again\nGoodbye\n", 0, 0);
        s.toggle_mode(); // Fuzzy → Literal
        assert_eq!(s.mode(), SearchMode::Literal);
        for c in "hello".chars() {
            s.append_char(c);
        }
        assert_eq!(s.matches().len(), 2);
        assert_eq!(s.matches()[0].line_number, 0);
        assert_eq!(s.matches()[1].line_number, 1);
    }

    #[test]
    fn regex_mode_filters() {
        let mut s = BufferSearch::new();
        s.open("foo 123\nbar 456\nbaz\n", 0, 0);
        s.toggle_mode(); // Fuzzy → Literal
        s.toggle_mode(); // Literal → Regex
        assert_eq!(s.mode(), SearchMode::Regex);
        for c in r"\d+".chars() {
            s.append_char(c);
        }
        assert_eq!(s.matches().len(), 2);
    }

    #[test]
    fn regex_invalid_pattern_no_crash() {
        let mut s = BufferSearch::new();
        s.open("test line\n", 0, 0);
        s.toggle_mode();
        s.toggle_mode(); // Regex
        s.append_char('('); // invalid regex
        assert!(s.matches().is_empty());
    }

    #[test]
    fn backspace_restores_broader_matches() {
        let mut s = BufferSearch::new();
        s.open("alpha\nbeta\nalpha beta\n", 0, 0);
        s.append_char('a');
        s.append_char('l');
        s.append_char('p');
        s.append_char('h');
        s.append_char('a');
        let narrow = s.matches().len();
        s.backspace();
        s.backspace();
        s.backspace();
        s.backspace();
        assert!(s.matches().len() > narrow);
    }

    #[test]
    fn selection_saturates_at_edges() {
        let mut s = BufferSearch::new();
        s.open("a\nb\nc\n", 0, 0);
        assert_eq!(s.selected_index(), 0);
        s.select_prev();
        assert_eq!(s.selected_index(), 0);
        s.select_next();
        s.select_next();
        s.select_next();
        s.select_next(); // past end
        // 4 lines including trailing empty
        assert_eq!(s.selected_index(), s.matches().len() - 1);
    }

    #[test]
    fn page_navigation() {
        let text: String = (0..30).map(|i| format!("line {i}\n")).collect();
        let mut s = BufferSearch::new();
        s.open(&text, 0, 0);
        s.select_next_n(8);
        assert_eq!(s.selected_index(), 8);
        s.select_prev_n(3);
        assert_eq!(s.selected_index(), 5);
    }

    #[test]
    fn close_resets_state() {
        let mut s = BufferSearch::new();
        s.open(sample_text(), 10, 2);
        s.append_char('f');
        s.close();
        assert!(!s.is_open());
        assert!(s.matches().is_empty());
        assert_eq!(s.query(), "");
    }

    #[test]
    fn history_navigation() {
        let mut s = BufferSearch::new();
        s.push_history("first".into());
        s.push_history("second".into());
        s.open("test\n", 0, 0);

        s.history_prev(); // → "second"
        assert_eq!(s.query(), "second");
        s.history_prev(); // → "first"
        assert_eq!(s.query(), "first");
        s.history_prev(); // saturates at oldest
        assert_eq!(s.query(), "first");

        s.history_next(); // → "second"
        assert_eq!(s.query(), "second");
        s.history_next(); // → original (empty)
        assert_eq!(s.query(), "");
    }

    #[test]
    fn mode_toggle_cycles() {
        let mut s = BufferSearch::new();
        assert_eq!(s.mode(), SearchMode::Fuzzy);
        s.toggle_mode();
        assert_eq!(s.mode(), SearchMode::Literal);
        s.toggle_mode();
        assert_eq!(s.mode(), SearchMode::Regex);
        s.toggle_mode();
        assert_eq!(s.mode(), SearchMode::Fuzzy);
    }

    #[test]
    fn saved_position_accessible() {
        let mut s = BufferSearch::new();
        s.open("test\n", 42, 7);
        assert_eq!(s.saved_cursor(), 42);
        assert_eq!(s.saved_scroll(), 7);
    }

    #[test]
    fn history_deduplicates() {
        let mut s = BufferSearch::new();
        s.push_history("query".into());
        s.push_history("other".into());
        s.push_history("query".into()); // moves to end
        assert_eq!(s.history.len(), 2);
        assert_eq!(s.history[0], "other");
        assert_eq!(s.history[1], "query");
    }

    #[test]
    fn refresh_clamps_stale_selection() {
        let mut s = BufferSearch::new();
        s.open("aaa\nbbb\nccc\n", 0, 0);
        s.select_next();
        s.select_next();
        assert_eq!(s.selected_index(), 2);
        // Narrow to fewer results.
        s.append_char('a');
        assert_eq!(s.matches().len(), 1);
        assert_eq!(s.selected_index(), 0);
    }
}
