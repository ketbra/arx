//! Window state (cursor position, scroll, which buffer is showing).
//!
//! Per spec §2.2, "each window is a view into a buffer with its own
//! cursor, scroll position, and display parameters." That's editor state,
//! not driver state — it belongs in [`crate::Editor`] so commands running
//! on the event loop can mutate it in the same `&mut Editor` as buffer
//! edits.
//!
//! This module is the minimal single-writer window store. Phase 1 only
//! supports one active window over one buffer; multi-window splits and a
//! tree-of-splits layout land in a later commit (the shape of
//! [`WindowData`] is deliberately small so migrating to a tree is cheap).
//!
//! The [`arx_render`] crate has its own `WindowState` struct that
//! includes a buffer *snapshot* and is what the render layer consumes.
//! The driver converts between this logical state and that view state on
//! each render.
//!
//! [`arx_render`]: https://docs.rs/arx-render

use std::collections::BTreeMap;

use arx_buffer::BufferId;

/// Opaque identifier for a window inside a [`WindowManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct WindowId(pub u64);

/// Persistent state for a single window.
///
/// Cursor and scroll are in *buffer coordinates*. Translating to screen
/// coordinates (columns / rows) is the render layer's job.
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
        }
    }
}

/// The editor's collection of windows.
#[derive(Debug, Default)]
pub struct WindowManager {
    next_id: u64,
    windows: BTreeMap<WindowId, WindowData>,
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

    /// Open a new window on `buffer_id`. If no window is currently active,
    /// the new window becomes active.
    pub fn open(&mut self, buffer_id: BufferId) -> WindowId {
        self.next_id += 1;
        let id = WindowId(self.next_id);
        self.windows.insert(id, WindowData::new(buffer_id));
        if self.active.is_none() {
            self.active = Some(id);
        }
        id
    }

    /// Close the window with `id`. If it was the active window, picks
    /// another arbitrary window as the new active (or `None` if empty).
    /// Returns `true` if a window was actually removed.
    pub fn close(&mut self, id: WindowId) -> bool {
        let removed = self.windows.remove(&id).is_some();
        if self.active == Some(id) {
            self.active = self.windows.keys().next().copied();
        }
        removed
    }

    /// Set the active window. No-op if `id` is unknown.
    pub fn set_active(&mut self, id: WindowId) {
        if self.windows.contains_key(&id) {
            self.active = Some(id);
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
    }

    #[test]
    fn open_second_window_does_not_steal_active() {
        let mut wm = WindowManager::new();
        let a = wm.open(BufferId(1));
        let b = wm.open(BufferId(2));
        assert_eq!(wm.active(), Some(a));
        assert!(wm.get(b).is_some());
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
}
