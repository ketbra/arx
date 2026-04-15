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
    Backend, CompletionEntry, CompletionView, Cursor, GlobalState, GutterConfig, KeditLineView,
    LayoutTree, PaletteEntry, PaletteView, Rect, RenderTree, ScrollPosition, SearchEntry,
    SearchView, Selection, SplitDirection, TerminalSize, TerminalViewCell, TerminalViewState,
    ViewState, WhichKeyEntry, WindowId as ViewWindowId, WindowState, diff, initial_paint, render,
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
    // Force a full repaint when the editor requests it (e.g. status
    // message change), to avoid stale-cell artifacts on terminals
    // that don't handle partial repaints well.
    let force_full = bus
        .invoke(|editor| editor.needs_full_repaint())
        .await
        .unwrap_or(false);
    let ops = match previous.as_ref() {
        Some(prev) if !force_full => diff(prev, &tree),
        _ => initial_paint(&tree),
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

/// Result of hit-testing a screen position against the layout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HitTest {
    /// The window (buffer pane or terminal pane) containing (x, y).
    pub window_id: CoreWindowId,
    /// Whether the hit is in a terminal pane.
    pub is_terminal: bool,
    /// Column relative to the pane's text area (0-based; 0 = first
    /// text column after the gutter). Clamped to `[0, text_width)`.
    pub text_col: u16,
    /// Row relative to the pane (0-based). Clamped to `[0, pane_height)`.
    pub row: u16,
}

/// How a left-mouse-down event was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickKind {
    /// First click: move cursor, clear selection.
    Single,
    /// Shift+click: move cursor, keep the current mark (extend
    /// selection). If no mark exists, plants one at the previous
    /// cursor position so the click produces a selection.
    ShiftClick,
    /// Second quick click at the same position: select the word at
    /// the click.
    DoubleClick,
    /// Third quick click at the same position: select the entire
    /// line at the click.
    TripleClick,
    /// Mouse drag: move the cursor while the mark stays where it was
    /// (or was anchored on mouse-down), creating a selection.
    Drag,
}

/// Handle a left-click or drag at screen position `(x, y)`.
/// See [`ClickKind`] for the semantics of each variant.
pub(crate) fn hit_test_and_click(
    editor: &mut arx_core::Editor,
    cols: u16,
    rows: u16,
    x: u16,
    y: u16,
    kind: ClickKind,
) {
    let Some(hit) = hit_test_at(editor, cols, rows, x, y) else {
        return;
    };

    // For terminal panes: focus on a fresh click but don't try to
    // place a "cursor" — the terminal handles its own cursor.
    if hit.is_terminal {
        if !matches!(kind, ClickKind::Drag) {
            editor.windows_mut().set_active(hit.window_id);
            editor.mark_dirty();
        }
        return;
    }

    // Buffer pane: focus on a fresh click.
    if !matches!(kind, ClickKind::Drag) {
        editor.windows_mut().set_active(hit.window_id);
    }

    let Some(data) = editor.windows().get(hit.window_id).cloned() else {
        return;
    };
    let Some(buffer) = editor.buffers().get(data.buffer_id) else {
        return;
    };
    let rope = buffer.rope();
    let target_line = data
        .scroll_top_line
        .saturating_add(hit.row as usize)
        .min(rope.len_lines().saturating_sub(1));
    let line_start = rope.line_to_byte(target_line);
    let line_end = if target_line + 1 < rope.len_lines() {
        rope.line_to_byte(target_line + 1).saturating_sub(1)
    } else {
        rope.len_bytes()
    };
    let line_text = rope.slice_to_string(line_start..line_end);
    let target_col = (hit.text_col as usize).saturating_add(data.scroll_left_col as usize);
    let byte_in_line =
        arx_core::column::display_col_to_byte(&line_text, target_col as u16);
    let target_byte = line_start + byte_in_line.min(line_end - line_start);

    match kind {
        ClickKind::Single => {
            editor.clear_mark(hit.window_id);
            if let Some(w) = editor.windows_mut().get_mut(hit.window_id) {
                w.cursor_byte = target_byte;
            }
        }
        ClickKind::ShiftClick => {
            // Extend selection: if no mark, anchor at old cursor so
            // the click produces a visible selection.
            if editor.mark(hit.window_id).is_none() {
                editor.set_mark(hit.window_id, data.cursor_byte);
            }
            if let Some(w) = editor.windows_mut().get_mut(hit.window_id) {
                w.cursor_byte = target_byte;
            }
        }
        ClickKind::Drag => {
            // Continue dragging: if no mark, anchor at the previous
            // cursor so the drag creates a selection from where it
            // started.
            if editor.mark(hit.window_id).is_none() {
                editor.set_mark(hit.window_id, data.cursor_byte);
            }
            if let Some(w) = editor.windows_mut().get_mut(hit.window_id) {
                w.cursor_byte = target_byte;
            }
        }
        ClickKind::DoubleClick => {
            // Select the word at target_byte.
            let (word_start, word_end) = word_range_at(&line_text, byte_in_line);
            let sel_start = line_start + word_start;
            let sel_end = line_start + word_end;
            editor.set_mark(hit.window_id, sel_start);
            if let Some(w) = editor.windows_mut().get_mut(hit.window_id) {
                w.cursor_byte = sel_end;
            }
        }
        ClickKind::TripleClick => {
            // Select the entire line (including trailing newline if
            // one exists, so "delete selection" removes the line
            // cleanly).
            let sel_end = if target_line + 1 < rope.len_lines() {
                rope.line_to_byte(target_line + 1)
            } else {
                rope.len_bytes()
            };
            editor.set_mark(hit.window_id, line_start);
            if let Some(w) = editor.windows_mut().get_mut(hit.window_id) {
                w.cursor_byte = sel_end;
            }
        }
    }
    editor.mark_dirty();
}

