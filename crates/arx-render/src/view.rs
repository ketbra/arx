//! Pure function: [`ViewState`] → [`RenderTree`].
//!
//! This is the heart of the rendering pipeline (spec §4). It walks each
//! window's buffer snapshot, iterates Unicode grapheme clusters with
//! display-width awareness (so CJK / emoji occupy two cells), applies
//! every relevant property-layer face on top of a theme default, and
//! stamps the result into a [`CellGrid`].
//!
//! ## Why it's a pure function
//!
//! Separating "what should appear on screen" from "how to get it there"
//! (the differ + backend) means:
//!
//! * The renderer can be tested in-memory with a [`TestBackend`] — no TTY.
//! * A future GPU backend can consume the same [`RenderTree`] without
//!   touching this file.
//! * Property-layer changes produce the same render output regardless of
//!   what produced them (tree-sitter, LSP, agent edits…).
//!
//! [`TestBackend`]: crate::backend::TestBackend

use compact_str::CompactString;
use smallvec::SmallVec;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use arx_buffer::{PropertyMap, StyledRun};

use crate::cell::{Cell, CellFlags, CellGrid};
use crate::face::{Color, ResolvedFace};
use crate::render_tree::{CursorRender, CursorStyle, RenderTree};
use crate::view_state::{
    GutterConfig, PaletteView, Rect, SplitDirection, TerminalSize, ViewState, WindowState,
};

/// Render a complete [`ViewState`] into a [`RenderTree`].
///
/// Phase 2 handles both single-window layouts and arbitrary nested
/// [`LayoutTree::Split`] trees. Each leaf is painted inside its
/// computed rectangle; a 1-cell separator is painted for every
/// internal split node.
///
/// When [`GlobalState::palette`] is `Some(_)`, the bottom of the
/// viewport is partitioned like this:
///
/// ```text
///     ┌────────────── text area (shrunk) ──────────────┐
///     │                                                │
///     │                                                │
///     ├─── matches row (N rows) ───────────────────────┤
///     ├─── prompt row  (1 row) ────────────────────────┤
///     └─── modeline    (1 row) ────────────────────────┘
/// ```
///
/// The shrinking happens before the layout walker runs so per-pane
/// cursor-visibility math doesn't collide with the overlay.
///
/// The terminal cursor is only emitted for the window whose id matches
/// [`ViewState::active_window`]. Inactive panes still paint their
/// text, but the cursor visually lives with the focused pane.
pub fn render(state: &ViewState, frame_id: u64) -> RenderTree {
    let TerminalSize { cols, rows } = state.size;
    let mut grid = CellGrid::new(cols, rows);
    let mut cursors: SmallVec<[CursorRender; 1]> = SmallVec::new();

    // Reserve the bottom row for the modeline if there's height to spare.
    let (mut text_rows, modeline_row) = if rows >= 1 {
        (rows - 1, Some(rows - 1))
    } else {
        (0, None)
    };

    // Palette overlay eats additional rows above the modeline.
    let palette_layout = state.global.palette.as_ref().map(|p| {
        let matches_rows = p.max_rows.min(text_rows);
        let prompt_row = if matches_rows < text_rows {
            Some(text_rows - matches_rows - 1)
        } else {
            None
        };
        let matches_top = prompt_row.map(|r| r + 1);
        PaletteLayout {
            view: p,
            prompt_row,
            matches_top,
            matches_rows,
        }
    });
    if let Some(layout) = palette_layout.as_ref() {
        if let Some(prompt) = layout.prompt_row {
            text_rows = prompt;
        }
    }

    // Search overlay — same bottom-overlay geometry as the palette.
    // Search and palette are mutually exclusive; the search overlay
    // only paints if no palette is open.
    let search_layout = if palette_layout.is_none() {
        state.global.search.as_ref().map(|s| {
            let matches_rows = s.max_rows.min(text_rows);
            let prompt_row = if matches_rows < text_rows {
                Some(text_rows - matches_rows - 1)
            } else {
                None
            };
            let matches_top = prompt_row.map(|r| r + 1);
            SearchLayout {
                view: s,
                prompt_row,
                matches_top,
                matches_rows,
            }
        })
    } else {
        None
    };
    if let Some(ref layout) = search_layout {
        if let Some(prompt) = layout.prompt_row {
            text_rows = prompt;
        }
    }

    // KEDIT persistent command line — one row above the modeline, but
    // below any palette/search overlay. Unlike the palette this is
    // *always* painted when enabled, whether or not it has focus, so
    // the user can see the prompt as a reminder of the profile.
    let kedit_row = if palette_layout.is_none()
        && search_layout.is_none()
        && state.global.kedit_line.is_some()
        && text_rows > 0
    {
        let row = text_rows - 1;
        text_rows = row;
        Some(row)
    } else {
        None
    };

    let root_rect = Rect::new(0, 0, cols, text_rows);
    let active = state.active_window;

    // Walk the layout once to paint every visible pane inside its
    // computed rect. Each leaf is either a buffer window or a
    // terminal pane.
    state.layout.walk_pane_rects(root_rect, &mut |id, rect| {
        let is_active = active == Some(id);
        if let Some(term) = state.terminal_panes.iter().find(|t| t.id == id) {
            render_terminal_pane(term, rect, is_active, &mut grid, &mut cursors);
        } else if let Some(window) = state.windows.iter().find(|w| w.id == id) {
            render_window(window, rect, is_active, &mut grid, &mut cursors);
        }
    });

    // Second walk for separators between panes.
    state
        .layout
        .walk_divider_rects(root_rect, &mut |rect, direction| {
            paint_divider(&mut grid, rect, direction);
        });

    if let Some(layout) = palette_layout {
        paint_palette(&layout, cols, &mut grid, &mut cursors);
    }

    if let Some(layout) = search_layout {
        paint_search(&layout, cols, &mut grid, &mut cursors);
    }

    if let (Some(row), Some(view)) = (kedit_row, state.global.kedit_line.as_ref()) {
        paint_kedit_line(view, row, cols, &mut grid, &mut cursors);
    }

    if let Some(ref completion) = state.global.completion {
        paint_completion(completion, cols, text_rows, &mut grid);
    }

    if let Some(ref entries) = state.global.which_key {
        paint_which_key(entries, cols, text_rows, &mut grid);
    }

    if let Some(row) = modeline_row {
        render_modeline(&state.global, row, cols, &mut grid);
    }

    RenderTree::new(grid, cursors, frame_id)
}

/// Paint a single-cell-wide or single-row-tall separator between two
/// panes. Uses box-drawing characters on top of the default face.
fn paint_divider(grid: &mut CellGrid, rect: Rect, direction: SplitDirection) {
    if rect.is_empty() {
        return;
    }
    let face = ResolvedFace {
        fg: Color::rgb(0x60, 0x60, 0x60),
        bg: ResolvedFace::DEFAULT.bg,
        ..ResolvedFace::DEFAULT
    };
    let glyph = match direction {
        SplitDirection::Vertical => "\u{2502}",  // │
        SplitDirection::Horizontal => "\u{2500}", // ─
    };
    for dy in 0..rect.height {
        for dx in 0..rect.width {
            grid.set(
                rect.x + dx,
                rect.y + dy,
                Cell {
                    grapheme: CompactString::new(glyph),
                    face,
                    flags: CellFlags::empty(),
                },
            );
        }
    }
}

