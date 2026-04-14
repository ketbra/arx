//! [`Driver`] — the top-level composition: editor state + event loop +
//! input task + render task + terminal setup / teardown.
//!
//! Two entry points:
//!
//! * [`Driver::run`] — runs against a real terminal. Enables raw mode,
//!   switches to the alternate screen, spawns an input task over
//!   [`crossterm::event::EventStream`], spawns a render task over
//!   [`arx_render::CrosstermBackend`] around `io::stdout`, and drives
//!   everything to completion. Restores the terminal on exit.
//!
//! * [`Driver::run_with`] — generic over any async `Stream` of
//!   [`crossterm::event::Event`]s and any [`arx_render::Backend`]. Used
//!   by tests to drive the whole pipeline with a scripted event stream
//!   and a [`arx_render::TestBackend`], with no TTY involved.

use std::future::Future;
use std::io::{self, Stdout, Write};
use std::pin::Pin;
use std::sync::Arc;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::{ExecutableCommand, cursor, terminal};
use futures_util::Stream;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use arx_core::{CommandBus, DEFAULT_BUS_CAPACITY, Editor, EventLoop};
use arx_render::{Backend, CrosstermBackend};

use crate::input::InputTask;
use crate::render::RenderTask;
use crate::state::{SharedTerminalSize, Shutdown};

/// Erased async hook that runs after the event loop has been spawned and
/// seeded. The driver stores one of these and runs it from `run_with`.
type AsyncHook = Box<dyn FnOnce(CommandBus) -> BoxFuture<'static, ()> + Send>;
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Errors that can happen while running the driver.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("event loop join failure")]
    Join(#[from] tokio::task::JoinError),
}

/// Top-level driver configuration.
///
/// The driver has two seed hooks:
///
/// * a synchronous **pre-spawn seed** that runs against the editor
///   before the event loop is spawned (for fast, deterministic state
///   setup in tests — it cannot use the command bus because the loop
///   hasn't started yet);
/// * an asynchronous **post-spawn hook** that runs on the tokio runtime
///   once the event loop is up and the tasks are running, with access
///   to the [`CommandBus`]. This is where real-file-I/O startup work
///   lives: e.g. the `arx` binary uses it to open files from the
///   command line via [`arx_core::open_file`].
///
/// Both are optional; either can be a no-op.
pub struct Driver {
    seed: Box<dyn FnOnce(&mut Editor) + Send>,
    async_hook: Option<AsyncHook>,
    profile: Option<arx_keymap::profiles::Profile>,
}

impl std::fmt::Debug for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Driver")
            .field("async_hook", &self.async_hook.as_ref().map(|_| "..."))
            .finish_non_exhaustive()
    }
}

impl Driver {
    /// Create a driver with an optional synchronous pre-spawn seed.
    /// The seed runs against the editor **before** the event loop is
    /// spawned, so it must not use the command bus.
    pub fn new<F>(seed: F) -> Self
    where
        F: FnOnce(&mut Editor) + Send + 'static,
    {
        Self {
            seed: Box::new(seed),
            async_hook: None,
            profile: None,
        }
    }

    /// Use a specific keymap profile instead of the default (Emacs).
    #[must_use]
    pub fn with_profile(mut self, profile: arx_keymap::profiles::Profile) -> Self {
        self.profile = Some(profile);
        self
    }

