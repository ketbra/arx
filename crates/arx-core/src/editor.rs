//! [`Editor`] state and the [`BufferManager`] that owns open buffers.
//!
//! `Editor` is the single-writer state container that lives entirely on the
//! event loop's task. Anything inside it is reachable only with `&mut`,
//! which we get exclusively from inside a [`crate::CommandBus`] dispatch.
//!
//! `BufferManager` is the part of the editor that holds open buffers and
//! publishes their snapshots to any number of readers via per-buffer
//! [`tokio::sync::watch`] channels — see `docs/spec.md` §3.4.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use arx_buffer::{Buffer, BufferId, BufferSnapshot, ByteRange, Edit, EditOrigin};
use tokio::sync::watch;

/// The editor's in-process state.
///
/// Owns every piece of mutable editor state today. Lives on the event loop
/// task only — never shared across threads — so it doesn't need to be
/// `Sync` (and isn't, deliberately, so we catch accidental cross-task use
/// at compile time).
#[derive(Debug, Default)]
pub struct Editor {
    buffers: BufferManager,
}

impl Editor {
    /// Create an empty editor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrow the [`BufferManager`].
    pub fn buffers(&self) -> &BufferManager {
        &self.buffers
    }

    /// Mutably borrow the [`BufferManager`].
    pub fn buffers_mut(&mut self) -> &mut BufferManager {
        &mut self.buffers
    }
}

// ---------------------------------------------------------------------------
// BufferManager
// ---------------------------------------------------------------------------

/// Owns every open buffer and publishes immutable snapshots to subscribers.
///
/// Each open buffer holds a [`watch::Sender`] of its current snapshot;
/// readers obtain a [`watch::Receiver`] via [`BufferManager::subscribe`] and
/// observe edits without taking any locks. The single-writer model is
/// preserved because mutations only happen via this struct's `&mut self`
/// methods, and the only way to get a `&mut BufferManager` is from inside
/// the event loop task.
#[derive(Debug, Default)]
pub struct BufferManager {
    next_id: u64,
    entries: HashMap<BufferId, BufferEntry>,
    paths: HashMap<PathBuf, BufferId>,
}

#[derive(Debug)]
struct BufferEntry {
    buffer: Buffer,
    path: Option<PathBuf>,
    snapshot_tx: watch::Sender<BufferSnapshot>,
}

