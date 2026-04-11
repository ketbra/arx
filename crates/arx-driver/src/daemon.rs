//! Daemon side of the thin-client architecture.
//!
//! Spec §7: the daemon owns the editor (buffers, windows, event loop,
//! commands, keymap) and the render pipeline. Clients are thin — they
//! only own the terminal and ship keys in / render ops out. This module
//! is the glue that binds a cross-platform IPC endpoint via
//! [`arx_protocol::IpcListener`] (Unix domain socket on Unix, Windows
//! named pipe on Windows), accepts a client, and stitches together:
//!
//! * an [`EventLoop`] on an [`Editor`] seeded by the caller;
//! * a [`RenderTask`] over a [`RemoteBackend`];
//! * two IPC tasks — a "reader" that drains [`ClientMessage`]s off the
//!   connection and feeds the editor, and a "writer" that ships
//!   [`DaemonMessage::RenderOps`] batches back out.
//!
//! Phase 1 handles a single client at a time: after the client
//! disconnects, the daemon loops back to `accept()` for the next one
//! (reusing the same [`Editor`] so state persists across reconnects).
//! Multi-client broadcast and named sessions land in follow-up
//! milestones; [`Session`](arx_core::Session) is already in the right
//! shape for them.
//!
//! A `TODO(phase-1c)` marker sits on [`DaemonServer::run`] where the
//! session persistence hook will land — load a session file at startup,
//! write one on clean shutdown.

use std::io;
use std::sync::Arc;

use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use arx_core::{CommandBus, Editor, EventLoop};
use arx_protocol::{
    ClientMessage, DaemonMessage, FrameError, HelloInfo, IpcAddress, IpcListener, IpcReadHalf,
    IpcStream, PROTOCOL_VERSION, ShutdownReason, TransportError, read_frame, write_frame,
};

use crate::remote_backend::RemoteBackend;
use crate::render::RenderTask;
use crate::state::{SharedTerminalSize, Shutdown};

