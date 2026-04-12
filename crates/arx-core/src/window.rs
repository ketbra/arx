//! Window state (cursor position, scroll, which buffer is showing)
//! and the logical layout tree that arranges multiple windows on
//! screen.
//!
//! Per spec §2.2, "each window is a view into a buffer with its own
//! cursor, scroll position, and display parameters." That's editor state,
//! not driver state — it belongs in [`crate::Editor`] so commands running
//! on the event loop can mutate it in the same `&mut Editor` as buffer
//! edits.
//!
//! Phase 2 splits: [`WindowManager`] now owns a [`Layout`] tree as well
//! as the flat map of [`WindowData`]. The layout describes *which*
//! windows are visible and *how the screen is partitioned*; the map
//! stores the per-window cursor/scroll state keyed by id. Session
//! restore and low-level code can still add windows to the map without
//! touching the layout (the Phase-1 behaviour), but interactive
//! splitting goes through [`WindowManager::split_active_horizontal`]
//! and friends so the layout stays in sync.
//!
//! The [`arx_render`] crate has its own `WindowState` / `LayoutTree`
//! types that include buffer *snapshots* and are what the render layer
//! consumes. The driver converts between this logical state and that
//! view state on each render.
//!
//! [`arx_render`]: https://docs.rs/arx-render

use std::collections::BTreeMap;

use arx_buffer::BufferId;

/// Opaque identifier for a window inside a [`WindowManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct WindowId(pub u64);

/// Orientation of a [`Layout::Split`].
///
/// Matches Vim's `:split` / `:vsplit` terminology:
///
/// * [`SplitAxis::Horizontal`] — the divider between children runs
///   **horizontally**, so the two children are stacked **top-to-bottom**.
///   Vim `:split` / `Ctrl-W s`.
/// * [`SplitAxis::Vertical`] — the divider runs **vertically**, so the
///   two children sit **side-by-side**. Vim `:vsplit` / `Ctrl-W v`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    /// Divider runs horizontally; children are stacked top-to-bottom.
    Horizontal,
    /// Divider runs vertically; children sit side-by-side.
    Vertical,
}

/// The editor's logical window layout.
///
/// A [`Layout`] is either a single visible window ([`Layout::Leaf`]) or
/// a [`Layout::Split`] dividing a region into two child layouts along
/// one axis. The leaves reference [`WindowId`]s stored in the
/// surrounding [`WindowManager`].
///
/// The layout mirrors what the render layer will paint — there is no
/// notion of "hidden panes". Windows that exist in the [`WindowManager`]
/// but are not referenced by any leaf in the layout (e.g. leftovers from
/// session restore) are simply not drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    /// A single window fills this region.
    Leaf(WindowId),
    /// Two child layouts partitioning this region along `axis`.
    Split {
        /// Which direction the divider runs.
        axis: SplitAxis,
        /// Share of the region allocated to `first`, clamped at render
        /// time so neither child vanishes entirely.
        ratio: f32,
        /// Top / left child.
        first: Box<Layout>,
        /// Bottom / right child.
        second: Box<Layout>,
    },
}

impl Layout {
    /// Default ratio used when creating a new split. Exactly half.
    pub const DEFAULT_RATIO: f32 = 0.5;

