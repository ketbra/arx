//! Render task: react to editor state changes and repaint the terminal.
//!
//! Composed of three tight loops:
//!
//! 1. Wait on a redraw [`Notify`](tokio::sync::Notify) pinged by the
//!    [`arx_core::EventLoop`] after each command runs.
//! 2. `invoke` the command bus for a fresh [`arx_render::ViewState`]
//!    snapshot.
//! 3. Call [`arx_render::render`], diff against the previous frame, and
//!    apply the diff ops to a [`arx_render::Backend`].
//!
//! The render task is written generically over the backend so tests can
//! drive it with a [`arx_render::TestBackend`] instead of
//! [`arx_render::CrosstermBackend`].

use std::sync::Arc;

use smallvec::smallvec;
use tokio::sync::Notify;
use tracing::{debug, trace, warn};

use arx_core::CommandBus;
use arx_render::{
    Backend, Cursor, GlobalState, GutterConfig, LayoutTree, RenderTree, ScrollPosition,
    TerminalSize, ViewState, WindowId as ViewWindowId, WindowState, diff, initial_paint, render,
};

use crate::state::{SharedTerminalSize, Shutdown};

/// Task state for the render loop. Constructed by the driver, then
/// consumed by [`RenderTask::run`].
pub struct RenderTask<B: Backend + Send + 'static> {
    pub backend: B,
    pub bus: CommandBus,
    pub redraw: Arc<Notify>,
    pub shutdown: Shutdown,
    pub size: SharedTerminalSize,
}

impl<B: Backend + Send + 'static> std::fmt::Debug for RenderTask<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderTask")
            .field("bus", &self.bus)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl<B: Backend + Send + 'static> RenderTask<B> {
    pub fn new(
        backend: B,
        bus: CommandBus,
        redraw: Arc<Notify>,
        shutdown: Shutdown,
        size: SharedTerminalSize,
    ) -> Self {
        Self {
            backend,
            bus,
            redraw,
            shutdown,
            size,
        }
    }

    /// Run the render loop until the shutdown notify fires or the command
    /// bus closes. Returns the backend so tests can inspect it.
    ///
    /// `run` consumes `self` and then works against locals only, so the
    /// generated state machine never holds `&self` across an `.await` —
    /// keeping the future `Send` even when `B` isn't `Sync`.
    pub async fn run(self) -> B {
        let RenderTask {
            mut backend,
            bus,
            redraw,
            shutdown,
            size,
        } = self;
        let mut frame_id: u64 = 0;
        let mut previous: Option<RenderTree> = None;

        // Always draw once so the user sees something even if no commands
        // have run yet.
        if let Err(err) =
            draw_once(&mut backend, &bus, &size, &mut frame_id, &mut previous).await
        {
            warn!(%err, "initial draw failed");
        }

        loop {
            if shutdown.is_fired() {
                debug!("render task shutting down");
                break;
            }
            tokio::select! {
                biased;
                () = shutdown.wait() => {
                    debug!("render task shutting down");
                    break;
                }
                () = redraw.notified() => {
                    if let Err(err) =
                        draw_once(&mut backend, &bus, &size, &mut frame_id, &mut previous).await
                    {
                        warn!(%err, "draw failed");
                    }
                }
            }
        }

        // One final draw at shutdown time so any mutations that landed
        // between the last redraw wake-up and the shutdown flag become
        // visible on the backend. Matters in tests (observe final state)
        // and in real use (if the user makes a last-millisecond edit
        // before Ctrl+Q, show it before tearing down the terminal).
        if let Err(err) =
            draw_once(&mut backend, &bus, &size, &mut frame_id, &mut previous).await
        {
            warn!(%err, "final draw failed");
        }
        backend
    }
}

async fn draw_once<B: Backend>(
    backend: &mut B,
    bus: &CommandBus,
    size: &SharedTerminalSize,
    frame_id: &mut u64,
    previous: &mut Option<RenderTree>,
) -> std::io::Result<()> {
    *frame_id = frame_id.wrapping_add(1);
    let (cols, rows) = size.get();
    let Some(state) = build_view_state(bus, cols, rows).await else {
        // No active window — nothing to draw.
        return Ok(());
    };
    let tree = render(&state, *frame_id);
    let ops = match previous.as_ref() {
        Some(prev) => diff(prev, &tree),
        None => initial_paint(&tree),
    };
    trace!(ops = ops.len(), "applying render ops");
    if !ops.is_empty() {
        backend.apply(&ops)?;
        backend.present()?;
    }
    *previous = Some(tree);
    Ok(())
}

