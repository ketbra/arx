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
use std::sync::Arc;

use arx_buffer::{Buffer, BufferId, BufferSnapshot, ByteRange, Edit, EditOrigin};
use arx_keymap::{FeedOutcome, KeyChord, KeymapEngine, Layer, Profile};
use tokio::sync::watch;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[cfg(feature = "syntax")]
use arx_highlight::HighlightManager;

use crate::command::CommandBus;
use crate::completion::CompletionPopup;
use crate::palette::CommandPalette;
use crate::registry::{CommandContext, CommandRegistry};
use crate::window::WindowManager;

/// What [`Editor::handle_key`] tells the input task to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyHandled {
    /// A command resolved and was executed. No further action.
    Executed,
    /// Key is part of a live prefix — keep accumulating.
    Pending,
    /// The key was unbound; the input task should self-insert `ch` if
    /// present (printable fallback) or ignore the key otherwise.
    Unbound { printable_fallback: Option<char> },
}

/// The editor's in-process state.
///
/// Owns every piece of mutable editor state today. Lives on the event loop
/// task only — never shared across threads — so it doesn't need to be
/// `Sync` (and isn't, deliberately, so we catch accidental cross-task use
/// at compile time).
pub struct Editor {
    buffers: BufferManager,
    windows: WindowManager,
    keymap: KeymapEngine,
    commands: CommandRegistry,
    palette: CommandPalette,
    completion: CompletionPopup,
    #[cfg(feature = "syntax")]
    highlight: HighlightManager,
    terminals: HashMap<crate::WindowId, arx_terminal::TerminalPane>,
    /// Redraw notify shared with terminal panes so they can wake the
    /// render task when PTY output arrives.
    terminal_redraw: Option<Arc<tokio::sync::Notify>>,
    #[cfg(feature = "lsp")]
    lsp_notifier: Option<tokio::sync::mpsc::Sender<arx_lsp::LspEvent>>,
    /// Kill ring — a stack of killed (cut/copied) text for yank.
    kill_ring: Vec<String>,
    /// Per-window mark (selection anchor) byte offsets.
    marks: HashMap<crate::WindowId, usize>,
    /// Transient message shown in the modeline (e.g. hover info, LSP
    /// status). Cleared on the next user keystroke.
    status_message: Option<String>,
    dirty: bool,
    quit_requested: bool,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("buffers", &self.buffers)
            .field("windows", &self.windows)
            .field("commands", &self.commands)
            .field("palette", &self.palette)
            .field("dirty", &self.dirty)
            .field("quit_requested", &self.quit_requested)
            .finish_non_exhaustive()
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::with_profile(arx_keymap::profiles::default())
    }
}

impl Editor {
    /// Create an empty editor with the default (Emacs) keymap profile.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an empty editor with a specific keymap profile already
    /// installed.
    pub fn with_profile(profile: Profile) -> Self {
        let mut keymap = KeymapEngine::new(profile.global);
        if let Some((id, map)) = profile.startup_layer {
            keymap.push_layer(Layer::new(id, map));
        }
        keymap.set_count_mode(profile.count_mode);
        let mut commands = CommandRegistry::new();
        crate::stock::register_stock(&mut commands);
        Self {
            buffers: BufferManager::default(),
            windows: WindowManager::default(),
            keymap,
            commands,
            palette: CommandPalette::new(),
            completion: CompletionPopup::new(),
            terminals: HashMap::new(),
            terminal_redraw: None,
            #[cfg(feature = "syntax")]
            highlight: HighlightManager::new(),
            #[cfg(feature = "lsp")]
            lsp_notifier: None,
            kill_ring: Vec::new(),
            marks: HashMap::new(),
            status_message: None,
            dirty: false,
            quit_requested: false,
        }
    }

    /// Borrow the [`BufferManager`].
    pub fn buffers(&self) -> &BufferManager {
        &self.buffers
    }

    /// Mutably borrow the [`BufferManager`].
    pub fn buffers_mut(&mut self) -> &mut BufferManager {
        &mut self.buffers
    }

    /// Borrow the [`WindowManager`].
    pub fn windows(&self) -> &WindowManager {
        &self.windows
    }

