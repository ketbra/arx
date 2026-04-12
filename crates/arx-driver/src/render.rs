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

use arx_core::{CommandBus, Layout as CoreLayout, SplitAxis, WindowId as CoreWindowId};
use arx_render::{
    Backend, Cursor, GlobalState, GutterConfig, LayoutTree, PaletteEntry, PaletteView, Rect,
    RenderTree, ScrollPosition, SplitDirection, TerminalSize, ViewState,
    WindowId as ViewWindowId, WindowState, diff, initial_paint, render,
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
/// Also writes the computed text-area size back into every visible
/// window's [`arx_core::WindowData`] so cursor-visibility and
/// page-scroll commands (which run in the event loop, not here) know
/// how much space they're scrolling against. In a split layout, every
/// leaf's dimensions are updated — not just the active pane's. The
/// mutations happen inside the same `invoke` closure as the read, so
/// there's no round-trip and no chance of racing a render against a
/// resize.
async fn build_view_state(bus: &CommandBus, cols: u16, rows: u16) -> Option<ViewState> {
    bus.invoke(move |editor| build_view_state_sync(editor, cols, rows))
        .await
        .ok()
        .flatten()
}

fn build_view_state_sync(
    editor: &mut arx_core::Editor,
    cols: u16,
    rows: u16,
) -> Option<ViewState> {
    let active = editor.windows().active()?;
    let layout = editor.windows().layout()?.clone();

    // Build the projection ViewState layout from the core layout
    // tree, collecting the set of visible window ids as we go.
    let mut visible_ids: Vec<CoreWindowId> = Vec::new();
    let view_layout = build_view_layout(&layout, &mut visible_ids);
    if visible_ids.is_empty() {
        return None;
    }

    // Walk the layout to get per-pane rects, then write each pane's
    // text-area dimensions back to WindowData.
    let text_rows = rows.saturating_sub(1);
    let root_rect = Rect::new(0, 0, cols, text_rows);
    let mut pane_rects: Vec<(ViewWindowId, Rect)> = Vec::new();
    view_layout.walk_pane_rects(root_rect, &mut |id, rect| {
        pane_rects.push((id, rect));
    });
    write_back_pane_dimensions(editor, &visible_ids, &pane_rects);

    // If the write-back just shifted the active pane's text area
    // (e.g. on a terminal resize or layout change), fix its scroll
    // position before building the ViewState so this very frame
    // reflects the corrected scroll.
    editor.ensure_active_cursor_visible();

    // Build the immutable WindowState projections using the freshly-
    // adjusted WindowData.
    let gutter = GutterConfig::default();
    let mut windows: Vec<WindowState> = Vec::with_capacity(visible_ids.len());
    for &id in &visible_ids {
        let data = editor.windows().get(id)?.clone();
        let snapshot = editor.buffers().snapshot(data.buffer_id)?;
        windows.push(WindowState {
            id: ViewWindowId(id.0),
            buffer: snapshot,
            cursors: smallvec![Cursor::at(data.cursor_byte)],
            scroll: ScrollPosition {
                top_line: data.scroll_top_line,
                left_col: data.scroll_left_col,
            },
            gutter,
        });
    }

    let global = build_global_state(editor, active)?;

    Some(ViewState {
        size: TerminalSize::new(cols, rows),
        layout: view_layout,
        windows,
        active_window: Some(ViewWindowId(active.0)),
        global,
    })
}

/// Write each visible pane's text-area size back into its
/// [`arx_core::WindowData`]. The text area is the pane rect minus the
/// width of its (per-pane) gutter. Commands like `scroll.page-down`
/// and [`arx_core::Editor::ensure_active_cursor_visible`] consume
/// these fields, so keeping them current per-frame is what lets
/// multi-pane layouts scroll correctly.
fn write_back_pane_dimensions(
    editor: &mut arx_core::Editor,
    visible_ids: &[CoreWindowId],
    pane_rects: &[(ViewWindowId, Rect)],
) {
    let gutter = GutterConfig::default();
    for &id in visible_ids {
        let rect = pane_rects
            .iter()
            .find(|(vid, _)| vid.0 == id.0)
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r);
        let Some(data) = editor.windows().get(id).cloned() else {
            continue;
        };
        let gutter_width = if gutter.line_numbers {
            let len_lines = editor
                .buffers()
                .get(data.buffer_id)
                .map_or(1, |b| b.rope().len_lines().max(1));
            let digits = digit_count(len_lines);
            (digits.max(gutter.min_width as usize) as u16) + 1
        } else {
            0
        };
        let text_width = rect.width.saturating_sub(gutter_width);
        if let Some(window) = editor.windows_mut().get_mut(id) {
            window.visible_rows = rect.height;
            window.visible_cols = text_width;
        }
    }
}

