//! Layered text properties.
//!
//! Properties are stored outside the rope in a [`PropertyMap`] composed of
//! named layers. Each layer carries a persistent interval tree plus a policy
//! describing how it reacts to buffer edits:
//!
//! * [`AdjustmentPolicy::TrackEdits`] — ranges are shifted/shrunk/extended
//!   following the sticky behaviour of each interval.
//! * [`AdjustmentPolicy::InvalidateOnEdit`] — the affected range is added to
//!   the layer's dirty list for later recomputation (e.g. Tree-sitter).
//! * [`AdjustmentPolicy::Static`] — the layer is never touched automatically
//!   (useful for snapshot annotations and search highlights).
//!
//! Rendering calls [`PropertyMap::styled_runs`] which intersects every
//! visible layer against a query range, merges the resulting [`Face`]s by
//! priority, and yields a stream of contiguous [`StyledRun`]s.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::buffer::Edit;
use crate::interval_tree::{Interval, IntervalTree};
use crate::rope::ByteRange;

pub type LayerId = String;

// ---------------------------------------------------------------------------
// Property values
// ---------------------------------------------------------------------------

/// Unique identifier for an agent author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub u64);

/// Severity classification for diagnostics, mirroring LSP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A diagnostic attached to a range of text.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: Arc<str>,
    pub code: Option<Arc<str>>,
    pub source: Option<Arc<str>>,
}

/// Underline rendering style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineStyle {
    Straight,
    Curly,
    Dashed,
    Dotted,
    Double,
}

/// A rendering face. Fields are `Option`s so layers can contribute only the
/// attributes they care about; unset fields are inherited from lower-priority
/// layers during merging.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Face {
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<UnderlineStyle>,
    pub strikethrough: Option<bool>,
    pub priority: i16,
}

impl Face {
    /// Merge `other` on top of `self`. Attributes with `other.priority >= self.priority`
    /// take precedence; otherwise the incumbent value wins.
    pub fn merge_over(&mut self, other: &Face) {
        let take = other.priority >= self.priority;
        if take {
            if other.fg.is_some() {
                self.fg = other.fg;
            }
            if other.bg.is_some() {
                self.bg = other.bg;
            }
            if other.bold.is_some() {
                self.bold = other.bold;
            }
            if other.italic.is_some() {
                self.italic = other.italic;
            }
            if other.underline.is_some() {
                self.underline = other.underline;
            }
            if other.strikethrough.is_some() {
                self.strikethrough = other.strikethrough;
            }
            self.priority = other.priority;
        } else {
            self.fg = self.fg.or(other.fg);
            self.bg = self.bg.or(other.bg);
            self.bold = self.bold.or(other.bold);
            self.italic = self.italic.or(other.italic);
            self.underline = self.underline.or(other.underline);
            self.strikethrough = self.strikethrough.or(other.strikethrough);
        }
    }
}

/// Bit flags for fast per-run property lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PropertyFlags(u32);

#[allow(dead_code)]
impl PropertyFlags {
    pub const READONLY: Self = Self(0b0000_0001);
    pub const SEARCH_MATCH: Self = Self(0b0000_0010);
    pub const SELECTION: Self = Self(0b0000_0100);
    pub const DIAGNOSTIC: Self = Self(0b0000_1000);
    pub const FOLDED: Self = Self(0b0001_0000);
    pub const AGENT_EDIT: Self = Self(0b0010_0000);
    pub const LINK: Self = Self(0b0100_0000);

    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn bits(self) -> u32 {
        self.0
    }
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for PropertyFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitAnd for PropertyFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}
impl std::ops::BitOrAssign for PropertyFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
impl std::ops::BitAndAssign for PropertyFlags {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

/// Values stored in a property interval.
#[derive(Debug, Clone)]
pub enum PropertyValue {
    /// A scoped syntax classification.
    Scope(Arc<str>),
    /// A diagnostic message.
    Diagnostic(Arc<Diagnostic>),
    /// A rendering decoration.
    Decoration(Face),
    /// A presence flag (used with sticky `RearSticky` to mark regions).
    Flag,
    /// Attribution of text to an agent edit.
    AgentAttribution {
        agent: AgentId,
        edit_id: u64,
    },
    /// A clickable link with a URI.
    Link(Arc<str>),
    /// A read-only region.
    ReadOnly,
}

// ---------------------------------------------------------------------------
// Sticky behaviour (mirrors spec §3.9)
// ---------------------------------------------------------------------------

/// How a property interval reacts when an edit touches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StickyBehavior {
    /// Insertions at boundaries extend the property.
    Grow,
    /// Insertions at the start stay outside; at the end they extend.
    #[default]
    RearSticky,
    /// Insertions at boundaries never extend; overlaps truncate.
    Shrink,
    /// An insertion inside the property splits it in two.
    Split,
}

// ---------------------------------------------------------------------------
// Adjustment policies
// ---------------------------------------------------------------------------