    /// Mutably borrow the [`WindowManager`].
    pub fn windows_mut(&mut self) -> &mut WindowManager {
        &mut self.windows
    }

    /// Borrow the keymap engine.
    pub fn keymap(&self) -> &KeymapEngine {
        &self.keymap
    }

    /// Mutably borrow the keymap engine.
    pub fn keymap_mut(&mut self) -> &mut KeymapEngine {
        &mut self.keymap
    }

    /// Borrow the command registry.
    pub fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    /// Mutably borrow the command registry. Extensions register their
    /// own commands through this handle in later milestones.
    pub fn commands_mut(&mut self) -> &mut CommandRegistry {
        &mut self.commands
    }

    /// Borrow the command palette state.
    pub fn palette(&self) -> &CommandPalette {
        &self.palette
    }

    /// Mutably borrow the command palette state.
    pub fn palette_mut(&mut self) -> &mut CommandPalette {
        &mut self.palette
    }

    /// Attach syntax highlighting to `buffer_id` based on `extension`.
    /// No-op if the extension doesn't map to a known grammar or if the
    /// `syntax` feature is disabled. Uses disjoint field borrowing so
    /// the highlight manager and buffer manager can both be touched
    /// without a double-borrow.
    pub fn attach_highlight(
        &mut self,
        id: arx_buffer::BufferId,
        extension: Option<&str>,
    ) {
        #[cfg(feature = "syntax")]
        if let Some(buffer) = self.buffers.get_mut(id) {
            self.highlight.attach_buffer(buffer, extension);
        }
        #[cfg(not(feature = "syntax"))]
        { let _ = (id, extension); }
    }

    /// Apply a user edit to `buffer_id` and update syntax highlights
    /// in one step. Uses disjoint field borrowing so the highlight
    /// manager and the buffer manager can both be touched in the same
    /// call without a double-borrow. Falls back to a plain
    /// `buffers.edit()` when the `syntax` feature is disabled.
    pub fn edit_with_highlight(
        &mut self,
        id: arx_buffer::BufferId,
        range: arx_buffer::ByteRange,
        text: &str,
        origin: arx_buffer::EditOrigin,
    ) -> Option<arx_buffer::Edit> {
        let edit = self.buffers.edit(id, range, text, origin)?;
        #[cfg(feature = "syntax")]
        if let Some(buffer) = self.buffers.get_mut(id) {
            self.highlight.on_edit(buffer, &edit);
        }
        Some(edit)
    }

    /// Borrow the completion popup state.
    pub fn completion(&self) -> &CompletionPopup {
        &self.completion
    }

    /// Mutably borrow the completion popup state.
    pub fn completion_mut(&mut self) -> &mut CompletionPopup {
        &mut self.completion
    }

    /// Set the redraw notify for terminal panes. Called by the driver.
    pub fn set_terminal_redraw(&mut self, notify: Arc<tokio::sync::Notify>) {
        self.terminal_redraw = Some(notify);
    }

    /// Whether `window_id` is a terminal pane rather than a buffer.
    pub fn is_terminal(&self, window_id: crate::WindowId) -> bool {
        self.terminals.contains_key(&window_id)
    }

    /// Borrow a terminal pane by its window id.
    pub fn terminal(&self, window_id: crate::WindowId) -> Option<&arx_terminal::TerminalPane> {
        self.terminals.get(&window_id)
    }

    /// Borrow the terminals map (for iteration in the render path).
    pub fn terminals(&self) -> &HashMap<crate::WindowId, arx_terminal::TerminalPane> {
        &self.terminals
    }

    /// Mutably borrow the terminals map.
    pub fn terminals_mut(&mut self) -> &mut HashMap<crate::WindowId, arx_terminal::TerminalPane> {
        &mut self.terminals
    }

