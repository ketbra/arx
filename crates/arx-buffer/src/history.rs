//! Persistent undo **tree** (not stack) for a [`crate::Buffer`].
//!
//! A flat undo stack throws away divergent history the moment you undo
//! and then type something new: the "redo" branch is gone forever.
//! An undo *tree* preserves every edit as a node in a rooted tree, so
//! typing into an undone state creates a *new branch* alongside the
//! old one. You can still navigate back to the old branch via
//! [`UndoTree::branch_next`] / [`UndoTree::branch_prev`] and replay it
//! with [`UndoTree::redo`]. Emacs's `undo-tree.el` and Vim's built-in
//! `:undolist` work the same way.
//!
//! ## Model
//!
//! The tree is rooted at a single node representing the buffer state
//! at construction time (an empty history). Each non-root node stores
//! the [`EditRecord`] that would produce it from its parent. Walking
//! the tree forward (redo) means applying the child edge's record as
//! a forward edit; walking backward (undo) means applying the inverse
//! of the current edge — i.e. replacing the *inserted* bytes with the
//! *removed* bytes that the record saved.
//!
//! Every node tracks its `last_active_child`: the most recently-
//! visited child. [`UndoTree::redo`] follows that pointer so the user
//! ends up back where they were before the last undo, even after a
//! branch switch. [`UndoTree::push`] updates the parent's
//! `last_active_child` to the newly-pushed node.
//!
//! ## What this crate records vs. what it applies
//!
//! [`UndoTree`] is a pure data structure. It never touches the
//! [`crate::Buffer`]. Callers (in `arx-core`'s stock edit commands)
//! push records into the tree *after* applying a user-initiated edit
//! to the buffer, and invert/reapply them by calling
//! [`crate::Buffer::edit`] directly on the [`EditRecord`] returned by
//! [`UndoTree::undo`] / [`UndoTree::redo`].
//!
//! Keeping the rope-mutation and tree-bookkeeping separate means the
//! buffer layer doesn't grow a cursor or window concept (undo records
//! carry cursor offsets, which is a `Editor` concern) and the undo
//! tree isn't involuntarily modified by `Io` or `System` edits that
//! shouldn't participate in user-visible history.

use std::time::SystemTime;

/// Opaque identifier for a node inside an [`UndoTree`]. Stable for
/// the life of the tree; node ids are allocated monotonically and
/// never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UndoNodeId(pub usize);

/// All the information needed to apply *or invert* a single edit.
///
/// `removed` is the text that was at `offset..offset+removed.len()`
/// in the *pre-edit* buffer. `inserted` is the text that replaced it
/// (length = post-edit span). Together they let the tree walk an edit
/// in either direction without consulting the buffer.
///
/// `cursor_before` / `cursor_after` record the invoking window's
/// cursor byte offset at the moment the edit was submitted and after
/// the command that produced it finished, respectively. Undo sets
/// the active cursor to `cursor_before`; redo sets it to
/// `cursor_after`. They're `usize` offsets into the rope (not
/// per-window — the caller decides which window to apply them to).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRecord {
    /// Byte offset of the edit in the *pre-edit* buffer.
    pub offset: usize,
    /// Bytes that were replaced (empty for pure insertions).
    pub removed: String,
    /// Bytes that were inserted (empty for pure deletions).
    pub inserted: String,
    /// Where the primary cursor sat before the edit.
    pub cursor_before: usize,
    /// Where the primary cursor sat after the edit.
    pub cursor_after: usize,
    /// When the edit was submitted. Used for future "revert to state
    /// from N minutes ago" UIs; unused by the tree itself.
    pub timestamp: SystemTime,
}

impl EditRecord {
    /// The range this edit replaced in the pre-edit buffer.
    pub fn pre_range(&self) -> std::ops::Range<usize> {
        self.offset..self.offset + self.removed.len()
    }

    /// The range the insertion occupies in the post-edit buffer.
    pub fn post_range(&self) -> std::ops::Range<usize> {
        self.offset..self.offset + self.inserted.len()
    }
}

