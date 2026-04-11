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

use std::io::{self, Stdout, Write};
use std::sync::Arc;

use crossterm::event::Event;
use crossterm::{cursor, terminal, ExecutableCommand};
use futures_util::Stream;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use arx_core::{CommandBus, DEFAULT_BUS_CAPACITY, Editor, EventLoop};
use arx_render::{Backend, CrosstermBackend};

use crate::input::InputTask;
use crate::render::RenderTask;
use crate::state::{SharedTerminalSize, Shutdown};

/// Errors that can happen while running the driver.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("event loop join failure")]
    Join(#[from] tokio::task::JoinError),
}

/// Convenience: a builder / holder for driver configuration.
///
/// For Phase 1 there's nothing to configure beyond which seed function
/// to run against the editor before the event loop starts (used to open
/// an initial buffer / window). The next milestones will grow this
/// struct to hold the keymap, theme, and file paths from the CLI.
pub struct Driver {
    seed: Box<dyn FnOnce(&mut Editor) + Send>,
}

impl std::fmt::Debug for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Driver").finish_non_exhaustive()
    }
}

impl Driver {
    /// Create a driver with an optional "seed" function that runs against
    /// the editor before the event loop begins accepting input. Use this
    /// to open files, restore session state, etc.
    pub fn new<F>(seed: F) -> Self
    where
        F: FnOnce(&mut Editor) + Send + 'static,
    {
        Self {
            seed: Box::new(seed),
        }
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

        let result = self
            .run_with(events, backend, size, |_| async {})
            .await;

        // Restore the terminal before returning either Ok or Err.
        drop(guard);
        result
    }

    /// Generic entry point used by `run` and by tests. Drives the whole
    /// pipeline until either the input or render task signals shutdown.
    pub async fn run_with<S, B, F, Fut>(
        self,
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

        // Build the editor and run the seed *before* the event loop
        // starts. Going through the bus here would deadlock, because
        // `invoke` awaits a reply from a loop that hasn't been spawned
        // yet. Using `with_editor` sidesteps that entirely.
        let mut editor = Editor::new();
        let seed = self.seed;
        seed(&mut editor);
        let (event_loop, bus) = EventLoop::with_editor(editor, DEFAULT_BUS_CAPACITY);
        let event_loop = event_loop.with_redraw_notify(redraw.clone());

        // Spawn the event loop driver.
        let loop_handle: JoinHandle<Editor> = tokio::spawn(event_loop.run());

        // Spawn input + render tasks.
        let input_task = InputTask {
            events,
            bus: bus.clone(),
            size: size.clone(),
            shutdown: shutdown.clone(),
        };
        let render_task = RenderTask::new(
            backend,
            bus.clone(),
            redraw.clone(),
            shutdown.clone(),
            size.clone(),
        );
        let input_handle = tokio::spawn(input_task.run());
        let render_handle = tokio::spawn(render_task.run());

        // Run any test / embedding hook (no-op in the real driver).
        hook(bus.clone()).await;

        // Wait for input to end. When it does, also signal render shutdown
        // (it might already be noticing the same flag) and drop the bus
        // so the event loop drains.
        let _ = input_handle.await;
        shutdown.fire();
        let _ = render_handle.await;
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
