//! Command palette — interactive `M-x`-style command search.
//!
//! Spec §15: "**Command palette** (`M-x`) with fuzzy search across all
//! registered commands". This module is the editor-side state machine
//! that owns the query, the filtered match list, and the selection
//! index. The render layer turns that state into a bottom overlay with
//! an input line and a scrolling match list; the driver's input task
//! routes key events at it through a dedicated keymap layer pushed
//! while the palette is open.
//!
//! # State lifecycle
//!
//! ```text
//!     closed ──open()──▶ open(query="", matches=<all>, selected=0)
//!        ▲                       │
//!        │ execute / cancel      │ append_char / backspace / next / prev
//!        │                       ▼
//!        └─────────────── refresh matches
//! ```
//!
//! [`CommandPalette::open`] seeds the initial match list from a
//! [`CommandRegistry`] snapshot. Subsequent query edits call
//! [`CommandPalette::refresh`] to recompute matches against the same
//! snapshot — the registry isn't consulted again mid-query because we
//! don't want a background extension registration to shuffle matches
//! underneath the user's cursor.
//!
//! # Matching
//!
//! Candidates are command names (`"cursor.word-forward"`) plus their
//! descriptions (`"Move the cursor forward one word"`). A query
//! matches if every character appears *in order* in the candidate's
//! name or description (case-insensitive). Scoring is a simple
//! position-plus-length heuristic:
//!
//! * Exact prefix match on the name → big bonus.
//! * Earlier subsequence start → bigger bonus.
//! * Shorter candidate → tie-break.
//!
//! Lower scores sort first. The empty query returns every command
//! sorted alphabetically.
//!
//! This is intentionally dumber than fzy / `nucleo`; it's enough to
//! satisfy "fuzzy search across all registered commands" without
//! pulling in a fuzzy-matcher crate. A later commit can swap in a
//! better scorer without changing the public API.

use crate::registry::CommandRegistry;

/// A single entry in the palette's filtered match list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteMatch {
    /// Command name, e.g. `"cursor.word-forward"`.
    pub name: String,
    /// Command description (may be empty).
    pub description: String,
    /// Lower-is-better score; see module-level docs for the scoring
    /// heuristic. Useful for tests; not rendered.
    pub score: i32,
}

/// Editor-side state for the command palette.
///
/// Always present on [`crate::Editor`] but normally closed (zero
/// allocations). [`open`](Self::open) switches it into the
/// query-accepting state and [`close`](Self::close) resets it.
/// What the palette does when the user presses Enter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PaletteMode {
    /// Execute the selected match as a command name.
    #[default]
    Command,
    /// Treat the query text as a file path and open it.
    FindFile,
    /// Switch to the buffer whose id is stored in the selected
    /// match's description field.
    SwitchBuffer,
}

#[derive(Debug, Default)]
pub struct CommandPalette {
    /// Whether the palette is currently accepting input.
    open: bool,
    /// What Enter does.
    mode: PaletteMode,
    /// The user's query so far.
    query: String,
    /// Prompt text shown before the query (e.g. "M-x " or "Find file: ").
    prompt: String,
    /// Index into `matches` for the highlighted row.
    selected: usize,
    /// Cached candidate list — every registered command, captured at
    /// `open` time so the set doesn't shift mid-query.
    candidates: Vec<Candidate>,
    /// Current filtered + scored + sorted view of `candidates`.
    matches: Vec<PaletteMatch>,
}

/// Immutable candidate entry — what `open()` captures from the
/// registry. Separating this from `PaletteMatch` lets us re-filter
/// without cloning strings every keystroke.
#[derive(Debug, Clone)]
struct Candidate {
    name: String,
    description: String,
}

impl CommandPalette {
    /// Create a fresh, closed palette.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the palette is currently accepting input.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Current query string.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The current mode (what Enter does).
    pub fn mode(&self) -> PaletteMode {
        self.mode
    }

    /// The prompt text shown before the query.
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Current filtered match list, best-first.
    pub fn matches(&self) -> &[PaletteMatch] {
        &self.matches
    }

    /// Index of the highlighted row in [`Self::matches`]. Returns `0`
    /// for an empty match list (which is fine — callers should check
    /// `matches().is_empty()` before indexing).
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The currently-highlighted match, or `None` if the match list
    /// is empty.
    pub fn selected_match(&self) -> Option<&PaletteMatch> {
        self.matches.get(self.selected)
    }