/// Render an embedded terminal pane into its bounding rectangle.
/// Reads from the terminal's grid snapshot and paints each cell with
/// its own foreground/background colours.
fn render_terminal_pane(
    term: &crate::view_state::TerminalViewState,
    rect: Rect,
    is_active: bool,
    grid: &mut CellGrid,
    cursors: &mut SmallVec<[CursorRender; 1]>,
) {
    if rect.is_empty() {
        return;
    }
    // Clear the pane rect first.
    for dy in 0..rect.height {
        for dx in 0..rect.width {
            grid.set(rect.x + dx, rect.y + dy, Cell::blank());
        }
    }
    // Paint each terminal cell.
    for (row_idx, row) in term.cells.iter().enumerate() {
        if row_idx as u16 >= rect.height {
            break;
        }
        let y = rect.y + row_idx as u16;
        for (col_idx, cell) in row.iter().enumerate() {
            if col_idx as u16 >= rect.width {
                break;
            }
            let x = rect.x + col_idx as u16;
            let face = ResolvedFace {
                fg: Color(cell.fg),
                bg: Color(cell.bg),
                bold: cell.bold,
                italic: cell.italic,
                underline: if cell.underline {
                    Some(arx_buffer::UnderlineStyle::Straight)
                } else {
                    None
                },
                ..ResolvedFace::DEFAULT
            };
            let grapheme = if cell.c.is_empty() || cell.c == "\0" {
                CompactString::const_new(" ")
            } else {
                CompactString::new(&cell.c)
            };
            grid.set(
                x,
                y,
                Cell {
                    grapheme,
                    face,
                    flags: CellFlags::empty(),
                },
            );
        }
    }
    // Cursor for the active terminal pane.
    if is_active {
        if let Some((col, row)) = term.cursor {
            let cx = rect.x + col;
            let cy = rect.y + row;
            if cx < rect.x + rect.width && cy < rect.y + rect.height {
                if let Some(cell) = grid.get_mut(cx, cy) {
                    cell.flags |= CellFlags::CURSOR_PRIMARY;
                }
                cursors.push(CursorRender {
                    col: cx,
                    row: cy,
                    style: CursorStyle::Block,
                });
            }
        }
    }
}

/// Pre-computed palette overlay geometry for the current frame.
struct PaletteLayout<'a> {
    view: &'a PaletteView,
    /// Row index of the single-line prompt. `None` means the terminal
    /// is too small to show the overlay; callers skip painting.
    prompt_row: Option<u16>,
    /// Row index of the first match row.
    matches_top: Option<u16>,
    /// Number of match rows (may be 0 if the list is empty or the
    /// terminal is very small).
    matches_rows: u16,
}

/// Render a single window into its bounding rectangle.
///
/// `is_active` controls whether the terminal cursor is emitted for this
/// pane. In multi-pane layouts only the focused pane gets a cursor so
/// users can tell at a glance which pane their keystrokes will go to.
fn render_window(
    window: &WindowState,
    rect: Rect,
    is_active: bool,
    grid: &mut CellGrid,
    cursors: &mut SmallVec<[CursorRender; 1]>,
) {
    if rect.is_empty() {
        return;
    }

    // First clear the pane to the default face so leftover cells from
    // a previous larger pane can't bleed through after a layout
    // change. The diff layer will compress this into O(changed) ops on
    // the next frame regardless.
    for dy in 0..rect.height {
        for dx in 0..rect.width {
            grid.set(rect.x + dx, rect.y + dy, Cell::blank());
        }
    }

    let buffer = &window.buffer;
    let rope = buffer.rope();
    let properties = buffer.properties();

    // How much of the window the gutter claims. We need the final line
    // number to know the gutter width.
    let last_visible_line =
        (window.scroll.top_line + rect.height as usize).min(rope.len_lines());
    let gutter_width = compute_gutter_width(window.gutter, last_visible_line);
    let text_x = rect.x + gutter_width;
    let text_width = rect.width.saturating_sub(gutter_width);

    for row_idx in 0..rect.height {
        let line_idx = window.scroll.top_line + row_idx as usize;
        if line_idx >= rope.len_lines() {
            break;
        }
        let y = rect.y + row_idx;
        if window.gutter.line_numbers && gutter_width > 0 {
            paint_gutter(grid, rect.x, y, gutter_width, line_idx + 1);
        }
        if text_width > 0 {
            paint_line(
                grid,
                &PaintLine {
                    buffer,
                    properties,
                    line_idx,
                    window,
                    text_x,
                    y,
                    text_width,
                },
            );
        }
    }

    // Selection highlight: paint the region between mark and cursor
    // with an inverted face so the user sees what's selected.
    if let Some(ref sel) = window.selection {
        paint_selection(window, sel, rect, gutter_width, text_width, grid);
    }

    // Primary cursor position — only for the active pane.
    if !is_active {
        return;
    }
    if let Some(cursor_render) = resolve_cursor(
        window.primary_cursor().byte_offset,
        window,
        rect,
        text_x,
        text_width,
    ) {
        // Flag the underlying cell too, so backends that want to paint a
        // block cursor purely through cell styling can do so.
        if let Some(cell) = grid.get_mut(cursor_render.col, cursor_render.row) {
            cell.flags |= CellFlags::CURSOR_PRIMARY;
        }
        cursors.push(cursor_render);
    }
}

/// Paint a line number at `(x, y)` right-justified to `width - 1` cells,
/// leaving the final column blank as padding between the gutter and the
/// text area.
/// Paint a selection highlight over the cells that fall within the
/// selection region. Dispatches to linear or rectangle painting.
fn paint_selection(
    window: &WindowState,
    sel: &crate::view_state::Selection,
    rect: Rect,
    gutter_width: u16,
    text_width: u16,
    grid: &mut CellGrid,
) {
    match sel {
        crate::view_state::Selection::Linear(range) => {
            paint_linear_selection(window, range, rect, gutter_width, text_width, grid);
        }
        crate::view_state::Selection::Rectangle {
            start_line,
            end_line,
            left_col,
            right_col,
        } => {
            paint_rect_selection(
                window,
                *start_line,
                *end_line,
                *left_col,
                *right_col,
                rect,
                gutter_width,
                text_width,
                grid,
            );
        }
    }
}

/// Paint a linear (contiguous) selection highlight.
fn paint_linear_selection(
    window: &WindowState,
    sel: &std::ops::Range<usize>,
    rect: Rect,
    gutter_width: u16,
    text_width: u16,
    grid: &mut CellGrid,
) {
    if sel.start == sel.end || rect.is_empty() || text_width == 0 {
        return;
    }
    let rope = window.buffer.rope();
    let sel_face = ResolvedFace {
        fg: Color::WHITE,
        bg: Color::rgb(0x26, 0x4F, 0x78),
        ..ResolvedFace::DEFAULT
    };
    let text_x = rect.x + gutter_width;

    for row_idx in 0..rect.height {
        let line_idx = window.scroll.top_line + row_idx as usize;
        if line_idx >= rope.len_lines() {
            break;
        }
        let line_start = rope.line_to_byte(line_idx);
        let line_end = if line_idx + 1 < rope.len_lines() {
            rope.line_to_byte(line_idx + 1)
        } else {
            rope.len_bytes()
        };
        if line_end <= sel.start || line_start >= sel.end {
            continue;
        }
        let sel_start_in_line = sel.start.max(line_start) - line_start;
        let sel_end_in_line = sel.end.min(line_end) - line_start;

        let line_text = rope.slice_to_string(line_start..line_end);
        let mut byte_in_line: usize = 0;
        let mut display_col: u16 = 0;
        for grapheme in line_text.grapheme_indices(true) {
            let (gi, g) = grapheme;
            let g_end = gi + g.len();
            let w = UnicodeWidthStr::width(g).clamp(1, 2) as u16;
            if gi >= sel_end_in_line {
                break;
            }
            if g_end > sel_start_in_line
                && gi < sel_end_in_line
                && display_col >= window.scroll.left_col
                && display_col - window.scroll.left_col < text_width
            {
                let screen_col = text_x + (display_col - window.scroll.left_col);
                let y = rect.y + row_idx;
                for dx in 0..w {
                    if let Some(cell) = grid.get_mut(screen_col + dx, y) {
                        cell.face = sel_face;
                    }
                }
            }
            display_col += w;
            byte_in_line = g_end;
        }
        let _ = byte_in_line;
    }
}

