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
//! # Session persistence
//!
//! [`DaemonServer::bind`] optionally takes a session-file path. If
//! given, [`DaemonServer::run`] does two things around its accept
//! loop:
//!
//! 1. At startup, before the first `accept`, it tries
//!    [`arx_core::Session::load_from_path`]. A missing file is fine
//!    — that's first run. A decode / version-mismatch error is
//!    logged and the daemon keeps going with a clean editor (we
//!    never refuse to start because the session file is broken).
//! 2. At shutdown, after the accept loop breaks, it snapshots the
//!    editor and writes the result with
//!    [`arx_core::Session::save_to_path`]. This is "Level 1"
//!    persistence per the ladder in
//!    [`arx_core::session`] — clean exits save, unclean exits
//!    don't.
//!
//! A [`Shutdown`] handle threaded into the accept loop lets callers
//! (a Ctrl+C handler, a test, a future `arx daemon stop` command)
//! break out cleanly so the save step actually runs.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use arx_core::{CommandBus, Editor, EventLoop, Session};
use arx_protocol::{
    ClientMessage, DaemonMessage, FrameError, HelloInfo, IpcAddress, IpcListener, IpcReadHalf,
    IpcStream, PROTOCOL_VERSION, SessionInfo, ShutdownReason, TransportError, read_frame,
    write_frame,
};

use crate::ext_host::ExtensionHost;
use crate::ext_watcher::ExtensionWatcher;
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
pub struct DaemonServer {
    address: IpcAddress,
    listener: IpcListener,
    editor: Editor,
    session_path: Option<PathBuf>,
    extensions_dir: Option<PathBuf>,
    shutdown: Shutdown,
}

impl std::fmt::Debug for DaemonServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonServer")
            .field("address", &self.address)
            .field("session_path", &self.session_path)
            .field("extensions_dir", &self.extensions_dir)
            .finish_non_exhaustive()
    }
}

impl DaemonServer {
    /// Bind `address` and take ownership of the initial [`Editor`]
    /// state. On Unix, any existing socket file at that path is
    /// removed first. On Windows the bind refuses if another live
    /// server is already listening on the same pipe name.
    ///
    /// The returned server has no session file wired up — call
    /// [`Self::with_session_path`] before [`Self::run`] to enable
    /// Level 1 persistence.
    pub fn bind(address: IpcAddress, editor: Editor) -> Result<Self, DaemonError> {
        let listener = IpcListener::bind(&address)?;
        info!(address = %address, "daemon listening");
        Ok(Self {
            address,
            listener,
            editor,
            session_path: None,
            extensions_dir: None,
            shutdown: Shutdown::new(),
        })
    }

    /// Attach a session file. When set, [`Self::run`] loads the file at
    /// startup (if present) and writes it on clean shutdown.
    #[must_use]
    pub fn with_session_path(mut self, path: PathBuf) -> Self {
        self.session_path = Some(path);
        self
    }

    /// Attach an extensions directory. When set, [`Self::run`]:
    ///
    /// 1. Creates the directory if it doesn't exist,
    /// 2. Walks it at startup and loads every dylib it finds, and
    /// 3. Spawns a [`ExtensionWatcher`] task that hot-reloads any
    ///    dylib modified while the daemon is running.
    ///
    /// Pass [`None`] (or don't call this) to disable extension
    /// loading entirely — the extension host stays empty and no
    /// watcher is spawned.
    #[must_use]
    pub fn with_extensions_dir(mut self, dir: PathBuf) -> Self {
        self.extensions_dir = Some(dir);
        self
    }

    /// Clone the internal [`Shutdown`] handle. Callers can wire this to
    /// a Ctrl+C handler or any other signal source to break the accept
    /// loop cleanly; [`Self::run`] polls the same handle inside its
    /// `tokio::select!`, so a `fire()` from outside unblocks a pending
    /// `accept()` and lets the save-on-shutdown path run.
    #[must_use]
    pub fn shutdown_handle(&self) -> Shutdown {
        self.shutdown.clone()
    }

