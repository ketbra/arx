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
use arx_protocol::{IpcAddress, default_address, default_session_path};
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

    /// Keymap profile: "emacs" (default), "vim", or "kedit". The
    /// `kedit` profile also enables the persistent bottom command
    /// line.
    #[arg(long, default_value = "emacs")]
    keymap: String,

    #[command(subcommand)]
    mode: Option<Mode>,
}

#[derive(Debug, Subcommand)]
enum Mode {
    /// Session management commands.
    Session {
        #[command(subcommand)]
        action: SessionAction,
        /// Endpoint to connect to.
        #[arg(long, global = true)]
        socket: Option<String>,
    },
    /// Run as a background daemon bound to an IPC endpoint.
    Daemon {
        /// Endpoint to bind. On Unix, a filesystem path (default:
        /// `$XDG_RUNTIME_DIR/arx.sock` or `/tmp/arx-<user>.sock`). On
        /// Windows, a named pipe like `\\.\pipe\arx-<user>`. Strings
        /// starting with `\\.\pipe\` or `\\?\pipe\` are parsed as
        /// named pipes; everything else is treated as a path.
        #[arg(long)]
        socket: Option<String>,
        /// Path to the session file for Level-1 persistence. If set,
        /// the daemon loads it at startup (if it exists) and saves
        /// the current editor state to it on clean shutdown. Pass
        /// `--no-session` to disable entirely. Default:
        /// `$XDG_STATE_HOME/arx/session.postcard` on Unix,
        /// `%LOCALAPPDATA%\arx\session.postcard` on Windows.
        #[arg(long, conflicts_with = "no_session")]
        session_file: Option<PathBuf>,
        /// Disable session persistence for this run.
        #[arg(long)]
        no_session: bool,
        /// Directory to scan for extension dylibs (`*.so` / `*.dylib`
        /// / `*.dll`). Every dylib in the directory is loaded at
        /// startup and watched for changes while a client is
        /// connected. Default: `~/.arx/extensions/`. Pass
        /// `--no-extensions` to skip extension loading entirely.
        #[arg(long, conflicts_with = "no_extensions")]
        extensions_dir: Option<PathBuf>,
        /// Disable the extension host for this run.
        #[arg(long)]
        no_extensions: bool,
    },
    /// Connect to a running daemon as a thin client.
    Client {
        /// Endpoint to connect to. Same default search path as
        /// `arx daemon`.
        #[arg(long)]
        socket: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SessionAction {
    /// List all active sessions on the daemon.
    List,
}

fn resolve_address(raw: Option<String>) -> IpcAddress {
    raw.and_then(|s| IpcAddress::from_str(&s).ok())
        .unwrap_or_else(default_address)
}

fn resolve_session_path(raw: Option<PathBuf>, disabled: bool) -> Option<PathBuf> {
    if disabled {
        None
    } else {
        Some(raw.unwrap_or_else(default_session_path))
    }
}

fn resolve_extensions_dir(raw: Option<PathBuf>, disabled: bool) -> Option<PathBuf> {
    if disabled {
        None
    } else {
        Some(raw.unwrap_or_else(default_extensions_dir))
    }
}

/// Default extensions directory.
///
/// Unix: `$HOME/.arx/extensions`. Windows: `%USERPROFILE%\.arx\extensions`.
/// Falls back to `./arx-extensions` relative to the working directory
/// if neither env var is set — preserves the "works out of the box"
/// property on sandboxed CI.
fn default_extensions_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".arx").join("extensions");
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.is_empty() {
            return PathBuf::from(profile).join(".arx").join("extensions");
        }
    }
    PathBuf::from("arx-extensions")
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let profile = match cli.keymap.as_str() {
        "vim" => arx_keymap::profiles::vim(),
        "kedit" => arx_keymap::profiles::kedit(),
        _ => arx_keymap::profiles::emacs(),
    };
    let result = match cli.mode {
        None => run_embedded(cli.files, profile).await,
        Some(Mode::Session { action, socket }) => {
            run_session_command(action, resolve_address(socket)).await
        }
        Some(Mode::Daemon {
            socket,
            session_file,
            no_session,
            extensions_dir,
            no_extensions,
        }) => {
            run_daemon(
                resolve_address(socket),
                resolve_session_path(session_file, no_session),
                resolve_extensions_dir(extensions_dir, no_extensions),
            )
            .await
        }
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

async fn run_embedded(
    files: Vec<PathBuf>,
    profile: arx_keymap::profiles::Profile,
) -> Result<(), Box<dyn std::error::Error>> {
    let driver = Driver::new(|_editor| {})
        .with_profile(profile)
        .with_async_hook(move |bus| async move {
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

async fn run_daemon(
    address: IpcAddress,
    session_path: Option<PathBuf>,
    extensions_dir: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    use arx_core::Editor;
    // `IpcListener`'s Drop impl removes the Unix socket on exit;
    // Windows named pipes clean up when the handle closes, so we
    // don't need a separate guard type any more.
    let mut server = DaemonServer::bind(address, Editor::new())?;
    if let Some(path) = session_path {
        server = server.with_session_path(path);
    }
    if let Some(dir) = extensions_dir {
        server = server.with_extensions_dir(dir);
    }
    // Wire Ctrl+C to the daemon's shutdown handle so the accept loop
    // breaks cleanly and the session save-on-shutdown path runs.
    // Without this, hitting Ctrl+C would kill the process mid-accept
    // and lose the final session state.
    let shutdown = server.shutdown_handle();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("ctrl+c received; shutting down daemon");
            shutdown.fire();
        }
    });
    let _editor = server.run().await?;
    Ok(())
}

async fn run_session_command(
    action: SessionAction,
    address: IpcAddress,
) -> Result<(), Box<dyn std::error::Error>> {
    use arx_protocol::{
        ClientMessage, DaemonMessage, HelloInfo, IpcStream, PROTOCOL_VERSION, read_frame,
        write_frame,
    };

    let stream = IpcStream::connect(&address).await?;
    let (mut reader, mut writer) = stream.into_split();

    // Handshake.
    let hello = ClientMessage::Hello(HelloInfo {
        protocol_version: PROTOCOL_VERSION,
        client_id: "arx-session-cli".into(),
        cols: 80,
        rows: 24,
    });
    write_frame(&mut writer, &hello).await?;
    let welcome: DaemonMessage = read_frame(&mut reader).await?;
    match welcome {
        DaemonMessage::Welcome { .. } => {}
        DaemonMessage::Shutdown(reason) => {
            return Err(format!("daemon refused connection: {reason:?}").into());
        }
        other => {
            return Err(format!("unexpected response: {other:?}").into());
        }
    }

    match action {
        SessionAction::List => {
            write_frame(&mut writer, &ClientMessage::ListSessions).await?;
            let response: DaemonMessage = read_frame(&mut reader).await?;
            match response {
                DaemonMessage::SessionList(sessions) => {
                    if sessions.is_empty() {
                        println!("No active sessions.");
                    } else {
                        println!("{:<6} {:<20} {:<10} WINDOWS", "ID", "NAME", "BUFFERS");
                        for s in &sessions {
                            let name = if s.name.is_empty() {
                                "(default)"
                            } else {
                                &s.name
                            };
                            println!(
                                "{:<6} {:<20} {:<10} {}",
                                s.id, name, s.buffer_count, s.window_count,
                            );
                        }
                    }
                }
                DaemonMessage::Error { message } => {
                    return Err(format!("daemon error: {message}").into());
                }
                other => {
                    return Err(format!("unexpected response: {other:?}").into());
                }
            }
        }
    }

    // Goodbye.
    let _ = write_frame(&mut writer, &ClientMessage::Goodbye).await;
    Ok(())
}

async fn run_client(address: IpcAddress) -> Result<(), Box<dyn std::error::Error>> {
    let client = DaemonClient::new(address);
    client.run().await?;
    Ok(())
}