/// How a [`PropertyLayer`] reacts to buffer edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentPolicy {
    /// Shift/shrink/extend ranges in-place. Used for most interactive layers.
    TrackEdits,
    /// Invalidate overlapping ranges; producer will recompute them.
    InvalidateOnEdit,
    /// Never touched — caller manages lifecycle manually.
    Static,
}

// ---------------------------------------------------------------------------
// Layer
// ---------------------------------------------------------------------------

/// A single named layer of properties.
#[derive(Debug, Clone)]
pub struct PropertyLayer {
    tree: IntervalTree,
    adjustment: AdjustmentPolicy,
    synced_to: u64,
    dirty: Vec<ByteRange>,
}

impl PropertyLayer {
    pub fn new(adjustment: AdjustmentPolicy) -> Self {
        Self {
            tree: IntervalTree::new(),
            adjustment,
            synced_to: 0,
            dirty: Vec::new(),
        }
    }

    pub fn adjustment(&self) -> AdjustmentPolicy {
        self.adjustment
    }

    pub fn tree(&self) -> &IntervalTree {
        &self.tree
    }

    pub fn synced_to(&self) -> u64 {
        self.synced_to
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    pub fn dirty_ranges(&self) -> &[ByteRange] {
        &self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    pub fn insert(&mut self, interval: Interval) {
        self.tree = self.tree.insert(interval);
    }

    pub fn clear(&mut self) {
        self.tree = self.tree.clear();
        self.dirty.clear();
    }

    pub fn remove_within(&mut self, range: ByteRange) {
        self.tree = self.tree.remove_within(range);
    }

    pub(crate) fn apply_edit(&mut self, edit: &Edit) {
        match self.adjustment {
            AdjustmentPolicy::TrackEdits => {
                self.tree = self.tree.apply_edit(edit.offset, edit.old_len, edit.new_len);
            }
            AdjustmentPolicy::InvalidateOnEdit => {
                let end = edit.offset + edit.new_len;
                self.dirty.push(edit.offset..end);
            }
            AdjustmentPolicy::Static => {}
        }
        self.synced_to = edit.version;
    }

    pub fn overlapping(&self, range: ByteRange) -> impl Iterator<Item = &Interval> + '_ {
        self.tree.overlapping(range)
    }
}

// ---------------------------------------------------------------------------
// Map
// ---------------------------------------------------------------------------

/// A collection of [`PropertyLayer`]s keyed by [`LayerId`].
#[derive(Debug, Clone, Default)]
pub struct PropertyMap {
    layers: BTreeMap<LayerId, PropertyLayer>,
}

impl PropertyMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn layer(&self, id: &str) -> Option<&PropertyLayer> {
        self.layers.get(id)
    }

    pub fn layer_mut(&mut self, id: &str) -> Option<&mut PropertyLayer> {
        self.layers.get_mut(id)
    }

    /// Get an existing layer, or insert a new one with `adjustment`.
    pub fn ensure_layer(&mut self, id: impl Into<LayerId>, adjustment: AdjustmentPolicy) -> &mut PropertyLayer {
        self.layers
            .entry(id.into())
            .or_insert_with(|| PropertyLayer::new(adjustment))
    }

    pub fn remove_layer(&mut self, id: &str) -> Option<PropertyLayer> {
        self.layers.remove(id)
    }

    pub fn layers(&self) -> impl Iterator<Item = (&LayerId, &PropertyLayer)> {
        self.layers.iter()
    }

    pub fn layer_ids(&self) -> impl Iterator<Item = &LayerId> {
        self.layers.keys()
    }

    pub(crate) fn apply_edit(&mut self, edit: &Edit) {
        for layer in self.layers.values_mut() {
            layer.apply_edit(edit);
        }
    }