/// Errors from running the daemon.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("IPC transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("IPC framing error: {0}")]
    Frame(#[from] FrameError),
    #[error("task join error")]
    Join(#[from] tokio::task::JoinError),
}

/// A daemon instance bound to a cross-platform IPC endpoint.
#[derive(Debug)]
pub struct DaemonServer {
    address: IpcAddress,
    listener: IpcListener,
    editor: Editor,
}

impl DaemonServer {
    /// Bind `address` and take ownership of the initial [`Editor`]
    /// state. On Unix, any existing socket file at that path is
    /// removed first. On Windows the bind refuses if another live
    /// server is already listening on the same pipe name.
    pub fn bind(address: IpcAddress, editor: Editor) -> Result<Self, DaemonError> {
        let listener = IpcListener::bind(&address)?;
        info!(address = %address, "daemon listening");
        Ok(Self {
            address,
            listener,
            editor,
        })
    }

    /// Run the daemon forever, handling clients serially.
    ///
    /// Returns the final [`Editor`] state when either the listener is
    /// closed externally or the server is signalled to stop. The
    /// [`Editor`] is preserved across client reconnects — detaching and
    /// reattaching keeps buffers, cursors, and scroll positions intact.
    pub async fn run(mut self) -> Result<Editor, DaemonError> {
        // TODO(phase-1c): load a SessionFile from disk here and apply
        // it to `self.editor` if present.
        loop {
            tokio::select! {
                accept = self.listener.accept() => {
                    match accept {
                        Ok(stream) => {
                            debug!("client connected");
                            self.editor = handle_client(stream, self.editor).await?;
                            debug!("client disconnected; editor state preserved");
                        }
                        Err(e) => {
                            warn!(%e, "accept failed");
                            return Err(e.into());
                        }
                    }
                }
            }
            // For now we loop forever. A clean shutdown path would come
            // from a separate `Shutdown` signal plumbed into the select
            // above — trivial to add once we have a CLI command for it.
            // TODO(phase-1c): also write the SessionFile here on the
            // shutdown branch.
        }
    }

    /// The IPC address this daemon is bound to.
    #[must_use]
    pub fn address(&self) -> &IpcAddress {
        &self.address
    }
}

/// Handle one client connection end-to-end.
///
/// Takes ownership of the [`Editor`] while the client is connected and
/// hands it back when the client disconnects. The daemon's outer loop
/// reuses that state for the next client.
async fn handle_client(
    stream: IpcStream,
    editor: Editor,
) -> Result<Editor, DaemonError> {
    let (mut reader, mut writer) = stream.into_split();

    // --- Handshake ---
    let hello: ClientMessage = match read_frame(&mut reader).await {
        Ok(m) => m,
        Err(e) => {
            warn!(%e, "failed to read hello");
            return Ok(editor);
        }
    };
    let hello_info = match hello {
        ClientMessage::Hello(info) => info,
        other => {
            warn!(?other, "client sent non-Hello first message");
            return Ok(editor);
        }
    };
    if hello_info.protocol_version != PROTOCOL_VERSION {
        let _ = write_frame(
            &mut writer,
            &DaemonMessage::Shutdown(ShutdownReason::VersionMismatch {
                daemon_version: PROTOCOL_VERSION,
            }),
        )
        .await;
        warn!(
            client = hello_info.protocol_version,
            daemon = PROTOCOL_VERSION,
            "protocol version mismatch"
        );
        return Ok(editor);
    }
    write_frame(
        &mut writer,
        &DaemonMessage::Welcome {
            protocol_version: PROTOCOL_VERSION,
            session_id: 1,
        },
    )
    .await?;

    // --- Session setup ---
    let (cols, rows) = (hello_info.cols, hello_info.rows);
    let redraw = Arc::new(Notify::new());
    let shutdown = Shutdown::new();
    let size = SharedTerminalSize::new(cols, rows);

    // Mark the editor dirty so the render task draws an initial frame.
    let mut editor = editor;
    editor.mark_dirty();
    let (event_loop, bus) = EventLoop::with_editor(editor, arx_core::DEFAULT_BUS_CAPACITY);
    let event_loop = event_loop.with_redraw_notify(redraw.clone());
    let loop_handle: JoinHandle<Editor> = tokio::spawn(event_loop.run());

    // Remote render backend + writer task.
    let (remote_backend, mut render_rx) = RemoteBackend::new(cols, rows);
    let render_task = RenderTask::new(
        remote_backend,
        bus.clone(),
        redraw.clone(),
        shutdown.clone(),
        size.clone(),
    );
    let render_handle: JoinHandle<RemoteBackend> = tokio::spawn(render_task.run());

    // Writer task: ships DiffOp batches from the channel to the socket.
    let writer_shutdown = shutdown.clone();
    let writer_handle = tokio::spawn(async move {
        while let Some(ops) = render_rx.recv().await {
            let msg = DaemonMessage::RenderOps(ops);
            if let Err(err) = write_frame(&mut writer, &msg).await {
                debug!(%err, "writer task: socket closed");
                break;
            }
        }
        // Best-effort goodbye on the way out.
        let _ = write_frame(&mut writer, &DaemonMessage::Shutdown(ShutdownReason::DaemonExit))
            .await;
        writer_shutdown.fire();
    });

    // Reader loop: drains ClientMessages off the socket into the bus.
    let reader_result =
        run_reader(&mut reader, &bus, &size, &shutdown).await;
    if let Err(err) = reader_result {
        debug!(%err, "reader task ended");
    }

    // Shut everything down cleanly.
    shutdown.fire();
    drop(bus); // releases the event loop
    let _ = render_handle.await;
    let _ = writer_handle.await;
    let editor = loop_handle.await?;
    Ok(editor)
}

/// Drive the reader loop: read framed [`ClientMessage`]s from the
/// connection and dispatch each against the [`CommandBus`].
async fn run_reader(
    reader: &mut IpcReadHalf,
    bus: &CommandBus,
    size: &SharedTerminalSize,
    shutdown: &Shutdown,
) -> Result<(), FrameError> {
    loop {
        if shutdown.is_fired() {
            return Ok(());
        }
        let msg: ClientMessage = match read_frame(reader).await {
            Ok(m) => m,
            Err(FrameError::UnexpectedEof) => {
                debug!("client closed connection");
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        match msg {
            ClientMessage::Hello(_) => {
                warn!("client sent Hello mid-session; ignoring");
            }
            ClientMessage::Goodbye => return Ok(()),
            ClientMessage::Resize { cols, rows } => {
                size.set(cols, rows);
                // Dispatch a dirty ping so the render task re-renders
                // at the new size.
                let _ = bus.dispatch(Editor::mark_dirty).await;
            }
            ClientMessage::Key(chord) => {
                let bus_clone = bus.clone();
                // Run through Editor::handle_key inline. If the keymap
                // reports Unbound + printable_fallback, fall back to
                // self-insert — mirroring the local input task's
                // behaviour for typed characters that aren't bound.
                let quit = bus
                    .invoke(move |editor| {
                        let outcome = editor.handle_key(&bus_clone, chord);
                        if let arx_core::KeyHandled::Unbound {
                            printable_fallback: Some(ch),
                        } = outcome
                        {
                            arx_core::stock::insert_at_cursor(
                                editor,
                                &ch.to_string(),
                            );
                        }
                        editor.quit_requested()
                    })
                    .await
                    .map_err(|_| {
                        FrameError::Io(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "bus closed",
                        ))
                    })?;
                if quit {
                    debug!("editor.quit requested; closing reader");
                    return Ok(());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Client side
// ---------------------------------------------------------------------------

/// Connect to a daemon over an IPC endpoint and run as a thin client.
///
/// Owns the terminal via [`TerminalGuard`](crate::driver::TerminalGuard)-
/// equivalent raw-mode setup; spawns:
///
/// * a local input task that reads `crossterm::event::EventStream` and
///   ships each event as a [`ClientMessage`];
/// * a render task that reads [`DaemonMessage::RenderOps`] and applies
///   them to a local [`arx_render::CrosstermBackend`].
///
/// The client returns when either the daemon sends
/// [`DaemonMessage::Shutdown`] or the user presses a key that causes
/// the daemon to close its side of the connection.
pub struct DaemonClient {
    address: IpcAddress,
}

impl std::fmt::Debug for DaemonClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonClient")
            .field("address", &self.address)
            .finish()
    }
}

impl DaemonClient {
    pub fn new(address: IpcAddress) -> Self {
        Self { address }
    }

    /// Connect and run until disconnect.
    pub async fn run(self) -> Result<(), DaemonError> {
        use std::io::Write;

        use crossterm::{
            ExecutableCommand,
            cursor,
            event::EventStream,
            terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
        };
        use futures_util::StreamExt;

        use arx_keymap::KeyChord;
        use arx_render::{Backend, CrosstermBackend};

        let stream = IpcStream::connect(&self.address).await?;
        let (mut reader, mut writer) = stream.into_split();

        // Send Hello.
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        write_frame(
            &mut writer,
            &ClientMessage::Hello(HelloInfo {
                protocol_version: PROTOCOL_VERSION,
                client_id: format!("arx-client-{}", std::process::id()),
                cols,
                rows,
            }),
        )
        .await?;

        // Read Welcome.
        let welcome: DaemonMessage = read_frame(&mut reader).await?;
        match welcome {
            DaemonMessage::Welcome { .. } => {}
            DaemonMessage::Shutdown(reason) => {
                return Err(DaemonError::Io(io::Error::other(format!(
                    "daemon refused: {reason:?}"
                ))));
            }
            other => {
                return Err(DaemonError::Io(io::Error::other(format!(
                    "unexpected daemon message: {other:?}"
                ))));
            }
        }

        // Enable raw mode + alternate screen. RAII-guarded.
        terminal::enable_raw_mode()?;
        {
            let mut stdout = io::stdout();
            stdout.execute(EnterAlternateScreen)?;
            stdout.execute(cursor::Hide)?;
            stdout.flush()?;
        }

        // Spawn the input writer task.
        let input_handle = tokio::spawn(async move {
            let mut events = EventStream::new();
            while let Some(ev) = events.next().await {
                let Ok(ev) = ev else { break };
                let msg = match ev {
                    crossterm::event::Event::Key(key) => {
                        ClientMessage::Key(KeyChord::from(&key))
                    }
                    crossterm::event::Event::Resize(cols, rows) => {
                        ClientMessage::Resize { cols, rows }
                    }
                    _ => continue,
                };
                if write_frame(&mut writer, &msg).await.is_err() {
                    break;
                }
            }
            let _ = write_frame(&mut writer, &ClientMessage::Goodbye).await;
        });

        // Main loop: drain daemon messages and apply render ops locally.
        let mut backend: CrosstermBackend<io::Stdout> =
            CrosstermBackend::new(io::stdout(), cols, rows);
        let client_result: Result<(), DaemonError> = loop {
            let msg: DaemonMessage = match read_frame(&mut reader).await {
                Ok(m) => m,
                Err(FrameError::UnexpectedEof) => break Ok(()),
                Err(e) => break Err(DaemonError::Frame(e)),
            };
            match msg {
                DaemonMessage::RenderOps(ops) => {
                    if let Err(err) = backend.apply(&ops) {
                        break Err(DaemonError::Io(err));
                    }
                    if let Err(err) = backend.present() {
                        break Err(DaemonError::Io(err));
                    }
                }
                DaemonMessage::Shutdown(_) => break Ok(()),
                DaemonMessage::Welcome { .. } | DaemonMessage::Bell => {}
            }
        };

        // Tear down terminal before returning.
        {
            let mut stdout = io::stdout();
            stdout.execute(cursor::Show).ok();
            stdout.execute(LeaveAlternateScreen).ok();
        }
        let _ = terminal::disable_raw_mode();
        input_handle.abort();
        client_result
    }
}