#[derive(Debug)]
struct UndoNode {
    parent: Option<UndoNodeId>,
    children: Vec<UndoNodeId>,
    /// The child that [`UndoTree::redo`] will follow from here. Set
    /// when a node is pushed under this one, and rotated by
    /// [`UndoTree::branch_next`] / [`UndoTree::branch_prev`].
    last_active_child: Option<UndoNodeId>,
    /// The edit record on the edge *into* this node (from its
    /// parent). `None` only at the root.
    edit: Option<EditRecord>,
}

/// The buffer's undo tree.
#[derive(Debug)]
pub struct UndoTree {
    nodes: Vec<UndoNode>,
    current: UndoNodeId,
}

impl Default for UndoTree {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoTree {
    /// Create a fresh tree containing only the root (empty history).
    pub fn new() -> Self {
        Self {
            nodes: vec![UndoNode {
                parent: None,
                children: Vec::new(),
                last_active_child: None,
                edit: None,
            }],
            current: UndoNodeId(0),
        }
    }

    /// The node the user is "currently at". Buffer content matches
    /// this node's worldline: applying the chain of edits from the
    /// root to here would reproduce the current buffer.
    pub fn current(&self) -> UndoNodeId {
        self.current
    }

    /// Root node of the tree. Its `edit` is always `None`.
    pub fn root(&self) -> UndoNodeId {
        UndoNodeId(0)
    }

    /// Total number of nodes allocated, including the root. Handy
    /// for tests and debugging; a fresh tree reports 1.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether this tree has only the root node (no edits yet).
    pub fn is_empty(&self) -> bool {
        self.nodes.len() == 1
    }

    /// Can [`UndoTree::undo`] move the cursor anywhere? `false` at the
    /// root.
    pub fn can_undo(&self) -> bool {
        self.nodes[self.current.0].parent.is_some()
    }

    /// Can [`UndoTree::redo`] move the cursor anywhere? `false` when
    /// `current` has no `last_active_child` (leaf, or branch cycled
    /// off the end).
    pub fn can_redo(&self) -> bool {
        self.nodes[self.current.0].last_active_child.is_some()
    }

    /// Children of the current node, in insertion order. Multiple
    /// children exist after an undo-and-new-edit sequence creates
    /// a branch alongside an older worldline.
    pub fn current_children(&self) -> &[UndoNodeId] {
        &self.nodes[self.current.0].children
    }

    /// Borrow a node by id. `None` for ids that never existed.
    #[cfg(test)]
    fn node(&self, id: UndoNodeId) -> Option<&UndoNode> {
        self.nodes.get(id.0)
    }

    /// The edit that would be inverted on the next [`UndoTree::undo`],
    /// without actually performing it. Useful for UIs that want to
    /// show a "will undo: insert(...)" hint.
    pub fn peek_undo(&self) -> Option<&EditRecord> {
        self.nodes[self.current.0].edit.as_ref()
    }

    /// The edit that would be re-applied on the next
    /// [`UndoTree::redo`], without actually performing it.
    pub fn peek_redo(&self) -> Option<&EditRecord> {
        let child = self.nodes[self.current.0].last_active_child?;
        self.nodes[child.0].edit.as_ref()
    }

    /// Append `record` as a new child of the current node and make it
    /// current. The old `last_active_child` of the parent is
    /// overwritten with the new node, so a subsequent
    /// [`UndoTree::redo`] (after undoing) replays this branch — but
    /// older branches aren't deleted, just unreachable via plain
    /// redo. [`UndoTree::branch_prev`] / [`UndoTree::branch_next`]
    /// bring them back.
    ///
    /// Returns the id of the newly-created node.
    pub fn push(&mut self, record: EditRecord) -> UndoNodeId {
        let new_id = UndoNodeId(self.nodes.len());
        let parent_id = self.current;
        self.nodes.push(UndoNode {
            parent: Some(parent_id),
            children: Vec::new(),
            last_active_child: None,
            edit: Some(record),
        });
        let parent = &mut self.nodes[parent_id.0];
        parent.children.push(new_id);
        parent.last_active_child = Some(new_id);
        self.current = new_id;
        new_id
    }