/// Paint a rectangular (column block) selection highlight.
#[allow(clippy::too_many_arguments)]
fn paint_rect_selection(
    window: &WindowState,
    start_line: usize,
    end_line: usize,
    left_col: u16,
    right_col: u16,
    rect: Rect,
    gutter_width: u16,
    text_width: u16,
    grid: &mut CellGrid,
) {
    if left_col == right_col || rect.is_empty() || text_width == 0 {
        return;
    }
    let rope = window.buffer.rope();
    let sel_face = ResolvedFace {
        fg: Color::WHITE,
        bg: Color::rgb(0x26, 0x4F, 0x78),
        ..ResolvedFace::DEFAULT
    };
    let text_x = rect.x + gutter_width;

    for row_idx in 0..rect.height {
        let line_idx = window.scroll.top_line + row_idx as usize;
        if line_idx >= rope.len_lines() {
            break;
        }
        if line_idx < start_line || line_idx > end_line {
            continue;
        }
        // Highlight the column range on this line.
        for col in left_col..right_col {
            if col >= window.scroll.left_col
                && col - window.scroll.left_col < text_width
            {
                let screen_col = text_x + (col - window.scroll.left_col);
                let y = rect.y + row_idx;
                if let Some(cell) = grid.get_mut(screen_col, y) {
                    cell.face = sel_face;
                }
            }
        }
    }
}

fn paint_gutter(grid: &mut CellGrid, x: u16, y: u16, width: u16, line_number: usize) {
    if width == 0 {
        return;
    }
    let label_width = (width - 1) as usize;
    let label = format!("{line_number:>label_width$}");
    let face = ResolvedFace::DEFAULT;
    let mut col = x;
    for ch in label.chars() {
        if col >= x + width - 1 {
            break;
        }
        grid.set(
            col,
            y,
            Cell {
                grapheme: CompactString::new(ch.encode_utf8(&mut [0; 4])),
                face,
                flags: CellFlags::empty(),
            },
        );
        col += 1;
    }
    // Explicit blank padding cell in the last gutter column.
    grid.set(
        x + width - 1,
        y,
        Cell {
            grapheme: CompactString::const_new(" "),
            face,
            flags: CellFlags::empty(),
        },
    );
}

/// Arguments for [`paint_line`], grouped so the function signature stays
/// under clippy's `too_many_arguments` threshold.
#[derive(Clone, Copy)]
struct PaintLine<'a> {
    buffer: &'a arx_buffer::BufferSnapshot,
    properties: &'a PropertyMap,
    line_idx: usize,
    window: &'a WindowState,
    text_x: u16,
    y: u16,
    text_width: u16,
}

/// Paint one buffer line of text into `grid`, starting at `text_x`, `y`.
///
/// Also overlays truncation indicators (`<` / `>`) on the first/last
/// visible columns when the line's content extends past the viewport
/// edges. Empty lines never get indicators — they'd be lying about
/// content that isn't there.
fn paint_line(grid: &mut CellGrid, args: &PaintLine<'_>) {
    let &PaintLine {
        buffer,
        properties,
        line_idx,
        window,
        text_x,
        y,
        text_width,
    } = args;
    let rope = buffer.rope();
    let line_start = rope.line_to_byte(line_idx);
    let line_end = if line_idx + 1 < rope.len_lines() {
        // Exclude the trailing newline.
        rope.line_to_byte(line_idx + 1).saturating_sub(1)
    } else {
        rope.len_bytes()
    };
    if line_start >= line_end {
        return;
    }
    let line_text = rope.slice_to_string(line_start..line_end);

    // Resolve styled runs once per line.
    let styled_runs = properties.styled_runs(line_start..line_end);

    // Track two cursors simultaneously:
    //   * `display_col` — column in the buffer's line (pre-clip).
    //   * `text_col`    — column inside the window's text area (post-clip).
    let mut display_col: u16 = 0;
    let mut text_col: u16 = 0;
    let mut truncated_right = false;

    for (byte_in_line, grapheme) in line_text.grapheme_indices(true) {
        let global_byte = line_start + byte_in_line;
        let width = UnicodeWidthStr::width(grapheme).clamp(1, 2) as u16;

        // Horizontal-scroll clipping: skip graphemes entirely left of the viewport.
        if display_col + width <= window.scroll.left_col {
            display_col += width;
            continue;
        }
        // Partial-clip case: a wide grapheme straddles the left edge.
        // Drop the whole grapheme and emit a blank in the first visible
        // column so the cursor math stays consistent.
        if display_col < window.scroll.left_col {
            display_col += width;
            if text_col < text_width {
                grid.set(text_x + text_col, y, Cell::blank());
                text_col += 1;
            }
            continue;
        }

        if text_col >= text_width {
            truncated_right = true;
            break;
        }

        let face = resolve_face(&styled_runs, global_byte);
        let flags_from_runs = flags_for_byte(&styled_runs, global_byte);

        let col_on_screen = text_x + text_col;
        if width == 2 && text_col + 2 > text_width {
            // Wide grapheme would overflow the window — replace with a
            // single blank and stop painting this line.
            grid.set(
                col_on_screen,
                y,
                Cell {
                    grapheme: CompactString::const_new(" "),
                    face,
                    flags: flags_from_runs,
                },
            );
            truncated_right = true;
            break;
        }

        grid.set(
            col_on_screen,
            y,
            Cell {
                grapheme: CompactString::new(grapheme),
                face,
                flags: flags_from_runs,
            },
        );
        if width == 2 {
            grid.set(col_on_screen + 1, y, Cell::wide_continuation(face));
        }
        text_col += width;
        display_col += width;
    }

    // Truncation indicators. An indicator is only worth drawing when
    // it actually has a cell to overwrite — i.e. the text area is at
    // least one column wide.
    if text_width == 0 {
        return;
    }
    let indicator_face = truncation_face();
    if window.scroll.left_col > 0 {
        // Left side: the line has content that's scrolled off. We only
        // know the line is non-empty because the guard above returned
        // for empty lines.
        grid.set(
            text_x,
            y,
            Cell {
                grapheme: CompactString::const_new("<"),
                face: indicator_face,
                flags: CellFlags::empty(),
            },
        );
    }
    if truncated_right {
        grid.set(
            text_x + text_width - 1,
            y,
            Cell {
                grapheme: CompactString::const_new(">"),
                face: indicator_face,
                flags: CellFlags::empty(),
            },
        );
    }
}

/// Face used for the `<` / `>` horizontal-truncation indicators. A
/// dim yellow on the default background distinguishes them from
/// regular text without being obnoxious.
fn truncation_face() -> ResolvedFace {
    ResolvedFace {
        fg: crate::face::Color::rgb(0xc0, 0xa0, 0x00),
        bg: ResolvedFace::DEFAULT.bg,
        bold: true,
        ..ResolvedFace::DEFAULT
    }
}

/// Look up the highest-priority face at `byte_offset` in the precomputed
/// styled runs. Assumes `runs` are sorted / contiguous (which
/// [`PropertyMap::styled_runs`] guarantees).
fn resolve_face(runs: &[StyledRun], byte_offset: usize) -> ResolvedFace {
    for run in runs {
        if run.range.start <= byte_offset && byte_offset < run.range.end {
            return ResolvedFace::resolve(ResolvedFace::DEFAULT, &sparse_from_run(run));
        }
    }
    ResolvedFace::DEFAULT
}

/// Convert a [`StyledRun`] back to a sparse buffer face. (A cleaner
/// factoring would have `PropertyMap::styled_runs` emit faces directly —
/// worth revisiting after we wire up tree-sitter.)
fn sparse_from_run(run: &StyledRun) -> arx_buffer::Face {
    run.face.clone()
}

/// Return the cell flags contributed by the property layers at `byte_offset`.
fn flags_for_byte(runs: &[StyledRun], byte_offset: usize) -> CellFlags {
    let mut flags = CellFlags::empty();
    for run in runs {
        if run.range.start <= byte_offset && byte_offset < run.range.end {
            if run.flags.contains(arx_buffer::PropertyFlags::SEARCH_MATCH) {
                flags |= CellFlags::SEARCH_MATCH;
            }
            if run.flags.contains(arx_buffer::PropertyFlags::DIAGNOSTIC) {
                flags |= CellFlags::DIAGNOSTIC_HINT;
            }
        }
    }
    flags
}