/// Find the start and end byte offsets of the word containing
/// `byte_in_line` within `line_text`. If the position is on a
/// non-word character, returns the range of the contiguous run of
/// non-word non-whitespace characters (so e.g. double-clicking on a
/// `->` selects the operator). Whitespace runs are preserved as-is.
fn word_range_at(line_text: &str, byte_in_line: usize) -> (usize, usize) {
    let bytes = line_text.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return (0, 0);
    }
    let pos = byte_in_line.min(len.saturating_sub(1));
    let is_word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let is_space = |b: u8| b == b' ' || b == b'\t';

    let classify = |b: u8| -> u8 {
        if is_word_char(b) {
            0
        } else if is_space(b) {
            1
        } else {
            2
        }
    };
    let target_class = classify(bytes[pos]);
    let mut start = pos;
    while start > 0 && classify(bytes[start - 1]) == target_class {
        start -= 1;
    }
    let mut end = pos;
    while end < len && classify(bytes[end]) == target_class {
        end += 1;
    }
    (start, end)
}

/// Scroll the pane under `(x, y)` by `delta` lines (positive = down,
/// negative = up).
pub(crate) fn mouse_scroll(
    editor: &mut arx_core::Editor,
    cols: u16,
    rows: u16,
    x: u16,
    y: u16,
    delta: i32,
) {
    let Some(hit) = hit_test_at(editor, cols, rows, x, y) else {
        return;
    };
    if hit.is_terminal {
        return;
    }
    let Some(data) = editor.windows().get(hit.window_id).cloned() else {
        return;
    };
    let Some(buffer) = editor.buffers().get(data.buffer_id) else {
        return;
    };
    let max_line = buffer.rope().len_lines().saturating_sub(1);
    let new_top = if delta < 0 {
        data.scroll_top_line.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        (data.scroll_top_line + delta as usize).min(max_line)
    };
    if let Some(w) = editor.windows_mut().get_mut(hit.window_id) {
        w.scroll_top_line = new_top;
    }
    editor.mark_dirty();
}