/// Build a fresh [`ViewState`] by round-tripping through the command
/// bus. Keeps the single-writer invariant: only the event-loop task
/// touches the `Editor`.
///
/// Also writes the computed text-area size back into the active
/// window's [`arx_core::WindowData`] so cursor-visibility and
/// page-scroll commands (which run in the event loop, not here) know
/// how much space they're scrolling against. That mutation happens
/// inside the same `invoke` closure as the read, so there's no
/// round-trip and no chance of racing a render against a resize.
async fn build_view_state(bus: &CommandBus, cols: u16, rows: u16) -> Option<ViewState> {
    bus.invoke(move |editor| {
        let active = editor.windows().active()?;
        let data = editor.windows().get(active)?.clone();
        let snapshot = editor.buffers().snapshot(data.buffer_id)?;
        let is_modified = editor
            .buffers()
            .get(data.buffer_id)
            .is_some_and(arx_buffer::Buffer::is_modified);
        let label = editor
            .buffers()
            .path(data.buffer_id)
            .and_then(|p| p.file_name())
            .map_or_else(
                || format!("buffer {}", data.buffer_id.0),
                |n| n.to_string_lossy().into_owned(),
            );
        let text = snapshot.text();
        let modified_tag = if is_modified { " [+]" } else { "" };
        let global = GlobalState {
            modeline_left: format!(
                "{label}{modified_tag}  (ln {}/{})",
                snapshot.rope().byte_to_line(data.cursor_byte) + 1,
                snapshot.rope().len_lines(),
            ),
            modeline_right: format!("{} bytes", text.len()),
        };

        // Mirror the layout calculation the view layer does so the
        // values we write back match what the renderer will actually
        // use. This is duplicated logic, but the shape is tiny: bottom
        // row reserved for the modeline, left gutter sized to fit the
        // largest line number (with the `GutterConfig::default()`
        // min_width of 4 plus a one-cell pad).
        let text_rows = rows.saturating_sub(1);
        let gutter = GutterConfig::default();
        let gutter_width = if gutter.line_numbers {
            let digits = digit_count(snapshot.rope().len_lines().max(1));
            (digits.max(gutter.min_width as usize) as u16) + 1
        } else {
            0
        };
        let text_cols = cols.saturating_sub(gutter_width);

        if let Some(window) = editor.windows_mut().get_mut(active) {
            window.visible_rows = text_rows;
            window.visible_cols = text_cols;
        }
        // If that update shifted the text area enough that the cursor
        // is no longer inside it (e.g. on a terminal resize), fix the
        // scroll position before building the view state so this very
        // frame reflects the corrected scroll.
        editor.ensure_active_cursor_visible();
        let data = editor.windows().get(active)?.clone();

        Some(ViewState {
            size: TerminalSize::new(cols, rows),
            layout: LayoutTree::Single(ViewWindowId(active.0)),
            windows: vec![WindowState {
                id: ViewWindowId(active.0),
                buffer: snapshot,
                cursors: smallvec![Cursor::at(data.cursor_byte)],
                scroll: ScrollPosition {
                    top_line: data.scroll_top_line,
                    left_col: data.scroll_left_col,
                },
                gutter,
            }],
            global,
        })
    })
    .await
    .ok()
    .flatten()
}

fn digit_count(mut n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut c = 0;
    while n > 0 {
        c += 1;
        n /= 10;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_core::EventLoop;
    use arx_render::TestBackend;

    #[tokio::test]
    async fn draws_the_current_buffer_into_the_backend() {
        let redraw = Arc::new(Notify::new());
        let shutdown = Shutdown::new();
        let (event_loop, bus) = EventLoop::new();
        let event_loop = event_loop.with_redraw_notify(redraw.clone());
        let loop_handle = tokio::spawn(event_loop.run());

        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello\nworld", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();

        let backend = TestBackend::new(30, 5);
        let task = RenderTask::new(
            backend,
            bus.clone(),
            redraw.clone(),
            shutdown.clone(),
            SharedTerminalSize::new(30, 5),
        );
        // Spawn the task and give it a moment to run the initial draw.
        let task_handle = tokio::spawn(task.run());
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        shutdown.fire();
        let backend = task_handle.await.unwrap();

        let text = backend.grid().to_debug_text();
        assert!(text.contains("hello"), "{text:?}");
        assert!(text.contains("world"), "{text:?}");

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn redraw_notify_picks_up_buffer_changes() {
        let redraw = Arc::new(Notify::new());
        let shutdown = Shutdown::new();
        let (event_loop, bus) = EventLoop::new();
        let event_loop = event_loop.with_redraw_notify(redraw.clone());
        let loop_handle = tokio::spawn(event_loop.run());

        let buf_id = bus
            .invoke(|editor| {
                let buf = editor.buffers_mut().create_from_text("one", None);
                editor.windows_mut().open(buf);
                buf
            })
            .await
            .unwrap();

        let backend = TestBackend::new(30, 5);
        let task = RenderTask::new(
            backend,
            bus.clone(),
            redraw.clone(),
            shutdown.clone(),
            SharedTerminalSize::new(30, 5),
        );
        let handle = tokio::spawn(task.run());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        bus.invoke(move |editor| {
            editor
                .buffers_mut()
                .edit(buf_id, 3..3, " two", arx_buffer::EditOrigin::User);
            editor.mark_dirty();
        })
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        shutdown.fire();
        let backend = handle.await.unwrap();
        let text = backend.grid().to_debug_text();
        assert!(text.contains("one two"), "{text:?}");

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }
}
