//! Persistent interval tree used by text property layers.
//!
//! Phase 1 uses a simple sorted-vector representation wrapped in [`Arc`] so
//! that cloning and snapshotting are `O(1)`. Every mutation allocates a new
//! vector, preserving the old one for any outstanding snapshots. This is
//! sufficient for the property-layer workloads we expect in early Arx:
//! dozens to low-thousands of intervals per layer, most operations bulk
//! inserts or edit-driven adjustments.
//!
//! The representation matches the spec (see `docs/spec.md` §3.8) functionally
//! if not yet in its asymptotic behaviour — swapping in a treap or an
//! augmented persistent BST later is an implementation detail behind the
//! same public API.

use std::sync::Arc;

use crate::properties::{PropertyValue, StickyBehavior};
use crate::rope::ByteRange;

/// A single annotated interval inside a property layer.
#[derive(Debug, Clone)]
pub struct Interval {
    pub range: ByteRange,
    pub value: PropertyValue,
    pub sticky: StickyBehavior,
}

impl Interval {
    pub fn new(range: ByteRange, value: PropertyValue, sticky: StickyBehavior) -> Self {
        Self {
            range,
            value,
            sticky,
        }
    }
}

/// An immutable, shareable collection of intervals sorted by `start`.
#[derive(Debug, Clone, Default)]
pub struct IntervalTree {
    intervals: Arc<Vec<Interval>>,
}

impl IntervalTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.intervals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    /// Iterate over every stored interval.
    pub fn iter(&self) -> impl Iterator<Item = &Interval> {
        self.intervals.iter()
    }

    /// Return a new tree with `interval` inserted. Intervals are kept sorted
    /// by `start` (stable w.r.t. insertion order for equal starts).
    pub fn insert(&self, interval: Interval) -> Self {
        let mut next: Vec<Interval> = (*self.intervals).clone();
        let idx = next.partition_point(|i| i.range.start <= interval.range.start);
        next.insert(idx, interval);
        Self {
            intervals: Arc::new(next),
        }
    }

    /// Iterate over every interval overlapping `query`.
    pub fn overlapping(&self, query: ByteRange) -> impl Iterator<Item = &Interval> + '_ {
        self.intervals
            .iter()
            .filter(move |i| i.range.start < query.end && i.range.end > query.start)
    }

    /// Return a new tree with every interval inside `range` removed.
    /// Overlapping-but-not-contained intervals are preserved unchanged.
    pub fn remove_within(&self, range: ByteRange) -> Self {
        let next: Vec<Interval> = self
            .intervals
            .iter()
            .filter(|i| !(i.range.start >= range.start && i.range.end <= range.end))
            .cloned()
            .collect();
        Self {
            intervals: Arc::new(next),
        }
    }

    /// Return a new tree with every interval removed.
    pub fn clear(&self) -> Self {
        Self::new()
    }

    /// Apply a buffer edit (replace `[offset, offset + old_len)` with a span
    /// of `new_len` bytes) to every interval, honouring each interval's
    /// [`StickyBehavior`].
    ///
    /// Implementation: each endpoint of each interval is mapped through the
    /// edit independently using [`map_position`]. When an endpoint lands
    /// inside the replacement region (either because it was deleted or
    /// because the edit is a pure insertion at exactly that point), the
    /// sticky behaviour decides whether to pick the leading edge (`offset`)
    /// or the trailing edge (`offset + new_len`).
    ///
    /// Intervals whose endpoints collapse are dropped; [`StickyBehavior::Split`]
    /// always produces the two-piece fracture of the survivor.
    pub fn apply_edit(&self, offset: usize, old_len: usize, new_len: usize) -> Self {
        let edit_end = offset + old_len;
        let delta = new_len as isize - old_len as isize;
        let mut out: Vec<Interval> = Vec::with_capacity(self.intervals.len() + 1);

        for iv in self.intervals.iter() {
            let (a, b) = (iv.range.start, iv.range.end);

            // Definitively-before fast path: nothing the edit does can affect it.
            if b < offset {
                out.push(iv.clone());
                continue;
            }
            // Definitively-after fast path: just shift.
            if a > edit_end {
                out.push(Interval {
                    range: shift(a, delta)..shift(b, delta),
                    value: iv.value.clone(),
                    sticky: iv.sticky,
                });
                continue;
            }

            // Split is always a two-piece fracture around the replacement.
            if iv.sticky == StickyBehavior::Split {
                // Left half: the prefix of the interval before the edit.
                if a < offset {
                    out.push(Interval {
                        range: a..offset,
                        value: iv.value.clone(),
                        sticky: iv.sticky,
                    });
                }
                // Right half: the suffix after the replacement, shifted.
                if b > edit_end {
                    let start = shift(edit_end, delta);
                    let end = shift(b, delta);
                    if end > start {
                        out.push(Interval {
                            range: start..end,
                            value: iv.value.clone(),
                            sticky: iv.sticky,
                        });
                    }
                }
                continue;
            }

            // Map both endpoints independently according to sticky edges.
            let (left_edge, right_edge) = match iv.sticky {
                StickyBehavior::Grow => (Edge::Leading, Edge::Trailing),
                StickyBehavior::RearSticky => (Edge::Trailing, Edge::Trailing),
                StickyBehavior::Shrink => (Edge::Trailing, Edge::Leading),
                StickyBehavior::Split => unreachable!("handled above"),
            };
            let new_a = map_position(a, offset, old_len, new_len, left_edge);
            let new_b = map_position(b, offset, old_len, new_len, right_edge);

            if new_b > new_a {
                out.push(Interval {
                    range: new_a..new_b,
                    value: iv.value.clone(),
                    sticky: iv.sticky,
                });
            }
        }

        // Re-sort: edit adjustments can reorder ranges via Split / Grow extension.
        out.sort_by_key(|i| i.range.start);
        Self {
            intervals: Arc::new(out),
        }
    }
}