/// Hit-test a screen position against the current layout. Returns the
/// pane under `(x, y)` along with pane-local coordinates, or `None`
/// if the position is outside any pane (e.g. in the modeline).
pub(crate) fn hit_test_at(
    editor: &arx_core::Editor,
    cols: u16,
    rows: u16,
    x: u16,
    y: u16,
) -> Option<HitTest> {
    let layout = editor.windows().layout()?;
    // Same geometry as build_view_state_sync: reserve 1 row for modeline.
    let text_rows = rows.saturating_sub(1);
    let root_rect = Rect::new(0, 0, cols, text_rows);

    let mut visible_ids: Vec<CoreWindowId> = Vec::new();
    let view_layout = build_view_layout(layout, &mut visible_ids);

    let mut hit: Option<HitTest> = None;
    view_layout.walk_pane_rects(root_rect, &mut |vid, rect| {
        if hit.is_some() || rect.is_empty() {
            return;
        }
        if x < rect.x
            || x >= rect.x + rect.width
            || y < rect.y
            || y >= rect.y + rect.height
        {
            return;
        }
        let window_id = CoreWindowId(vid.0);
        let is_terminal = editor.terminal(window_id).is_some();

        // Compute gutter width for buffer panes so we can return a
        // text-area-relative column. Terminal panes have no gutter.
        let gutter_width = if is_terminal {
            0
        } else if let Some(data) = editor.windows().get(window_id) {
            let len_lines = editor
                .buffers()
                .get(data.buffer_id)
                .map_or(1, |b| b.rope().len_lines().max(1));
            let digits = digit_count(len_lines);
            let gc = GutterConfig::default();
            if gc.line_numbers {
                (digits.max(gc.min_width as usize) as u16) + 1
            } else {
                0
            }
        } else {
            0
        };

        let pane_col = x.saturating_sub(rect.x);
        let row = y.saturating_sub(rect.y);
        let text_col = pane_col.saturating_sub(gutter_width);
        hit = Some(HitTest {
            window_id,
            is_terminal,
            text_col,
            row,
        });
    });
    hit
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

    // Build the immutable WindowState / TerminalViewState projections
    // using the freshly-adjusted WindowData. Terminal panes are
    // identified by the side-table on Editor; everything else is a
    // buffer window.
    let gutter = GutterConfig::default();
    let mut windows: Vec<WindowState> = Vec::new();
    let mut terminal_panes: Vec<TerminalViewState> = Vec::new();
    for &id in &visible_ids {
        if let Some(term) = editor.terminal(id) {
            let snap = term.snapshot();
            terminal_panes.push(TerminalViewState {
                id: ViewWindowId(id.0),
                cells: snap
                    .cells
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|c| TerminalViewCell {
                                c: c.c,
                                fg: c.fg,
                                bg: c.bg,
                                bold: c.bold,
                                italic: c.italic,
                                underline: c.underline,
                            })
                            .collect()
                    })
                    .collect(),
                cursor: snap.cursor,
                cols: snap.cols,
                rows: snap.rows,
            });
        } else {
            let data = editor.windows().get(id)?.clone();
            let snapshot = editor.buffers().snapshot(data.buffer_id)?;
            let selection = editor.mark_state(id).and_then(|ms| {
                let cursor = data.cursor_byte;
                match ms.mode {
                    arx_core::SelectionMode::Linear => {
                        let start = ms.byte.min(cursor);
                        let end = ms.byte.max(cursor);
                        Some(Selection::Linear(start..end))
                    }
                    arx_core::SelectionMode::Rectangle => {
                        let rect = arx_core::column::RectRegion::from_mark_cursor(
                            editor, data.buffer_id, ms.byte, cursor,
                        )?;
                        Some(Selection::Rectangle {
                            start_line: rect.start_line,
                            end_line: rect.end_line,
                            left_col: rect.left_col,
                            right_col: rect.right_col,
                        })
                    }
                }
            });
            // KEDIT `ALL` filter: project the per-buffer excluded
            // line set into the ViewState so the renderer can skip
            // hidden lines without reaching back into the editor.
            let excluded_lines = editor
                .filter(data.buffer_id)
                .map(|f| f.excluded.clone())
                .unwrap_or_default();
            windows.push(WindowState {
                id: ViewWindowId(id.0),
                buffer: snapshot,
                cursors: smallvec![Cursor::at(data.cursor_byte)],
                scroll: ScrollPosition {
                    top_line: data.scroll_top_line,
                    left_col: data.scroll_left_col,
                },
                gutter,
                selection,
                excluded_lines,
            });
        }
    }

    let global = build_global_state(editor, active)?;

    Some(ViewState {
        size: TerminalSize::new(cols, rows),
        layout: view_layout,
        windows,
        terminal_panes,
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
        // Resize terminal panes to match their allocated rect.
        if let Some(term) = editor.terminal(id) {
            term.resize(rect.width, rect.height);
        }
    }
}