/// Build the global (modeline + palette overlay) state from the
/// currently-active pane.
fn build_global_state(
    editor: &arx_core::Editor,
    active: CoreWindowId,
) -> Option<GlobalState> {
    let active_data = editor.windows().get(active)?.clone();
    let snapshot = editor.buffers().snapshot(active_data.buffer_id)?;
    let is_modified = editor
        .buffers()
        .get(active_data.buffer_id)
        .is_some_and(arx_buffer::Buffer::is_modified);
    let label = editor
        .buffers()
        .path(active_data.buffer_id)
        .and_then(|p| p.file_name())
        .map_or_else(
            || format!("buffer {}", active_data.buffer_id.0),
            |n| n.to_string_lossy().into_owned(),
        );
    let text = snapshot.text();
    let modified_tag = if is_modified { " [+]" } else { "" };
    let palette_view = if editor.palette().is_open() {
        // Cap the visible match list at 8 rows — a nice middle
        // ground: enough to browse stock commands without swallowing
        // the whole editor.
        const MAX_PALETTE_ROWS: u16 = 8;
        let entries = editor
            .palette()
            .matches()
            .iter()
            .map(|m| PaletteEntry {
                name: m.name.clone(),
                description: m.description.clone(),
            })
            .collect::<Vec<_>>();
        Some(PaletteView {
            query: editor.palette().query().to_owned(),
            matches: entries,
            selected: editor.palette().selected_index(),
            max_rows: MAX_PALETTE_ROWS,
        })
    } else {
        None
    };
    Some(GlobalState {
        modeline_left: format!(
            "{label}{modified_tag}  (ln {}/{})",
            snapshot.rope().byte_to_line(active_data.cursor_byte) + 1,
            snapshot.rope().len_lines(),
        ),
        modeline_right: format!("{} bytes", text.len()),
        palette: palette_view,
    })
}

/// Recursively translate a logical [`CoreLayout`] into the render
/// layer's [`LayoutTree`], collecting the visible window ids along the
/// way. The traversal order matches [`CoreLayout::leaves`] so callers
/// can rely on the ordering.
fn build_view_layout(layout: &CoreLayout, out: &mut Vec<CoreWindowId>) -> LayoutTree {
    match layout {
        CoreLayout::Leaf(id) => {
            out.push(*id);
            LayoutTree::Single(ViewWindowId(id.0))
        }
        CoreLayout::Split {
            axis,
            ratio,
            first,
            second,
        } => LayoutTree::Split {
            direction: split_axis_to_direction(*axis),
            ratio: *ratio,
            first: Box::new(build_view_layout(first, out)),
            second: Box::new(build_view_layout(second, out)),
        },
    }
}

fn split_axis_to_direction(axis: SplitAxis) -> SplitDirection {
    match axis {
        SplitAxis::Horizontal => SplitDirection::Horizontal,
        SplitAxis::Vertical => SplitDirection::Vertical,
    }
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

    #[tokio::test]
    async fn split_layout_renders_both_panes_and_divider() {
        // End-to-end: create a single window, split it vertically, and
        // confirm the backend grid shows content from both panes plus a
        // divider glyph. This proves the core ↔ view-state ↔ render
        // path composes correctly for split layouts.
        let redraw = Arc::new(Notify::new());
        let shutdown = Shutdown::new();
        let (event_loop, bus) = EventLoop::new();
        let event_loop = event_loop.with_redraw_notify(redraw.clone());
        let loop_handle = tokio::spawn(event_loop.run());

        bus.invoke(|editor| {
            let buf = editor
                .buffers_mut()
                .create_from_text("alpha\nbeta", None);
            editor.windows_mut().open(buf);
            // Split into two panes viewing the same buffer.
            editor
                .windows_mut()
                .split_active(SplitAxis::Vertical, buf)
                .unwrap();
            editor.mark_dirty();
        })
        .await
        .unwrap();

        let backend = TestBackend::new(40, 6);
        let task = RenderTask::new(
            backend,
            bus.clone(),
            redraw.clone(),
            shutdown.clone(),
            SharedTerminalSize::new(40, 6),
        );
        let task_handle = tokio::spawn(task.run());
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        shutdown.fire();
        let backend = task_handle.await.unwrap();

        let text = backend.grid().to_debug_text();
        let first_row = text.split('\n').next().unwrap();
        // A vertical split paints a "│" divider glyph somewhere on
        // every row of the text area.
        assert!(
            first_row.contains('\u{2502}'),
            "no vertical divider in: {first_row:?}",
        );
        // Both panes show their buffer text ("alpha" shows up on
        // each side of the divider).
        let alpha_count = first_row.matches("alpha").count();
        assert_eq!(
            alpha_count, 2,
            "expected two copies of 'alpha' on row 0: {first_row:?}",
        );

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn close_window_collapses_back_to_single_pane() {
        let redraw = Arc::new(Notify::new());
        let shutdown = Shutdown::new();
        let (event_loop, bus) = EventLoop::new();
        let event_loop = event_loop.with_redraw_notify(redraw.clone());
        let loop_handle = tokio::spawn(event_loop.run());

        bus.invoke(|editor| {
            let buf = editor
                .buffers_mut()
                .create_from_text("solo", None);
            editor.windows_mut().open(buf);
            editor
                .windows_mut()
                .split_active(SplitAxis::Horizontal, buf)
                .unwrap();
            // Active is now the new (second) pane. Close it.
            let active = editor.windows().active().unwrap();
            editor.windows_mut().close(active);
            editor.mark_dirty();
        })
        .await
        .unwrap();

        let layout_leaves = bus
            .invoke(|editor| editor.windows().layout().unwrap().leaves().len())
            .await
            .unwrap();
        assert_eq!(layout_leaves, 1);

        let backend = TestBackend::new(30, 5);
        let task = RenderTask::new(
            backend,
            bus.clone(),
            redraw.clone(),
            shutdown.clone(),
            SharedTerminalSize::new(30, 5),
        );
        let handle = tokio::spawn(task.run());
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        shutdown.fire();
        let backend = handle.await.unwrap();

        let text = backend.grid().to_debug_text();
        // No divider glyph after close.
        assert!(
            !text.contains('\u{2502}') && !text.contains('\u{2500}'),
            "divider should be gone: {text:?}",
        );
        assert!(text.contains("solo"));

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }
}
