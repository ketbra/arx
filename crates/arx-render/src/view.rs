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
use crate::face::ResolvedFace;
use crate::render_tree::{CursorRender, CursorStyle, RenderTree};
use crate::view_state::{GutterConfig, LayoutTree, TerminalSize, ViewState, WindowState};

/// Render a complete [`ViewState`] into a [`RenderTree`].
///
/// Phase 1 only draws the single-window case. Splits will be added when
/// we thread a per-window bounding box through [`render_window`].
pub fn render(state: &ViewState, frame_id: u64) -> RenderTree {
    let TerminalSize { cols, rows } = state.size;
    let mut grid = CellGrid::new(cols, rows);
    let mut cursors: SmallVec<[CursorRender; 1]> = SmallVec::new();

    // Reserve the bottom row for the modeline if there's height to spare.
    let (text_rows, modeline_row) = if rows >= 1 {
        (rows - 1, Some(rows - 1))
    } else {
        (0, None)
    };

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

    if let Some(row) = modeline_row {
        render_modeline(&state.global, row, cols, &mut grid);
    }

    RenderTree::new(grid, cursors, frame_id)
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
}