/// Build the global (modeline + palette overlay) state from the
/// currently-active pane.
#[allow(clippy::too_many_lines)]
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
            .map(|m| {
                // Append the keybinding shortcut to the description
                // so the user can discover bindings from the palette.
                let binding = editor.keymap().binding_for(&m.name);
                let desc = match binding {
                    Some(keys) if m.description.is_empty() => keys,
                    Some(keys) => format!("{} ({})", m.description, keys),
                    None => m.description.clone(),
                };
                PaletteEntry {
                    name: m.name.clone(),
                    description: desc,
                }
            })
            .collect::<Vec<_>>();
        Some(PaletteView {
            prompt: editor.palette().prompt().to_owned(),
            query: editor.palette().query().to_owned(),
            matches: entries,
            selected: editor.palette().selected_index(),
            max_rows: MAX_PALETTE_ROWS,
        })
    } else {
        None
    };
    let completion_view = if editor.completion().is_open() {
        const MAX_COMPLETION_ROWS: u16 = 8;
        let items = editor
            .completion()
            .items()
            .iter()
            .map(|i| CompletionEntry {
                label: i.label.clone(),
                detail: i.detail.clone().unwrap_or_default(),
                kind: i.kind.clone().unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        // Compute anchor position: where the cursor is on screen.
        let cursor_line = snapshot
            .rope()
            .byte_to_line(active_data.cursor_byte);
        let cursor_row =
            cursor_line.saturating_sub(active_data.scroll_top_line) as u16;
        let line_start = snapshot.rope().line_to_byte(cursor_line);
        let cursor_col = (active_data.cursor_byte - line_start) as u16;
        // Account for gutter width.
        let gutter_w = if GutterConfig::default().line_numbers {
            let digits = digit_count(snapshot.rope().len_lines().max(1));
            (digits.max(GutterConfig::default().min_width as usize) as u16) + 1
        } else {
            0
        };
        Some(CompletionView {
            items,
            selected: editor.completion().selected_index(),
            max_rows: MAX_COMPLETION_ROWS,
            anchor_col: gutter_w + cursor_col,
            anchor_row: cursor_row,
        })
    } else {
        None
    };

    // If there's a status message (hover info, LSP status), show it
    // in the modeline instead of the default line/byte info.
    // KEDIT `ALL` hidden-line count. Shown after the line-position
    // info so users can tell at a glance how much of the buffer is
    // filtered out.
    let filter_tag = editor
        .filter(active_data.buffer_id)
        .map(|f| format!("  [ALL /{pat}/  {n} excluded]", pat = f.pattern, n = f.excluded_count()))
        .unwrap_or_default();
    let left = if let Some(status) = editor.status_message() {
        status.to_owned()
    } else {
        format!(
            "{label}{modified_tag}  (ln {}/{}){filter_tag}",
            snapshot.rope().byte_to_line(active_data.cursor_byte) + 1,
            snapshot.rope().len_lines(),
        )
    };

    let which_key = editor.which_key().map(|entries| {
        entries
            .iter()
            .map(|(key, cmd)| WhichKeyEntry {
                key: key.clone(),
                command: cmd.clone(),
            })
            .collect()
    });

    let search_view = if editor.search().is_open() {
        const MAX_SEARCH_ROWS: u16 = 10;
        let total = editor.search().matches().len();
        let entries = editor
            .search()
            .matches()
            .iter()
            .map(|m| SearchEntry {
                line_number: m.line_number,
                line_text: m.line_text.clone(),
            })
            .collect::<Vec<_>>();
        Some(SearchView {
            prompt: format!("Search ({}): ", editor.search().mode().label()),
            query: editor.search().query().to_owned(),
            matches: entries,
            selected: editor.search().selected_index(),
            max_rows: MAX_SEARCH_ROWS,
            total_matches: total,
        })
    } else {
        None
    };

    let kedit_line = if editor.kedit().is_enabled() {
        Some(KeditLineView {
            prompt: "====> ".to_owned(),
            query: editor.kedit().query().to_owned(),
            cursor: editor.kedit().cursor(),
            focused: editor.kedit().is_focused(),
            message: editor.kedit().message().map(str::to_owned),
        })
    } else {
        None
    };

    Some(GlobalState {
        modeline_left: left,
        modeline_right: format!("{} bytes", text.len()),
        palette: palette_view,
        completion: completion_view,
        which_key,
        search: search_view,
        kedit_line,
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

    #[test]
    fn word_range_selects_alphanumeric_run() {
        assert_eq!(word_range_at("hello world", 0), (0, 5));
        assert_eq!(word_range_at("hello world", 2), (0, 5));
        assert_eq!(word_range_at("hello world", 4), (0, 5));
        assert_eq!(word_range_at("hello world", 6), (6, 11));
        assert_eq!(word_range_at("hello world", 10), (6, 11));
    }

    #[test]
    fn word_range_selects_whitespace_run() {
        assert_eq!(word_range_at("a   b", 1), (1, 4));
        assert_eq!(word_range_at("a   b", 2), (1, 4));
        assert_eq!(word_range_at("a   b", 3), (1, 4));
    }

    #[test]
    fn word_range_selects_punctuation_run() {
        assert_eq!(word_range_at("foo->bar", 3), (3, 5));
        assert_eq!(word_range_at("foo->bar", 4), (3, 5));
        assert_eq!(word_range_at("a == b", 2), (2, 4));
    }

    #[test]
    fn word_range_handles_underscore_as_word_char() {
        assert_eq!(word_range_at("my_var = 1", 0), (0, 6));
        assert_eq!(word_range_at("my_var = 1", 3), (0, 6));
    }

    #[test]
    fn word_range_handles_empty_and_past_end() {
        assert_eq!(word_range_at("", 0), (0, 0));
        assert_eq!(word_range_at("abc", 100), (0, 3));
    }

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
