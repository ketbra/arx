//! Mutable [`Buffer`] built on top of the persistent [`Rope`], plus the
//! cheap-to-clone [`BufferSnapshot`] type that readers hold.
//!
//! Every call to [`Buffer::edit`] produces a new [`Rope`] (sharing structure
//! with the old one) and publishes an immutable [`BufferSnapshot`] that
//! callers can keep past the edit. Snapshots are `Send + Sync` so they can
//! be handed to agents, background jobs, and the renderer without locks.

use std::sync::Arc;

use crate::properties::PropertyMap;
use crate::rope::{ByteRange, Rope, TextSummary};

/// Opaque identifier for a buffer inside a [`BufferManager`]. The wider Arx
/// system assigns these; this crate just plumbs them through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub u64);

/// Who or what initiated an edit. Used for auditing, history grouping, and
/// undo-by-origin queries later in Phase 2/3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOrigin {
    /// A direct user edit via keyboard or command.
    User,
    /// An edit produced by an extension.
    Extension(Arc<str>),
    /// An edit produced by an agent.
    Agent {
        agent_id: u64,
        session_id: u64,
    },
    /// An edit applied from disk reload or similar I/O.
    Io,
    /// An internal or synthetic edit (e.g. OT transform).
    System,
}

/// Description of a single replacement edit, recorded after it has been
/// applied to the rope.
#[derive(Debug, Clone)]
pub struct Edit {
    /// Byte offset at which the replacement started.
    pub offset: usize,
    /// Number of bytes replaced from the pre-edit buffer.
    pub old_len: usize,
    /// Number of bytes inserted.
    pub new_len: usize,
    /// Post-edit buffer version.
    pub version: u64,
    /// Attribution.
    pub origin: EditOrigin,
}

impl Edit {
    /// The range of the edit in the pre-edit buffer coordinate space.
    pub fn pre_range(&self) -> ByteRange {
        self.offset..self.offset + self.old_len
    }

    /// The range of the edit in the post-edit buffer coordinate space.
    pub fn post_range(&self) -> ByteRange {
        self.offset..self.offset + self.new_len
    }

    /// Net change in byte length.
    pub fn delta(&self) -> isize {
        self.new_len as isize - self.old_len as isize
    }
}

/// A buffer: a versioned rope with layered text properties.
#[derive(Debug)]
pub struct Buffer {
    id: BufferId,
    rope: Rope,
    properties: PropertyMap,
    version: u64,
}

impl Buffer {
    /// Create an empty buffer with the given id.
    pub fn new(id: BufferId) -> Self {
        Self {
            id,
            rope: Rope::new(),
            properties: PropertyMap::new(),
            version: 0,
        }
    }

