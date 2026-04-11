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

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use arx_buffer::BufferId;
use crate::WindowId;

/// Top-level on-disk session file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFile {
    /// Schema version. Start at 1; bump on incompatible changes.
    pub version: u32,
    /// The actual session state.
    pub session: Session,
}

impl SessionFile {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new(session: Session) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            session,
        }
    }
}

/// A serialisable snapshot of the editor's persistent state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Session {
    /// All open buffers at snapshot time.
    pub buffers: Vec<SerializedBuffer>,
    /// All open windows at snapshot time.
    pub windows: Vec<SerializedWindow>,
    /// Which window was active.
    pub active_window: Option<u64>,
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
        Session {
            buffers,
            windows,
            active_window,
        }
    }
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
        };
        let file = SessionFile::new(session.clone());
        let bytes = postcard::to_stdvec(&file).unwrap();
        let decoded: SessionFile = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.version, SessionFile::CURRENT_VERSION);
        assert_eq!(decoded.session, session);
    }
}
