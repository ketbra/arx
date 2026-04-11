//! Arx editor binary.
//!
//! Thin shim: parse CLI arguments and run one of three modes:
//!
//! * **embedded** (default): spin up the editor in-process on the
//!   current terminal. This is what Phase 1 has shipped up to now and
//!   what tests exercise directly.
//! * **`arx daemon`**: run as a background server bound to a Unix
//!   socket, waiting for clients. State survives client disconnects.
//! * **`arx client`**: connect to a running daemon over its Unix socket
//!   and act as a thin terminal front-end.
//!
//! All the real work lives in the library crates (`arx-driver`,
//! `arx-core`, etc.). This file is just argument parsing + mode
//! selection.

use std::path::PathBuf;
use std::process::ExitCode;

use arx_driver::{DaemonClient, DaemonServer, Driver};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "arx",
    about = "Arx editor",
    version,
    disable_help_subcommand = true
)]
struct Cli {
    /// Files to open when running in embedded mode. Ignored for the
    /// `daemon` and `client` subcommands.
    files: Vec<PathBuf>,

    #[command(subcommand)]
    mode: Option<Mode>,
}

#[derive(Debug, Subcommand)]
enum Mode {
    /// Run as a background daemon bound to a Unix socket.
    Daemon {
        /// Socket path to bind. Defaults to
        /// `$XDG_RUNTIME_DIR/arx.sock` or `/tmp/arx-<uid>.sock`.
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Connect to a running daemon as a thin client.
    Client {
        /// Socket path to connect to. Same default search path as
        /// `arx daemon`.
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

fn default_socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("arx.sock");
    }
    // Fall back to /tmp with a per-user suffix so concurrent users
    // don't collide on the same path.
    let uid = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
    PathBuf::from(format!("/tmp/arx-{uid}.sock"))
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match &cli.mode {
        None => run_embedded(cli.files).await,
        Some(Mode::Daemon { socket }) => {
            let path = socket.clone().unwrap_or_else(default_socket_path);
            run_daemon(path).await
        }
        Some(Mode::Client { socket }) => {
            let path = socket.clone().unwrap_or_else(default_socket_path);
            run_client(path).await
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("arx: {err}");
            let mut source = err.source();
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

async fn run_embedded(files: Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let driver = Driver::new(|_editor| {}).with_async_hook(move |bus| async move {
        for path in files {
            match arx_core::open_file(&bus, path.clone()).await {
                Ok((buffer_id, window_id)) => {
                    tracing::info!(
                        ?buffer_id,
                        ?window_id,
                        path = %path.display(),
                        "opened file"
                    );
                }
                Err(err) => {
                    tracing::warn!(%err, path = %path.display(), "failed to open file");
                }
            }
        }
        let _ = bus
            .dispatch(|editor| {
                if editor.windows().active().is_none() {
                    let buf = editor.buffers_mut().create_scratch();
                    editor.windows_mut().open(buf);
                    editor.mark_dirty();
                }
            })
            .await;
    });
    driver.run().await?;
    Ok(())
}

async fn run_daemon(socket: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    use arx_core::Editor;
    let server = DaemonServer::bind(socket, Editor::new())?;
    // Best-effort socket cleanup on normal return.
    let socket_path = server.socket_path().to_path_buf();
    let _guard = SocketGuard(socket_path);
    let _editor = server.run().await?;
    Ok(())
}

async fn run_client(socket: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let client = DaemonClient::new(socket);
    client.run().await?;
    Ok(())
}

/// RAII guard that removes a Unix socket on drop. Best-effort; errors
/// are logged but never panic.
struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
