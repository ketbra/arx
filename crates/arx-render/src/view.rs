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
    GutterConfig, LayoutTree, PaletteView, TerminalSize, ViewState, WindowState,
};

/// Render a complete [`ViewState`] into a [`RenderTree`].
///
/// Phase 1 only draws the single-window case. Splits will be added when
/// we thread a per-window bounding box through [`render_window`].
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
/// The shrinking happens before `render_window` is called so the
/// primary window's cursor-visibility math (which depends on
/// [`WindowState::scroll`] and the rect height passed here) doesn't
/// collide with the overlay.
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
        // Shrink the text area to make room for the overlay. `prompt_row`
        // being `None` would mean the terminal is too short for a useful
        // overlay — in that case we leave text_rows alone and skip
        // painting the palette below.
        if let Some(prompt) = layout.prompt_row {
            text_rows = prompt;
        }
    }

    match &state.layout {
        LayoutTree::Single(window_id) => {
            if let Some(window) = state.windows.iter().find(|w| w.id == *window_id) {
                render_window(window, Rect::new(0, 0, cols, text_rows), &mut grid, &mut cursors);
            }
        }
        LayoutTree::Split { .. } => {
            // TODO(phase-2): recurse into splits with per-pane bounding boxes.
        }
    }

    if let Some(layout) = palette_layout {
        paint_palette(&layout, cols, &mut grid, &mut cursors);
    }

    if let Some(row) = modeline_row {
        render_modeline(&state.global, row, cols, &mut grid);
    }

    RenderTree::new(grid, cursors, frame_id)
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
fn render_window(
    window: &WindowState,
    rect: Rect,
    grid: &mut CellGrid,
    cursors: &mut SmallVec<[CursorRender; 1]>,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
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

    // Primary cursor position.
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
    for g in "M-x ".graphemes(true) {
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

// ---------------------------------------------------------------------------
// Little helper rect
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

impl Rect {
    const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self { x, y, width, height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_buffer::{Buffer, BufferId};
    use smallvec::smallvec as sv;

    use crate::view_state::{Cursor, GlobalState, ScrollPosition};

    fn window_for(text: &str) -> WindowState {
        let buf = Buffer::from_str(BufferId(1), text);
        WindowState {
            id: crate::view_state::WindowId(1),
            buffer: buf.snapshot(),
            cursors: sv![Cursor::at(0)],
            scroll: ScrollPosition::default(),
            gutter: GutterConfig::default(),
        }
    }

    fn state_for(window: WindowState, cols: u16, rows: u16) -> ViewState {
        ViewState {
            size: TerminalSize::new(cols, rows),
            layout: LayoutTree::Single(window.id),
            windows: vec![window],
            global: GlobalState {
                modeline_left: String::new(),
                modeline_right: String::new(),
                palette: None,
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
        ViewState {
            size: TerminalSize::new(cols, rows),
            layout: LayoutTree::Single(window.id),
            windows: vec![window],
            global: GlobalState {
                modeline_left: String::new(),
                modeline_right: String::new(),
                palette: Some(palette),
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
}
