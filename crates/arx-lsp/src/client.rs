//! High-level LSP client that wraps [`crate::transport::LspTransport`]
//! with typed methods for the subset of the protocol we use.

use lsp_types::{
    ClientCapabilities, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, GeneralClientCapabilities,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, ServerCapabilities, TextDocumentClientCapabilities,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};
use serde_json::Value;

use crate::config::LspServerConfig;
use crate::transport::{LspTransport, TransportError};

/// A running LSP client for one language server. Wraps the raw
/// transport with typed helpers for the protocol subset we use (open,
/// change, hover, shutdown).
pub struct LspClient {
    transport: LspTransport,
    capabilities: Option<ServerCapabilities>,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("capabilities", &self.capabilities.is_some())
            .finish_non_exhaustive()
    }
}

impl LspClient {
    /// Spawn the language server described by `config` and return an
    /// uninitialised client. Call [`LspClient::initialize`] before
    /// sending any other messages.
    pub fn spawn(config: &LspServerConfig) -> std::io::Result<Self> {
        let transport = LspTransport::spawn(config.command, config.args)?;
        Ok(Self {
            transport,
            capabilities: None,
        })
    }

    /// Perform the LSP `initialize` + `initialized` handshake.
    /// Stores the server's capabilities for later feature detection.
    pub async fn initialize(&mut self, root_uri: Uri) -> Result<(), LspClientError> {
        #[allow(deprecated)] // root_uri is simpler for MVP than workspace_folders
        let params = InitializeParams {
            root_uri: Some(root_uri),
            capabilities: ClientCapabilities {
                general: Some(GeneralClientCapabilities::default()),
                text_document: Some(TextDocumentClientCapabilities::default()),
                ..ClientCapabilities::default()
            },
            ..InitializeParams::default()
        };
        let value = serde_json::to_value(params).map_err(LspClientError::Serde)?;
        let rx = self
            .transport
            .send_request("initialize", value)
            .await
            .map_err(LspClientError::Transport)?;
        let result_value = rx.await.map_err(|_| LspClientError::ResponseDropped)?;
        let result: InitializeResult =
            serde_json::from_value(result_value).map_err(LspClientError::Serde)?;
        self.capabilities = Some(result.capabilities);

        self.transport
            .send_notification(
                "initialized",
                serde_json::to_value(InitializedParams {}).unwrap(),
            )
            .await
            .map_err(LspClientError::Transport)?;
        Ok(())
    }

    /// Does the server support hover? (Checks the stored capabilities.)
    pub fn supports_hover(&self) -> bool {
        self.capabilities.as_ref().is_some_and(|c| {
            matches!(
                c.hover_provider,
                Some(HoverProviderCapability::Simple(true) | HoverProviderCapability::Options(_))
            )
        })
    }

    /// Does the server support full document sync? (Always true for
    /// MVP — we send full content on every change.)
    pub fn sync_kind(&self) -> TextDocumentSyncKind {
        self.capabilities
            .as_ref()
            .and_then(|c| match &c.text_document_sync {
                Some(TextDocumentSyncCapability::Kind(k)) => Some(*k),
                _ => None,
            })
            .unwrap_or(TextDocumentSyncKind::FULL)
    }

    /// Does the server advertise completion support?
    pub fn supports_completion(&self) -> bool {
        self.capabilities
            .as_ref()
            .and_then(|c| c.completion_provider.as_ref())
            .is_some()
    }

    /// Send `textDocument/didOpen`.
    pub async fn did_open(
        &self,
        uri: Uri,
        language_id: &str,
        version: i32,
        text: String,
    ) -> Result<(), LspClientError> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language_id.to_owned(),
                version,
                text,
            },
        };
        self.transport
            .send_notification(
                "textDocument/didOpen",
                serde_json::to_value(params).unwrap(),
            )
            .await
            .map_err(LspClientError::Transport)
    }

    /// Send `textDocument/didChange` with full document content.
    pub async fn did_change(
        &self,
        uri: Uri,
        version: i32,
        full_text: String,
    ) -> Result<(), LspClientError> {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: full_text,
            }],
        };
        self.transport
            .send_notification(
                "textDocument/didChange",
                serde_json::to_value(params).unwrap(),
            )
            .await
            .map_err(LspClientError::Transport)
    }

    /// Send `textDocument/didClose`.
    pub async fn did_close(&self, uri: Uri) -> Result<(), LspClientError> {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        };
        self.transport
            .send_notification(
                "textDocument/didClose",
                serde_json::to_value(params).unwrap(),
            )
            .await
            .map_err(LspClientError::Transport)
    }

    /// Send `textDocument/hover` and await the response.
    pub async fn hover(
        &self,
        uri: Uri,
        position: lsp_types::Position,
    ) -> Result<Option<lsp_types::Hover>, LspClientError> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let rx = self
            .transport
            .send_request(
                "textDocument/hover",
                serde_json::to_value(params).unwrap(),
            )
            .await
            .map_err(LspClientError::Transport)?;
        let value = rx.await.map_err(|_| LspClientError::ResponseDropped)?;
        if value.is_null() {
            return Ok(None);
        }
        let hover: lsp_types::Hover =
            serde_json::from_value(value).map_err(LspClientError::Serde)?;
        Ok(Some(hover))
    }

    /// Send `shutdown` request followed by `exit` notification.
    pub async fn shutdown(&mut self) -> Result<(), LspClientError> {
        let rx = self
            .transport
            .send_request("shutdown", Value::Null)
            .await
            .map_err(LspClientError::Transport)?;
        // Wait for the response (or timeout from the task being killed).
        let _ = rx.await;
        self.transport
            .send_notification("exit", Value::Null)
            .await
            .map_err(LspClientError::Transport)?;
        self.transport.kill();
        Ok(())
    }

    /// Receive the next server-initiated notification (e.g.
    /// `textDocument/publishDiagnostics`). Returns `None` when the
    /// server closes.
    pub async fn recv_notification(&mut self) -> Option<(String, Value)> {
        self.transport.recv_notification().await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LspClientError {
    #[error("transport: {0}")]
    Transport(#[source] TransportError),
    #[error("serde: {0}")]
    Serde(#[source] serde_json::Error),
    #[error("response oneshot dropped")]
    ResponseDropped,
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}