    /// Does this subtree contain a leaf with `id`?
    pub fn contains(&self, id: WindowId) -> bool {
        match self {
            Layout::Leaf(x) => *x == id,
            Layout::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    /// Return every leaf in depth-first order: first child before
    /// second, so the caller can rely on it to implement
    /// next / previous focus cycling.
    pub fn leaves(&self) -> Vec<WindowId> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<WindowId>) {
        match self {
            Layout::Leaf(id) => out.push(*id),
            Layout::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    /// Replace the leaf with `id` in this subtree with `replacement`.
    /// Returns `true` if a replacement happened.
    ///
    /// The walk short-circuits on the first matching leaf so this is
    /// cheap even on large layouts.
    pub fn replace_leaf(&mut self, id: WindowId, replacement: Layout) -> bool {
        match self {
            Layout::Leaf(x) if *x == id => {
                *self = replacement;
                true
            }
            Layout::Leaf(_) => false,
            Layout::Split { first, second, .. } => {
                if first.contains(id) {
                    first.replace_leaf(id, replacement)
                } else if second.contains(id) {
                    second.replace_leaf(id, replacement)
                } else {
                    false
                }
            }
        }
    }

    /// Return a new [`Layout`] with the leaf `id` removed.
    ///
    /// * Returns `None` if the entire subtree collapses to nothing
    ///   (i.e. `self` was `Leaf(id)`). The caller is responsible for
    ///   handling the now-empty layout (typically by dropping it).
    /// * Returns `Some(unchanged)` if the leaf wasn't present.
    /// * Otherwise returns a new tree with the parent [`Layout::Split`]
    ///   containing the removed leaf collapsed into its surviving
    ///   sibling.
    #[must_use]
    pub fn without_leaf(self, id: WindowId) -> Option<Self> {
        match self {
            Layout::Leaf(x) if x == id => None,
            leaf @ Layout::Leaf(_) => Some(leaf),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                if first.contains(id) {
                    match first.without_leaf(id) {
                        Some(new_first) => Some(Layout::Split {
                            axis,
                            ratio,
                            first: Box::new(new_first),
                            second,
                        }),
                        None => Some(*second),
                    }
                } else if second.contains(id) {
                    match second.without_leaf(id) {
                        Some(new_second) => Some(Layout::Split {
                            axis,
                            ratio,
                            first,
                            second: Box::new(new_second),
                        }),
                        None => Some(*first),
                    }
                } else {
                    Some(Layout::Split {
                        axis,
                        ratio,
                        first,
                        second,
                    })
                }
            }
        }
    }
}

/// Persistent state for a single window.
///
/// Cursor and scroll are in *buffer coordinates*. Translating to screen
/// coordinates (columns / rows) is the render layer's job.
///
/// The `visible_rows` / `visible_cols` fields are **cached viewport
/// dimensions** written back by the render task each frame so that
/// commands like `page-down` and [`crate::Editor::ensure_active_cursor_visible`]
/// know the actual text area size. They're 0 until the window has been
/// rendered at least once; commands that depend on them fall back to
/// sensible defaults.
#[derive(Debug, Clone)]
pub struct WindowData {
    /// Which buffer this window is viewing.
    pub buffer_id: BufferId,
    /// Primary cursor as a byte offset into the buffer.
    pub cursor_byte: usize,
    /// First line visible in the window.
    pub scroll_top_line: usize,
    /// First visible column (for horizontal scroll).
    pub scroll_left_col: u16,
    /// Last-known number of visible text rows (not counting the
    /// modeline). Updated by the render task each frame.
    pub visible_rows: u16,
    /// Last-known number of visible text columns (not counting the
    /// gutter). Updated by the render task each frame.
    pub visible_cols: u16,
}

impl WindowData {
    /// Create a fresh window over `buffer_id` with the cursor at offset 0
    /// and scroll at the top.
    pub fn new(buffer_id: BufferId) -> Self {
        Self {
            buffer_id,
            cursor_byte: 0,
            scroll_top_line: 0,
            scroll_left_col: 0,
            visible_rows: 0,
            visible_cols: 0,
        }
    }
}

/// The editor's collection of windows and their layout tree.
#[derive(Debug, Default)]
pub struct WindowManager {
    next_id: u64,
    windows: BTreeMap<WindowId, WindowData>,
    layout: Option<Layout>,
    active: Option<WindowId>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// The currently active window, if any.
    pub fn active(&self) -> Option<WindowId> {
        self.active
    }

    /// Convenience: borrow the active window's data.
    pub fn active_data(&self) -> Option<&WindowData> {
        self.active.and_then(|id| self.windows.get(&id))
    }

    /// Convenience: mutably borrow the active window's data.
    pub fn active_data_mut(&mut self) -> Option<&mut WindowData> {
        let id = self.active?;
        self.windows.get_mut(&id)
    }

    /// Borrow a specific window's data.
    pub fn get(&self, id: WindowId) -> Option<&WindowData> {
        self.windows.get(&id)
    }

    /// Mutably borrow a specific window's data.
    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut WindowData> {
        self.windows.get_mut(&id)
    }

    /// Iterate over `(window_id, data)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (WindowId, &WindowData)> + '_ {
        self.windows.iter().map(|(id, data)| (*id, data))
    }

    /// Borrow the current layout tree, if any. `None` means no windows
    /// have been opened yet, or every window was closed.
    pub fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    /// Replace the current layout tree with `layout`, pruning any
    /// leaves that reference unknown window ids (collapsing their
    /// parent splits into the surviving sibling). Returns `true` if
    /// the resulting layout has at least one leaf and was installed;
    /// `false` if every leaf was pruned (no-op, current layout
    /// preserved).
    ///
    /// Used by session restore to rebuild a saved layout tree against
    /// the freshly-reopened windows.
    pub fn set_layout(&mut self, layout: Layout) -> bool {
        let pruned = prune_layout(layout, &self.windows);
        match pruned {
            Some(l) => {
                self.layout = Some(l);
                true
            }
            None => false,
        }
    }

    /// Open a new window on `buffer_id`.
    ///
    /// * If no window is currently active, the new window becomes active.
    /// * If the manager has no layout yet (first-ever window, or every
    ///   pane was closed), the layout is initialised to a single leaf
    ///   referencing the new window.
    /// * Otherwise the layout is left unchanged — the window is added to
    ///   the manager's map but isn't visible until something (e.g.
    ///   [`WindowManager::set_active`] or a split command) puts it in
    ///   the tree. This preserves the Phase-1 behaviour used by session
    ///   restore.
    pub fn open(&mut self, buffer_id: BufferId) -> WindowId {
        self.next_id += 1;
        let id = WindowId(self.next_id);
        self.windows.insert(id, WindowData::new(buffer_id));
        if self.active.is_none() {
            self.active = Some(id);
        }
        if self.layout.is_none() {
            self.layout = Some(Layout::Leaf(id));
        }
        id
    }

    /// Close the window with `id`.
    ///
    /// * Removes the window from the manager's map.
    /// * Removes the window's leaf from the layout, collapsing the
    ///   enclosing [`Layout::Split`] into the surviving sibling.
    /// * If the closed window was active, picks a new active window —
    ///   preferring a leaf from the (surviving) layout, falling back to
    ///   any window still in the map.
    ///
    /// Returns `true` if a window was actually removed.
    pub fn close(&mut self, id: WindowId) -> bool {
        let removed = self.windows.remove(&id).is_some();
        if !removed {
            return false;
        }
        if let Some(layout) = self.layout.take() {
            self.layout = layout.without_leaf(id);
        }
        if self.active == Some(id) {
            self.active = self
                .layout
                .as_ref()
                .and_then(|l| l.leaves().first().copied())
                .or_else(|| self.windows.keys().next().copied());
        }
        true
    }

    /// Set the active window.
    ///
    /// * No-op if `id` isn't known.
    /// * If `id` isn't currently in the layout, the layout is reset to a
    ///   single leaf referencing `id`. This matches Phase-1 semantics —
    ///   calling `set_active` on an "orphaned" window (as session
    ///   restore does) makes it the visible pane.
    pub fn set_active(&mut self, id: WindowId) {
        if !self.windows.contains_key(&id) {
            return;
        }
        self.active = Some(id);
        let needs_reset = self.layout.as_ref().is_none_or(|l| !l.contains(id));
        if needs_reset {
            self.layout = Some(Layout::Leaf(id));
        }
    }

    /// Split the active window along `axis`, creating a new window on
    /// `buffer_id`. The new window becomes active.
    ///
    /// Returns `Some(new_id)` on success, `None` if there's no active
    /// window to split.
    ///
    /// The active leaf is replaced by a [`Layout::Split`] whose `first`
    /// child is the old active leaf and whose `second` child is the new
    /// leaf. The ratio is [`Layout::DEFAULT_RATIO`] (half each).
    pub fn split_active(&mut self, axis: SplitAxis, buffer_id: BufferId) -> Option<WindowId> {
        let active = self.active?;
        self.next_id += 1;
        let new_id = WindowId(self.next_id);
        self.windows.insert(new_id, WindowData::new(buffer_id));

        let replaced = self.layout.as_mut().is_some_and(|layout| {
            layout.replace_leaf(
                active,
                Layout::Split {
                    axis,
                    ratio: Layout::DEFAULT_RATIO,
                    first: Box::new(Layout::Leaf(active)),
                    second: Box::new(Layout::Leaf(new_id)),
                },
            )
        });
        if !replaced {
            // Active wasn't in the layout (orphaned). Fall back to
            // making the new window the sole visible pane so the user
            // still sees *something* after calling split.
            self.layout = Some(Layout::Leaf(new_id));
        }
        self.active = Some(new_id);
        Some(new_id)
    }

    /// Move focus to the next leaf in the layout's depth-first order,
    /// wrapping at the end. Returns the new active window id, or `None`
    /// if the layout is empty.
    pub fn focus_next(&mut self) -> Option<WindowId> {
        let layout = self.layout.as_ref()?;
        let leaves = layout.leaves();
        if leaves.is_empty() {
            return None;
        }
        let current = self.active?;
        let idx = leaves.iter().position(|id| *id == current).unwrap_or(0);
        let next = leaves[(idx + 1) % leaves.len()];
        self.active = Some(next);
        Some(next)
    }

    /// Move focus to the previous leaf in the layout's depth-first
    /// order, wrapping at the start.
    pub fn focus_prev(&mut self) -> Option<WindowId> {
        let layout = self.layout.as_ref()?;
        let leaves = layout.leaves();
        if leaves.is_empty() {
            return None;
        }
        let current = self.active?;
        let idx = leaves.iter().position(|id| *id == current).unwrap_or(0);
        let prev = if idx == 0 {
            leaves[leaves.len() - 1]
        } else {
            leaves[idx - 1]
        };
        self.active = Some(prev);
        Some(prev)
    }
}

/// Walk `layout` and drop any leaf that isn't a key in `known`,
/// collapsing parent splits into their surviving sibling. Returns
/// `None` if every leaf was pruned.
fn prune_layout(
    layout: Layout,
    known: &BTreeMap<WindowId, WindowData>,
) -> Option<Layout> {
    match layout {
        Layout::Leaf(id) => {
            if known.contains_key(&id) {
                Some(Layout::Leaf(id))
            } else {
                None
            }
        }
        Layout::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let new_first = prune_layout(*first, known);
            let new_second = prune_layout(*second, known);
            match (new_first, new_second) {
                (Some(a), Some(b)) => Some(Layout::Split {
                    axis,
                    ratio,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_first_window_becomes_active() {
        let mut wm = WindowManager::new();
        assert!(wm.is_empty());
        let id = wm.open(BufferId(7));
        assert_eq!(wm.active(), Some(id));
        assert_eq!(wm.len(), 1);
        assert_eq!(wm.get(id).unwrap().buffer_id, BufferId(7));
        assert_eq!(wm.layout(), Some(&Layout::Leaf(id)));
    }

    #[test]
    fn open_second_window_does_not_steal_active() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.open(BufferId(2));
        assert_eq!(wm.active(), Some(a));
        assert!(wm.get(b).is_some());
        // Phase 2: second open does NOT auto-split the layout — it just
        // adds to the map.
        assert_eq!(wm.layout(), Some(&Layout::Leaf(a)));
    }

    #[test]
    fn close_active_picks_another() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.open(BufferId(2));
        assert!(wm.close(a));
        assert_eq!(wm.active(), Some(b));
    }

    #[test]
    fn close_last_leaves_none_active() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        wm.close(a);
        assert!(wm.active().is_none());
        assert!(wm.is_empty());
        assert!(wm.layout().is_none());
    }

    #[test]
    fn mutate_active_data() {
        let mut wm = WindowManager::new();
        wm.open(BufferId(1));
        wm.active_data_mut().unwrap().cursor_byte = 42;
        assert_eq!(wm.active_data().unwrap().cursor_byte, 42);
    }

    #[test]
    fn set_active_ignores_unknown() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        wm.set_active(WindowId(999));
        assert_eq!(wm.active(), Some(a));
    }

    // ---- Splits ----

    #[test]
    fn split_active_horizontal_creates_split() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.split_active(SplitAxis::Horizontal, BufferId(2)).unwrap();
        assert_eq!(wm.active(), Some(b));
        assert_eq!(wm.len(), 2);
        let layout = wm.layout().unwrap();
        assert_eq!(layout.leaves(), vec![a, b]);
        assert!(layout.contains(a));
        assert!(layout.contains(b));
    }

