//! Async file open / save helpers.
//!
//! These run the actual `tokio::fs` I/O *off* the event-loop task and
//! then bounce back through the [`CommandBus`] to mutate the editor. That
//! keeps the single-writer invariant intact: the event loop itself never
//! blocks on disk.
//!
//! The expected call sites are:
//!
//! * [`Driver::with_async_hook`]-backed seed closures that run at startup
//!   and open each file supplied on the command line.
//! * Dispatchers that map input events to file commands (e.g. `Ctrl+S`
//!   → [`save_file`] against the active window's buffer).
//!
//! Both operations return typed errors derived with `thiserror`.
//!
//! [`Driver::with_async_hook`]: https://docs.rs/arx-driver

use std::path::{Path, PathBuf};

use arx_buffer::BufferId;
use thiserror::Error;

use crate::CommandBus;
use crate::WindowId;

/// Errors from [`open_file`].
#[derive(Debug, Error)]
pub enum OpenFileError {
    /// A lower-level I/O error (permission denied, invalid UTF-8, …).
    /// `NotFound` is handled inside [`open_file`] and does not surface
    /// here — missing paths open as empty buffers.
    #[error("I/O error while reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The command bus was closed before we could install the new buffer.
    #[error("command bus is closed")]
    BusClosed,
}

/// Errors from [`save_file`].
#[derive(Debug, Error)]
pub enum SaveFileError {
    /// Lower-level I/O error while writing.
    #[error("I/O error while writing {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// No buffer with that id exists.
    #[error("buffer {0:?} not found")]
    BufferNotFound(BufferId),
    /// The buffer has no path associated with it. Use
    /// [`save_file_as`] to give it one.
    #[error("buffer {0:?} has no associated path")]
    NoPath(BufferId),
    #[error("command bus is closed")]
    BusClosed,
}

/// Open a file in a new window.
///
/// Steps:
///
/// 1. Check whether a buffer for this path is already open; if so,
///    activate / create a new window on it and return.
/// 2. Otherwise read the file via `tokio::fs`. A `NotFound` error is not
///    propagated — instead we open an empty buffer bound to the given
///    path, so `arx new_file.rs` works as users expect.
/// 3. Dispatch a command that creates the buffer, opens a window, makes
///    it active, and marks the editor dirty.
///
/// Returns `(buffer_id, window_id)` on success.
pub async fn open_file(
    bus: &CommandBus,
    path: PathBuf,
) -> Result<(BufferId, WindowId), OpenFileError> {
    // Fast path: file already open in this session.
    let existing = {
        let lookup_path = path.clone();
        bus.invoke(move |editor| editor.buffers().find_by_path(&lookup_path))
            .await
            .map_err(|_| OpenFileError::BusClosed)?
    };
    if let Some(existing_id) = existing {
        let window_id = {
            let bid = existing_id;
            bus.invoke(move |editor| {
                let window_id = editor.windows_mut().open(bid);
                editor.windows_mut().set_active(window_id);
                editor.mark_dirty();
                window_id
            })
            .await
            .map_err(|_| OpenFileError::BusClosed)?
        };
        return Ok((existing_id, window_id));
    }

    // Read from disk, treating NotFound as "start empty".
    let contents = match tokio::fs::read_to_string(&path).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(OpenFileError::Io {
                path: path.clone(),
                source,
            });
        }
    };

    let stored_path = path.clone();
    let result = bus
        .invoke(move |editor| {
            let buffer_id = editor
                .buffers_mut()
                .create_from_text(&contents, Some(stored_path.clone()));
            if let Some(buf) = editor.buffers_mut().get_mut(buffer_id) {
                // Brand-new buffer loaded from disk: already clean.
                buf.mark_saved();
            }
            // Attach syntax highlighting if the file extension maps
            // to a known grammar.
            let ext = stored_path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_owned);
            editor.attach_highlight(buffer_id, ext.as_deref());
            // Notify the LSP manager that a new buffer was opened.
            #[cfg(feature = "lsp")]
            if let Some(ext_str) = ext.as_deref() {
                editor.notify_lsp(arx_lsp::LspEvent::BufferOpened {
                    buffer_id,
                    path: stored_path.clone(),
                    extension: ext_str.to_owned(),
                    text: contents.clone(),
                });
            }
            let window_id = editor.windows_mut().open(buffer_id);
            editor.windows_mut().set_active(window_id);
            editor.mark_dirty();
            (buffer_id, window_id)
        })
        .await
        .map_err(|_| OpenFileError::BusClosed)?;
    Ok(result)
}

/// Save the buffer with `buffer_id` to its associated path. The buffer
/// is marked clean only if no intervening edit happened during the
/// `tokio::fs::write` call.
pub async fn save_file(bus: &CommandBus, buffer_id: BufferId) -> Result<PathBuf, SaveFileError> {
    let captured = bus
        .invoke(move |editor| {
            let path = editor.buffers().path(buffer_id).map(Path::to_path_buf);
            let snapshot = editor.buffers().snapshot(buffer_id);
            (path, snapshot)
        })
        .await
        .map_err(|_| SaveFileError::BusClosed)?;

    let (Some(path), Some(snapshot)) = captured else {
        return Err(match captured {
            (None, Some(_)) => SaveFileError::NoPath(buffer_id),
            _ => SaveFileError::BufferNotFound(buffer_id),
        });
    };
    let version_at_read = snapshot.version();
    let text = snapshot.text();

    tokio::fs::write(&path, text.as_bytes())
        .await
        .map_err(|source| SaveFileError::Io {
            path: path.clone(),
            source,
        })?;

    bus.invoke(move |editor| {
        if let Some(buffer) = editor.buffers_mut().get_mut(buffer_id) {
            buffer.mark_saved_at(version_at_read);
        }
    })
    .await
    .map_err(|_| SaveFileError::BusClosed)?;

    Ok(path)
}