/// Convert a cursor's byte offset in the buffer to a `(col, row)` on the
/// terminal grid, or `None` if the cursor is scrolled out of view.
fn resolve_cursor(
    byte_offset: usize,
    window: &WindowState,
    rect: Rect,
    text_x: u16,
    text_width: u16,
) -> Option<CursorRender> {
    let rope = window.buffer.rope();
    let len = rope.len_bytes();
    let byte = byte_offset.min(len);
    let line = rope.byte_to_line(byte);
    if line < window.scroll.top_line {
        return None;
    }
    let row_offset = line - window.scroll.top_line;
    if row_offset >= rect.height as usize {
        return None;
    }
    let row = rect.y + row_offset as u16;

    // Column: count display widths from line start to cursor.
    let line_start = rope.line_to_byte(line);
    let text_before = rope.slice_to_string(line_start..byte);
    let mut col: u16 = 0;
    for g in text_before.graphemes(true) {
        col = col.saturating_add(UnicodeWidthStr::width(g).clamp(1, 2) as u16);
    }
    if col < window.scroll.left_col {
        return None;
    }
    let viewport_col = col - window.scroll.left_col;
    if viewport_col >= text_width {
        return None;
    }
    Some(CursorRender {
        col: text_x + viewport_col,
        row,
        style: CursorStyle::Block,
    })
}

/// Paint the command-palette overlay: one prompt row with the query,
/// plus up to `matches_rows` match rows above it. The highlighted row
/// uses an inverted face; the rest share the prompt's face. A primary
/// cursor is added at the end of the query so the cursor ends up
/// where the user's next keystroke will land.
fn paint_palette(
    layout: &PaletteLayout<'_>,
    cols: u16,
    grid: &mut CellGrid,
    cursors: &mut SmallVec<[CursorRender; 1]>,
) {
    let Some(prompt_row) = layout.prompt_row else {
        return;
    };
    let prompt_face = ResolvedFace {
        fg: Color::WHITE,
        bg: Color::rgb(0x20, 0x20, 0x30),
        ..ResolvedFace::DEFAULT
    };
    let selected_face = ResolvedFace {
        fg: Color::BLACK,
        bg: Color::rgb(0xd0, 0xd0, 0xe0),
        bold: true,
        ..ResolvedFace::DEFAULT
    };

    let cursor_col = paint_palette_prompt(layout.view, prompt_row, cols, prompt_face, grid);

    // A bar cursor lives at the end of the query. Replaces any window
    // cursor the render_window pass might have emitted, since the
    // palette owns focus while open.
    cursors.clear();
    cursors.push(CursorRender {
        col: cursor_col,
        row: prompt_row,
        style: CursorStyle::Bar,
    });

    if let Some(matches_top) = layout.matches_top {
        paint_palette_matches(
            layout.view,
            matches_top,
            layout.matches_rows,
            cols,
            prompt_face,
            selected_face,
            grid,
        );
    }
}

/// Paint the "M-x <query>" prompt line and return the column where a
/// follow-up cursor should sit (end of the visible query).
fn paint_palette_prompt(
    view: &PaletteView,
    row: u16,
    cols: u16,
    face: ResolvedFace,
    grid: &mut CellGrid,
) -> u16 {
    clear_row(grid, row, cols, face);
    let mut x: u16 = 0;
    for g in view.prompt.graphemes(true) {
        if x >= cols {
            break;
        }
        grid.set(
            x,
            row,
            Cell {
                grapheme: CompactString::new(g),
                face,
                flags: CellFlags::empty(),
            },
        );
        x = x.saturating_add(UnicodeWidthStr::width(g).clamp(1, 2) as u16);
    }
    for g in view.query.graphemes(true) {
        if x >= cols {
            break;
        }
        let w = UnicodeWidthStr::width(g).clamp(1, 2) as u16;
        grid.set(
            x,
            row,
            Cell {
                grapheme: CompactString::new(g),
                face,
                flags: CellFlags::empty(),
            },
        );
        if w == 2 && x + 1 < cols {
            grid.set(x + 1, row, Cell::wide_continuation(face));
        }
        x = x.saturating_add(w);
    }
    x.min(cols.saturating_sub(1))
}

/// Paint the match list beneath the prompt. Scrolls to keep the
/// selected row visible; highlights it with `selected_face`.
fn paint_palette_matches(
    view: &PaletteView,
    matches_top: u16,
    matches_rows: u16,
    cols: u16,
    face: ResolvedFace,
    selected_face: ResolvedFace,
    grid: &mut CellGrid,
) {
    if matches_rows == 0 {
        return;
    }
    // Scroll the visible window of matches so the selection is
    // centred (with saturation at both edges).
    let total = view.matches.len();
    let selected = view.selected.min(total.saturating_sub(1));
    let rows_cap = matches_rows as usize;
    let scroll_top = if total <= rows_cap || selected < rows_cap / 2 {
        0
    } else if selected + rows_cap / 2 >= total {
        total - rows_cap
    } else {
        selected - rows_cap / 2
    };

    for row_idx in 0..matches_rows {
        let y = matches_top + row_idx;
        let match_idx = scroll_top + row_idx as usize;
        let Some(entry) = view.matches.get(match_idx) else {
            clear_row(grid, y, cols, face);
            continue;
        };
        let row_face = if match_idx == selected {
            selected_face
        } else {
            face
        };
        clear_row(grid, y, cols, row_face);
        paint_palette_match_row(entry, y, cols, row_face, grid);
    }
}

/// Paint `"  name  — description"` into a single match row. Handles
/// width 2 graphemes and description truncation at `cols`.
fn paint_palette_match_row(
    entry: &crate::view_state::PaletteEntry,
    y: u16,
    cols: u16,
    face: ResolvedFace,
    grid: &mut CellGrid,
) {
    let mut col: u16 = 2;
    col = paint_palette_text(&entry.name, col, y, cols, face, grid);
    if !entry.description.is_empty() && col + 3 < cols {
        col = paint_palette_text("  — ", col, y, cols, face, grid);
        paint_palette_text(&entry.description, col, y, cols, face, grid);
    }
}

/// Paint `text` starting at `(col, y)` and return the new cursor
/// column. Stops at `cols` (no wrapping).
fn paint_palette_text(
    text: &str,
    start_col: u16,
    y: u16,
    cols: u16,
    face: ResolvedFace,
    grid: &mut CellGrid,
) -> u16 {
    let mut col = start_col;
    for g in text.graphemes(true) {
        if col >= cols {
            break;
        }
        let w = UnicodeWidthStr::width(g).clamp(1, 2) as u16;
        grid.set(
            col,
            y,
            Cell {
                grapheme: CompactString::new(g),
                face,
                flags: CellFlags::empty(),
            },
        );
        if w == 2 && col + 1 < cols {
            grid.set(col + 1, y, Cell::wide_continuation(face));
        }
        col = col.saturating_add(w);
    }
    col
}

// ---------------------------------------------------------------------------
// Search overlay
// ---------------------------------------------------------------------------

/// Pre-computed search overlay geometry for the current frame.
struct SearchLayout<'a> {
    view: &'a crate::view_state::SearchView,
    prompt_row: Option<u16>,
    matches_top: Option<u16>,
    matches_rows: u16,
}