    /// Move `current` up to the parent and return a clone of the
    /// [`EditRecord`] that was on the edge we just walked. The caller
    /// should *invert* that record against the buffer (replace the
    /// inserted bytes with the removed bytes).
    ///
    /// Returns `None` when `current` is already the root — the caller
    /// should treat that as "nothing more to undo".
    pub fn undo(&mut self) -> Option<EditRecord> {
        let parent = self.nodes[self.current.0].parent?;
        let walked_from = self.current.0;
        self.current = parent;
        self.nodes[walked_from].edit.clone()
    }

    /// Move `current` down to `last_active_child` and return a clone
    /// of the edit record on the edge we just walked. The caller
    /// should *re-apply* that record against the buffer.
    ///
    /// Returns `None` when `current` has no `last_active_child` — the
    /// user is at a leaf (or at a node whose descent history has
    /// been cleared by [`UndoTree::branch_prev`] off the end).
    pub fn redo(&mut self) -> Option<EditRecord> {
        let child = self.nodes[self.current.0].last_active_child?;
        self.current = child;
        self.nodes[child.0].edit.clone()
    }

    /// Rotate the current node's `last_active_child` pointer one
    /// position forward, so the next [`UndoTree::redo`] follows a
    /// different branch. No-op (returns `false`) when the current
    /// node has fewer than two children.
    pub fn branch_next(&mut self) -> bool {
        self.rotate_last_active(1)
    }

    /// Rotate the current node's `last_active_child` pointer one
    /// position backward.
    pub fn branch_prev(&mut self) -> bool {
        self.rotate_last_active(-1)
    }

    fn rotate_last_active(&mut self, delta: isize) -> bool {
        let node = &mut self.nodes[self.current.0];
        if node.children.len() <= 1 {
            return false;
        }
        let count = node.children.len() as isize;
        let current_idx = match node.last_active_child {
            Some(id) => node
                .children
                .iter()
                .position(|c| *c == id)
                .map_or(0, |i| i as isize),
            None => 0,
        };
        let new_idx = (((current_idx + delta) % count) + count) % count;
        node.last_active_child = Some(node.children[new_idx as usize]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        offset: usize,
        removed: &str,
        inserted: &str,
        cursor_before: usize,
        cursor_after: usize,
    ) -> EditRecord {
        EditRecord {
            offset,
            removed: removed.to_owned(),
            inserted: inserted.to_owned(),
            cursor_before,
            cursor_after,
            timestamp: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn fresh_tree_is_root_only() {
        let tree = UndoTree::new();
        assert!(tree.is_empty());
        assert!(!tree.can_undo());
        assert!(!tree.can_redo());
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.current(), tree.root());
    }

    #[test]
    fn push_creates_a_child_and_advances_current() {
        let mut tree = UndoTree::new();
        let a = tree.push(rec(0, "", "a", 0, 1));
        assert_eq!(tree.current(), a);
        assert_eq!(tree.len(), 2);
        assert!(tree.can_undo());
        assert!(!tree.can_redo());
    }

    #[test]
    fn undo_returns_record_and_rewinds() {
        let mut tree = UndoTree::new();
        let _a = tree.push(rec(0, "", "a", 0, 1));
        let _b = tree.push(rec(1, "", "b", 1, 2));
        let undone = tree.undo().unwrap();
        assert_eq!(undone.inserted, "b");
        assert!(tree.can_redo(), "redo available after undo");
    }

    #[test]
    fn redo_replays_last_active_branch() {
        let mut tree = UndoTree::new();
        let _a = tree.push(rec(0, "", "a", 0, 1));
        let b = tree.push(rec(1, "", "b", 1, 2));
        tree.undo(); // current = a
        let replayed = tree.redo().unwrap();
        assert_eq!(replayed.inserted, "b");
        assert_eq!(tree.current(), b);
    }

    #[test]
    fn new_edit_after_undo_creates_a_sibling_branch() {
        // 'a' then 'b', undo 'b', push 'c'. The new node `c` is a
        // sibling of `b` under `a`. Both branches are reachable from
        // `a` via branch_next/branch_prev, but `c` is the last active
        // one (just pushed), so plain redo from a different position
        // would follow `c`.
        let mut tree = UndoTree::new();
        let _a = tree.push(rec(0, "", "a", 0, 1));
        let b = tree.push(rec(1, "", "b", 1, 2));
        tree.undo();
        let c = tree.push(rec(1, "", "c", 1, 2));
        // Both b and c exist as children of a.
        tree.undo(); // back to a
        let children = tree.current_children();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&b));
        assert!(children.contains(&c));
        // last_active_child points at c; redo follows it.
        let replayed = tree.redo().unwrap();
        assert_eq!(replayed.inserted, "c");
    }