    /// Open the palette with an empty query. Snapshots every registered
    /// command from `registry` as the candidate pool for the session.
    pub fn open(&mut self, registry: &CommandRegistry) {
        let entries: Vec<(String, String)> = registry
            .iter()
            .map(|(name, desc)| (name.to_owned(), desc.to_owned()))
            .collect();
        self.open_with_entries(entries);
    }

    /// Open the palette with a pre-built candidate list. Split out
    /// from [`Self::open`] so callers who already hold a snapshot
    /// (typically because they want to drop the registry borrow
    /// before taking `&mut self`) don't have to pay for a second
    /// walk of the registry.
    pub fn open_with_entries(&mut self, entries: Vec<(String, String)>) {
        self.open = true;
        self.mode = PaletteMode::Command;
        "M-x ".clone_into(&mut self.prompt);
        self.query.clear();
        self.selected = 0;
        self.candidates = entries
            .into_iter()
            .map(|(name, description)| Candidate { name, description })
            .collect();
        self.refresh();
    }

    /// Open the palette in switch-buffer mode. Entries are
    /// `(display_name, buffer_id_string)`.
    pub fn open_switch_buffer(&mut self, entries: Vec<(String, String)>) {
        self.open = true;
        self.mode = PaletteMode::SwitchBuffer;
        "Switch buffer: ".clone_into(&mut self.prompt);
        self.query.clear();
        self.selected = 0;
        self.candidates = entries
            .into_iter()
            .map(|(name, description)| Candidate { name, description })
            .collect();
        self.refresh();
    }

    /// Open the palette in find-file mode. The user types a path;
    /// Enter opens it. Seeds the candidate list with the current
    /// directory's entries.
    pub fn open_find_file(&mut self) {
        self.open = true;
        self.mode = PaletteMode::FindFile;
        "Find file: ".clone_into(&mut self.prompt);
        self.query.clear();
        self.selected = 0;
        self.refresh_find_file();
    }

    /// Set the query text directly (used by find-file directory
    /// navigation to replace the query with a directory path).
    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    /// Public wrapper for `refresh_find_file` so stock commands can
    /// re-scan after updating the query.
    pub fn refresh_find_file_pub(&mut self) {
        self.refresh_find_file();
    }

    /// Scan the directory implied by the current query and populate
    /// the match list with its entries. If the query looks like a
    /// partial filename inside a directory (e.g. `src/ma`), list
    /// `src/` entries filtered by the prefix `ma`.
    fn refresh_find_file(&mut self) {
        use std::path::Path;

        self.candidates.clear();
        self.matches.clear();

        let query = &self.query;
        let path = Path::new(if query.is_empty() { "." } else { query });

        // Determine the directory to list and the filename prefix to
        // filter by. If the query ends with `/` or is a directory,
        // list it directly with no filter. Otherwise, list the parent
        // and filter by the filename component.
        let (dir, prefix) = if path.is_dir() {
            (path.to_path_buf(), String::new())
        } else {
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let prefix = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            (dir, prefix)
        };

        let prefix_lower = prefix.to_lowercase();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut items: Vec<(String, bool)> = Vec::new();
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Skip hidden files unless the prefix starts with '.'
                if name.starts_with('.') && !prefix.starts_with('.') {
                    continue;
                }
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                if !prefix_lower.is_empty()
                    && !name.to_lowercase().starts_with(&prefix_lower)
                {
                    continue;
                }
                items.push((name, is_dir));
            }
            // Sort: directories first, then alphabetical.
            items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

            let dir_str = if dir == Path::new(".") {
                String::new()
            } else {
                let mut s = dir.to_string_lossy().into_owned();
                if !s.ends_with('/') {
                    s.push('/');
                }
                s
            };

