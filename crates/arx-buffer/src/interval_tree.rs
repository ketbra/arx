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
    /// * Intervals entirely before the edit are left alone.
    /// * Intervals entirely after are shifted by `delta`.
    /// * Intervals straddling the edit are extended, shrunk, or split per
    ///   their sticky policy.
    ///
    /// Intervals that collapse to an empty range (and aren't flagged
    /// [`StickyBehavior::Grow`]) are dropped.
    pub fn apply_edit(&self, offset: usize, old_len: usize, new_len: usize) -> Self {
        let edit_end = offset + old_len;
        let delta = new_len as isize - old_len as isize;
        let mut out: Vec<Interval> = Vec::with_capacity(self.intervals.len());

        for iv in self.intervals.iter() {
            let (a, b) = (iv.range.start, iv.range.end);
            // Entirely before the edit -> untouched.
            if b <= offset {
                out.push(iv.clone());
                continue;
            }
            // Entirely after the edit -> shift.
            if a >= edit_end {
                out.push(Interval {
                    range: shift(a, delta)..shift(b, delta),
                    value: iv.value.clone(),
                    sticky: iv.sticky,
                });
                continue;
            }
            // Edit and interval overlap. Apply per-sticky handling.
            match iv.sticky {
                StickyBehavior::Grow => {
                    // Extend the interval to cover the replacement.
                    let new_start = a.min(offset);
                    let new_end = b.max(edit_end);
                    let new_start_shifted = new_start;
                    let new_end_shifted = shift(new_end, delta);
                    out.push(Interval {
                        range: new_start_shifted..new_end_shifted,
                        value: iv.value.clone(),
                        sticky: iv.sticky,
                    });
                }
                StickyBehavior::Shrink => {
                    // Keep only the parts not covered by the edit.
                    if a < offset {
                        out.push(Interval {
                            range: a..offset,
                            value: iv.value.clone(),
                            sticky: iv.sticky,
                        });
                    }
                    if b > edit_end {
                        let start = shift(edit_end, delta).max(shift(offset, delta));
                        let end = shift(b, delta);
                        if end > start {
                            out.push(Interval {
                                range: start..end,
                                value: iv.value.clone(),
                                sticky: iv.sticky,
                            });
                        }
                    }
                }
                StickyBehavior::RearSticky => {
                    // Insertions at the start don't extend; at the end do.
                    // If old_len == 0 this is a pure insertion inside the
                    // interval -> grow the end only.
                    let new_start = if a < offset { a } else { shift(a, delta) };
                    let new_end_pre = if b >= edit_end { b.max(edit_end) } else { b };
                    let new_end = shift(new_end_pre, delta);
                    if new_end > new_start {
                        out.push(Interval {
                            range: new_start..new_end,
                            value: iv.value.clone(),
                            sticky: iv.sticky,
                        });
                    }
                }
                StickyBehavior::Split => {
                    // Preserve the left half; emit a second span for the
                    // right half at the post-edit position.
                    if a < offset {
                        out.push(Interval {
                            range: a..offset,
                            value: iv.value.clone(),
                            sticky: iv.sticky,
                        });
                    }
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
                }
            }
        }

        // Re-sort: edit adjustments can reorder ranges via RearSticky growth.
        out.sort_by_key(|i| i.range.start);
        Self {
            intervals: Arc::new(out),
        }
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

    fn flag(range: ByteRange, sticky: StickyBehavior) -> Interval {
        Interval::new(range, PropertyValue::Flag, sticky)
    }

    #[test]
    fn insert_keeps_sorted() {
        let t = IntervalTree::new()
            .insert(flag(10..20, StickyBehavior::Shrink))
            .insert(flag(0..5, StickyBehavior::Shrink))
            .insert(flag(5..8, StickyBehavior::Shrink));
        let starts: Vec<_> = t.iter().map(|i| i.range.start).collect();
        assert_eq!(starts, vec![0, 5, 10]);
    }

    #[test]
    fn overlapping_query() {
        let t = IntervalTree::new()
            .insert(flag(0..5, StickyBehavior::Shrink))
            .insert(flag(10..20, StickyBehavior::Shrink))
            .insert(flag(15..30, StickyBehavior::Shrink));
        let hits: Vec<_> = t.overlapping(12..17).collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].range, 10..20);
        assert_eq!(hits[1].range, 15..30);
    }

    #[test]
    fn apply_edit_before_shifts() {
        let t = IntervalTree::new().insert(flag(10..20, StickyBehavior::Shrink));
        // Insert 3 bytes at offset 0.
        let t2 = t.apply_edit(0, 0, 3);
        let iv = t2.iter().next().unwrap();
        assert_eq!(iv.range, 13..23);
    }

    #[test]
    fn apply_edit_after_untouched() {
        let t = IntervalTree::new().insert(flag(10..20, StickyBehavior::Shrink));
        let t2 = t.apply_edit(30, 2, 5);
        let iv = t2.iter().next().unwrap();
        assert_eq!(iv.range, 10..20);
    }

    #[test]
    fn apply_edit_grow_extends() {
        let t = IntervalTree::new().insert(flag(10..20, StickyBehavior::Grow));
        // Replace [15..18) with 6 bytes: delta = +3, grow should cover it.
        let t2 = t.apply_edit(15, 3, 6);
        let iv = t2.iter().next().unwrap();
        assert_eq!(iv.range, 10..23);
    }

    #[test]
    fn apply_edit_shrink_cuts_middle() {
        let t = IntervalTree::new().insert(flag(10..20, StickyBehavior::Shrink));
        // Delete [13..17).
        let t2 = t.apply_edit(13, 4, 0);
        let ranges: Vec<ByteRange> = t2.iter().map(|i| i.range.clone()).collect();
        assert_eq!(ranges, vec![10..13, 13..16]);
    }

    #[test]
    fn apply_edit_split_separates() {
        let t = IntervalTree::new().insert(flag(10..20, StickyBehavior::Split));
        // Insert 2 bytes at offset 15.
        let t2 = t.apply_edit(15, 0, 2);
        let ranges: Vec<ByteRange> = t2.iter().map(|i| i.range.clone()).collect();
        assert_eq!(ranges, vec![10..15, 17..22]);
    }

    #[test]
    fn apply_edit_rear_sticky_insertion_inside() {
        let t = IntervalTree::new().insert(flag(10..20, StickyBehavior::RearSticky));
        // Insert 4 bytes at offset 15 -> extends the rear.
        let t2 = t.apply_edit(15, 0, 4);
        let iv = t2.iter().next().unwrap();
        assert_eq!(iv.range, 10..24);
    }

    #[test]
    fn remove_within_drops_contained() {
        let t = IntervalTree::new()
            .insert(flag(0..5, StickyBehavior::Shrink))
            .insert(flag(10..15, StickyBehavior::Shrink))
            .insert(flag(20..30, StickyBehavior::Shrink));
        let t2 = t.remove_within(8..16);
        let starts: Vec<_> = t2.iter().map(|i| i.range.start).collect();
        assert_eq!(starts, vec![0, 20]);
    }
}