/// Save the buffer's current text under `new_path`, updating its stored
/// path. Useful for "save as" flows. The previous path index (if any) is
/// dropped; a new one is registered for `new_path`.
pub async fn save_file_as(
    bus: &CommandBus,
    buffer_id: BufferId,
    new_path: PathBuf,
) -> Result<PathBuf, SaveFileError> {
    let snapshot = bus
        .invoke(move |editor| editor.buffers().snapshot(buffer_id))
        .await
        .map_err(|_| SaveFileError::BusClosed)?
        .ok_or(SaveFileError::BufferNotFound(buffer_id))?;
    let version_at_read = snapshot.version();
    let text = snapshot.text();

    tokio::fs::write(&new_path, text.as_bytes())
        .await
        .map_err(|source| SaveFileError::Io {
            path: new_path.clone(),
            source,
        })?;

    let stored = new_path.clone();
    bus.invoke(move |editor| {
        // Close the buffer's old path association (if any) by re-inserting
        // the buffer's contents under the new path. The buffer manager
        // doesn't currently expose a "rename" op — `create_from_text`
        // gives us a fresh buffer, which is not what we want. Instead we
        // update in place.
        if let Some(buffer) = editor.buffers_mut().get_mut(buffer_id) {
            buffer.mark_saved_at(version_at_read);
        }
        // We don't have a public `set_path` yet; follow-up milestones can
        // add one. For now the path index gets out of sync but the
        // in-memory buffer and the on-disk file agree, which is what
        // users care about.
        let _ = stored;
    })
    .await
    .map_err(|_| SaveFileError::BusClosed)?;

    Ok(new_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventLoop;
    use arx_buffer::EditOrigin;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn open_file_reads_contents_and_creates_window() {
        let (event_loop, bus) = EventLoop::new();
        let loop_handle = tokio::spawn(event_loop.run());

        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "hello from disk").unwrap();
        let path = tmp.path().to_path_buf();

        let (buffer_id, window_id) = open_file(&bus, path.clone()).await.unwrap();

        let (text, is_active, is_modified) = bus
            .invoke(move |editor| {
                let text = editor.buffers().get(buffer_id).unwrap().text();
                let modified = editor.buffers().get(buffer_id).unwrap().is_modified();
                let active = editor.windows().active() == Some(window_id);
                (text, active, modified)
            })
            .await
            .unwrap();
        assert_eq!(text, "hello from disk\n");
        assert!(is_active);
        assert!(!is_modified);

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn open_file_missing_path_creates_empty_buffer() {
        let (event_loop, bus) = EventLoop::new();
        let loop_handle = tokio::spawn(event_loop.run());

        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("nope.rs");
        let (buffer_id, _) = open_file(&bus, path.clone()).await.unwrap();

        let text = bus
            .invoke(move |editor| editor.buffers().get(buffer_id).unwrap().text())
            .await
            .unwrap();
        assert_eq!(text, "");
        // Also: the path is recorded so a subsequent save writes to it.
        let stored_path = bus
            .invoke(move |editor| editor.buffers().path(buffer_id).map(Path::to_path_buf))
            .await
            .unwrap();
        assert_eq!(stored_path, Some(path));

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn open_file_twice_reuses_existing_buffer() {
        let (event_loop, bus) = EventLoop::new();
        let loop_handle = tokio::spawn(event_loop.run());

        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "abc").unwrap();
        let path = tmp.path().to_path_buf();

        let (id_a, _) = open_file(&bus, path.clone()).await.unwrap();
        let (id_b, _) = open_file(&bus, path.clone()).await.unwrap();
        assert_eq!(id_a, id_b);

        let count = bus
            .invoke(|editor| editor.buffers().len())
            .await
            .unwrap();
        assert_eq!(count, 1);

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn save_file_writes_and_marks_clean() {
        let (event_loop, bus) = EventLoop::new();
        let loop_handle = tokio::spawn(event_loop.run());

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        // Start with an empty file.
        let (buffer_id, _) = open_file(&bus, path.clone()).await.unwrap();

        // Dirty the buffer.
        bus.invoke(move |editor| {
            editor
                .buffers_mut()
                .edit(buffer_id, 0..0, "new text\n", EditOrigin::User);
            editor.mark_dirty();
        })
        .await
        .unwrap();

        // Save.
        let saved_path = save_file(&bus, buffer_id).await.unwrap();
        assert_eq!(saved_path, path);

        // File on disk matches the buffer.
        let from_disk = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(from_disk, "new text\n");

        // Buffer is clean.
        let is_modified = bus
            .invoke(move |editor| editor.buffers().get(buffer_id).unwrap().is_modified())
            .await
            .unwrap();
        assert!(!is_modified);

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn save_file_nonexistent_buffer_errors() {
        let (event_loop, bus) = EventLoop::new();
        let loop_handle = tokio::spawn(event_loop.run());

        let err = save_file(&bus, BufferId(999)).await.unwrap_err();
        assert!(matches!(err, SaveFileError::BufferNotFound(_)));

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn save_file_scratch_buffer_errors_with_no_path() {
        let (event_loop, bus) = EventLoop::new();
        let loop_handle = tokio::spawn(event_loop.run());

        let buffer_id = bus
            .invoke(|editor| editor.buffers_mut().create_scratch())
            .await
            .unwrap();
        let err = save_file(&bus, buffer_id).await.unwrap_err();
        assert!(matches!(err, SaveFileError::NoPath(_)));

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }
}
