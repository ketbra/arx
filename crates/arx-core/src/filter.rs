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

/// State held per buffer that has an active `ALL` filter.
///
/// [`excluded`](Self::excluded) is a `BTreeSet` so callers that scan
/// for "next visible line after N" can use `range(N..)` in O(log n).
#[derive(Debug, Clone)]
pub struct FilterState {
    /// The source pattern text the user typed (for display).
    pub pattern: String,
    /// The compiled regex, applied against each line's text.
    pub regex: Regex,
    /// Buffer line indices (0-based) that are currently hidden.
    pub excluded: BTreeSet<usize>,
}

impl FilterState {
    /// Compile `pattern` and scan `text` line-by-line, returning a
    /// fresh `FilterState` whose `excluded` set contains every line
    /// that *doesn't* match. Returns a `regex::Error` if the pattern
    /// is malformed.
    pub fn build(pattern: &str, text: &str) -> Result<Self, regex::Error> {
        let regex = Regex::new(pattern)?;
        let mut excluded = BTreeSet::new();
        for (i, line) in text.split('\n').enumerate() {
            if !regex.is_match(line) {
                excluded.insert(i);
            }
        }
        Ok(Self {
            pattern: pattern.to_owned(),
            regex,
            excluded,
        })
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
}