            for (name, is_dir) in items {
                let full = format!("{dir_str}{name}{}", if is_dir { "/" } else { "" });
                let kind = if is_dir { "dir" } else { "file" };
                self.matches.push(PaletteMatch {
                    name: full,
                    description: kind.to_owned(),
                    score: 0,
                });
            }
        }

        if self.selected >= self.matches.len() {
            self.selected = self.matches.len().saturating_sub(1);
        }
    }

    /// Close the palette and drop the cached candidate list.
    pub fn close(&mut self) {
        self.open = false;
        self.mode = PaletteMode::Command;
        self.prompt.clear();
        self.query.clear();
        self.selected = 0;
        self.candidates.clear();
        self.matches.clear();
    }

    /// Append one character to the query and refilter.
    pub fn append_char(&mut self, c: char) {
        self.query.push(c);
        if self.mode == PaletteMode::FindFile {
            self.refresh_find_file();
        } else {
            self.refresh();
        }
    }

    /// Remove one character from the query (if non-empty) and refilter.
    pub fn backspace(&mut self) {
        if self.query.pop().is_some() {
            if self.mode == PaletteMode::FindFile {
                self.refresh_find_file();
            } else {
                self.refresh();
            }
        }
    }

    /// Move the selection highlight down one row (saturates at the
    /// last match).
    pub fn select_next(&mut self) {
        if self.matches.is_empty() {
            self.selected = 0;
            return;
        }
        if self.selected + 1 < self.matches.len() {
            self.selected += 1;
        }
    }

    /// Move the selection highlight up one row (saturates at 0).
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Recompute `matches` from `candidates` under the current query.
    /// Empty query → all candidates sorted alphabetically by name.
    fn refresh(&mut self) {
        self.matches.clear();
        if self.query.is_empty() {
            self.matches.extend(self.candidates.iter().map(|c| PaletteMatch {
                name: c.name.clone(),
                description: c.description.clone(),
                score: c.name.len() as i32,
            }));
            self.matches.sort_by(|a, b| a.name.cmp(&b.name));
        } else {
            let lowered = self.query.to_lowercase();
            for c in &self.candidates {
                if let Some(score) = score_candidate(&lowered, &c.name, &c.description) {
                    self.matches.push(PaletteMatch {
                        name: c.name.clone(),
                        description: c.description.clone(),
                        score,
                    });
                }
            }
            self.matches.sort_by(|a, b| {
                a.score
                    .cmp(&b.score)
                    .then_with(|| a.name.cmp(&b.name))
            });
        }
        // After any refresh, pin the selection inside the new list.
        if self.selected >= self.matches.len() {
            self.selected = self.matches.len().saturating_sub(1);
        }
    }
}

/// Score a candidate against a lowercased query. Returns `None` if the
/// query doesn't match (no subsequence in name or description), or
/// `Some(score)` with lower = better.
///
/// The scorer prefers:
/// 1. Exact prefix on the name (largest bonus).
/// 2. Substring match on the name at a low position.
/// 3. Subsequence match on the name.
/// 4. Anything matching only through the description.
fn score_candidate(query_lc: &str, name: &str, description: &str) -> Option<i32> {
    let name_lc = name.to_lowercase();

    // (1) Exact prefix wins hands-down.
    if name_lc.starts_with(query_lc) {
        return Some(-10_000 + name.len() as i32);
    }

    // (2) Substring on the name.
    if let Some(pos) = name_lc.find(query_lc) {
        // -1000 base + position + length tie-breaker.
        return Some(-1_000 + pos as i32 + name.len() as i32);
    }

    // (3) Subsequence on the name.
    if let Some(first_hit) = subsequence_position(query_lc, &name_lc) {
        return Some(first_hit as i32 + name.len() as i32);
    }

    // (4) Fall back to description match (substring or subsequence).
    let desc_lc = description.to_lowercase();
    if desc_lc.contains(query_lc) || subsequence_position(query_lc, &desc_lc).is_some() {
        return Some(10_000 + name.len() as i32);
    }

    None
}

