//! Per-buffer line-exclusion filter (KEDIT `ALL` command).
//!
//! KEDIT's `ALL <pattern>` hides every line that doesn't match the
//! pattern. The hidden lines are "excluded" — not painted, not stepped
//! through by cursor motion, and not editable. Running `ALL` with no
//! argument clears the filter; running it again with a new pattern
//! replaces the previous filter (each `ALL` is evaluated against the
//! full buffer).
//!
//! This module is pure data: it compiles the regex, walks a text
//! snapshot, and returns the set of excluded line indices. The editor
//! stores the result keyed per buffer and consults it from the
//! renderer (to skip lines), the cursor motion helpers (to step only
//! through visible lines), and the edit guard (to refuse edits that
//! would touch excluded lines).

use std::collections::BTreeSet;

use regex::Regex;

/// One filter step applied to a buffer. The cumulative filter is the
/// sequence of steps in order; the excluded-line set on
/// [`FilterState`] is the result of replaying them all against the
/// buffer. Kept separately so the modeline can show the history as
/// `ALL /foo/ MORE /bar/` rather than just the last pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterStep {
    pub kind: FilterStepKind,
    pub pattern: String,
}

/// Which flavour of filter the step represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterStepKind {
    /// `ALL <pattern>` — reset filter, then keep matching lines only.
    All,
    /// `MORE <pattern>` — narrow further: among currently visible
    /// lines, keep only those also matching the new pattern.
    More,
    /// `LESS <pattern>` — broaden: re-include excluded lines that
    /// match the new pattern.
    Less,
}

impl FilterStepKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::More => "MORE",
            Self::Less => "LESS",
        }
    }
}

/// State held per buffer that has an active `ALL` / `MORE` / `LESS`
/// filter chain.
///
/// [`excluded`](Self::excluded) is a `BTreeSet` so callers that scan
/// for "next visible line after N" can use `range(N..)` in O(log n).
#[derive(Debug, Clone)]
pub struct FilterState {
    /// History of filter steps, most recent last. Used by the
    /// modeline for a compact display.
    pub steps: Vec<FilterStep>,
    /// Buffer line indices (0-based) that are currently hidden.
    pub excluded: BTreeSet<usize>,
}

impl FilterState {
    /// Pattern of the last applied step, for modeline display.
    pub fn latest_pattern(&self) -> &str {
        self.steps.last().map_or("", |s| s.pattern.as_str())
    }