/// Which edge of a deletion/replacement a collapsing endpoint picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    /// Pick the pre-edit offset (text that was before the replacement).
    Leading,
    /// Pick the post-edit end (text that comes after the replacement).
    Trailing,
}

/// Map a pre-edit byte position to a post-edit byte position.
///
/// * Positions strictly before the edit stay where they are.
/// * Positions strictly after the edit shift by `new_len - old_len`.
/// * Positions inside `[offset, offset + old_len]` (or, for pure insertions,
///   exactly at `offset`) collapse to one of the two edit boundaries
///   depending on `edge`.
fn map_position(
    pos: usize,
    offset: usize,
    old_len: usize,
    new_len: usize,
    edge: Edge,
) -> usize {
    let edit_end = offset + old_len;
    if pos < offset {
        return pos;
    }
    if pos > edit_end {
        let delta = new_len as isize - old_len as isize;
        return shift(pos, delta);
    }
    // pos is in [offset, edit_end].
    // For a pure insertion (old_len == 0, edit_end == offset), this is the
    // single ambiguous point where sticky policy decides.
    match edge {
        Edge::Leading => offset,
        Edge::Trailing => offset + new_len,
    }
}

fn shift(pos: usize, delta: isize) -> usize {
    if delta >= 0 {
        pos + delta as usize
    } else {
        pos.saturating_sub((-delta) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::properties::{PropertyValue, StickyBehavior};

    fn iv(range: ByteRange, sticky: StickyBehavior) -> Interval {
        Interval::new(range, PropertyValue::Flag, sticky)
    }

    fn ranges(t: &IntervalTree) -> Vec<ByteRange> {
        t.iter().map(|i| i.range.clone()).collect()
    }

    // ---- basic tree operations ----

    #[test]
    fn insert_keeps_sorted() {
        let t = IntervalTree::new()
            .insert(iv(10..20, StickyBehavior::Shrink))
            .insert(iv(0..5, StickyBehavior::Shrink))
            .insert(iv(5..8, StickyBehavior::Shrink));
        let starts: Vec<_> = t.iter().map(|i| i.range.start).collect();
        assert_eq!(starts, vec![0, 5, 10]);
    }

    #[test]
    fn overlapping_query() {
        let t = IntervalTree::new()
            .insert(iv(0..5, StickyBehavior::Shrink))
            .insert(iv(10..20, StickyBehavior::Shrink))
            .insert(iv(15..30, StickyBehavior::Shrink));
        let hits: Vec<_> = t.overlapping(12..17).collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].range, 10..20);
        assert_eq!(hits[1].range, 15..30);
    }

    #[test]
    fn remove_within_drops_contained() {
        let t = IntervalTree::new()
            .insert(iv(0..5, StickyBehavior::Shrink))
            .insert(iv(10..15, StickyBehavior::Shrink))
            .insert(iv(20..30, StickyBehavior::Shrink));
        let t2 = t.remove_within(8..16);
        let starts: Vec<_> = t2.iter().map(|i| i.range.start).collect();
        assert_eq!(starts, vec![0, 20]);
    }

    // ---- shift-only cases (edit outside the interval) ----

    #[test]
    fn edit_entirely_before_shifts() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::RearSticky));
        assert_eq!(ranges(&t.apply_edit(0, 0, 3)), vec![13..23]);
        assert_eq!(ranges(&t.apply_edit(0, 2, 5)), vec![13..23]);
        assert_eq!(ranges(&t.apply_edit(0, 5, 2)), vec![7..17]);
    }

    #[test]
    fn edit_entirely_after_is_untouched() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::RearSticky));
        assert_eq!(ranges(&t.apply_edit(30, 2, 5)), vec![10..20]);
        // A pure insertion strictly past the rear is also a no-op.
        assert_eq!(ranges(&t.apply_edit(25, 0, 7)), vec![10..20]);
    }

    // ---- pure insertion at the front boundary (offset == a) ----

    #[test]
    fn insert_at_front_boundary_grow_extends_backward() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Grow));
        assert_eq!(ranges(&t.apply_edit(10, 0, 3)), vec![10..23]);
    }

    #[test]
    fn insert_at_front_boundary_rear_sticky_does_not_extend() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::RearSticky));
        assert_eq!(ranges(&t.apply_edit(10, 0, 3)), vec![13..23]);
    }

    #[test]
    fn insert_at_front_boundary_shrink_does_not_extend() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Shrink));
        assert_eq!(ranges(&t.apply_edit(10, 0, 3)), vec![13..23]);
    }

    // ---- pure insertion at the rear boundary (offset == b) ----

    #[test]
    fn insert_at_rear_boundary_grow_extends() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Grow));
        assert_eq!(ranges(&t.apply_edit(20, 0, 3)), vec![10..23]);
    }

    #[test]
    fn insert_at_rear_boundary_rear_sticky_extends() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::RearSticky));
        assert_eq!(ranges(&t.apply_edit(20, 0, 3)), vec![10..23]);
    }

    #[test]
    fn insert_at_rear_boundary_shrink_does_not_extend() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Shrink));
        assert_eq!(ranges(&t.apply_edit(20, 0, 3)), vec![10..20]);
    }

    // ---- insertion strictly inside ----

    #[test]
    fn insert_inside_grow_extends() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Grow));
        assert_eq!(ranges(&t.apply_edit(15, 0, 4)), vec![10..24]);
    }

    #[test]
    fn insert_inside_rear_sticky_extends() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::RearSticky));
        assert_eq!(ranges(&t.apply_edit(15, 0, 4)), vec![10..24]);
    }

    #[test]
    fn insert_inside_shrink_extends_because_nothing_was_removed() {
        // Shrink only truncates on *deletion*; a pure insertion inside
        // extends the interval (there is nothing to clip to).
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Shrink));
        assert_eq!(ranges(&t.apply_edit(15, 0, 4)), vec![10..24]);
    }

    #[test]
    fn insert_inside_split_fractures() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Split));
        assert_eq!(ranges(&t.apply_edit(15, 0, 2)), vec![10..15, 17..22]);
    }

    // ---- deletion inside the interval ----

    #[test]
    fn delete_inside_grow_covers_replacement() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Grow));
        // Delete [13..17): 4-byte delete inside the interval.
        assert_eq!(ranges(&t.apply_edit(13, 4, 0)), vec![10..16]);
    }

    #[test]
    fn delete_inside_rear_sticky_shrinks() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::RearSticky));
        assert_eq!(ranges(&t.apply_edit(13, 4, 0)), vec![10..16]);
    }

    #[test]
    fn delete_inside_shrink_produces_single_contracted_piece() {
        // Shrink is about *insertions* at boundaries not extending — on a
        // pure deletion the surviving bytes form a single contiguous run,
        // so the interval stays as one piece. Use `Split` for fracturing.
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Shrink));
        assert_eq!(ranges(&t.apply_edit(13, 4, 0)), vec![10..16]);
    }

    #[test]
    fn delete_inside_split_produces_two_pieces() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Split));
        assert_eq!(ranges(&t.apply_edit(13, 4, 0)), vec![10..13, 13..16]);
    }

    // ---- replacement overlapping the interval start ----

    #[test]
    fn replacement_overlaps_front_grow_extends_backward() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Grow));
        // Replace [8..12) with 4 bytes: delta=0, edit_end=12.
        assert_eq!(ranges(&t.apply_edit(8, 4, 4)), vec![8..20]);
    }

    #[test]
    fn replacement_overlaps_front_rear_sticky_clips_to_replacement_end() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::RearSticky));
        assert_eq!(ranges(&t.apply_edit(8, 4, 4)), vec![12..20]);
    }

    #[test]
    fn replacement_overlaps_front_shrink_clips_to_replacement_end() {
        let t = IntervalTree::new().insert(iv(10..20, StickyBehavior::Shrink));
        // Shrink emits [a..offset] (empty, a > offset) plus
        // [shift(edit_end, delta)..shift(b, delta)] = [12..20].
        assert_eq!(ranges(&t.apply_edit(8, 4, 4)), vec![12..20]);
    }

    // ---- interval fully inside a replacement ----

    #[test]
    fn interval_inside_replacement_rear_sticky_is_dropped() {
        let t = IntervalTree::new().insert(iv(14..16, StickyBehavior::RearSticky));
        // Replace [10..20) with 5 bytes: interval collapses to [15..15].
        assert_eq!(ranges(&t.apply_edit(10, 10, 5)), Vec::<ByteRange>::new());
    }

    #[test]
    fn interval_inside_replacement_grow_absorbs_replacement() {
        let t = IntervalTree::new().insert(iv(14..16, StickyBehavior::Grow));
        assert_eq!(ranges(&t.apply_edit(10, 10, 5)), vec![10..15]);
    }

    #[test]
    fn interval_inside_deletion_shrink_is_dropped() {
        let t = IntervalTree::new().insert(iv(14..16, StickyBehavior::Shrink));
        assert_eq!(ranges(&t.apply_edit(10, 10, 0)), Vec::<ByteRange>::new());
    }

    // ---- large replacements + multiple intervals ----

    #[test]
    fn multiple_intervals_around_replacement() {
        let t = IntervalTree::new()
            .insert(iv(0..5, StickyBehavior::RearSticky))
            .insert(iv(8..12, StickyBehavior::RearSticky))
            .insert(iv(15..20, StickyBehavior::RearSticky))
            .insert(iv(25..30, StickyBehavior::RearSticky));
        // Replace [10..18) with 3 bytes: delta = -5, edit_end = 18.
        // - [0..5)   entirely before  -> [0..5)
        // - [8..12)  overlaps right edge (b in deletion) -> [8..13)  (Trailing -> 10+3=13)
        // - [15..20) overlaps left  edge (a in deletion) -> [13..15) (shift 20 -> 15)
        // - [25..30) entirely after -> [20..25)
        let out = ranges(&t.apply_edit(10, 8, 3));
        assert_eq!(out, vec![0..5, 8..13, 13..15, 20..25]);
    }

    #[test]
    fn apply_edit_preserves_insertion_order_for_equal_starts() {
        let t = IntervalTree::new()
            .insert(iv(10..15, StickyBehavior::RearSticky))
            .insert(iv(10..20, StickyBehavior::RearSticky));
        // A no-op edit far away shouldn't reorder.
        let t2 = t.apply_edit(100, 0, 0);
        let ranges: Vec<_> = t2.iter().map(|i| i.range.clone()).collect();
        assert_eq!(ranges, vec![10..15, 10..20]);
    }
}