    /// Open a terminal pane in a split of the active window. Returns
    /// the new window id, or `None` if there's no active window or
    /// no redraw notify is set.
    pub fn open_terminal(
        &mut self,
        axis: crate::window::SplitAxis,
    ) -> Option<crate::WindowId> {
        let redraw = self.terminal_redraw.clone()?;
        // We need a buffer for the window manager (it requires a
        // BufferId). Create a scratch buffer as a placeholder — the
        // render path will detect it's a terminal and skip the buffer.
        let placeholder_buf = self.buffers.create_scratch();
        let new_id = self.windows.split_active(axis, placeholder_buf)?;
        // Get the viewport size from the new window.
        let data = self.windows.get(new_id)?;
        let cols = if data.visible_cols > 0 { data.visible_cols } else { 80 };
        let rows = if data.visible_rows > 0 { data.visible_rows } else { 24 };
        match arx_terminal::TerminalPane::spawn(cols, rows, None, redraw) {
            Ok(pane) => {
                self.terminals.insert(new_id, pane);
                self.mark_dirty();
                Some(new_id)
            }
            Err(err) => {
                tracing::warn!(%err, "failed to spawn terminal");
                // Clean up the window we just created.
                self.windows.close(new_id);
                None
            }
        }
    }

    /// Close a terminal pane, removing it from both the window
    /// manager and the terminals map.
    pub fn close_terminal(&mut self, window_id: crate::WindowId) -> bool {
        if self.terminals.remove(&window_id).is_some() {
            self.windows.close(window_id);
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    /// Set the LSP event notifier. Called by the driver at startup
    /// once the LSP manager task is running.
    #[cfg(feature = "lsp")]
    pub fn set_lsp_notifier(&mut self, tx: tokio::sync::mpsc::Sender<arx_lsp::LspEvent>) {
        self.lsp_notifier = Some(tx);
    }

    /// Drop the LSP notifier so the manager task's receiver sees
    /// the channel close and exits cleanly. Called by the driver at
    /// shutdown.
    #[cfg(feature = "lsp")]
    pub fn clear_lsp_notifier(&mut self) {
        self.lsp_notifier = None;
    }

    /// Send an LSP event (best-effort, non-blocking). No-op if the
    /// `lsp` feature is disabled or no notifier is set.
    #[cfg(feature = "lsp")]
    pub fn notify_lsp(&self, event: arx_lsp::LspEvent) {
        if let Some(tx) = &self.lsp_notifier {
            let _ = tx.try_send(event);
        }
    }

    /// Stub when the `lsp` feature is off.
    #[cfg(not(feature = "lsp"))]
    pub fn notify_lsp(&self, _event: ()) {}

    /// Handle a printable character that the keymap layer reported as
    /// unbound. Single entry point called by the driver's input task
    /// (embedded *and* daemon variants) so the routing decision lives
    /// in editor state rather than duplicated across input paths.
    ///
    /// If the command palette is open, the character extends the
    /// palette's query and the buffer is untouched; otherwise the
    /// character is self-inserted at the active cursor via
    /// [`crate::stock::insert_at_cursor`].
    pub fn handle_printable_fallback(&mut self, ch: char) {
        if self.palette.is_open() {
            self.palette.append_char(ch);
            self.mark_dirty();
        } else {
            crate::stock::insert_at_cursor(self, &ch.to_string());
        }
    }

    /// Push text onto the kill ring.
    pub fn kill_ring_push(&mut self, text: String) {
        self.kill_ring.push(text);
        // Cap at 64 entries.
        if self.kill_ring.len() > 64 {
            self.kill_ring.remove(0);
        }
    }

    /// Peek at the top of the kill ring (most recently killed text).
    pub fn kill_ring_top(&self) -> Option<&str> {
        self.kill_ring.last().map(String::as_str)
    }

    /// Set the mark (selection anchor) for `window_id`.
    pub fn set_mark(&mut self, window_id: crate::WindowId, byte: usize) {
        self.marks.insert(window_id, byte);
    }

    /// Get the mark for `window_id`, if set.
    pub fn mark(&self, window_id: crate::WindowId) -> Option<usize> {
        self.marks.get(&window_id).copied()
    }

    /// Clear the mark for `window_id`.
    pub fn clear_mark(&mut self, window_id: crate::WindowId) {
        self.marks.remove(&window_id);
    }

    /// Set a transient status message shown in the modeline. Cleared
    /// on the next keystroke via [`Self::clear_status`].
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.mark_dirty();
    }

    /// Clear the status message.
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// The current status message, if any.
    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    /// Mark the editor as "display-affecting since the last frame" so the
    /// next tick of the event loop will ping the redraw notify (if any).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Consume the dirty flag and return whether a redraw should fire.
    /// Called by [`crate::EventLoop`] after each dispatched command.
    #[must_use]
    pub fn take_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.dirty, false)
    }

    /// Whether a full repaint should be forced on the next frame
    /// (e.g. after a status message changed). The render task checks
    /// this and drops its cached previous frame when set.
    pub fn needs_full_repaint(&self) -> bool {
        // Status message changes can leave stale cells on some
        // terminals if the differ only repaints the modeline row.
        // Force a full repaint whenever the status changes.
        self.status_message.is_some()
    }

    /// Request that the driver shut down cleanly. Called by the
    /// `editor.quit` stock command.
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
        self.mark_dirty();
    }

    /// Whether a quit has been requested. The driver polls this after
    /// each command and fires its shutdown signal when it flips.
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// Feed a key to the keymap engine. If it resolves to a command,
    /// invoke it inline against `&mut self`. Reports the outcome so the
    /// input task knows whether to fall back to self-insert.
    ///
    /// `bus` is cloned into the [`CommandContext`] so commands can spawn
    /// async follow-ups (e.g. `buffer.save`). After the command runs,
    /// [`Self::ensure_active_cursor_visible`] is called so any cursor
    /// movement or buffer edit that pushed the cursor off-screen pulls
    /// the scroll position along with it.
    pub fn handle_key(&mut self, bus: &CommandBus, chord: KeyChord) -> KeyHandled {
        // Clear the transient status message on every keystroke.
        self.status_message = None;
        let outcome = match self.keymap.feed(chord) {
            FeedOutcome::Execute { command, count } => {
                // Clone the Arc out so we release the borrow of
                // `self.commands` before taking `&mut self.editor`.
                let cmd = self.commands.get(&command.name);
                if let Some(cmd) = cmd {
                    let mut cx = CommandContext {
                        editor: self,
                        bus: bus.clone(),
                        count,
                    };
                    cmd.run(&mut cx);
                } else {
                    tracing::warn!(name = %command.name, "unknown command");
                }
                KeyHandled::Executed
            }
            FeedOutcome::Pending => KeyHandled::Pending,
            FeedOutcome::Unbound { printable_fallback } => {
                KeyHandled::Unbound { printable_fallback }
            }
        };
        // Every command path (including the printable-fallback path
        // taken by the input layer when this returns Unbound) may have
        // moved the cursor, so run the viewport-follow step here.
        // When the fallback is used, the input task applies the
        // self-insert and then calls this method again via a dispatch;
        // see `arx_driver::input`. In either case the cursor ends up
        // visible before the next frame is rendered.
        self.ensure_active_cursor_visible();
        outcome
    }

    /// Adjust the active window's scroll position so its primary cursor
    /// is inside the visible text area. Called from [`Self::handle_key`]
    /// after every command; safe to call directly after any explicit
    /// mutation of cursor / scroll state.
    ///
    /// Adjusts both the vertical scroll (`scroll_top_line`) and the
    /// horizontal scroll (`scroll_left_col`). Horizontal uses display
    /// columns computed from grapheme widths so multi-byte characters
    /// and wide CJK glyphs come out right.
    ///
    /// If the window has never been rendered (`visible_rows == 0` or
    /// `visible_cols == 0`) this is a no-op — we can't follow the
    /// cursor into a window we don't know the size of yet.
    pub fn ensure_active_cursor_visible(&mut self) {
        let Some(window_id) = self.windows.active() else {
            return;
        };
        let Some(data) = self.windows.get(window_id).cloned() else {
            return;
        };
        let visible_rows = data.visible_rows;
        let visible_cols = data.visible_cols;
        if visible_rows == 0 || visible_cols == 0 {
            // Window hasn't been rendered yet; nothing to align to.
            return;
        }
        let Some(buffer) = self.buffers.get(data.buffer_id) else {
            return;
        };
        let rope = buffer.rope();
        let cursor_byte = data.cursor_byte.min(rope.len_bytes());
        let cursor_line = rope.byte_to_line(cursor_byte);

        // Vertical follow.
        let rows = visible_rows as usize;
        let mut new_top = data.scroll_top_line;
        if cursor_line < new_top {
            new_top = cursor_line;
        } else if cursor_line >= new_top.saturating_add(rows) {
            new_top = cursor_line + 1 - rows;
        }
        // Don't bother clamping to len_lines — scrolling past the end
        // just shows blank rows, which is fine and matches how we
        // handle page-down that overshoots.

        // Horizontal follow. Compute the cursor's *display* column
        // (grapheme widths summed) from the start of its line.
        let line_start = rope.line_to_byte(cursor_line);
        let text_to_cursor = rope.slice_to_string(line_start..cursor_byte);
        let mut cursor_col: u16 = 0;
        for g in text_to_cursor.graphemes(true) {
            cursor_col = cursor_col
                .saturating_add(UnicodeWidthStr::width(g).clamp(1, 2) as u16);
        }
        let cols = visible_cols;
        let mut new_left = data.scroll_left_col;
        if cursor_col < new_left {
            new_left = cursor_col;
        } else if cursor_col >= new_left.saturating_add(cols) {
            new_left = cursor_col + 1 - cols;
        }

        if new_top != data.scroll_top_line || new_left != data.scroll_left_col {
            if let Some(w) = self.windows.get_mut(window_id) {
                w.scroll_top_line = new_top;
                w.scroll_left_col = new_left;
            }
            self.mark_dirty();
        }
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

    // ---- Editor::ensure_active_cursor_visible ----

    fn editor_with_window(text: &str, rows: u16, cols: u16) -> (Editor, crate::WindowId) {
        let mut editor = Editor::new();
        let buf = editor.buffers_mut().create_from_text(text, None);
        let id = editor.windows_mut().open(buf);
        let data = editor.windows_mut().get_mut(id).unwrap();
        data.visible_rows = rows;
        data.visible_cols = cols;
        (editor, id)
    }

    #[test]
    fn cursor_below_viewport_scrolls_down() {
        let text = (0..20)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (mut editor, id) = editor_with_window(&text, 5, 20);
        // Place cursor on line 10; viewport starts at 0 with height 5 ->
        // cursor is below. After ensure, top_line should have followed.
        let line_10_byte = editor
            .buffers()
            .get(editor.windows().get(id).unwrap().buffer_id)
            .unwrap()
            .rope()
            .line_to_byte(10);
        editor.windows_mut().get_mut(id).unwrap().cursor_byte = line_10_byte;
        editor.ensure_active_cursor_visible();
        let data = editor.windows().get(id).unwrap();
        assert!(
            data.scroll_top_line <= 10,
            "top {} should cover line 10",
            data.scroll_top_line
        );
        assert!(data.scroll_top_line + 5 > 10);
    }

    #[test]
    fn cursor_above_viewport_scrolls_up() {
        let text = (0..20)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (mut editor, id) = editor_with_window(&text, 5, 20);
        editor.windows_mut().get_mut(id).unwrap().scroll_top_line = 10;
        // Cursor at line 0 but scroll is at line 10 — cursor is above.
        editor.windows_mut().get_mut(id).unwrap().cursor_byte = 0;
        editor.ensure_active_cursor_visible();
        assert_eq!(editor.windows().get(id).unwrap().scroll_top_line, 0);
    }

    #[test]
    fn cursor_past_right_edge_scrolls_right() {
        let text = "a".repeat(80);
        let (mut editor, id) = editor_with_window(&text, 5, 20);
        editor.windows_mut().get_mut(id).unwrap().cursor_byte = 50;
        editor.ensure_active_cursor_visible();
        let data = editor.windows().get(id).unwrap();
        // Cursor column 50 must be inside [left, left + 20).
        assert!(data.scroll_left_col <= 50);
        assert!(50 < data.scroll_left_col + 20);
    }

    #[test]
    fn ensure_visible_is_noop_when_viewport_unknown() {
        // visible_rows/cols = 0 → can't compute anything, must leave
        // scroll state alone.
        let mut editor = Editor::new();
        let buf = editor.buffers_mut().create_from_text("abc", None);
        let id = editor.windows_mut().open(buf);
        editor.windows_mut().get_mut(id).unwrap().scroll_top_line = 7;
        editor.ensure_active_cursor_visible();
        assert_eq!(editor.windows().get(id).unwrap().scroll_top_line, 7);
    }
}