fn paint_search(
    layout: &SearchLayout<'_>,
    cols: u16,
    grid: &mut CellGrid,
    cursors: &mut SmallVec<[CursorRender; 1]>,
) {
    let Some(prompt_row) = layout.prompt_row else {
        return;
    };
    let prompt_face = ResolvedFace {
        fg: Color::WHITE,
        bg: Color::rgb(0x1a, 0x20, 0x30),
        ..ResolvedFace::DEFAULT
    };
    let selected_face = ResolvedFace {
        fg: Color::BLACK,
        bg: Color::rgb(0xd0, 0xd0, 0xe0),
        bold: true,
        ..ResolvedFace::DEFAULT
    };

    // Paint the prompt row: "Search (fuzzy): <query>  [N matches]"
    clear_row(grid, prompt_row, cols, prompt_face);
    let mut x: u16 = 0;
    // Paint prompt text.
    x = paint_palette_text(&layout.view.prompt, x, prompt_row, cols, prompt_face, grid);
    // Paint query text.
    let cursor_col = {
        let before = x;
        for g in layout.view.query.graphemes(true) {
            if x >= cols {
                break;
            }
            let w = UnicodeWidthStr::width(g).clamp(1, 2) as u16;
            grid.set(
                x,
                prompt_row,
                Cell {
                    grapheme: CompactString::new(g),
                    face: prompt_face,
                    flags: CellFlags::empty(),
                },
            );
            if w == 2 && x + 1 < cols {
                grid.set(x + 1, prompt_row, Cell::wide_continuation(prompt_face));
            }
            x = x.saturating_add(w);
        }
        if x == before { before } else { x }
    };
    // Paint match count on the right side.
    let count_str = format!("  [{} matches]", layout.view.total_matches);
    if x + count_str.len() as u16 + 2 < cols {
        let count_face = ResolvedFace {
            fg: Color::rgb(0x80, 0x80, 0x90),
            ..prompt_face
        };
        paint_palette_text(&count_str, x, prompt_row, cols, count_face, grid);
    }

    // Cursor at the end of the query.
    cursors.clear();
    cursors.push(CursorRender {
        col: cursor_col.min(cols.saturating_sub(1)),
        row: prompt_row,
        style: CursorStyle::Bar,
    });

    // Paint match rows.
    if let Some(matches_top) = layout.matches_top {
        paint_search_matches(
            layout.view,
            matches_top,
            layout.matches_rows,
            cols,
            prompt_face,
            selected_face,
            grid,
        );
    }
}

fn paint_search_matches(
    view: &crate::view_state::SearchView,
    matches_top: u16,
    matches_rows: u16,
    cols: u16,
    face: ResolvedFace,
    selected_face: ResolvedFace,
    grid: &mut CellGrid,
) {
    if matches_rows == 0 {
        return;
    }
    let total = view.matches.len();
    let selected = view.selected.min(total.saturating_sub(1));
    let rows_cap = matches_rows as usize;
    let scroll_top = if total <= rows_cap || selected < rows_cap / 2 {
        0
    } else if selected + rows_cap / 2 >= total {
        total - rows_cap
    } else {
        selected - rows_cap / 2
    };

    let line_num_face = ResolvedFace {
        fg: Color::rgb(0x80, 0x80, 0x90),
        ..face
    };

    for row_idx in 0..matches_rows {
        let y = matches_top + row_idx;
        let match_idx = scroll_top + row_idx as usize;
        let Some(entry) = view.matches.get(match_idx) else {
            clear_row(grid, y, cols, face);
            continue;
        };
        let row_face = if match_idx == selected {
            selected_face
        } else {
            face
        };
        let num_face = if match_idx == selected {
            selected_face
        } else {
            line_num_face
        };
        clear_row(grid, y, cols, row_face);
        // Paint "  NNN: line text"
        let num_str = format!("{:>5}: ", entry.line_number + 1);
        let col = paint_palette_text(&num_str, 0, y, cols, num_face, grid);
        paint_palette_text(&entry.line_text, col, y, cols, row_face, grid);
    }
}

/// Paint the KEDIT persistent command line at `row`. Renders the
/// prompt glyph, then the query (or the transient message when the
/// cmd line isn't focused), and places a bar cursor at the end of
/// the prompt when focused.
fn paint_kedit_line(
    view: &crate::view_state::KeditLineView,
    row: u16,
    cols: u16,
    grid: &mut CellGrid,
    cursors: &mut SmallVec<[CursorRender; 1]>,
) {
    // Two faces: focused (brighter prompt background) vs blurred
    // (dimmer) so the user can tell at a glance where keystrokes go.
    let face = if view.focused {
        ResolvedFace {
            fg: Color::WHITE,
            bg: Color::rgb(0x20, 0x30, 0x20),
            ..ResolvedFace::DEFAULT
        }
    } else {
        ResolvedFace {
            fg: Color::rgb(0xc0, 0xc0, 0xc0),
            bg: Color::rgb(0x18, 0x20, 0x18),
            ..ResolvedFace::DEFAULT
        }
    };
    clear_row(grid, row, cols, face);
    let mut x: u16 = 0;
    x = paint_palette_text(&view.prompt, x, row, cols, face, grid);
    if view.focused {
        // Paint the query and put a bar cursor at `view.cursor`.
        let mut cursor_col = x;
        let mut byte: usize = 0;
        for g in view.query.graphemes(true) {
            if x >= cols {
                break;
            }
            let w = UnicodeWidthStr::width(g).clamp(1, 2) as u16;
            grid.set(
                x,
                row,
                Cell {
                    grapheme: CompactString::new(g),
                    face,
                    flags: CellFlags::empty(),
                },
            );
            if w == 2 && x + 1 < cols {
                grid.set(x + 1, row, Cell::wide_continuation(face));
            }
            x = x.saturating_add(w);
            byte += g.len();
            if byte <= view.cursor {
                cursor_col = x;
            }
        }
        cursors.clear();
        cursors.push(CursorRender {
            col: cursor_col.min(cols.saturating_sub(1)),
            row,
            style: CursorStyle::Bar,
        });
    } else if let Some(msg) = view.message.as_deref() {
        let msg_face = ResolvedFace {
            fg: Color::rgb(0xff, 0xd0, 0x80),
            ..face
        };
        paint_palette_text(msg, x, row, cols, msg_face, grid);
    } else {
        paint_palette_text(&view.query, x, row, cols, face, grid);
    }
}

/// Fill every cell in `row` with a space of `face`.
fn clear_row(grid: &mut CellGrid, row: u16, cols: u16, face: ResolvedFace) {
    for x in 0..cols {
        grid.set(
            x,
            row,
            Cell {
                grapheme: CompactString::const_new(" "),
                face,
                flags: CellFlags::empty(),
            },
        );
    }
}

/// Paint the bottom modeline. Left-aligned text, right-aligned right text,
/// padded to the full width with the default face.
fn render_modeline(global: &crate::view_state::GlobalState, row: u16, cols: u16, grid: &mut CellGrid) {
    let face = ResolvedFace {
        fg: crate::face::Color::BLACK,
        bg: crate::face::Color(0xc0_c0_c0),
        ..ResolvedFace::DEFAULT
    };
    // Clear the row with the modeline background.
    for x in 0..cols {
        grid.set(
            x,
            row,
            Cell {
                grapheme: CompactString::const_new(" "),
                face,
                flags: CellFlags::empty(),
            },
        );
    }
    // Left text.
    let mut x: u16 = 0;
    for g in global.modeline_left.graphemes(true) {
        let w = UnicodeWidthStr::width(g).clamp(1, 2) as u16;
        if x + w > cols {
            break;
        }
        grid.set(
            x,
            row,
            Cell {
                grapheme: CompactString::new(g),
                face,
                flags: CellFlags::empty(),
            },
        );
        if w == 2 {
            grid.set(x + 1, row, Cell::wide_continuation(face));
        }
        x += w;
    }
    // Right text (right-aligned).
    let right_width = UnicodeWidthStr::width(global.modeline_right.as_str()) as u16;
    if right_width >= cols {
        return;
    }
    let mut rx = cols - right_width;
    // Leave a one-cell padding between left and right text when they would
    // otherwise overlap.
    if rx < x + 1 {
        return;
    }
    for g in global.modeline_right.graphemes(true) {
        let w = UnicodeWidthStr::width(g).clamp(1, 2) as u16;
        if rx + w > cols {
            break;
        }
        grid.set(
            rx,
            row,
            Cell {
                grapheme: CompactString::new(g),
                face,
                flags: CellFlags::empty(),
            },
        );
        if w == 2 {
            grid.set(rx + 1, row, Cell::wide_continuation(face));
        }
        rx += w;
    }
}