impl BufferManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of open buffers.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no buffers are open.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over the IDs of every open buffer.
    pub fn ids(&self) -> impl Iterator<Item = BufferId> + '_ {
        self.entries.keys().copied()
    }

    /// Borrow the [`Buffer`] for `id`, if it exists.
    pub fn get(&self, id: BufferId) -> Option<&Buffer> {
        self.entries.get(&id).map(|e| &e.buffer)
    }

    /// Mutably borrow the [`Buffer`] for `id`, if it exists.
    ///
    /// Mutating the buffer through this handle bypasses snapshot
    /// publishing — prefer the higher-level methods on this struct
    /// ([`BufferManager::edit`], etc.) so subscribers always see updates.
    pub fn get_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.entries.get_mut(&id).map(|e| &mut e.buffer)
    }

    /// Take an `O(1)` snapshot of the buffer for `id`.
    pub fn snapshot(&self, id: BufferId) -> Option<BufferSnapshot> {
        self.entries.get(&id).map(|e| e.buffer.snapshot())
    }

    /// Subscribe to snapshot updates for `id`. The receiver yields the
    /// current snapshot immediately on the first call to
    /// [`tokio::sync::watch::Receiver::borrow`].
    pub fn subscribe(&self, id: BufferId) -> Option<watch::Receiver<BufferSnapshot>> {
        self.entries.get(&id).map(|e| e.snapshot_tx.subscribe())
    }

    /// The path associated with `id`, if any.
    pub fn path(&self, id: BufferId) -> Option<&Path> {
        self.entries.get(&id).and_then(|e| e.path.as_deref())
    }

    /// Look up an open buffer by absolute path.
    pub fn find_by_path(&self, path: &Path) -> Option<BufferId> {
        self.paths.get(path).copied()
    }

    /// Create a new empty scratch buffer.
    pub fn create_scratch(&mut self) -> BufferId {
        let id = self.allocate_id();
        let buffer = Buffer::new(id);
        self.insert(id, buffer, None);
        id
    }

    /// Create a buffer from `text`, optionally associated with `path`.
    ///
    /// If `path` is `Some` and another buffer already maps to it, the new
    /// buffer is created anyway but the path index is overwritten — callers
    /// that need de-duplication should check [`Self::find_by_path`] first.
    pub fn create_from_text(&mut self, text: &str, path: Option<PathBuf>) -> BufferId {
        let id = self.allocate_id();
        let buffer = Buffer::from_str(id, text);
        self.insert(id, buffer, path);
        id
    }

    /// Apply an edit to a buffer and publish the new snapshot to all
    /// subscribers. Returns `None` if no buffer with `id` exists.
    pub fn edit(
        &mut self,
        id: BufferId,
        range: ByteRange,
        text: &str,
        origin: EditOrigin,
    ) -> Option<Edit> {
        let entry = self.entries.get_mut(&id)?;
        let edit = entry.buffer.edit(range, text, origin);
        // `send_replace` always succeeds, even if no receivers are alive
        // right now. New subscribers via `subscribe()` see the latest
        // value, so we never need to special-case "no listeners".
        entry.snapshot_tx.send_replace(entry.buffer.snapshot());
        Some(edit)
    }

    /// Replace the buffer's contents wholesale (e.g. on disk reload).
    pub fn replace_all(
        &mut self,
        id: BufferId,
        text: &str,
        origin: EditOrigin,
    ) -> Option<Edit> {
        let entry = self.entries.get_mut(&id)?;
        let edit = entry.buffer.replace_all(text, origin);
        entry.snapshot_tx.send_replace(entry.buffer.snapshot());
        Some(edit)
    }

    /// Close the buffer with `id`, dropping its snapshot publisher. Any
    /// outstanding subscribers see the channel close. Returns `true` if a
    /// buffer was actually removed.
    pub fn close(&mut self, id: BufferId) -> bool {
        if let Some(entry) = self.entries.remove(&id) {
            if let Some(path) = entry.path {
                self.paths.remove(&path);
            }
            true
        } else {
            false
        }
    }

    fn insert(&mut self, id: BufferId, buffer: Buffer, path: Option<PathBuf>) {
        let snapshot = buffer.snapshot();
        // Drop the initial receiver immediately — `send_replace` works
        // without listeners, and `subscribe()` re-attaches new ones.
        let (snapshot_tx, _) = watch::channel(snapshot);
        if let Some(ref p) = path {
            self.paths.insert(p.clone(), id);
        }
        self.entries.insert(
            id,
            BufferEntry {
                buffer,
                path,
                snapshot_tx,
            },
        );
    }

    fn allocate_id(&mut self) -> BufferId {
        self.next_id += 1;
        BufferId(self.next_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_get_scratch() {
        let mut mgr = BufferManager::new();
        let id = mgr.create_scratch();
        assert_eq!(mgr.len(), 1);
        let buf = mgr.get(id).expect("scratch buffer");
        assert_eq!(buf.text(), "");
    }

    #[test]
    fn create_from_text_with_path_indexes_lookup() {
        let mut mgr = BufferManager::new();
        let id = mgr.create_from_text("hello", Some(PathBuf::from("/tmp/x.txt")));
        assert_eq!(mgr.find_by_path(Path::new("/tmp/x.txt")), Some(id));
        assert_eq!(mgr.path(id), Some(Path::new("/tmp/x.txt")));
        assert_eq!(mgr.get(id).unwrap().text(), "hello");
    }

    #[test]
    fn edit_publishes_snapshot_to_subscribers() {
        let mut mgr = BufferManager::new();
        let id = mgr.create_from_text("hello", None);
        let mut rx = mgr.subscribe(id).unwrap();
        // Initial value visible immediately.
        assert_eq!(rx.borrow_and_update().text(), "hello");

        mgr.edit(id, 5..5, " world", EditOrigin::User);
        // Mark the new version unread → borrow → check.
        assert!(rx.has_changed().unwrap_or(false));
        assert_eq!(rx.borrow_and_update().text(), "hello world");
    }

    #[test]
    fn snapshot_survives_buffer_mutation() {
        let mut mgr = BufferManager::new();
        let id = mgr.create_from_text("abc", None);
        let snap_before = mgr.snapshot(id).unwrap();
        mgr.edit(id, 1..2, "X", EditOrigin::User);
        assert_eq!(snap_before.text(), "abc");
        assert_eq!(mgr.get(id).unwrap().text(), "aXc");
    }

    #[test]
    fn close_removes_path_index() {
        let mut mgr = BufferManager::new();
        let id = mgr.create_from_text("x", Some(PathBuf::from("/tmp/y.rs")));
        assert!(mgr.close(id));
        assert_eq!(mgr.find_by_path(Path::new("/tmp/y.rs")), None);
        assert!(!mgr.close(id));
    }

    #[test]
    fn replace_all_publishes_too() {
        let mut mgr = BufferManager::new();
        let id = mgr.create_from_text("old", None);
        let mut rx = mgr.subscribe(id).unwrap();
        rx.borrow_and_update();

        mgr.replace_all(id, "new contents", EditOrigin::Io);
        assert_eq!(rx.borrow_and_update().text(), "new contents");
    }

    #[test]
    fn ids_are_unique_and_monotonic() {
        let mut mgr = BufferManager::new();
        let a = mgr.create_scratch();
        let b = mgr.create_scratch();
        assert_ne!(a, b);
        assert!(b.0 > a.0);
    }
}
