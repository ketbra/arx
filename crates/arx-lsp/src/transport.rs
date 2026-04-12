//! Async JSON-RPC transport over a child process's stdio.
//!
//! [`LspTransport`] owns the spawned language server process and
//! exposes a request/response + notification API. Internally it runs
//! two tokio tasks:
//!
//! * **Writer task** — drains outbound messages from an mpsc channel
//!   and writes them to the child's stdin.
//! * **Reader task** — reads length-prefixed JSON frames from the
//!   child's stdout, dispatches responses to pending oneshot channels,
//!   and forwards notifications to a broadcast channel.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::codec::{self, FrameReader};

/// A running LSP transport. Drop it to kill the child process and
/// stop the reader/writer tasks.
pub struct LspTransport {
    child: Child,
    outbound_tx: mpsc::Sender<Vec<u8>>,
    next_id: Arc<AtomicI64>,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    notification_rx: Option<mpsc::Receiver<(String, Value)>>,
    // Tasks are detached; they stop when channels close.
}

impl std::fmt::Debug for LspTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspTransport")
            .field("child_id", &self.child.id())
            .finish_non_exhaustive()
    }
}

impl LspTransport {
    /// Spawn a language server and wire up the transport.
    pub fn spawn(command: &str, args: &[&str]) -> std::io::Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        let (outbound_tx, outbound_rx) = mpsc::channel::<Vec<u8>>(64);
        let (notification_tx, notification_rx) = mpsc::channel::<(String, Value)>(64);
        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Writer task.
        tokio::spawn(writer_task(stdin, outbound_rx));

        // Reader task.
        tokio::spawn(reader_task(
            stdout,
            Arc::clone(&pending),
            notification_tx,
        ));

        Ok(Self {
            child,
            outbound_tx,
            next_id: Arc::new(AtomicI64::new(1)),
            pending,
            notification_rx: Some(notification_rx),
        })
    }

    /// Send a JSON-RPC request and return a receiver for the response.
    /// The caller `.await`s on the receiver to get the result.
    pub async fn send_request(
        &self,
        method: &str,
        params: Value,
    ) -> Result<oneshot::Receiver<Value>, TransportError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.outbound_tx
            .send(codec::encode(&msg))
            .await
            .map_err(|_| TransportError::Closed)?;
        Ok(rx)
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn send_notification(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(), TransportError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.outbound_tx
            .send(codec::encode(&msg))
            .await
            .map_err(|_| TransportError::Closed)?;
        Ok(())
    }

    /// Take the notification receiver out of the transport. After
    /// this call, [`LspTransport::recv_notification`] always returns
    /// `None`. The extracted receiver is typically handed to a
    /// dedicated notification-processing task.
    pub fn take_notification_rx(
        &mut self,
    ) -> Option<mpsc::Receiver<(String, Value)>> {
        self.notification_rx.take()
    }

    /// Receive the next server-initiated notification. Returns `None`
    /// when the reader task exits (server closed stdout) or when
    /// [`LspTransport::take_notification_rx`] was called.
    pub async fn recv_notification(&mut self) -> Option<(String, Value)> {
        self.notification_rx.as_mut()?.recv().await
    }

    /// Kill the child process. Call after sending `shutdown` + `exit`.
    pub fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

async fn writer_task(
    mut stdin: tokio::process::ChildStdin,
    mut rx: mpsc::Receiver<Vec<u8>>,
) {
    while let Some(bytes) = rx.recv().await {
        if stdin.write_all(&bytes).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
    }
}

async fn reader_task(
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    notification_tx: mpsc::Sender<(String, Value)>,
) {
    let mut reader = FrameReader::new(stdout);
    loop {
        let msg = match reader.read_message().await {
            Ok(Some(msg)) => msg,
            Ok(None) => break, // EOF
            Err(err) => {
                tracing::warn!(%err, "LSP reader error");
                break;
            }
        };
        // Dispatch: response (has "id") or notification (has "method", no "id").
        if let Some(id) = msg.get("id").and_then(Value::as_i64) {
            let mut map = pending.lock().await;
            if let Some(tx) = map.remove(&id) {
                let result = msg
                    .get("result")
                    .cloned()
                    .unwrap_or_else(|| msg.get("error").cloned().unwrap_or(Value::Null));
                let _ = tx.send(result);
            }
        } else if let Some(method) = msg.get("method").and_then(Value::as_str) {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let _ = notification_tx.send((method.to_owned(), params)).await;
        }
    }
}

/// Errors from [`LspTransport`] send methods.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("transport channel closed")]
    Closed,
}