    /// Produce a contiguous stream of [`StyledRun`]s covering `range`,
    /// merging faces from every layer by priority.
    pub fn styled_runs(&self, range: ByteRange) -> Vec<StyledRun> {
        // 1. Gather every boundary (start/end) that falls inside the query.
        let mut boundaries: Vec<usize> = vec![range.start, range.end];
        let mut relevant: Vec<&Interval> = Vec::new();
        for layer in self.layers.values() {
            for iv in layer.overlapping(range.clone()) {
                boundaries.push(iv.range.start.max(range.start));
                boundaries.push(iv.range.end.min(range.end));
                relevant.push(iv);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        // 2. For each [b_i, b_{i+1}) compute the merged face and flags.
        let mut runs = Vec::with_capacity(boundaries.len().saturating_sub(1));
        for window in boundaries.windows(2) {
            let span = window[0]..window[1];
            if span.start == span.end {
                continue;
            }
            let mut face = Face::default();
            let mut flags = PropertyFlags::empty();
            let mut diagnostics: Vec<Arc<Diagnostic>> = Vec::new();
            for iv in &relevant {
                if iv.range.start >= span.end || iv.range.end <= span.start {
                    continue;
                }
                apply_value(&iv.value, &mut face, &mut flags, &mut diagnostics);
            }
            runs.push(StyledRun {
                range: span,
                face,
                flags,
                diagnostics,
            });
        }
        runs
    }
}

fn apply_value(
    value: &PropertyValue,
    face: &mut Face,
    flags: &mut PropertyFlags,
    diagnostics: &mut Vec<Arc<Diagnostic>>,
) {
    match value {
        PropertyValue::Decoration(f) => {
            face.merge_over(f);
        }
        PropertyValue::Diagnostic(d) => {
            flags.insert(PropertyFlags::DIAGNOSTIC);
            diagnostics.push(d.clone());
        }
        PropertyValue::ReadOnly => {
            flags.insert(PropertyFlags::READONLY);
        }
        PropertyValue::Link(_) => {
            flags.insert(PropertyFlags::LINK);
        }
        PropertyValue::AgentAttribution { .. } => {
            flags.insert(PropertyFlags::AGENT_EDIT);
        }
        PropertyValue::Scope(_) | PropertyValue::Flag => {}
    }
}

/// A contiguous run of identically-styled text produced by
/// [`PropertyMap::styled_runs`].
#[derive(Debug, Clone)]
pub struct StyledRun {
    pub range: ByteRange,
    pub face: Face,
    pub flags: PropertyFlags,
    pub diagnostics: Vec<Arc<Diagnostic>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval_tree::Interval;

    fn decoration(priority: i16, bold: bool) -> PropertyValue {
        PropertyValue::Decoration(Face {
            bold: Some(bold),
            priority,
            ..Default::default()
        })
    }

    #[test]
    fn ensure_layer_creates_once() {
        let mut map = PropertyMap::new();
        map.ensure_layer("syntax", AdjustmentPolicy::InvalidateOnEdit);
        map.ensure_layer("syntax", AdjustmentPolicy::InvalidateOnEdit);
        assert_eq!(map.layers().count(), 1);
    }

    #[test]
    fn styled_runs_merges_priority() {
        let mut map = PropertyMap::new();
        let layer = map.ensure_layer("syntax", AdjustmentPolicy::TrackEdits);
        layer.insert(Interval::new(0..10, decoration(0, false), StickyBehavior::RearSticky));
        layer.insert(Interval::new(4..8, decoration(10, true), StickyBehavior::RearSticky));

        let runs = map.styled_runs(0..10);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].range, 0..4);
        assert_eq!(runs[0].face.bold, Some(false));
        assert_eq!(runs[1].range, 4..8);
        assert_eq!(runs[1].face.bold, Some(true));
        assert_eq!(runs[2].range, 8..10);
        assert_eq!(runs[2].face.bold, Some(false));
    }

    #[test]
    fn flags_surface_in_runs() {
        let mut map = PropertyMap::new();
        let layer = map.ensure_layer("edits", AdjustmentPolicy::TrackEdits);
        layer.insert(Interval::new(
            5..15,
            PropertyValue::AgentAttribution {
                agent: AgentId(1),
                edit_id: 42,
            },
            StickyBehavior::RearSticky,
        ));
        layer.insert(Interval::new(
            10..12,
            PropertyValue::ReadOnly,
            StickyBehavior::RearSticky,
        ));

        let runs = map.styled_runs(0..20);
        let agent_run = runs
            .iter()
            .find(|r| r.range.start == 5 && r.range.end == 10)
            .unwrap();
        assert!(agent_run.flags.contains(PropertyFlags::AGENT_EDIT));
        assert!(!agent_run.flags.contains(PropertyFlags::READONLY));

        let both_run = runs
            .iter()
            .find(|r| r.range.start == 10 && r.range.end == 12)
            .unwrap();
        assert!(both_run.flags.contains(PropertyFlags::AGENT_EDIT));
        assert!(both_run.flags.contains(PropertyFlags::READONLY));
    }

    #[test]
    fn apply_edit_invalidates_layer() {
        let mut map = PropertyMap::new();
        let layer = map.ensure_layer("syntax", AdjustmentPolicy::InvalidateOnEdit);
        layer.insert(Interval::new(
            0..5,
            PropertyValue::Scope(Arc::from("keyword")),
            StickyBehavior::RearSticky,
        ));
        map.apply_edit(&Edit {
            offset: 2,
            old_len: 0,
            new_len: 3,
            version: 1,
            origin: crate::buffer::EditOrigin::User,
        });
        let syntax = map.layer("syntax").unwrap();
        assert_eq!(syntax.dirty_ranges().len(), 1);
        // The layer's tree is not adjusted under InvalidateOnEdit.
        assert_eq!(syntax.tree().iter().count(), 1);
    }

    #[test]
    fn apply_edit_tracks_layer() {
        let mut map = PropertyMap::new();
        let layer = map.ensure_layer("diagnostics", AdjustmentPolicy::TrackEdits);
        layer.insert(Interval::new(
            10..15,
            PropertyValue::Flag,
            StickyBehavior::RearSticky,
        ));
        map.apply_edit(&Edit {
            offset: 0,
            old_len: 0,
            new_len: 4,
            version: 1,
            origin: crate::buffer::EditOrigin::User,
        });
        let iv = map.layer("diagnostics").unwrap().tree().iter().next().unwrap();
        assert_eq!(iv.range, 14..19);
    }
}
