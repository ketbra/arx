//! Arx editor binary.
//!
//! Thin shim: parse CLI arguments and run one of three modes:
//!
//! * **embedded** (default): spin up the editor in-process on the
//!   current terminal. This is what Phase 1 has shipped up to now and
//!   what tests exercise directly.
//! * **`arx daemon`**: run as a background server bound to a
//!   cross-platform IPC endpoint (Unix domain socket on Unix, Windows
//!   named pipe on Windows), waiting for clients. State survives
//!   client disconnects.
//! * **`arx client`**: connect to a running daemon over its IPC
//!   endpoint and act as a thin terminal front-end.
//!
//! All the real work lives in the library crates (`arx-driver`,
//! `arx-core`, etc.). This file is just argument parsing + mode
//! selection.

use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use arx_driver::{DaemonClient, DaemonServer, Driver};
use arx_protocol::{IpcAddress, default_address};
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
    /// Run as a background daemon bound to an IPC endpoint.
    Daemon {
        /// Endpoint to bind. On Unix, a filesystem path (default:
        /// `$XDG_RUNTIME_DIR/arx.sock` or `/tmp/arx-<user>.sock`). On
        /// Windows, a named pipe like `\\.\pipe\arx-<user>`. Strings
        /// starting with `\\.\pipe\` or `\\?\pipe\` are parsed as
        /// named pipes; everything else is treated as a path.
        #[arg(long)]
        socket: Option<String>,
    },
    /// Connect to a running daemon as a thin client.
    Client {
        /// Endpoint to connect to. Same default search path as
        /// `arx daemon`.
        #[arg(long)]
        socket: Option<String>,
    },
}

fn resolve_address(raw: Option<String>) -> IpcAddress {
    raw.and_then(|s| IpcAddress::from_str(&s).ok())
        .unwrap_or_else(default_address)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.mode {
        None => run_embedded(cli.files).await,
        Some(Mode::Daemon { socket }) => run_daemon(resolve_address(socket)).await,
        Some(Mode::Client { socket }) => run_client(resolve_address(socket)).await,
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

async fn run_daemon(address: IpcAddress) -> Result<(), Box<dyn std::error::Error>> {
    use arx_core::Editor;
    // `IpcListener`'s Drop impl removes the Unix socket on exit;
    // Windows named pipes clean up when the handle closes, so we
    // don't need a separate guard type any more.
    let server = DaemonServer::bind(address, Editor::new())?;
    let _editor = server.run().await?;
    Ok(())
}

async fn run_client(address: IpcAddress) -> Result<(), Box<dyn std::error::Error>> {
    let client = DaemonClient::new(address);
    client.run().await?;
    Ok(())
}