/// Compute the width of the gutter for a window. Ensures the largest
/// visible line number fits in the gutter (plus one cell of padding).
#[allow(clippy::too_many_lines)]
/// Paint a completion popup near the cursor. The popup is a small
/// floating box showing completion items with the selected one
/// highlighted. Positioned just below the anchor row, clamped to
/// fit within the terminal.
fn paint_completion(
    view: &crate::view_state::CompletionView,
    cols: u16,
    max_height: u16,
    grid: &mut CellGrid,
) {
    if view.items.is_empty() {
        return;
    }
    let visible_rows = view.max_rows.min(view.items.len() as u16).min(max_height);
    if visible_rows == 0 {
        return;
    }
    // Position: just below the anchor row, at the anchor column.
    let start_row = (view.anchor_row + 1).min(max_height.saturating_sub(visible_rows));
    let start_col = view.anchor_col.min(cols.saturating_sub(1));

    // Compute popup width: max label length + kind + padding, capped.
    let max_label: u16 = view
        .items
        .iter()
        .map(|e| e.label.len() as u16 + if e.kind.is_empty() { 0 } else { e.kind.len() as u16 + 1 })
        .max()
        .unwrap_or(10)
        .clamp(10, 40);
    let popup_width = (max_label + 4).min(cols - start_col);

    let normal_face = ResolvedFace {
        fg: Color::rgb(0xD0, 0xD0, 0xD0),
        bg: Color::rgb(0x2C, 0x2C, 0x3C),
        ..ResolvedFace::DEFAULT
    };
    let selected_face = ResolvedFace {
        fg: Color::BLACK,
        bg: Color::rgb(0x61, 0xAF, 0xEF),
        bold: true,
        ..ResolvedFace::DEFAULT
    };
    let kind_face = ResolvedFace {
        fg: Color::rgb(0xE5, 0xC0, 0x7B),
        bg: normal_face.bg,
        ..ResolvedFace::DEFAULT
    };
    let kind_selected_face = ResolvedFace {
        fg: Color::rgb(0x30, 0x30, 0x30),
        bg: selected_face.bg,
        ..ResolvedFace::DEFAULT
    };

    // Scroll to keep selection visible.
    let scroll_top = if view.selected < visible_rows as usize {
        0
    } else {
        view.selected - visible_rows as usize + 1
    };

    for row_idx in 0..visible_rows {
        let y = start_row + row_idx;
        if y >= max_height {
            break;
        }
        let item_idx = scroll_top + row_idx as usize;
        let Some(entry) = view.items.get(item_idx) else {
            continue;
        };
        let is_sel = item_idx == view.selected;
        let face = if is_sel { selected_face } else { normal_face };
        let kf = if is_sel { kind_selected_face } else { kind_face };

        // Clear the row.
        for dx in 0..popup_width {
            grid.set(
                start_col + dx,
                y,
                Cell {
                    grapheme: CompactString::const_new(" "),
                    face,
                    flags: CellFlags::empty(),
                },
            );
        }
        // Paint kind indicator.
        let mut x = start_col + 1;
        if !entry.kind.is_empty() {
            for ch in entry.kind.chars() {
                if x >= start_col + popup_width {
                    break;
                }
                grid.set(
                    x,
                    y,
                    Cell {
                        grapheme: CompactString::new(ch.encode_utf8(&mut [0; 4])),
                        face: kf,
                        flags: CellFlags::empty(),
                    },
                );
                x += 1;
            }
            x += 1; // space after kind
        }
        // Paint label.
        for ch in entry.label.chars() {
            if x >= start_col + popup_width - 1 {
                break;
            }
            grid.set(
                x,
                y,
                Cell {
                    grapheme: CompactString::new(ch.encode_utf8(&mut [0; 4])),
                    face,
                    flags: CellFlags::empty(),
                },
            );
            x += 1;
        }
    }
}

/// Paint the which-key overlay: a horizontal bar at the bottom of
/// the text area showing available completions for the pending
/// prefix chord. Each entry is `key → command` in a compact layout.
fn paint_which_key(
    entries: &[crate::view_state::WhichKeyEntry],
    cols: u16,
    text_rows: u16,
    grid: &mut CellGrid,
) {
    if entries.is_empty() || text_rows == 0 {
        return;
    }
    let face = ResolvedFace {
        fg: Color::rgb(0xD0, 0xD0, 0xD0),
        bg: Color::rgb(0x1E, 0x1E, 0x2E),
        ..ResolvedFace::DEFAULT
    };
    let key_face = ResolvedFace {
        fg: Color::rgb(0x61, 0xAF, 0xEF),
        bg: face.bg,
        bold: true,
        ..ResolvedFace::DEFAULT
    };

    // Paint onto the last row of the text area (just above modeline).
    let y = text_rows - 1;
    // Clear the row.
    for x in 0..cols {
        grid.set(
            x,
            y,
            Cell {
                grapheme: CompactString::const_new(" "),
                face,
                flags: CellFlags::empty(),
            },
        );
    }
    // Layout entries as "key→cmd  key→cmd  ..." with 2-space gaps.
    let mut x: u16 = 1;
    for entry in entries {
        if x >= cols.saturating_sub(4) {
            break;
        }
        // Paint key in blue bold.
        for ch in entry.key.chars() {
            if x >= cols {
                break;
            }
            grid.set(
                x,
                y,
                Cell {
                    grapheme: CompactString::new(ch.encode_utf8(&mut [0; 4])),
                    face: key_face,
                    flags: CellFlags::empty(),
                },
            );
            x += 1;
        }
        // Arrow separator.
        if x + 1 < cols {
            grid.set(
                x,
                y,
                Cell {
                    grapheme: CompactString::const_new("\u{2192}"),
                    face,
                    flags: CellFlags::empty(),
                },
            );
            x += 1;
        }
        // Command name (truncated).
        let cmd_display = if entry.command.len() > 16 {
            &entry.command[..16]
        } else {
            &entry.command
        };
        for ch in cmd_display.chars() {
            if x >= cols {
                break;
            }
            grid.set(
                x,
                y,
                Cell {
                    grapheme: CompactString::new(ch.encode_utf8(&mut [0; 4])),
                    face,
                    flags: CellFlags::empty(),
                },
            );
            x += 1;
        }
        // 2-space gap between entries.
        x += 2;
    }
}