    #[test]
    fn split_split_nests_correctly() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.split_active(SplitAxis::Vertical, BufferId(2)).unwrap();
        let c = wm.split_active(SplitAxis::Horizontal, BufferId(3)).unwrap();
        // DFS leaf order: a, b, c. `c` was split off `b`, so the order
        // is a (left), then (b, c) stacked on the right.
        assert_eq!(wm.layout().unwrap().leaves(), vec![a, b, c]);
        assert_eq!(wm.active(), Some(c));
    }

    #[test]
    fn close_collapses_split_into_sibling() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.split_active(SplitAxis::Vertical, BufferId(2)).unwrap();
        assert!(wm.close(b));
        // After close, the split collapses back to a single leaf on a.
        assert_eq!(wm.layout(), Some(&Layout::Leaf(a)));
        assert_eq!(wm.active(), Some(a));
        assert_eq!(wm.len(), 1);
    }

    #[test]
    fn close_nested_leaf_preserves_other_branch() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.split_active(SplitAxis::Vertical, BufferId(2)).unwrap();
        let c = wm.split_active(SplitAxis::Horizontal, BufferId(3)).unwrap();
        // Close c; (b,c) split collapses to b. Root is still a split
        // of (a | b).
        assert!(wm.close(c));
        let layout = wm.layout().unwrap();
        assert_eq!(layout.leaves(), vec![a, b]);
    }

    #[test]
    fn focus_next_cycles_leaves() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.split_active(SplitAxis::Vertical, BufferId(2)).unwrap();
        let c = wm.split_active(SplitAxis::Horizontal, BufferId(3)).unwrap();
        // Currently active = c (just split). Cycling forward wraps.
        assert_eq!(wm.active(), Some(c));
        wm.focus_next();
        assert_eq!(wm.active(), Some(a));
        wm.focus_next();
        assert_eq!(wm.active(), Some(b));
        wm.focus_next();
        assert_eq!(wm.active(), Some(c));
    }

    #[test]
    fn focus_prev_cycles_backwards() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm
            .split_active(SplitAxis::Vertical, BufferId(2))
            .unwrap();
        // Active = b, prev = a, prev again wraps to b.
        wm.focus_prev();
        assert_eq!(wm.active(), Some(a));
        wm.focus_prev();
        assert_eq!(wm.active(), Some(b));
    }

    #[test]
    fn set_active_to_orphan_resets_layout() {
        // Emulate the session-restore path: open one window (it becomes
        // the layout), then `open` additional windows which land in the
        // map only, then call set_active on one of the orphans.
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.open(BufferId(2));
        assert_eq!(wm.layout(), Some(&Layout::Leaf(a)));
        wm.set_active(b);
        assert_eq!(wm.active(), Some(b));
        assert_eq!(wm.layout(), Some(&Layout::Leaf(b)));
    }

    #[test]
    fn set_active_within_layout_preserves_split() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        wm.split_active(SplitAxis::Vertical, BufferId(2)).unwrap();
        let before = wm.layout().cloned();
        wm.set_active(a);
        assert_eq!(wm.active(), Some(a));
        assert_eq!(wm.layout().cloned(), before);
    }

    #[test]
    fn layout_without_leaf_preserves_siblings() {
        let layout = Layout::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(Layout::Leaf(WindowId(1))),
            second: Box::new(Layout::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(Layout::Leaf(WindowId(2))),
                second: Box::new(Layout::Leaf(WindowId(3))),
            }),
        };
        let without = layout.clone().without_leaf(WindowId(2)).unwrap();
        assert_eq!(without.leaves(), vec![WindowId(1), WindowId(3)]);
    }

    #[test]
    fn layout_without_only_leaf_returns_none() {
        let layout = Layout::Leaf(WindowId(5));
        assert!(layout.without_leaf(WindowId(5)).is_none());
    }

    #[test]
    fn layout_without_missing_leaf_is_unchanged() {
        let layout = Layout::Leaf(WindowId(1));
        let after = layout.clone().without_leaf(WindowId(99)).unwrap();
        assert_eq!(after, layout);
    }

    // ---- set_layout (session restore plumbing) ----

    #[test]
    fn set_layout_installs_matching_tree() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.open(BufferId(2));
        // Default layout from `open` is Leaf(a). Install a split.
        let replacement = Layout::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(Layout::Leaf(a)),
            second: Box::new(Layout::Leaf(b)),
        };
        assert!(wm.set_layout(replacement));
        assert_eq!(wm.layout().unwrap().leaves(), vec![a, b]);
    }

    #[test]
    fn set_layout_prunes_unknown_leaves() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        // Candidate references a phantom WindowId(99) that doesn't
        // exist in the manager; it should collapse away and leave us
        // with Leaf(a).
        let replacement = Layout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(Layout::Leaf(a)),
            second: Box::new(Layout::Leaf(WindowId(99))),
        };
        assert!(wm.set_layout(replacement));
        assert_eq!(wm.layout(), Some(&Layout::Leaf(a)));
    }

    #[test]
    fn set_layout_with_no_valid_leaves_is_noop() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let before = wm.layout().cloned();
        let bogus = Layout::Leaf(WindowId(999));
        assert!(!wm.set_layout(bogus));
        // Original layout preserved.
        assert_eq!(wm.layout().cloned(), before);
        assert_eq!(wm.active(), Some(a));
    }
}