    /// Run the daemon, handling clients serially until the shutdown
    /// handle fires or the listener errors.
    ///
    /// Returns the final [`Editor`] state. The editor is preserved
    /// across client reconnects and, if a session path was configured
    /// via [`Self::with_session_path`], its final state is serialised
    /// to that path before returning.
    pub async fn run(mut self) -> Result<Editor, DaemonError> {
        // --- Load phase ---
        if let Some(path) = self.session_path.clone() {
            match Session::load_from_path(&path).await {
                Ok(Some(session)) => {
                    // Apply via the event loop so the single-writer
                    // invariant is respected. The event loop then
                    // shuts down and returns the restored editor.
                    let editor = std::mem::replace(&mut self.editor, Editor::new());
                    self.editor = restore_session(editor, session).await;
                    info!(path = %path.display(), "session restored");
                }
                Ok(None) => {
                    debug!(path = %path.display(), "no session file yet");
                }
                Err(e) => {
                    // Never refuse to start because a session file is
                    // corrupt: a user whose state got corrupted still
                    // wants to use their editor.
                    warn!(%e, path = %path.display(), "session load failed; starting fresh");
                }
            }
        }

        // --- Extensions phase ---
        //
        // Walk the extensions directory synchronously and load every
        // dylib before any client connects. Individual load failures
        // are logged but don't abort startup — one broken extension
        // shouldn't lock the user out of their editor.
        //
        // The host lives behind an Arc<Mutex<>> so the watcher task
        // (spawned per client connection) can share it with the
        // daemon. Between clients the watcher is gone, so the host
        // sits idle inside the arc.
        let host = Arc::new(Mutex::new(ExtensionHost::new()));
        if let Some(dir) = self.extensions_dir.clone() {
            let mut locked = host.lock().await;
            match locked.load_dir_sync(&dir, &mut self.editor) {
                Ok(count) => info!(count, dir = %dir.display(), "loaded extensions"),
                Err(e) => warn!(%e, dir = %dir.display(), "extension scan failed"),
            }
        }

        // --- Accept loop ---
        let accept_result = loop {
            tokio::select! {
                biased;
                () = self.shutdown.wait() => {
                    debug!("daemon: shutdown signal received");
                    break Ok(());
                }
                accept = self.listener.accept() => {
                    match accept {
                        Ok(stream) => {
                            debug!("client connected");
                            self.editor = handle_client(
                                stream,
                                self.editor,
                                host.clone(),
                                self.extensions_dir.clone(),
                            )
                            .await?;
                            debug!("client disconnected; editor state preserved");
                        }
                        Err(e) => {
                            warn!(%e, "accept failed");
                            break Err(DaemonError::Transport(e));
                        }
                    }
                }
            }
        };

        // --- Unload phase ---
        {
            let mut locked = host.lock().await;
            locked.unload_all_sync(&mut self.editor);
        }

        // --- Save phase ---
        if let Some(path) = self.session_path.as_ref() {
            let session = Session::from_editor(&self.editor);
            if let Err(e) = session.save_to_path(path).await {
                warn!(%e, path = %path.display(), "session save failed");
            } else {
                info!(path = %path.display(), "session saved");
            }
        }

        accept_result.map(|()| self.editor)
    }

    /// The IPC address this daemon is bound to.
    #[must_use]
    pub fn address(&self) -> &IpcAddress {
        &self.address
    }
}

/// Replay a restored [`Session`] against `editor` via a short-lived
/// event loop — this is the only way to get a `&mut Editor` from the
/// `Session::restore` code path without duplicating the single-writer
/// invariant.
///
/// If the restore closure errors (bus closed, which can't happen here
/// because we own both ends), the original editor comes back
/// unmodified. Missing files on disk are logged and skipped per the
/// `Session::restore` contract.
async fn restore_session(editor: Editor, session: Session) -> Editor {
    let (event_loop, bus) = EventLoop::with_editor(editor, arx_core::DEFAULT_BUS_CAPACITY);
    let handle: JoinHandle<Editor> = tokio::spawn(event_loop.run());
    match session.restore(&bus).await {
        Ok(summary) => {
            debug!(
                restored_buffers = summary.restored_buffers,
                skipped_buffers = summary.skipped_buffers,
                restored_windows = summary.restored_windows,
                "session restore complete",
            );
        }
        Err(e) => warn!(%e, "session restore failed"),
    }
    drop(bus);
    handle.await.unwrap_or_else(|e| {
        warn!(%e, "restore event loop panicked; returning empty editor");
        Editor::new()
    })
}