    /// Create a buffer from a starting text blob.
    pub fn from_str(id: BufferId, text: &str) -> Self {
        Self {
            id,
            rope: Rope::from_str(text),
            properties: PropertyMap::new(),
            version: 0,
        }
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn properties(&self) -> &PropertyMap {
        &self.properties
    }

    pub fn properties_mut(&mut self) -> &mut PropertyMap {
        &mut self.properties
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn summary(&self) -> TextSummary {
        self.rope.summary()
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Take an `O(1)` snapshot of the buffer at its current version.
    pub fn snapshot(&self) -> BufferSnapshot {
        BufferSnapshot {
            id: self.id,
            rope: self.rope.clone(),
            version: self.version,
            properties: Arc::new(self.properties.clone()),
        }
    }

    /// Replace `range` with `text`. Increments the version counter and
    /// propagates the edit to every property layer. Returns a description of
    /// the applied edit.
    pub fn edit(&mut self, range: ByteRange, text: &str, origin: EditOrigin) -> Edit {
        let old_len = range.end - range.start;
        let new_len = text.len();
        self.rope = self.rope.edit(range.clone(), text);
        self.version += 1;
        let edit = Edit {
            offset: range.start,
            old_len,
            new_len,
            version: self.version,
            origin,
        };
        self.properties.apply_edit(&edit);
        edit
    }

    /// Replace the buffer contents wholesale (e.g. on file reload).
    pub fn replace_all(&mut self, text: &str, origin: EditOrigin) -> Edit {
        let old_len = self.rope.len_bytes();
        self.edit(0..old_len, text, origin)
    }
}

/// An immutable, cheap-to-clone view of a buffer at a fixed version.
#[derive(Debug, Clone)]
pub struct BufferSnapshot {
    pub id: BufferId,
    pub rope: Rope,
    pub version: u64,
    pub properties: Arc<PropertyMap>,
}

impl BufferSnapshot {
    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn properties(&self) -> &PropertyMap {
        &self.properties
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Materialise a byte range into a freshly allocated string.
    pub fn slice_to_string(&self, range: ByteRange) -> String {
        self.rope.slice_to_string(range)
    }

    /// Extract the full text of line `line` (0-indexed), without its
    /// trailing newline. Out-of-range requests return an empty string.
    pub fn line(&self, line: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        let start = self.rope.line_to_byte(line);
        let end = if line + 1 >= self.rope.len_lines() {
            self.rope.len_bytes()
        } else {
            // Exclude the trailing newline.
            self.rope.line_to_byte(line + 1).saturating_sub(1)
        };
        self.rope.slice_to_string(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval_tree::Interval;
    use crate::properties::{AdjustmentPolicy, PropertyValue, StickyBehavior};

    #[test]
    fn snapshot_is_independent_of_later_edits() {
        let mut buf = Buffer::from_str(BufferId(1), "hello world");
        let snap_a = buf.snapshot();
        buf.edit(6..11, "rope!", EditOrigin::User);
        assert_eq!(snap_a.text(), "hello world");
        assert_eq!(buf.text(), "hello rope!");
        assert_eq!(snap_a.version(), 0);
        assert_eq!(buf.version(), 1);
    }

    #[test]
    fn edit_tracks_properties() {
        let mut buf = Buffer::from_str(BufferId(1), "abcdefghij");
        buf.properties_mut()
            .ensure_layer("diag", AdjustmentPolicy::TrackEdits)
            .insert(Interval::new(
                5..9,
                PropertyValue::Flag,
                StickyBehavior::RearSticky,
            ));

        // Insert 3 bytes at offset 2.
        buf.edit(2..2, "XYZ", EditOrigin::System);
        let iv = buf
            .properties()
            .layer("diag")
            .unwrap()
            .tree()
            .iter()
            .next()
            .unwrap();
        assert_eq!(iv.range, 8..12);
    }

    #[test]
    fn invalidate_policy_marks_dirty() {
        let mut buf = Buffer::from_str(BufferId(1), "pub fn foo() {}");
        buf.properties_mut()
            .ensure_layer("syntax", AdjustmentPolicy::InvalidateOnEdit)
            .insert(Interval::new(
                0..3,
                PropertyValue::Scope("keyword".into()),
                StickyBehavior::RearSticky,
            ));
        buf.edit(7..10, "bar", EditOrigin::User);
        let dirty = buf.properties().layer("syntax").unwrap().dirty_ranges();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0], 7..10);
    }

    #[test]
    fn line_access() {
        let buf = Buffer::from_str(BufferId(1), "alpha\nbeta\ngamma");
        let snap = buf.snapshot();
        assert_eq!(snap.line(0), "alpha");
        assert_eq!(snap.line(1), "beta");
        assert_eq!(snap.line(2), "gamma");
        assert_eq!(snap.line(3), "");
    }

    #[test]
    fn replace_all_produces_single_edit() {
        let mut buf = Buffer::from_str(BufferId(1), "old contents");
        let e = buf.replace_all("new", EditOrigin::Io);
        assert_eq!(e.offset, 0);
        assert_eq!(e.old_len, 12);
        assert_eq!(e.new_len, 3);
        assert_eq!(buf.text(), "new");
        assert_eq!(buf.version(), 1);
    }
}