fn compute_gutter_width(config: GutterConfig, last_line: usize) -> u16 {
    if !config.line_numbers {
        return 0;
    }
    let digits = digit_count(last_line.max(1));
    (digits.max(config.min_width as usize) as u16) + 1 // +1 for padding column
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
    use arx_buffer::{Buffer, BufferId};
    use smallvec::smallvec as sv;

    use crate::view_state::{Cursor, GlobalState, LayoutTree, ScrollPosition};

    fn window_for(text: &str) -> WindowState {
        let buf = Buffer::from_str(BufferId(1), text);
        WindowState {
            id: crate::view_state::WindowId(1),
            buffer: buf.snapshot(),
            cursors: sv![Cursor::at(0)],
            scroll: ScrollPosition::default(),
            gutter: GutterConfig::default(),
            selection: None,
        }
    }

    fn state_for(window: WindowState, cols: u16, rows: u16) -> ViewState {
        let id = window.id;
        ViewState {
            size: TerminalSize::new(cols, rows),
            layout: LayoutTree::Single(id),
            windows: vec![window],
            terminal_panes: vec![],
            active_window: Some(id),
            global: GlobalState {
                modeline_left: String::new(),
                modeline_right: String::new(),
                palette: None,
                completion: None,
                which_key: None,
                search: None,
                kedit_line: None,
            },
        }
    }

    #[test]
    fn single_line_renders_with_gutter_and_modeline() {
        let w = window_for("hello");
        let state = state_for(w, 20, 3);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 3);
        // Gutter ("   1 ") + "hello" + padding.
        assert!(lines[0].trim_start().starts_with("1 hello"), "{:?}", lines[0]);
        // Second row is empty (only one line in the buffer).
        assert_eq!(lines[1].trim(), "");
        // Modeline row is all spaces in this state (no text).
        assert_eq!(lines[2].trim(), "");
    }

    #[test]
    fn multi_line_buffer_paints_each_row() {
        let w = window_for("alpha\nbeta\ngamma");
        let state = state_for(w, 20, 5);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert!(lines[0].contains("alpha"));
        assert!(lines[1].contains("beta"));
        assert!(lines[2].contains("gamma"));
    }

    #[test]
    fn cursor_lands_on_text_column() {
        let mut w = window_for("hello");
        w.cursors = sv![Cursor::at(3)]; // after "hel"
        let state = state_for(w, 20, 3);
        let tree = render(&state, 0);
        assert_eq!(tree.cursors.len(), 1);
        // Gutter "   1 " = 5 cells (min_width 4 + 1 padding) + "hel" = col 8.
        // But we render line number right-aligned to min_width so the
        // first digit sits at col 3 -> "    1 " = 6 cells. Actually with
        // min_width 4 the label is "   1" (4 cells) + 1 pad = 5, then
        // "hel" puts the cursor at col 5 + 3 = 8.
        assert_eq!(tree.cursors[0].col, 8);
        assert_eq!(tree.cursors[0].row, 0);
    }

    #[test]
    fn wide_grapheme_occupies_two_cells() {
        let w = window_for("中文");
        let state = state_for(w, 20, 2);
        let tree = render(&state, 0);
        // Find the first non-blank cell after the gutter padding.
        let row0: Vec<&Cell> = (0..tree.cells.width())
            .map(|x| tree.cells.get(x, 0).unwrap())
            .collect();
        let first_wide = row0
            .iter()
            .position(|c| c.grapheme.as_str() == "中")
            .unwrap();
        // The cell immediately after a wide grapheme must be a
        // WIDE_CONTINUATION.
        assert!(
            row0[first_wide + 1]
                .flags
                .contains(CellFlags::WIDE_CONTINUATION)
        );
        assert_eq!(row0[first_wide + 2].grapheme.as_str(), "文");
        assert!(
            row0[first_wide + 3]
                .flags
                .contains(CellFlags::WIDE_CONTINUATION)
        );
    }

    #[test]
    fn emoji_grapheme_cluster_is_a_single_cell_group() {
        // Family emoji is a single grapheme cluster composed of multiple
        // scalar values joined by ZWJ. It has width 2.
        let w = window_for("a👨‍👩‍👧‍👦b");
        let state = state_for(w, 30, 2);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        assert!(
            text.lines().next().unwrap().contains("a👨‍👩‍👧‍👦b"),
            "{text:?}"
        );
    }

    #[test]
    fn scroll_top_line_skips_rows() {
        let mut w = window_for("a\nb\nc\nd\ne");
        w.scroll.top_line = 2;
        let state = state_for(w, 20, 4);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert!(lines[0].contains('c'));
        assert!(lines[1].contains('d'));
        assert!(lines[2].contains('e'));
    }

    #[test]
    fn modeline_text_is_left_and_right_aligned() {
        let w = window_for("x");
        let mut state = state_for(w, 20, 3);
        state.global.modeline_left = "LEFT".into();
        state.global.modeline_right = "RIGHT".into();
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let modeline = text.split('\n').nth(2).unwrap();
        assert!(modeline.starts_with("LEFT"), "{modeline:?}");
        assert!(modeline.ends_with("RIGHT"), "{modeline:?}");
    }

    #[test]
    fn cursor_out_of_view_produces_no_entry() {
        let mut w = window_for("a\nb\nc\nd");
        w.cursors = sv![Cursor::at(0)]; // line 0
        w.scroll.top_line = 2; // view starts at line 2
        let state = state_for(w, 20, 3);
        let tree = render(&state, 0);
        assert!(tree.cursors.is_empty());
    }

    #[test]
    fn right_truncation_marker_appears_on_overflow_line() {
        // A line that's clearly wider than the visible text area gets a
        // `>` in the final visible column.
        let long = "a".repeat(200);
        let w = window_for(&long);
        let state = state_for(w, 20, 3);
        let tree = render(&state, 0);
        let row0: Vec<&Cell> = (0..tree.cells.width())
            .map(|x| tree.cells.get(x, 0).unwrap())
            .collect();
        // The last cell on row 0 must be the `>` indicator.
        assert_eq!(
            row0.last().unwrap().grapheme.as_str(),
            ">",
            "row0 = {:?}",
            row0.iter().map(|c| c.grapheme.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn left_truncation_marker_appears_when_scrolled_horizontally() {
        // With scroll.left_col > 0 the first visible column of the
        // text area (right after the gutter) becomes a `<` indicator.
        let long = "abcdefghij".repeat(20);
        let mut w = window_for(&long);
        w.scroll.left_col = 10;
        let state = state_for(w, 20, 3);
        let tree = render(&state, 0);
        let row0: Vec<&str> = (0..tree.cells.width())
            .map(|x| tree.cells.get(x, 0).unwrap().grapheme.as_str())
            .collect();
        // The gutter is right-aligned digits padded with spaces; the
        // `<` sits at the first non-gutter column. Look for it
        // directly — its mere presence on the row proves the marker
        // got painted.
        assert!(
            row0.contains(&"<"),
            "no left indicator on row0: {row0:?}",
        );
        // And it should appear BEFORE any of the visible text, i.e.
        // no letters before the `<`.
        let lt_pos = row0.iter().position(|c| *c == "<").unwrap();
        let has_letter_before = row0[..lt_pos]
            .iter()
            .any(|c| c.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic()));
        assert!(!has_letter_before, "letters before `<`: {row0:?}");
    }

    #[test]
    fn short_line_inside_viewport_gets_no_truncation_markers() {
        let w = window_for("hi");
        let state = state_for(w, 20, 3);
        let tree = render(&state, 0);
        let row0: Vec<&Cell> = (0..tree.cells.width())
            .map(|x| tree.cells.get(x, 0).unwrap())
            .collect();
        // Neither marker should appear anywhere on row 0.
        assert!(row0.iter().all(|c| c.grapheme.as_str() != "<"));
        assert!(row0.iter().all(|c| c.grapheme.as_str() != ">"));
    }

    // ---- Command palette overlay ----

    fn state_with_palette(window: WindowState, cols: u16, rows: u16, palette: PaletteView) -> ViewState {
        let id = window.id;
        ViewState {
            size: TerminalSize::new(cols, rows),
            layout: LayoutTree::Single(id),
            windows: vec![window],
            terminal_panes: vec![],
            active_window: Some(id),
            global: GlobalState {
                modeline_left: String::new(),
                modeline_right: String::new(),
                palette: Some(palette),
                completion: None,
                which_key: None,
                search: None,
                kedit_line: None,
            },
        }
    }

    fn entry(name: &str, desc: &str) -> crate::view_state::PaletteEntry {
        crate::view_state::PaletteEntry {
            name: name.to_owned(),
            description: desc.to_owned(),
        }
    }

    #[test]
    fn palette_overlay_paints_prompt_and_matches() {
        let w = window_for("hello");
        let palette = PaletteView {
            prompt: "M-x ".to_owned(),
            query: "cur".to_owned(),
            matches: vec![
                entry("cursor.left", "Move left"),
                entry("cursor.right", "Move right"),
            ],
            selected: 1,
            max_rows: 2,
        };
        // 30 cols × 10 rows: plenty of space. Layout from the bottom
        // up: modeline (row 9), match row 1 (row 8), match row 0
        // (row 7), prompt (row 6). Text area = rows 0..=5.
        let state = state_with_palette(w, 30, 10, palette);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 10);

        // Prompt row is max_rows (2) above the modeline, so row 6.
        let prompt = lines[6];
        assert!(prompt.starts_with("M-x cur"), "prompt = {prompt:?}");

        // Rows 7, 8 are the two match rows in order.
        assert!(lines[7].contains("cursor.left"), "row 7: {:?}", lines[7]);
        assert!(lines[8].contains("cursor.right"), "row 8: {:?}", lines[8]);

        // A bar-style cursor sits at the end of the query on the
        // prompt row.
        assert_eq!(tree.cursors.len(), 1);
        assert_eq!(tree.cursors[0].style, CursorStyle::Bar);
        // "M-x " is 4 cells; "cur" is 3 more → cursor at col 7.
        assert_eq!(tree.cursors[0].col, 7);
        assert_eq!(tree.cursors[0].row, 6);
    }

    #[test]
    fn palette_overlay_shrinks_text_area() {
        // A tall buffer should only paint as many rows as the text
        // area minus the overlay.
        let w = window_for("l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10");
        let palette = PaletteView {
            prompt: "M-x ".to_owned(),
            query: String::new(),
            matches: vec![entry("a", ""), entry("b", ""), entry("c", "")],
            selected: 0,
            max_rows: 3,
        };
        // 10 rows, minus 1 modeline minus 3 matches minus 1 prompt
        // = 5 rows of text area.
        let state = state_with_palette(w, 20, 10, palette);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();

        // Lines 0..5 should contain l1..l5. Line 6+ is palette/modeline.
        for (i, label) in (0..5).zip(["l1", "l2", "l3", "l4", "l5"]) {
            assert!(
                lines[i].contains(label),
                "row {i} should contain {label:?}, got {:?}",
                lines[i]
            );
        }
        // Line 5 is the start of the palette area — it should NOT
        // contain l6 (the 6th buffer line).
        assert!(!lines[5].contains("l6"), "row 5: {:?}", lines[5]);
    }

    #[test]
    fn palette_overlay_hides_window_cursor() {
        // Even with the primary window carrying a cursor, the palette
        // owns focus and the only emitted cursor should be its bar.
        let mut w = window_for("hello");
        w.cursors = sv![Cursor::at(2)];
        let palette = PaletteView {
            prompt: "M-x ".to_owned(),
            query: String::new(),
            matches: vec![entry("cursor.left", "")],
            selected: 0,
            max_rows: 1,
        };
        let state = state_with_palette(w, 30, 10, palette);
        let tree = render(&state, 0);
        assert_eq!(tree.cursors.len(), 1);
        assert_eq!(tree.cursors[0].style, CursorStyle::Bar);
    }

    // ---- Split layouts ----

    fn window_with_id(id: u64, text: &str) -> WindowState {
        let buf = Buffer::from_str(BufferId(id), text);
        WindowState {
            id: crate::view_state::WindowId(id),
            buffer: buf.snapshot(),
            cursors: sv![Cursor::at(0)],
            scroll: ScrollPosition::default(),
            gutter: GutterConfig::default(),
            selection: None,
        }
    }

    #[test]
    fn vertical_split_paints_both_panes_and_a_divider() {
        let left = window_with_id(1, "L1\nL2");
        let right = window_with_id(2, "R1\nR2");
        let state = ViewState {
            size: TerminalSize::new(21, 4),
            layout: LayoutTree::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutTree::Single(left.id)),
                second: Box::new(LayoutTree::Single(right.id)),
            },
            windows: vec![left, right],
            terminal_panes: vec![],
            active_window: Some(crate::view_state::WindowId(1)),
            global: GlobalState::default(),
        };
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        // Row 0 must contain text from both panes.
        assert!(lines[0].contains("L1"), "left pane missing: {:?}", lines[0]);
        assert!(lines[0].contains("R1"), "right pane missing: {:?}", lines[0]);
        // Divider column runs vertically through rows 0..text_rows.
        // The divider char is U+2502 (│).
        let divider_col = lines[0]
            .chars()
            .position(|c| c == '\u{2502}')
            .expect("divider glyph not painted");
        // The divider should also appear on row 1.
        let row1 = lines[1];
        let row1_divider = row1.chars().position(|c| c == '\u{2502}').unwrap();
        assert_eq!(divider_col, row1_divider);
    }

    #[test]
    fn horizontal_split_paints_both_panes_stacked() {
        let top = window_with_id(1, "topline");
        let bot = window_with_id(2, "botline");
        let state = ViewState {
            size: TerminalSize::new(20, 7),
            layout: LayoutTree::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutTree::Single(top.id)),
                second: Box::new(LayoutTree::Single(bot.id)),
            },
            windows: vec![top, bot],
            terminal_panes: vec![],
            active_window: Some(crate::view_state::WindowId(2)),
            global: GlobalState::default(),
        };
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        // Top pane lives on row 0 (inside its rect).
        assert!(lines[0].contains("topline"), "top row: {:?}", lines[0]);
        // Bottom pane lives somewhere on rows >= top-half + 1.
        let bot_row = lines
            .iter()
            .position(|l| l.contains("botline"))
            .expect("botline not painted");
        assert!(bot_row > 0);
    }

    // ---- KEDIT command line ----

    fn state_with_kedit(window: WindowState, cols: u16, rows: u16, view: crate::view_state::KeditLineView) -> ViewState {
        let id = window.id;
        ViewState {
            size: TerminalSize::new(cols, rows),
            layout: LayoutTree::Single(id),
            windows: vec![window],
            terminal_panes: vec![],
            active_window: Some(id),
            global: GlobalState {
                modeline_left: String::new(),
                modeline_right: String::new(),
                palette: None,
                completion: None,
                which_key: None,
                search: None,
                kedit_line: Some(view),
            },
        }
    }

    #[test]
    fn kedit_line_paints_prompt_above_modeline() {
        let w = window_for("hello");
        let view = crate::view_state::KeditLineView {
            prompt: "====> ".into(),
            query: "QUIT".into(),
            cursor: 4,
            focused: true,
            message: None,
        };
        // 20 cols × 5 rows: text rows are 0..3, kedit at row 3, modeline at row 4.
        let state = state_with_kedit(w, 20, 5, view);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 5);
        // Kedit row contains prompt + query.
        assert!(lines[3].starts_with("====> QUIT"), "kedit row: {:?}", lines[3]);
        // Cursor must live on the kedit row because it's focused.
        assert_eq!(tree.cursors.len(), 1);
        assert_eq!(tree.cursors[0].row, 3);
    }

    #[test]
    fn kedit_line_shrinks_text_area() {
        // A tall buffer should only paint as many rows as the text area minus the kedit row.
        let w = window_for("l1\nl2\nl3\nl4\nl5");
        let view = crate::view_state::KeditLineView {
            prompt: "====> ".into(),
            query: String::new(),
            cursor: 0,
            focused: false,
            message: None,
        };
        // 20 cols × 5 rows: modeline=row4, kedit=row3, text rows 0..=2.
        let state = state_with_kedit(w, 20, 5, view);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        // Row 3 is the kedit prompt, not buffer content.
        assert!(!lines[3].contains("l4"), "row 3 should be kedit, got {:?}", lines[3]);
        // And buffer rows should contain the first three lines.
        assert!(lines[0].contains("l1"));
        assert!(lines[1].contains("l2"));
        assert!(lines[2].contains("l3"));
    }

    #[test]
    fn kedit_line_paints_message_when_not_focused() {
        let w = window_for("hello");
        let view = crate::view_state::KeditLineView {
            prompt: "====> ".into(),
            query: String::new(),
            cursor: 0,
            focused: false,
            message: Some("Saved".into()),
        };
        let state = state_with_kedit(w, 20, 5, view);
        let tree = render(&state, 0);
        let text = tree.cells.to_debug_text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert!(lines[3].contains("Saved"), "kedit row: {:?}", lines[3]);
    }

    #[test]
    fn inactive_pane_does_not_emit_a_cursor() {
        let mut a = window_with_id(1, "aaa");
        a.cursors = sv![Cursor::at(1)];
        let mut b = window_with_id(2, "bbb");
        b.cursors = sv![Cursor::at(2)];
        let state = ViewState {
            size: TerminalSize::new(21, 4),
            layout: LayoutTree::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutTree::Single(a.id)),
                second: Box::new(LayoutTree::Single(b.id)),
            },
            windows: vec![a, b],
            terminal_panes: vec![],
            active_window: Some(crate::view_state::WindowId(2)),
            global: GlobalState::default(),
        };
        let tree = render(&state, 0);
        // Exactly one cursor, and it belongs to the active (right) pane.
        // Active window `b` has text "bbb" with cursor at byte 2 so the
        // cursor must land somewhere in the right half of the layout
        // (col > divider).
        assert_eq!(tree.cursors.len(), 1);
        let divider_col = {
            let t = tree.cells.to_debug_text();
            t.split('\n')
                .next()
                .unwrap()
                .chars()
                .position(|c| c == '\u{2502}')
                .unwrap() as u16
        };
        assert!(
            tree.cursors[0].col > divider_col,
            "cursor at col {} should be right of divider at {}",
            tree.cursors[0].col,
            divider_col
        );
    }
}
