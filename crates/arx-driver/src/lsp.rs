//! LSP manager task: spawns and manages language servers on behalf of
//! the editor.
//!
//! The manager runs as its own tokio task, listening for [`LspEvent`]s
//! from the editor (buffer opened, edited, closed) and routing them to
//! the appropriate [`LspClient`]. Server-initiated notifications like
//! `textDocument/publishDiagnostics` are handled by a dedicated
//! per-server notification task that converts them into `CommandBus`
//! dispatches to mutate the editor's property map.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use arx_buffer::{AdjustmentPolicy, BufferId};
use arx_core::CommandBus;
use arx_lsp::{LspClient, LspEvent, config_for_extension, diagnostics, find_root};

/// Per-buffer bookkeeping for the LSP manager.
#[derive(Debug)]
struct TrackedBuffer {
    path: PathBuf,
    #[allow(dead_code)]
    extension: String,
    version: i32,
    language_id: &'static str,
}

/// A running server and its tracked documents.
#[derive(Debug)]
struct ServerEntry {
    client: LspClient,
    buffers: Vec<BufferId>,
}

/// The manager's state. Runs inside its own tokio task.
#[derive(Debug)]
pub struct LspManager {
    bus: CommandBus,
    servers: HashMap<&'static str, ServerEntry>,
    tracked: HashMap<BufferId, TrackedBuffer>,
    /// Maps absolute path to `BufferId` so the notification task can
    /// resolve `publishDiagnostics` URIs back to buffer ids.
    path_index: Arc<std::sync::Mutex<HashMap<PathBuf, BufferId>>>,
}

impl LspManager {
    pub fn new(bus: CommandBus) -> Self {
        Self {
            bus,
            servers: HashMap::new(),
            tracked: HashMap::new(),
            path_index: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Run the manager loop, consuming events until the channel
    /// closes (editor shutdown).
    pub async fn run(mut self, mut rx: mpsc::Receiver<LspEvent>) {
        debug!("LSP manager started");
        while let Some(event) = rx.recv().await {
            match event {
                LspEvent::BufferOpened {
                    buffer_id,
                    path,
                    extension,
                    text,
                } => {
                    self.handle_open(buffer_id, path, extension, text).await;
                }
                LspEvent::BufferEdited {
                    buffer_id,
                    new_text,
                } => {
                    self.handle_edit(buffer_id, new_text).await;
                }
                LspEvent::BufferClosed { buffer_id } => {
                    self.handle_close(buffer_id).await;
                }
            }
        }
        debug!("LSP manager shutting down, stopping servers");
        self.shutdown_all().await;
    }

    async fn handle_open(
        &mut self,
        buffer_id: BufferId,
        path: PathBuf,
        extension: String,
        text: String,
    ) {
        let Some(config) = config_for_extension(&extension) else {
            return;
        };

        // Update the path index so the notification task can resolve
        // diagnostic URIs.
        {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir().unwrap_or_default().join(&path)
            };
            self.path_index.lock().unwrap().insert(abs, buffer_id);
        }

        self.tracked.insert(
            buffer_id,
            TrackedBuffer {
                path: path.clone(),
                extension,
                version: 0,
                language_id: config.language_id,
            },
        );

        // Ensure the server for this language is running.
        if !self.servers.contains_key(config.language_id) {
            if let Err(err) = self.start_server(config, &path).await {
                warn!(%err, language = config.name, "failed to start LSP server");
                return;
            }
        }

        let Some(entry) = self.servers.get_mut(config.language_id) else {
            return;
        };
        entry.buffers.push(buffer_id);

        let uri = path_to_uri(&path);
        if let Err(err) = entry
            .client
            .did_open(uri, config.language_id, 0, text)
            .await
        {
            warn!(%err, "didOpen failed");
        }
    }

    async fn handle_edit(&mut self, buffer_id: BufferId, new_text: String) {
        let Some(tracked) = self.tracked.get_mut(&buffer_id) else {
            return;
        };
        tracked.version += 1;
        let version = tracked.version;
        let uri = path_to_uri(&tracked.path);
        let lang_id = tracked.language_id;

        let Some(entry) = self.servers.get(lang_id) else {
            return;
        };
        if let Err(err) = entry.client.did_change(uri, version, new_text).await {
            warn!(%err, "didChange failed");
        }
    }

    async fn handle_close(&mut self, buffer_id: BufferId) {
        let Some(tracked) = self.tracked.remove(&buffer_id) else {
            return;
        };
        let abs = if tracked.path.is_absolute() {
            tracked.path.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(&tracked.path)
        };
        self.path_index.lock().unwrap().remove(&abs);

        let uri = path_to_uri(&tracked.path);
        if let Some(entry) = self.servers.get_mut(tracked.language_id) {
            if let Err(err) = entry.client.did_close(uri).await {
                warn!(%err, "didClose failed");
            }
            entry.buffers.retain(|id| *id != buffer_id);
        }
    }

    async fn start_server(
        &mut self,
        config: &'static arx_lsp::LspServerConfig,
        first_file: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            language = config.name,
            command = config.command,
            "spawning LSP server",
        );
        let mut client = LspClient::spawn(config)?;

        let root = first_file.parent().map_or_else(
            || PathBuf::from("."),
            |dir| find_root(dir, config.root_markers),
        );
        let root_uri = path_to_uri(&root);

        client.initialize(root_uri).await?;

        // Extract the notification receiver and spawn a dedicated
        // task that processes server-initiated messages.
        if let Some(notif_rx) = client.take_notification_rx() {
            let bus = self.bus.clone();
            let path_index = Arc::clone(&self.path_index);
            tokio::spawn(notification_task(notif_rx, bus, path_index));
        }

        self.servers.insert(
            config.language_id,
            ServerEntry {
                client,
                buffers: Vec::new(),
            },
        );
        Ok(())
    }