    /// Compact one-line description of the filter chain, e.g.
    /// `"ALL /foo/ MORE /bar/"`. Returns an empty string when no
    /// steps are recorded.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(step.kind.label());
            out.push_str(" /");
            out.push_str(&step.pattern);
            out.push('/');
        }
        out
    }

    /// Compile `pattern` and scan `text` line-by-line, returning a
    /// fresh `FilterState` whose `excluded` set contains every line
    /// that *doesn't* match. Returns a `regex::Error` if the pattern
    /// is malformed. This is the `ALL` entry point; subsequent
    /// narrowing/broadening goes through [`Self::narrow`] /
    /// [`Self::broaden`].
    pub fn build(pattern: &str, text: &str) -> Result<Self, regex::Error> {
        let regex = Regex::new(pattern)?;
        let mut excluded = BTreeSet::new();
        for (i, line) in text.split('\n').enumerate() {
            if !regex.is_match(line) {
                excluded.insert(i);
            }
        }
        Ok(Self {
            steps: vec![FilterStep {
                kind: FilterStepKind::All,
                pattern: pattern.to_owned(),
            }],
            excluded,
        })
    }

    /// `MORE <pattern>`: narrow the filter further. Lines that were
    /// already hidden stay hidden; lines that were visible are
    /// re-tested against the new pattern and excluded if they don't
    /// match. Pushes a step onto `steps`.
    pub fn narrow(&mut self, pattern: &str, text: &str) -> Result<(), regex::Error> {
        let regex = Regex::new(pattern)?;
        for (i, line) in text.split('\n').enumerate() {
            if !self.excluded.contains(&i) && !regex.is_match(line) {
                self.excluded.insert(i);
            }
        }
        self.steps.push(FilterStep {
            kind: FilterStepKind::More,
            pattern: pattern.to_owned(),
        });
        Ok(())
    }

    /// `LESS <pattern>`: broaden the filter. Excluded lines that now
    /// match the pattern are re-included. Pushes a step onto `steps`.
    pub fn broaden(&mut self, pattern: &str, text: &str) -> Result<(), regex::Error> {
        let regex = Regex::new(pattern)?;
        let lines: Vec<&str> = text.split('\n').collect();
        self.excluded.retain(|&i| {
            let Some(line) = lines.get(i) else { return true };
            !regex.is_match(line)
        });
        self.steps.push(FilterStep {
            kind: FilterStepKind::Less,
            pattern: pattern.to_owned(),
        });
        Ok(())
    }

    /// Is `line` hidden by this filter?
    pub fn is_excluded(&self, line: usize) -> bool {
        self.excluded.contains(&line)
    }

    /// How many lines are hidden.
    pub fn excluded_count(&self) -> usize {
        self.excluded.len()
    }

    /// Walk from `start` in `direction` (positive = down, negative =
    /// up) and return the first *visible* line in that direction.
    /// `total_lines` is the buffer's line count (for clamping). If
    /// every line in the chosen direction is excluded, returns
    /// `start` unchanged.
    ///
    /// The search starts one step past `start`; the caller is
    /// expected to pass the line they're currently on.
    pub fn step_visible(&self, start: usize, direction: i32, total_lines: usize) -> usize {
        if total_lines == 0 {
            return 0;
        }
        if direction == 0 {
            return start;
        }
        let mut line = start;
        if direction > 0 {
            let mut remaining = direction;
            while remaining > 0 {
                if line + 1 >= total_lines {
                    return line;
                }
                line += 1;
                if !self.is_excluded(line) {
                    remaining -= 1;
                }
            }
            line
        } else {
            let mut remaining = -direction;
            while remaining > 0 {
                if line == 0 {
                    return line;
                }
                line -= 1;
                if !self.is_excluded(line) {
                    remaining -= 1;
                }
            }
            line
        }
    }

    /// Number of visible lines strictly between `from` and `to`
    /// (exclusive of both). Used by viewport-visibility logic to
    /// decide when scrolling is needed. `from` and `to` can be given
    /// in either order; the result is always non-negative.
    pub fn visible_lines_between(&self, from: usize, to: usize) -> usize {
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        if hi <= lo + 1 {
            return 0;
        }
        let inner = hi - lo - 1;
        let hidden_inner = self
            .excluded
            .range((lo + 1)..hi)
            .count();
        inner.saturating_sub(hidden_inner)
    }

    /// Snap `line` to the nearest visible line, preferring downward
    /// movement. Used to place the cursor after activating a filter
    /// that happens to hide the current line.
    pub fn snap_to_visible(&self, line: usize, total_lines: usize) -> usize {
        if total_lines == 0 || !self.is_excluded(line) {
            return line.min(total_lines.saturating_sub(1));
        }
        // Try downward first — feels more natural to "fall into" the
        // next visible line.
        for l in (line + 1)..total_lines {
            if !self.is_excluded(l) {
                return l;
            }
        }
        // Then upward.
        for l in (0..line).rev() {
            if !self.is_excluded(l) {
                return l;
            }
        }
        // Entire buffer filtered out. Stay put.
        line
    }

    /// Shift every excluded-line index strictly greater than
    /// `edit_line` by `delta`. Called after an edit that changed the
    /// buffer's total line count (inserting or removing newlines) so
    /// the excluded set continues to point at the *same content* it
    /// did before.
    ///
    /// Semantics match KEDIT's selection-level model: the filter
    /// names specific source lines, and those lines' exclusions
    /// travel with them as neighbouring edits shift their position.
    /// New lines created by the edit (which land in the shift gap)
    /// are visible by default — the filter is a snapshot, not a live
    /// match.
    ///
    /// `delta` can be negative (edit removed newlines), zero (no
    /// change — call is a no-op), or positive (edit inserted
    /// newlines). The caller is responsible for having rejected any
    /// edit that would touch an excluded line *through the edit
    /// guard in `user_edit`*, so we can assume no excluded line is
    /// "inside" the edited range.
    pub fn shift_indices(&mut self, edit_line: usize, delta: i64) {
        if delta == 0 {
            return;
        }
        let old = std::mem::take(&mut self.excluded);
        for idx in old {
            if idx <= edit_line {
                self.excluded.insert(idx);
            } else {
                // saturating_add_signed keeps us safe against any
                // underflow if a malformed delta were somehow passed
                // (shouldn't happen in practice given the guard).
                let new_idx = (idx as i64 + delta).max(0) as usize;
                self.excluded.insert(new_idx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_marks_non_matching_lines_excluded() {
        let f = FilterState::build("foo", "foo\nbar\nfoobar\nbaz").unwrap();
        // Lines 0 ("foo") and 2 ("foobar") match; 1 and 3 are excluded.
        assert!(!f.is_excluded(0));
        assert!(f.is_excluded(1));
        assert!(!f.is_excluded(2));
        assert!(f.is_excluded(3));
        assert_eq!(f.excluded_count(), 2);
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let err = FilterState::build("(", "anything").unwrap_err();
        // regex::Error::Syntax or similar; any error is fine.
        let _ = err;
    }

    #[test]
    fn step_visible_skips_excluded_lines() {
        let f = FilterState::build("foo", "foo\nbar\nbaz\nfoo\nfoo").unwrap();
        // Excluded: 1, 2. Visible: 0, 3, 4.
        // Down from 0 by 1 → 3.
        assert_eq!(f.step_visible(0, 1, 5), 3);
        // Down from 0 by 2 → 4.
        assert_eq!(f.step_visible(0, 2, 5), 4);
        // Up from 4 by 1 → 3.
        assert_eq!(f.step_visible(4, -1, 5), 3);
        // Up from 3 by 1 → 0.
        assert_eq!(f.step_visible(3, -1, 5), 0);
    }

    #[test]
    fn step_visible_saturates_at_buffer_edges() {
        let f = FilterState::build(".", "a\nb\nc").unwrap();
        assert_eq!(f.step_visible(2, 10, 3), 2);
        assert_eq!(f.step_visible(0, -10, 3), 0);
    }

    #[test]
    fn step_visible_zero_delta_is_identity() {
        let f = FilterState::build(".", "a").unwrap();
        assert_eq!(f.step_visible(0, 0, 1), 0);
    }

    #[test]
    fn visible_lines_between_counts_intervening() {
        let f = FilterState::build("foo", "foo\nbar\nbaz\nfoo\nfoo").unwrap();
        // Between lines 0 and 4: intervening lines are 1,2,3. Of those,
        // 1 and 2 are excluded, 3 is visible → 1 visible line between.
        assert_eq!(f.visible_lines_between(0, 4), 1);
        assert_eq!(f.visible_lines_between(4, 0), 1);
        // Adjacent lines have 0 visible between.
        assert_eq!(f.visible_lines_between(0, 1), 0);
        // Same line → 0.
        assert_eq!(f.visible_lines_between(2, 2), 0);
    }

    #[test]
    fn snap_to_visible_moves_down_then_up() {
        let f = FilterState::build("foo", "foo\nbar\nbaz\nfoo").unwrap();
        // Excluded: 1, 2. Snap from 1 → 3 (next visible down).
        assert_eq!(f.snap_to_visible(1, 4), 3);
        // If nothing visible below, snap up.
        let g = FilterState::build("^foo", "foo\nbar\nbaz").unwrap();
        assert_eq!(g.snap_to_visible(2, 3), 0);
    }

    #[test]
    fn snap_to_visible_passes_through_already_visible_line() {
        let f = FilterState::build("foo", "foo\nbar\nfoo").unwrap();
        assert_eq!(f.snap_to_visible(0, 3), 0);
        assert_eq!(f.snap_to_visible(2, 3), 2);
    }

    #[test]
    fn narrow_hides_additional_non_matching_lines() {
        // Buffer: 0 "foo alpha", 1 "foo beta", 2 "bar alpha", 3 "foo alpha"
        let text = "foo alpha\nfoo beta\nbar alpha\nfoo alpha";
        let mut f = FilterState::build("foo", text).unwrap();
        // After ALL /foo/: 0, 1, 3 visible; 2 excluded.
        assert_eq!(f.excluded_count(), 1);
        // MORE /alpha/: among visible, keep only those matching alpha.
        // So 1 "foo beta" gets excluded too. Final visible: 0, 3.
        f.narrow("alpha", text).unwrap();
        assert_eq!(f.excluded_count(), 2);
        assert!(f.is_excluded(1));
        assert!(f.is_excluded(2));
        assert!(!f.is_excluded(0));
        assert!(!f.is_excluded(3));
    }

    #[test]
    fn narrow_does_not_revive_already_hidden_lines() {
        // Line 2 "bar alpha" was hidden by ALL /foo/; MORE /alpha/
        // must not bring it back just because it matches alpha.
        let text = "foo alpha\nbar alpha\nfoo beta";
        let mut f = FilterState::build("foo", text).unwrap();
        f.narrow("alpha", text).unwrap();
        assert!(f.is_excluded(1), "bar alpha should stay hidden");
    }

    #[test]
    fn broaden_reincludes_matching_excluded_lines() {
        let text = "foo\nbar\nbaz\nbar";
        // ALL /foo/: excluded = {1, 2, 3}.
        let mut f = FilterState::build("foo", text).unwrap();
        assert_eq!(f.excluded_count(), 3);
        // LESS /bar/: re-include 1 and 3; 2 stays excluded.
        f.broaden("bar", text).unwrap();
        assert_eq!(f.excluded_count(), 1);
        assert!(f.is_excluded(2));
        assert!(!f.is_excluded(1));
        assert!(!f.is_excluded(3));
    }

    #[test]
    fn narrow_rejects_invalid_regex_and_leaves_state_unchanged() {
        let text = "foo\nbar";
        let mut f = FilterState::build("foo", text).unwrap();
        let before = f.excluded.clone();
        let result = f.narrow("(", text);
        assert!(result.is_err());
        assert_eq!(f.excluded, before);
    }

    #[test]
    fn describe_shows_chain() {
        let text = "foo alpha\nfoo beta\nbar alpha";
        let mut f = FilterState::build("foo", text).unwrap();
        f.narrow("alpha", text).unwrap();
        f.broaden("beta", text).unwrap();
        assert_eq!(f.describe(), "ALL /foo/ MORE /alpha/ LESS /beta/");
    }

    #[test]
    fn shift_indices_inserts_pass_through_earlier_lines() {
        let mut f = FilterState::build("foo", "foo\nbar\nbaz").unwrap();
        // excluded = {1, 2}. Insert a newline at edit_line 0 → delta +1.
        f.shift_indices(0, 1);
        let got: Vec<usize> = f.excluded.iter().copied().collect();
        assert_eq!(got, vec![2, 3]);
    }

    #[test]
    fn shift_indices_delete_pulls_later_lines_up() {
        let mut f = FilterState::build("foo", "foo\nbar\nbaz").unwrap();
        // excluded = {1, 2}. Edit at line 0 removes a newline → delta -1.
        // Line 1 shifts to 0? No — shift only affects idx > edit_line, so
        // {1, 2} → {0, 1}. That's correct if the caller deleted a newline
        // belonging to line 0 (collapsing line 0+1 into a single line).
        f.shift_indices(0, -1);
        let got: Vec<usize> = f.excluded.iter().copied().collect();
        assert_eq!(got, vec![0, 1]);
    }

    #[test]
    fn shift_indices_leaves_earlier_excluded_lines_alone() {
        // excluded = {0, 3} where 0 is before the edit and 3 is after.
        let mut f = FilterState::build("foo", "bar\nfoo\nfoo\nbar\nfoo").unwrap();
        f.shift_indices(1, 2); // edit inserted 2 newlines at line 1
        // {0, 3} → {0, 5}
        let got: Vec<usize> = f.excluded.iter().copied().collect();
        assert_eq!(got, vec![0, 5]);
    }

    #[test]
    fn shift_indices_zero_delta_is_a_noop() {
        let mut f = FilterState::build("foo", "foo\nbar\nfoo").unwrap();
        let before = f.excluded.clone();
        f.shift_indices(0, 0);
        assert_eq!(f.excluded, before);
    }
}