/// If `query` appears as a subsequence in `text`, return the index of
/// the first matched character in `text`. Otherwise `None`. Both
/// inputs are assumed already lowercased.
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
    use crate::registry::{Command, CommandContext};

    /// Tiny test command type so we don't pull in the full stock
    /// catalogue here.
    struct Stub {
        name: &'static str,
        desc: &'static str,
    }
    impl Command for Stub {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &'static str {
            self.desc
        }
        fn run(&self, _cx: &mut CommandContext<'_>) {}
    }

    fn registry_with(names: &[(&'static str, &'static str)]) -> CommandRegistry {
        let mut reg = CommandRegistry::new();
        for (name, desc) in names {
            reg.register(Stub { name, desc });
        }
        reg
    }

    #[test]
    fn opening_seeds_matches_from_registry() {
        let reg = registry_with(&[
            ("cursor.left", "Move left"),
            ("cursor.right", "Move right"),
            ("buffer.save", "Save"),
        ]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        assert!(p.is_open());
        assert_eq!(p.query(), "");
        assert_eq!(p.matches().len(), 3);
        // Empty query returns alphabetical order.
        assert_eq!(p.matches()[0].name, "buffer.save");
        assert_eq!(p.matches()[1].name, "cursor.left");
        assert_eq!(p.matches()[2].name, "cursor.right");
    }

    #[test]
    fn prefix_matches_beat_subsequence_matches() {
        let reg = registry_with(&[
            ("cursor.left", ""),
            ("color.set", ""), // "co" is a prefix here
            ("cursor.right", ""),
        ]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        p.append_char('c');
        p.append_char('o');
        // Prefix winner first.
        assert_eq!(p.matches()[0].name, "color.set");
    }

    #[test]
    fn substring_match_wins_over_unrelated_subsequence() {
        let reg = registry_with(&[
            ("buffer.save-as", ""),    // substring "save"
            ("anvil.save", ""),        // substring "save"
            ("sneak-value-end", ""),   // subsequence s-a-v-e only
        ]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        for c in "save".chars() {
            p.append_char(c);
        }
        let names: Vec<&str> = p.matches().iter().map(|m| m.name.as_str()).collect();
        // The two substring matches should come before the subsequence one.
        let save_as_idx = names.iter().position(|n| *n == "buffer.save-as").unwrap();
        let anvil_idx = names.iter().position(|n| *n == "anvil.save").unwrap();
        let sneak_idx = names.iter().position(|n| *n == "sneak-value-end").unwrap();
        assert!(save_as_idx < sneak_idx);
        assert!(anvil_idx < sneak_idx);
    }

    #[test]
    fn non_matching_query_produces_empty_matches() {
        let reg = registry_with(&[("cursor.left", "")]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        p.append_char('z');
        p.append_char('z');
        p.append_char('z');
        assert!(p.matches().is_empty());
        // Selection stays valid (pinned to 0) even with no matches.
        assert_eq!(p.selected_index(), 0);
        assert!(p.selected_match().is_none());
    }

    #[test]
    fn backspace_restores_broader_matches() {
        let reg = registry_with(&[
            ("cursor.left", ""),
            ("cursor.right", ""),
        ]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        p.append_char('l');
        assert_eq!(p.matches().len(), 1);
        p.backspace();
        assert_eq!(p.matches().len(), 2);
    }

    #[test]
    fn selection_arrows_saturate_at_edges() {
        let reg = registry_with(&[
            ("a", ""),
            ("b", ""),
            ("c", ""),
        ]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        assert_eq!(p.selected_index(), 0);
        p.select_prev();
        assert_eq!(p.selected_index(), 0);
        p.select_next();
        assert_eq!(p.selected_index(), 1);
        p.select_next();
        p.select_next();
        p.select_next();
        assert_eq!(p.selected_index(), 2);
    }

    #[test]
    fn refresh_clamps_stale_selection() {
        let reg = registry_with(&[
            ("cursor.left", ""),
            ("cursor.right", ""),
            ("cursor.up", ""),
        ]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        p.select_next();
        p.select_next();
        assert_eq!(p.selected_index(), 2);
        // Narrow the match list so the old index no longer fits.
        p.append_char('l'); // only "cursor.left" matches
        assert_eq!(p.matches().len(), 1);
        assert_eq!(p.selected_index(), 0);
    }

    #[test]
    fn close_resets_state() {
        let reg = registry_with(&[("cursor.left", "")]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        p.append_char('l');
        p.close();
        assert!(!p.is_open());
        assert!(p.matches().is_empty());
        assert_eq!(p.query(), "");
    }

    #[test]
    fn description_subsequence_is_a_last_resort_match() {
        let reg = registry_with(&[
            ("buffer.save", "Write to disk"),
            ("cursor.right", "Move cursor right"),
        ]);
        let mut p = CommandPalette::new();
        p.open(&reg);
        // "wri" only matches "buffer.save" through its description.
        for c in "wri".chars() {
            p.append_char(c);
        }
        let names: Vec<&str> = p.matches().iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"buffer.save"), "{names:?}");
    }
}