    /// Attach a post-spawn async hook. The hook runs once after the
    /// event loop and input / render tasks have been spawned and has
    /// access to the [`CommandBus`]; use it for async startup work like
    /// reading files from disk.
    #[must_use]
    pub fn with_async_hook<F, Fut>(mut self, hook: F) -> Self
    where
        F: FnOnce(CommandBus) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.async_hook = Some(Box::new(move |bus| Box::pin(hook(bus))));
        self
    }

    /// Run against a real terminal (stdin / stdout). Sets up raw mode +
    /// the alternate screen, runs until shutdown, and restores the
    /// terminal on exit (even on error / panic).
    pub async fn run(self) -> Result<Editor, DriverError> {
        let mut stdout = io::stdout();
        let guard = TerminalGuard::enable(&mut stdout)?;
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        let size = SharedTerminalSize::new(cols, rows);

        let backend = CrosstermBackend::new(io::stdout(), cols, rows);
        let events = crossterm::event::EventStream::new();

        // `run_with` already consults the stored async hook — we just
        // supply a no-op extension hook.
        let result = self.run_with(events, backend, size, |_| async {}).await;

        // Restore the terminal before returning either Ok or Err.
        drop(guard);
        result
    }

    /// Generic entry point used by `run` and by tests. Drives the whole
    /// pipeline until either the input or render task signals shutdown.
    ///
    /// If `self` has an async hook installed via [`Self::with_async_hook`],
    /// it runs **before** the caller-supplied `hook`. That means
    /// `run_with` is composition-safe: the builder's hook is always
    /// honoured, and the explicit parameter is a per-call extension
    /// (typically a test waiter that lets render frames land before
    /// shutdown).
    pub async fn run_with<S, B, F, Fut>(
        mut self,
        events: S,
        backend: B,
        size: SharedTerminalSize,
        hook: F,
    ) -> Result<Editor, DriverError>
    where
        S: Stream<Item = io::Result<Event>> + Unpin + Send + 'static,
        B: Backend + Send + 'static,
        F: FnOnce(CommandBus) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let redraw = Arc::new(Notify::new());
        let shutdown = Shutdown::new();

        // Build the editor and run the synchronous seed *before* the
        // event loop starts. Going through the bus here would deadlock,
        // because `invoke` awaits a reply from a loop that hasn't been
        // spawned yet. Using `with_editor` sidesteps that entirely.
        let mut editor = match self.profile {
            Some(p) => Editor::with_profile(p),
            None => Editor::new(),
        };
        let seed = self.seed;
        seed(&mut editor);
        let (event_loop, bus) = EventLoop::with_editor(editor, DEFAULT_BUS_CAPACITY);
        let event_loop = event_loop.with_redraw_notify(redraw.clone());

        // Spawn the event loop driver.
        let loop_handle: JoinHandle<Editor> = tokio::spawn(event_loop.run());

        // Spawn the render task before the input task so it can catch
        // the redraws triggered by the async hook's `open_file` calls.
        let render_task = RenderTask::new(
            backend,
            bus.clone(),
            redraw.clone(),
            shutdown.clone(),
            size.clone(),
        );
        let render_handle = tokio::spawn(render_task.run());

        // Spawn the LSP manager task and install the notifier on the
        // editor so buffer events reach it.
        let (lsp_tx, lsp_rx) = tokio::sync::mpsc::channel(64);
        let lsp_manager = crate::lsp::LspManager::new(bus.clone());
        let _lsp_handle = tokio::spawn(lsp_manager.run(lsp_rx));
        let terminal_redraw = redraw.clone();
        bus.invoke(move |editor| {
            editor.set_lsp_notifier(lsp_tx);
            editor.set_terminal_redraw(terminal_redraw);
        })
        .await
        .ok();

        // Run the builder-provided async hook (e.g. "open files") to
        // completion **before** spawning the input task. Otherwise the
        // input task would start consuming events against an editor
        // that has no buffers or windows yet, and they'd all be no-ops.
        if let Some(builder_hook) = self.async_hook.take() {
            builder_hook(bus.clone()).await;
        }

        // Now it's safe to start consuming input.
        let input_task = InputTask {
            events,
            bus: bus.clone(),
            size: size.clone(),
            shutdown: shutdown.clone(),
        };
        let input_handle = tokio::spawn(input_task.run());

        // Per-call hook (e.g. "wait for frames to land" in tests).
        hook(bus.clone()).await;

        // Wait for input to end. When it does, signal render shutdown,
        // tear down the LSP notifier so the manager task can exit,
        // then drop the bus so the event loop drains.
        let _ = input_handle.await;
        shutdown.fire();
        let _ = render_handle.await;
        // Clear the LSP notifier sender inside the editor so the
        // LspManager's receiver sees the channel close and its run()
        // loop can exit cleanly. Without this, the sender lives inside
        // the Editor returned by the event loop and the manager hangs.
        bus.dispatch(|editor| {
            editor.clear_lsp_notifier();
        })
        .await
        .ok();
        drop(bus);
        let editor = loop_handle.await?;
        debug!("driver shut down cleanly");
        Ok(editor)
    }
}

/// RAII guard that enables raw mode + alternate screen on construction
/// and restores them on drop. Written so that panics on the render or
/// input task can't leave the user's terminal in a wedged state.
struct TerminalGuard {
    enabled: bool,
}

impl TerminalGuard {
    fn enable(out: &mut Stdout) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        out.execute(terminal::EnterAlternateScreen)?;
        out.execute(cursor::Hide)?;
        // Enable mouse reporting so the user can click to move the
        // cursor, drag to select, and scroll with the wheel.
        out.execute(EnableMouseCapture)?;
        out.flush()?;
        Ok(Self { enabled: true })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        let mut out = io::stdout();
        if let Err(err) = out.execute(DisableMouseCapture) {
            warn!(%err, "failed to disable mouse capture");
        }
        if let Err(err) = out.execute(cursor::Show) {
            warn!(%err, "failed to restore cursor visibility");
        }
        if let Err(err) = out.execute(terminal::LeaveAlternateScreen) {
            warn!(%err, "failed to leave alternate screen");
        }
        if let Err(err) = terminal::disable_raw_mode() {
            warn!(%err, "failed to disable raw mode");
        }
    }
}
