//! Session state: a serialisable snapshot of everything the daemon
//! needs to reopen after a restart.
//!
//! # Scope
//!
//! For Phase 1, a [`Session`] records the *stable* shape of the editor:
//! which files are open, the window layout, and per-window cursor /
//! scroll state. It intentionally does **not** yet persist in-flight
//! unsaved buffer text — that's the "crashed daemon, please undo-close
//! my tab" ask and lands in a follow-up milestone once we've nailed
//! down atomic snapshot writes and a journal.
//!
//! Rough ladder for how persistence grows from here:
//!
//! | Level | What's saved | When | Recovers |
//! |---|---|---|---|
//! | 0 | — | never | daemon survives client disconnects |
//! | 1 | `Session` on clean shutdown | exit | file paths + cursors + layout |
//! | 2 | `Session` periodically | every N seconds / edits | + unclean shutdowns (some data loss) |
//! | 3 | + unsaved buffer journal | after each edit | exact state incl. unsaved edits |
//!
//! This commit ships the *type* so the daemon can be written against
//! it, and marks the integration points with `TODO(phase-1b)` so the
//! next commit slots in a write-on-shutdown path without touching the
//! daemon's core flow.
//!
//! # Versioning
//!
//! Every saved file starts with a [`SessionFile::version`] number.
//! Bump it whenever the on-disk shape changes. Older files that fail
//! to parse can be migrated or safely ignored.
//!
//! # On-disk layout & atomicity
//!
//! [`Session::save_to_path`] serialises with [`postcard`] (same wire
//! format the IPC layer uses) and writes via temp-file-plus-rename so
//! a crash mid-write never leaves a truncated session behind. The
//! tempfile is created in the same directory as the final file, so
//! the rename is a single atomic `renameat` on Unix and
//! `MoveFileExW`-with-`MOVEFILE_REPLACE_EXISTING` on Windows.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::window::{Layout, SplitAxis};
use crate::{CommandBus, WindowId};
use arx_buffer::BufferId;

/// Top-level on-disk session file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionFile {
    /// Schema version. Start at 1; bump on incompatible changes.
    pub version: u32,
    /// The actual session state.
    pub session: Session,
}

impl SessionFile {
    /// Current on-disk schema version.
    ///
    /// * **v1** — Phase 1. No layout tree; sessions restored to a
    ///   single-leaf layout on the active window.
    /// * **v2** — Phase 2. Adds an optional [`SerializedLayout`] so
    ///   nested splits survive a restart. v1 files still load through
    ///   the backward-compat path in [`Session::load_from_path`].
    pub const CURRENT_VERSION: u32 = 2;

    pub fn new(session: Session) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            session,
        }
    }
}

/// A serialisable snapshot of the editor's persistent state.
///
/// Changing the shape of this struct is a schema break — bump
/// [`SessionFile::CURRENT_VERSION`] and add a compat branch in
/// [`Session::load_from_path`] to parse the older on-disk layout.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Session {
    /// All open buffers at snapshot time.
    pub buffers: Vec<SerializedBuffer>,
    /// All open windows at snapshot time.
    pub windows: Vec<SerializedWindow>,
    /// Which window was active.
    pub active_window: Option<u64>,
    /// The logical layout tree at snapshot time. `None` for sessions
    /// that were saved before Phase 2 introduced splits (v1 on disk);
    /// in that case restore collapses to a single-leaf layout on the
    /// active window, matching Phase-1 behaviour.
    pub layout: Option<SerializedLayout>,
}

/// Backward-compat helper for the v1 on-disk schema. Identical to
/// [`Session`] minus the `layout` field. Parsed when a v1 file is
/// encountered in [`Session::load_from_path`] and then lifted into the
/// current [`Session`] shape with `layout = None`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct LegacySessionV1 {
    buffers: Vec<SerializedBuffer>,
    windows: Vec<SerializedWindow>,
    active_window: Option<u64>,
}

impl From<LegacySessionV1> for Session {
    fn from(v1: LegacySessionV1) -> Self {
        Session {
            buffers: v1.buffers,
            windows: v1.windows,
            active_window: v1.active_window,
            layout: None,
        }
    }
}

