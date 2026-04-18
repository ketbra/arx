//! LSP client for the Arx editor.
//!
//! This crate provides a minimal but functional Language Server
//! Protocol client: spawn a language server, keep it in sync with
//! buffer edits, display diagnostics, and request hover info.
//!
//! ## Architecture
//!
//! The transport runs in its own tokio tasks (reader + writer) and
//! communicates with the editor through channels. Server-initiated
//! notifications (like `publishDiagnostics`) are converted into
//! [`LspEvent`]s that the driver's LSP manager task dispatches into
//! the editor via `CommandBus::dispatch`.
//!
//! The crate is deliberately agnostic of `arx-core` — it depends only
//! on `arx-buffer` for the rope and property-map types. Integration
//! wiring lives in `arx-core` (feature-gated) and `arx-driver`.

pub mod client;
pub mod codec;
pub mod config;
pub mod diagnostics;
pub mod position;
pub mod transport;

use std::path::PathBuf;

use arx_buffer::BufferId;

pub use client::{LspClient, LspClientError};
pub use config::{
    LspRegistry, LspServerConfig, OwnedLspServerConfig, ResolvedLspConfig,
    config_for_extension,
};
pub use transport::{LspTransport, TransportError};

/// Events that the editor sends to the LSP manager task.
#[derive(Debug, Clone)]
pub enum LspEvent {
    /// A buffer was opened with a known file path and extension.
    BufferOpened {
        buffer_id: BufferId,
        path: PathBuf,
        extension: String,
        text: String,
    },
    /// A buffer's content changed after an edit.
    BufferEdited {
        buffer_id: BufferId,
        new_text: String,
    },
    /// A buffer was closed.
    BufferClosed {
        buffer_id: BufferId,
    },
}

/// Detect the workspace root for an LSP server by walking up from
/// `file_dir` and looking for any of `markers`. Returns `file_dir`
/// itself if no marker is found.
pub fn find_root(file_dir: &std::path::Path, markers: &[&str]) -> PathBuf {
    let mut dir = file_dir.to_path_buf();
    loop {
        for marker in markers {
            if dir.join(marker).exists() {
                return dir;
            }
        }
        if !dir.pop() {
            return file_dir.to_path_buf();
        }
    }
}
