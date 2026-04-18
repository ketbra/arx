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
use arx_lsp::{
    LspClient, LspEvent, LspRegistry, ResolvedLspConfig, diagnostics, find_root,
};

/// Per-buffer bookkeeping for the LSP manager.
#[derive(Debug)]
struct TrackedBuffer {
    path: PathBuf,
    #[allow(dead_code)]
    extension: String,
    version: i32,
    /// Owned copy of the resolved server's `languageId`. Owned (not
    /// `&'static`) because user-config overrides carry heap-owned
    /// strings; built-ins still work because `ResolvedLspConfig`'s
    /// accessors normalise to `&str`.
    language_id: String,
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
    servers: HashMap<String, ServerEntry>,
    tracked: HashMap<BufferId, TrackedBuffer>,
    /// Merged registry of built-in + user-override server configs.
    registry: Arc<LspRegistry>,
    /// Maps absolute path to `BufferId` so the notification task can
    /// resolve `publishDiagnostics` URIs back to buffer ids.
    path_index: Arc<std::sync::Mutex<HashMap<PathBuf, BufferId>>>,
}

impl LspManager {
    pub fn new(bus: CommandBus) -> Self {
        Self::new_with_registry(bus, Arc::new(LspRegistry::builtin_only()))
    }

    /// Construct a manager with a specific registry. Used by the
    /// driver to inject user overrides from the config file.
    pub fn new_with_registry(bus: CommandBus, registry: Arc<LspRegistry>) -> Self {
        Self {
            bus,
            servers: HashMap::new(),
            tracked: HashMap::new(),
            registry,
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
        let registry = self.registry.clone();
        let Some(config) = registry.config_for_extension(&extension) else {
            return;
        };
        let language_id: String = config.language_id().to_owned();
        let name: String = config.name().to_owned();

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
                language_id: language_id.clone(),
            },
        );

        // Ensure the server for this language is running.
        if !self.servers.contains_key(&language_id) {
            if let Err(err) = self.start_server(&config, &language_id, &name, &path).await {
                warn!(%err, language = %name, "failed to start LSP server");
                return;
            }
        }

        let Some(entry) = self.servers.get_mut(&language_id) else {
            return;
        };
        entry.buffers.push(buffer_id);

        let uri = path_to_uri(&path);
        if let Err(err) = entry.client.did_open(uri, &language_id, 0, text).await {
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
        let lang_id = tracked.language_id.clone();

        let Some(entry) = self.servers.get(&lang_id) else {
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
        if let Some(entry) = self.servers.get_mut(&tracked.language_id) {
            if let Err(err) = entry.client.did_close(uri).await {
                warn!(%err, "didClose failed");
            }
            entry.buffers.retain(|id| *id != buffer_id);
        }
    }

    async fn start_server(
        &mut self,
        config: &ResolvedLspConfig<'_>,
        language_id: &str,
        name: &str,
        first_file: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            language = %name,
            command = %config.command(),
            "spawning LSP server",
        );
        let mut client = LspClient::spawn_resolved(config)?;

        let root_markers: Vec<&str> = config.root_markers().collect();
        let root = first_file.parent().map_or_else(
            || PathBuf::from("."),
            |dir| find_root(dir, &root_markers),
        );
        let root_uri = path_to_uri(&root);

        let init_options = config.initialization_options().cloned();
        client.initialize_with_options(root_uri, init_options).await?;

        // Extract the notification receiver and spawn a dedicated
        // task that processes server-initiated messages.
        if let Some(notif_rx) = client.take_notification_rx() {
            let bus = self.bus.clone();
            let path_index = Arc::clone(&self.path_index);
            tokio::spawn(notification_task(notif_rx, bus, path_index));
        }

        self.servers.insert(
            language_id.to_owned(),
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