/// Serialisable mirror of [`crate::window::Layout`]. Uses raw `u64`
/// window ids (so old and new ids can be remapped on restore) and a
/// local copy of [`crate::window::SplitAxis`] so `Session` doesn't
/// depend on postcard's willingness to serialise foreign enums.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SerializedLayout {
    /// A single pane containing the window with this saved id.
    Leaf(u64),
    /// A split of two child layouts.
    Split {
        axis: SerializedSplitAxis,
        ratio: f32,
        first: Box<SerializedLayout>,
        second: Box<SerializedLayout>,
    },
}

/// Local copy of [`crate::window::SplitAxis`] for on-disk encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SerializedSplitAxis {
    Horizontal,
    Vertical,
}

impl From<SplitAxis> for SerializedSplitAxis {
    fn from(axis: SplitAxis) -> Self {
        match axis {
            SplitAxis::Horizontal => SerializedSplitAxis::Horizontal,
            SplitAxis::Vertical => SerializedSplitAxis::Vertical,
        }
    }
}

impl From<SerializedSplitAxis> for SplitAxis {
    fn from(axis: SerializedSplitAxis) -> Self {
        match axis {
            SerializedSplitAxis::Horizontal => SplitAxis::Horizontal,
            SerializedSplitAxis::Vertical => SplitAxis::Vertical,
        }
    }
}

impl SerializedLayout {
    /// Snapshot a [`Layout`] for storage. The saved ids are the
    /// current in-memory [`WindowId`]s; the restore path remaps them
    /// to the freshly-allocated ids of reopened windows.
    pub fn from_layout(layout: &Layout) -> Self {
        match layout {
            Layout::Leaf(id) => SerializedLayout::Leaf(id.0),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => SerializedLayout::Split {
                axis: (*axis).into(),
                ratio: *ratio,
                first: Box::new(SerializedLayout::from_layout(first)),
                second: Box::new(SerializedLayout::from_layout(second)),
            },
        }
    }