    async fn shutdown_all(&mut self) {
        for (lang, entry) in &mut self.servers {
            info!(language = lang, "shutting down LSP server");
            if let Err(err) = entry.client.shutdown().await {
                warn!(%err, language = lang, "LSP shutdown failed");
            }
        }
        self.servers.clear();
    }
}

/// Per-server task that reads notifications from the language server
/// and dispatches them into the editor via the command bus.
async fn notification_task(
    mut rx: mpsc::Receiver<(String, serde_json::Value)>,
    bus: CommandBus,
    path_index: Arc<std::sync::Mutex<HashMap<PathBuf, BufferId>>>,
) {
    while let Some((method, params)) = rx.recv().await {
        match method.as_str() {
            "textDocument/publishDiagnostics" => {
                handle_publish_diagnostics(&bus, &path_index, params).await;
            }
            other => {
                debug!(method = other, "ignoring LSP notification");
            }
        }
    }
    debug!("notification task exiting (server closed)");
}

async fn handle_publish_diagnostics(
    bus: &CommandBus,
    path_index: &Arc<std::sync::Mutex<HashMap<PathBuf, BufferId>>>,
    params: serde_json::Value,
) {
    let Ok(params) =
        serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params)
    else {
        warn!("failed to parse publishDiagnostics params");
        return;
    };

    // Resolve the URI to a buffer ID.
    let uri_str = params.uri.as_str();
    let path = uri_str.strip_prefix("file://").unwrap_or(uri_str);
    let path = PathBuf::from(path);
    let buffer_id = {
        let index = path_index.lock().unwrap();
        index.get(&path).copied()
    };
    let Some(buffer_id) = buffer_id else {
        debug!(path = %path.display(), "diagnostics for unknown buffer, ignoring");
        return;
    };

    let lsp_diags = params.diagnostics;
    let _ = bus
        .dispatch(move |editor| {
            let Some(buffer) = editor.buffers_mut().get_mut(buffer_id) else {
                return;
            };
            let rope = buffer.rope().clone();
            let converted = diagnostics::convert(&rope, &lsp_diags);
            let layer = buffer
                .properties_mut()
                .ensure_layer("diagnostics", AdjustmentPolicy::TrackEdits);
            layer.clear();
            for (_range, interval) in converted {
                layer.insert(interval);
            }
            layer.clear_dirty();
            editor.mark_dirty();
        })
        .await;
}

fn path_to_uri(path: &std::path::Path) -> lsp_types::Uri {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(path)
    };
    let s = format!("file://{}", abs.display());
    // lsp_types::Uri in 0.97 implements FromStr.
    s.parse::<lsp_types::Uri>().unwrap_or_else(|_| {
        "file:///".parse().unwrap()
    })
}