/// Handle one client connection end-to-end.
///
/// Takes ownership of the [`Editor`] while the client is connected and
/// hands it back when the client disconnects. The daemon's outer loop
/// reuses that state for the next client.
async fn handle_client(
    stream: IpcStream,
    editor: Editor,
    host: Arc<Mutex<ExtensionHost>>,
    extensions_dir: Option<PathBuf>,
) -> Result<Editor, DaemonError> {
    let (mut reader, writer) = stream.into_split();

    // --- Handshake ---
    let writer = Arc::new(Mutex::new(writer));
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
            &mut *writer.lock().await,
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
        &mut *writer.lock().await,
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
    let writer_for_task = Arc::clone(&writer);
    let writer_handle = tokio::spawn(async move {
        while let Some(ops) = render_rx.recv().await {
            let msg = DaemonMessage::RenderOps(ops);
            let mut w = writer_for_task.lock().await;
            if let Err(err) = write_frame(&mut *w, &msg).await {
                debug!(%err, "writer task: socket closed");
                break;
            }
        }
        // Best-effort goodbye on the way out.
        let mut w = writer_for_task.lock().await;
        let _ = write_frame(&mut *w, &DaemonMessage::Shutdown(ShutdownReason::DaemonExit))
            .await;
        writer_shutdown.fire();
    });

    // Extension hot-reload watcher — only while a client is
    // connected, because that's when the user is looking at the
    // editor and when there's a live bus for the watcher to use.
    // Dropped on disconnect, which kills its task.
    let _watcher = extensions_dir.as_ref().map(|dir| {
        ExtensionWatcher::spawn(dir, host.clone(), bus.clone(), shutdown.clone())
            .inspect_err(|err| {
                warn!(%err, dir = %dir.display(), "extension watcher failed to start");
            })
            .ok()
    });

    // Reader loop: drains ClientMessages off the socket into the bus.
    let reader_result =
        run_reader(&mut reader, &writer, &bus, &size, &shutdown).await;
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
    writer: &Arc<Mutex<arx_protocol::IpcWriteHalf>>,
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
            ClientMessage::Goodbye | ClientMessage::DetachSession => {
                return Ok(());
            }
            ClientMessage::Resize { cols, rows } => {
                size.set(cols, rows);
                let _ = bus.dispatch(Editor::mark_dirty).await;
            }
            ClientMessage::ListSessions => {
                // For now, report the single running session.
                let info = bus
                    .invoke(|editor| SessionInfo {
                        id: 1,
                        name: String::new(),
                        buffer_count: editor.buffers().len() as u32,
                        window_count: editor.windows().len() as u32,
                    })
                    .await
                    .ok();
                if let Some(info) = info {
                    let mut w = writer.lock().await;
                    let _ = write_frame(
                        &mut *w,
                        &DaemonMessage::SessionList(vec![info]),
                    )
                    .await;
                }
            }
            ClientMessage::CreateSession { .. } | ClientMessage::AttachSession { .. } => {
                // MVP: only one session. Respond with an error for
                // unsupported operations.
                let mut w = writer.lock().await;
                let _ = write_frame(
                    &mut *w,
                    &DaemonMessage::Error {
                        message: "multi-session not yet supported; use the default session".into(),
                    },
                )
                .await;
            }
            ClientMessage::Key(chord) => {
                let bus_clone = bus.clone();
                let quit = bus
                    .invoke(move |editor| {
                        let outcome = editor.handle_key(&bus_clone, chord);
                        if let arx_core::KeyHandled::Unbound {
                            printable_fallback: Some(ch),
                        } = outcome
                        {
                            editor.handle_printable_fallback(ch);
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
                DaemonMessage::Welcome { .. }
                | DaemonMessage::Bell
                | DaemonMessage::SessionList(_)
                | DaemonMessage::SessionAttached { .. }
                | DaemonMessage::Error { .. } => {}
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