    /// Rehydrate a [`Layout`] using `remap` to translate saved window
    /// ids into the current editor's [`WindowId`]s.
    ///
    /// Leaves whose saved id isn't in `remap` (because the window
    /// couldn't be restored — its buffer was skipped, for example) are
    /// dropped and their enclosing [`Layout::Split`] is collapsed into
    /// the surviving sibling. Returns `None` if every leaf in this
    /// subtree was dropped.
    #[must_use]
    pub fn to_layout(&self, remap: &HashMap<u64, WindowId>) -> Option<Layout> {
        match self {
            SerializedLayout::Leaf(old_id) => remap.get(old_id).map(|id| Layout::Leaf(*id)),
            SerializedLayout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let new_first = first.to_layout(remap);
                let new_second = second.to_layout(remap);
                match (new_first, new_second) {
                    (Some(a), Some(b)) => Some(Layout::Split {
                        axis: (*axis).into(),
                        ratio: *ratio,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }
}

/// A single buffer entry in a session.
///
/// For Phase 1 this is **path-only** — the buffer's contents are
/// re-read from disk on restore. A later milestone will add an
/// `unsaved_text: Option<String>` field (or, more likely, a separate
/// journal file) so in-flight edits survive a daemon crash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedBuffer {
    pub id: u64,
    pub path: Option<PathBuf>,
    // TODO(phase-1b): optional `unsaved_text: Option<String>` or a
    // pointer to a per-buffer journal file.
}

impl SerializedBuffer {
    pub fn from_id_and_path(id: BufferId, path: Option<PathBuf>) -> Self {
        Self { id: id.0, path }
    }
}

/// A single window entry in a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedWindow {
    pub id: u64,
    pub buffer_id: u64,
    pub cursor_byte: usize,
    pub scroll_top_line: usize,
    pub scroll_left_col: u16,
}

impl SerializedWindow {
    pub fn from_ids(id: WindowId, buffer_id: BufferId) -> Self {
        Self {
            id: id.0,
            buffer_id: buffer_id.0,
            cursor_byte: 0,
            scroll_top_line: 0,
            scroll_left_col: 0,
        }
    }
}

impl Session {
    /// Capture the current editor state.
    pub fn from_editor(editor: &crate::Editor) -> Self {
        let buffers: Vec<SerializedBuffer> = editor
            .buffers()
            .ids()
            .map(|id| {
                SerializedBuffer::from_id_and_path(
                    id,
                    editor.buffers().path(id).map(std::path::Path::to_path_buf),
                )
            })
            .collect();
        let windows: Vec<SerializedWindow> = editor
            .windows()
            .iter()
            .map(|(id, data)| SerializedWindow {
                id: id.0,
                buffer_id: data.buffer_id.0,
                cursor_byte: data.cursor_byte,
                scroll_top_line: data.scroll_top_line,
                scroll_left_col: data.scroll_left_col,
            })
            .collect();
        let active_window = editor.windows().active().map(|id| id.0);
        let layout = editor.windows().layout().map(SerializedLayout::from_layout);
        Session {
            buffers,
            windows,
            active_window,
            layout,
        }
    }

    /// Atomically persist this session to `path`, wrapped in a
    /// [`SessionFile`] for forward-compat versioning.
    ///
    /// Writes the postcard bytes to a sibling tempfile
    /// (`<filename>.tmp-<pid>`) first, fsyncs it, then renames it over
    /// the destination. If `path` has a parent directory, it's created
    /// with `mkdir -p` semantics. The rename is atomic on every
    /// filesystem we care about, so a crash mid-write leaves either
    /// the old session or no session — never a truncated half.
    pub async fn save_to_path(&self, path: &Path) -> Result<(), SessionIoError> {
        let file = SessionFile::new(self.clone());
        let bytes = postcard::to_stdvec(&file).map_err(SessionIoError::Encode)?;

        let parent = path.parent();
        if let Some(parent) = parent {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|source| SessionIoError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
            }
        }

        let tmp_name = match path.file_name() {
            Some(name) => {
                let mut s = name.to_os_string();
                s.push(format!(".tmp-{}", std::process::id()));
                s
            }
            None => {
                return Err(SessionIoError::InvalidPath {
                    path: path.to_path_buf(),
                });
            }
        };
        let tmp_path =
            parent.map_or_else(|| PathBuf::from(&tmp_name), |p| p.join(&tmp_name));

        {
            let mut f =
                tokio::fs::File::create(&tmp_path)
                    .await
                    .map_err(|source| SessionIoError::Io {
                        path: tmp_path.clone(),
                        source,
                    })?;
            f.write_all(&bytes)
                .await
                .map_err(|source| SessionIoError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
            // Force the bytes to disk before the rename, so a crash
            // between rename and power-off can't leave the renamed
            // file pointing at unwritten blocks on some filesystems.
            f.sync_all()
                .await
                .map_err(|source| SessionIoError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
        }
        tokio::fs::rename(&tmp_path, path)
            .await
            .map_err(|source| SessionIoError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(())
    }

    /// Load a session from `path`. Returns `Ok(None)` if the file
    /// doesn't exist (expected on first run); returns
    /// `Err(SessionIoError::VersionMismatch)` for versions this build
    /// doesn't know how to parse, and bubbles any other I/O or decode
    /// errors to the caller.
    ///
    /// v1 files (pre-Phase-2) are read via a separate compat schema
    /// that doesn't know about the layout tree; they come back with
    /// `layout = None` and restore to a single-leaf layout on the
    /// active window, matching the Phase-1 experience.
    pub async fn load_from_path(path: &Path) -> Result<Option<Self>, SessionIoError> {
        let bytes = match tokio::fs::read(path).await {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(SessionIoError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        // `SessionFile` encodes as `varint(version) ++ encode(session)`
        // with no struct delimiters (postcard is field-concatenation),
        // so we can peel the version off the front and then branch on
        // it to pick the right schema for the rest of the bytes.
        let (version, rest): (u32, &[u8]) =
            postcard::take_from_bytes(&bytes).map_err(SessionIoError::Decode)?;
        match version {
            1 => {
                let legacy: LegacySessionV1 =
                    postcard::from_bytes(rest).map_err(SessionIoError::Decode)?;
                Ok(Some(legacy.into()))
            }
            2 => {
                let session: Session =
                    postcard::from_bytes(rest).map_err(SessionIoError::Decode)?;
                Ok(Some(session))
            }
            _ => Err(SessionIoError::VersionMismatch {
                found: version,
                expected: SessionFile::CURRENT_VERSION,
            }),
        }
    }

    /// Apply this session to an editor: re-open each buffer that has
    /// a path, open a window per [`SerializedWindow`], restore
    /// cursor / scroll / active-window state, and — if the session
    /// was saved with a layout tree — rebuild the split layout.
    ///
    /// This is best-effort: buffers whose files failed to read (e.g.
    /// the user moved them since last session) are skipped, and any
    /// windows pointing at skipped buffers are dropped. The layout
    /// tree's [`SerializedLayout::to_layout`] collapses splits whose
    /// leaves disappeared, so a partial restore still produces a
    /// well-formed tree.
    ///
    /// Goes through the command bus so it respects the single-writer
    /// invariant — callers can invoke it from anywhere with access to
    /// a [`CommandBus`], typically from the daemon's startup path.
    pub async fn restore(&self, bus: &CommandBus) -> Result<RestoreSummary, crate::DispatchError> {
        let mut summary = RestoreSummary::default();

        // Map old buffer ids (in the saved file) to the freshly-
        // assigned ids we get from `open_file`. Windows look up their
        // target buffer through this map.
        let mut id_remap: HashMap<u64, BufferId> = HashMap::new();

        for serialized in &self.buffers {
            let Some(path) = serialized.path.as_ref() else {
                // Scratch buffers don't carry content across restarts
                // in Level 1. Skip — the window pointing at it will
                // be dropped and we'll fall back to a fresh scratch
                // later.
                summary.skipped_buffers += 1;
                continue;
            };
            match crate::open_file(bus, path.clone()).await {
                Ok((buffer_id, _window_id)) => {
                    // `open_file` also creates a window on the buffer,
                    // but we don't actually want the auto-created one
                    // because we're about to restore the saved window
                    // layout. Close it so the restore step can open a
                    // fresh window with the saved cursor / scroll.
                    let active_cleanup = bus
                        .invoke(move |editor| {
                            if let Some(win) = editor.windows().active() {
                                editor.windows_mut().close(win);
                            }
                        })
                        .await;
                    if active_cleanup.is_err() {
                        return Err(crate::DispatchError::Closed);
                    }
                    id_remap.insert(serialized.id, buffer_id);
                    summary.restored_buffers += 1;
                }
                Err(err) => {
                    tracing::warn!(
                        %err,
                        path = %path.display(),
                        "session restore: failed to reopen buffer",
                    );
                    summary.skipped_buffers += 1;
                }
            }
        }

        // Re-open windows in the same order they were saved, building
        // the saved-id → new-id remap alongside so the layout tree
        // (if any) can be rehydrated afterwards.
        let windows = self.windows.clone();
        let active_source_id = self.active_window;
        let serialized_layout = self.layout.clone();
        let (restored, layout_applied) = bus
            .invoke(move |editor| {
                let mut restored = 0usize;
                let mut new_active: Option<crate::WindowId> = None;
                let mut window_id_remap: HashMap<u64, crate::WindowId> = HashMap::new();
                for w in &windows {
                    let Some(&buffer_id) = id_remap.get(&w.buffer_id) else {
                        continue;
                    };
                    let window_id = editor.windows_mut().open(buffer_id);
                    if let Some(data) = editor.windows_mut().get_mut(window_id) {
                        data.cursor_byte = w.cursor_byte;
                        data.scroll_top_line = w.scroll_top_line;
                        data.scroll_left_col = w.scroll_left_col;
                    }
                    window_id_remap.insert(w.id, window_id);
                    if active_source_id == Some(w.id) {
                        new_active = Some(window_id);
                    }
                    restored += 1;
                }

                // Rehydrate the saved layout against the new ids and
                // install it. When the session has no layout (v1 on
                // disk, or an empty editor) or when every leaf was
                // pruned, fall through to the Phase-1 behaviour where
                // `set_active` below resets the layout to a single
                // leaf on the active window.
                let layout_applied = serialized_layout
                    .as_ref()
                    .and_then(|l| l.to_layout(&window_id_remap))
                    .is_some_and(|rebuilt| editor.windows_mut().set_layout(rebuilt));

                if let Some(id) = new_active {
                    editor.windows_mut().set_active(id);
                }
                editor.mark_dirty();
                (restored, layout_applied)
            })
            .await?;
        summary.restored_windows = restored;
        summary.restored_layout = layout_applied;
        Ok(summary)
    }
}

/// Summary of a [`Session::restore`] call. Mostly useful for logging
/// and tests — callers typically just propagate any underlying error
/// and otherwise ignore these counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestoreSummary {
    pub restored_buffers: usize,
    pub skipped_buffers: usize,
    pub restored_windows: usize,
    /// Whether a saved layout tree was rehydrated and installed on
    /// the editor. `false` either means the session had no layout
    /// (pre-Phase-2 v1 file, or an empty editor at save time) or
    /// that every leaf got pruned because its window couldn't be
    /// restored. In both cases the editor falls back to the Phase-1
    /// "single-leaf on active window" behaviour.
    pub restored_layout: bool,
}

/// Errors from [`Session::save_to_path`] / [`Session::load_from_path`].
#[derive(Debug, Error)]
pub enum SessionIoError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to encode session: {0}")]
    Encode(#[source] postcard::Error),
    #[error("failed to decode session: {0}")]
    Decode(#[source] postcard::Error),
    #[error("session file has version {found}; expected {expected}")]
    VersionMismatch { found: u32, expected: u32 },
    #[error("session path has no filename: {path}")]
    InvalidPath { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventLoop;

    #[tokio::test]
    async fn session_from_editor_captures_shape() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());

        let (buf_id, win_id) = bus
            .invoke(|editor| {
                let buf = editor
                    .buffers_mut()
                    .create_from_text("hello", Some(PathBuf::from("/tmp/x.rs")));
                let win = editor.windows_mut().open(buf);
                (buf, win)
            })
            .await
            .unwrap();

        let session = bus
            .invoke(|editor| Session::from_editor(editor))
            .await
            .unwrap();
        assert_eq!(session.buffers.len(), 1);
        assert_eq!(session.buffers[0].id, buf_id.0);
        assert_eq!(session.buffers[0].path.as_deref(), Some(std::path::Path::new("/tmp/x.rs")));
        assert_eq!(session.windows.len(), 1);
        assert_eq!(session.windows[0].buffer_id, buf_id.0);
        assert_eq!(session.active_window, Some(win_id.0));

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[test]
    fn session_file_roundtrip_postcard() {
        let session = Session {
            buffers: vec![SerializedBuffer {
                id: 1,
                path: Some(PathBuf::from("/tmp/x.rs")),
            }],
            windows: vec![SerializedWindow {
                id: 1,
                buffer_id: 1,
                cursor_byte: 42,
                scroll_top_line: 3,
                scroll_left_col: 0,
            }],
            active_window: Some(1),
            layout: Some(SerializedLayout::Leaf(1)),
        };
        let file = SessionFile::new(session.clone());
        let bytes = postcard::to_stdvec(&file).unwrap();
        let decoded: SessionFile = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.version, SessionFile::CURRENT_VERSION);
        assert_eq!(decoded.session, session);
    }

    #[tokio::test]
    async fn save_to_path_then_load_from_path_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        // Deliberately nest under a *non-existent* subdirectory so we
        // exercise the `create_dir_all` path inside save_to_path.
        let path = dir.path().join("nested").join("sub").join("session.postcard");

        let session = Session {
            buffers: vec![SerializedBuffer {
                id: 3,
                path: Some(PathBuf::from("/tmp/hello.rs")),
            }],
            windows: vec![SerializedWindow {
                id: 7,
                buffer_id: 3,
                cursor_byte: 12,
                scroll_top_line: 1,
                scroll_left_col: 4,
            }],
            active_window: Some(7),
            layout: Some(SerializedLayout::Leaf(7)),
        };
        session.save_to_path(&path).await.unwrap();
        assert!(path.exists());

        let loaded = Session::load_from_path(&path).await.unwrap().unwrap();
        assert_eq!(loaded, session);
    }

    #[tokio::test]
    async fn load_from_path_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("never-written.postcard");
        let loaded = Session::load_from_path(&missing).await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn load_from_path_rejects_future_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.postcard");
        let file = SessionFile {
            version: SessionFile::CURRENT_VERSION + 1,
            session: Session::default(),
        };
        let bytes = postcard::to_stdvec(&file).unwrap();
        tokio::fs::write(&path, &bytes).await.unwrap();
        let err = Session::load_from_path(&path).await.unwrap_err();
        assert!(matches!(err, SessionIoError::VersionMismatch { .. }));
    }

    #[tokio::test]
    async fn save_atomically_replaces_existing_file() {
        // Write once, then save over the same path and verify the
        // result has the *new* contents and no tempfile left over.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.postcard");
        let first = Session::default();
        first.save_to_path(&path).await.unwrap();
        let second = Session {
            buffers: vec![SerializedBuffer {
                id: 99,
                path: Some(PathBuf::from("/tmp/b.rs")),
            }],
            ..Session::default()
        };
        second.save_to_path(&path).await.unwrap();
        let loaded = Session::load_from_path(&path).await.unwrap().unwrap();
        assert_eq!(loaded, second);
        // Tempfile shouldn't leak into the directory.
        let mut read_dir = tokio::fs::read_dir(dir.path()).await.unwrap();
        let mut names = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        assert_eq!(names, vec!["session.postcard".to_string()]);
    }

    #[tokio::test]
    async fn restore_reopens_files_and_rehomes_windows() {
        // Write a real file to disk, snapshot a session pointing at
        // it with an unusual cursor position, then round-trip through
        // save + load + restore and check the cursor is back.
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("hello.txt");
        tokio::fs::write(&source_path, "the quick brown fox").await.unwrap();
        let session_path = dir.path().join("session.postcard");

        // Session 1: open the file, move cursor, save.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        let open_path = source_path.clone();
        let (buf_id, win_id) = bus
            .invoke(move |editor| {
                let contents = std::fs::read_to_string(&open_path).unwrap();
                let buf = editor
                    .buffers_mut()
                    .create_from_text(&contents, Some(open_path.clone()));
                let win = editor.windows_mut().open(buf);
                editor.windows_mut().get_mut(win).unwrap().cursor_byte = 10;
                editor.windows_mut().get_mut(win).unwrap().scroll_top_line = 2;
                editor.windows_mut().get_mut(win).unwrap().scroll_left_col = 5;
                (buf, win)
            })
            .await
            .unwrap();
        let _ = buf_id;
        let _ = win_id;
        let session = bus
            .invoke(|editor| Session::from_editor(editor))
            .await
            .unwrap();
        session.save_to_path(&session_path).await.unwrap();
        drop(bus);
        let _ = handle.await.unwrap();

        // Session 2: fresh event loop, reload, restore.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        let loaded = Session::load_from_path(&session_path).await.unwrap().unwrap();
        let summary = loaded.restore(&bus).await.unwrap();
        assert_eq!(summary.restored_buffers, 1);
        assert_eq!(summary.restored_windows, 1);

        let data = bus
            .invoke(|editor| {
                let id = editor.windows().active().unwrap();
                editor.windows().get(id).cloned()
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(data.cursor_byte, 10);
        assert_eq!(data.scroll_top_line, 2);
        assert_eq!(data.scroll_left_col, 5);
        // And the buffer itself is back with the original text.
        let text = bus
            .invoke(move |editor| {
                let id = editor.windows().active().unwrap();
                let buf_id = editor.windows().get(id).unwrap().buffer_id;
                editor.buffers().get(buf_id).unwrap().text()
            })
            .await
            .unwrap();
        assert_eq!(text, "the quick brown fox");

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn restore_skips_buffers_with_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing_path = dir.path().join("was-deleted.txt");

        let session = Session {
            buffers: vec![SerializedBuffer {
                id: 42,
                path: Some(missing_path.clone()),
            }],
            windows: vec![SerializedWindow {
                id: 1,
                buffer_id: 42,
                cursor_byte: 0,
                scroll_top_line: 0,
                scroll_left_col: 0,
            }],
            active_window: Some(1),
            layout: Some(SerializedLayout::Leaf(1)),
        };
        // `open_file` treats NotFound as "start empty", so a missing
        // file does not, in fact, fail restore — it comes back as an
        // empty buffer at the saved path. Verify the restore completes
        // and reports the buffer as restored, not skipped.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        let summary = session.restore(&bus).await.unwrap();
        assert_eq!(summary.restored_buffers, 1);
        assert_eq!(summary.restored_windows, 1);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    // ---- Layout persistence (Phase 2) ----

    /// A v1-on-disk `SessionFile` for backward-compat coverage. Must
    /// match the v1 wire shape exactly so postcard's field-by-field
    /// decoder rebuilds the same bytes.
    #[derive(Serialize)]
    struct LegacySessionFileV1 {
        version: u32,
        session: LegacySessionV1,
    }

    #[test]
    fn serialized_layout_roundtrips_through_window_layout() {
        let original = Layout::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(Layout::Leaf(WindowId(1))),
            second: Box::new(Layout::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.7,
                first: Box::new(Layout::Leaf(WindowId(2))),
                second: Box::new(Layout::Leaf(WindowId(3))),
            }),
        };
        let serialized = SerializedLayout::from_layout(&original);
        let mut remap: HashMap<u64, WindowId> = HashMap::new();
        remap.insert(1, WindowId(1));
        remap.insert(2, WindowId(2));
        remap.insert(3, WindowId(3));
        let rehydrated = serialized.to_layout(&remap).unwrap();
        assert_eq!(rehydrated, original);
    }

    #[test]
    fn serialized_layout_remaps_ids_correctly() {
        let saved = SerializedLayout::Split {
            axis: SerializedSplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(SerializedLayout::Leaf(10)),
            second: Box::new(SerializedLayout::Leaf(20)),
        };
        let mut remap = HashMap::new();
        remap.insert(10, WindowId(101));
        remap.insert(20, WindowId(202));
        let layout = saved.to_layout(&remap).unwrap();
        assert_eq!(
            layout.leaves(),
            vec![WindowId(101), WindowId(202)],
        );
    }

    #[test]
    fn serialized_layout_drops_unmapped_leaves() {
        // Simulate a session where window id 2 couldn't be restored:
        // the split should collapse into the surviving leaf (id 1).
        let saved = SerializedLayout::Split {
            axis: SerializedSplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(SerializedLayout::Leaf(1)),
            second: Box::new(SerializedLayout::Leaf(2)),
        };
        let mut remap = HashMap::new();
        remap.insert(1, WindowId(100));
        let layout = saved.to_layout(&remap).unwrap();
        assert_eq!(layout, Layout::Leaf(WindowId(100)));
    }

    #[test]
    fn serialized_layout_returns_none_when_everything_pruned() {
        let saved = SerializedLayout::Split {
            axis: SerializedSplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(SerializedLayout::Leaf(1)),
            second: Box::new(SerializedLayout::Leaf(2)),
        };
        let remap = HashMap::new();
        assert!(saved.to_layout(&remap).is_none());
    }

    #[tokio::test]
    async fn from_editor_captures_split_layout() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let a = editor
                .buffers_mut()
                .create_from_text("one", Some(PathBuf::from("/tmp/a.rs")));
            editor.windows_mut().open(a);
            editor
                .windows_mut()
                .split_active(SplitAxis::Vertical, a)
                .unwrap();
        })
        .await
        .unwrap();
        let session = bus
            .invoke(|editor| Session::from_editor(editor))
            .await
            .unwrap();
        let layout = session.layout.expect("layout captured");
        match layout {
            SerializedLayout::Split {
                axis: SerializedSplitAxis::Vertical,
                ..
            } => {}
            other => panic!("expected vertical split, got {other:?}"),
        }
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn restore_rebuilds_split_layout_with_new_ids() {
        // Write two files to disk, open both in a single-pane session
        // and split, save, then restore into a fresh editor and
        // verify the layout tree has the same shape (two leaves,
        // vertical split).
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        tokio::fs::write(&file_a, "aaaa").await.unwrap();
        tokio::fs::write(&file_b, "bbbb").await.unwrap();
        let session_path = dir.path().join("session.postcard");

        // Session 1: two windows split vertically, second pane active.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        let path_a = file_a.clone();
        let path_b = file_b.clone();
        bus.invoke(move |editor| {
            let a = editor.buffers_mut().create_from_text(
                &std::fs::read_to_string(&path_a).unwrap(),
                Some(path_a),
            );
            let b = editor.buffers_mut().create_from_text(
                &std::fs::read_to_string(&path_b).unwrap(),
                Some(path_b),
            );
            editor.windows_mut().open(a);
            editor.windows_mut().split_active(SplitAxis::Vertical, b).unwrap();
        })
        .await
        .unwrap();
        let session = bus
            .invoke(|editor| Session::from_editor(editor))
            .await
            .unwrap();
        session.save_to_path(&session_path).await.unwrap();
        drop(bus);
        let _ = handle.await.unwrap();

        // Session 2: restore and inspect the layout.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        let loaded = Session::load_from_path(&session_path).await.unwrap().unwrap();
        let summary = loaded.restore(&bus).await.unwrap();
        assert_eq!(summary.restored_buffers, 2);
        assert_eq!(summary.restored_windows, 2);
        assert!(summary.restored_layout, "layout should be rehydrated");

        let shape = bus
            .invoke(|editor| {
                let layout = editor.windows().layout().unwrap().clone();
                let leaves = layout.leaves().len();
                let is_vertical_split = matches!(
                    layout,
                    Layout::Split {
                        axis: SplitAxis::Vertical,
                        ..
                    }
                );
                (leaves, is_vertical_split)
            })
            .await
            .unwrap();
        assert_eq!(shape.0, 2, "two leaves after restore");
        assert!(shape.1, "should be a vertical split at the root");

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn load_from_path_reads_v1_files_as_session_without_layout() {
        // Hand-craft a v1-on-disk session (no layout field), write it
        // out, and verify load_from_path lifts it into the current
        // Session shape with layout = None.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.postcard");
        let legacy = LegacySessionFileV1 {
            version: 1,
            session: LegacySessionV1 {
                buffers: vec![SerializedBuffer {
                    id: 1,
                    path: Some(PathBuf::from("/tmp/old.rs")),
                }],
                windows: vec![SerializedWindow {
                    id: 1,
                    buffer_id: 1,
                    cursor_byte: 7,
                    scroll_top_line: 0,
                    scroll_left_col: 0,
                }],
                active_window: Some(1),
            },
        };
        let bytes = postcard::to_stdvec(&legacy).unwrap();
        tokio::fs::write(&path, &bytes).await.unwrap();

        let loaded = Session::load_from_path(&path).await.unwrap().unwrap();
        assert_eq!(loaded.buffers.len(), 1);
        assert_eq!(loaded.windows.len(), 1);
        assert_eq!(loaded.active_window, Some(1));
        assert!(
            loaded.layout.is_none(),
            "v1 file should have no layout after load",
        );
    }

    #[tokio::test]
    async fn restore_v1_session_falls_back_to_single_leaf_layout() {
        // A v1 session restore has no layout — the restored editor
        // should still have a single-leaf layout on the active
        // window, matching the Phase-1 experience.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("legacy.txt");
        tokio::fs::write(&file, "legacy content").await.unwrap();

        let session = Session {
            buffers: vec![SerializedBuffer {
                id: 1,
                path: Some(file.clone()),
            }],
            windows: vec![SerializedWindow {
                id: 1,
                buffer_id: 1,
                cursor_byte: 3,
                scroll_top_line: 0,
                scroll_left_col: 0,
            }],
            active_window: Some(1),
            layout: None,
        };

        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        let summary = session.restore(&bus).await.unwrap();
        assert!(!summary.restored_layout);

        let leaves_and_active = bus
            .invoke(|editor| {
                let layout = editor.windows().layout().unwrap().clone();
                let active = editor.windows().active().unwrap();
                (layout.leaves().len(), active, layout)
            })
            .await
            .unwrap();
        assert_eq!(leaves_and_active.0, 1);
        assert_eq!(leaves_and_active.2, Layout::Leaf(leaves_and_active.1));

        drop(bus);
        let _ = handle.await.unwrap();
    }
}