    #[test]
    fn branch_next_cycles_last_active_child() {
        let mut tree = UndoTree::new();
        tree.push(rec(0, "", "a", 0, 1)); // current = a
        let b = tree.push(rec(1, "", "b", 1, 2));
        tree.undo(); // back to a
        let c = tree.push(rec(1, "", "c", 1, 2));
        tree.undo(); // back to a
        // last_active_child is c. Cycle to b.
        assert!(tree.branch_next());
        // Redo should now follow b, not c.
        let replayed = tree.redo().unwrap();
        assert_eq!(replayed.inserted, "b");
        assert_eq!(tree.current(), b);
        // Sanity: branch machinery sees children through `a`, so go
        // back and cycle once more.
        tree.undo();
        assert!(tree.branch_next()); // c again
        let replayed = tree.redo().unwrap();
        assert_eq!(replayed.inserted, "c");
        let _ = c;
    }

    #[test]
    fn undo_past_root_returns_none() {
        let mut tree = UndoTree::new();
        tree.push(rec(0, "", "x", 0, 1));
        tree.undo();
        // Second undo: we're at root, no edit to return.
        assert!(tree.undo().is_none());
        assert!(!tree.can_undo());
    }

    #[test]
    fn redo_at_leaf_returns_none() {
        let mut tree = UndoTree::new();
        tree.push(rec(0, "", "x", 0, 1));
        // current is the new leaf; no last_active_child set.
        assert!(tree.redo().is_none());
    }

    #[test]
    fn branch_next_is_noop_with_single_child() {
        let mut tree = UndoTree::new();
        tree.push(rec(0, "", "a", 0, 1));
        tree.undo();
        // Root has one child (a). Nothing to cycle.
        assert!(!tree.branch_next());
    }

    #[test]
    fn peek_undo_shows_next_undo_record() {
        let mut tree = UndoTree::new();
        tree.push(rec(0, "", "hello", 0, 5));
        let peek = tree.peek_undo().unwrap();
        assert_eq!(peek.inserted, "hello");
        // peek doesn't move current.
        assert!(tree.can_undo());
    }

    #[test]
    fn peek_redo_shows_next_redo_record() {
        let mut tree = UndoTree::new();
        tree.push(rec(0, "", "hi", 0, 2));
        tree.undo();
        let peek = tree.peek_redo().unwrap();
        assert_eq!(peek.inserted, "hi");
    }

    #[test]
    fn record_ranges_are_consistent() {
        let r = rec(3, "old", "brand-new", 3, 12);
        assert_eq!(r.pre_range(), 3..6);
        assert_eq!(r.post_range(), 3..12);
    }

    #[test]
    fn node_accessor_returns_some_for_valid_ids() {
        let mut tree = UndoTree::new();
        let id = tree.push(rec(0, "", "x", 0, 1));
        assert!(tree.node(id).is_some());
        assert!(tree.node(tree.root()).is_some());
        assert!(tree.node(UndoNodeId(999)).is_none());
    }
}
