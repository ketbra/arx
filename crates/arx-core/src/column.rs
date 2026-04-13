//! Display-column helpers for rectangle (column block) operations.
//!
//! Terminal editors operate on a character grid where each grapheme
//! cluster occupies 1 or 2 cells (CJK glyphs = 2). These helpers
//! convert between byte offsets within a line and display column
//! positions, which is necessary for column-aligned operations like
//! rectangle kill/yank.

use arx_buffer::BufferId;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::editor::Editor;
use crate::WindowId;

/// Convert a byte offset within a line to a 0-based display column.
pub fn byte_to_display_col(line_text: &str, byte_in_line: usize) -> u16 {
    let mut col: u16 = 0;
    for (gi, g) in line_text.grapheme_indices(true) {
        if gi >= byte_in_line {
            break;
        }
        col += UnicodeWidthStr::width(g).max(1) as u16;
    }
    col
}

/// Convert a 0-based display column to a byte offset within a line.
/// Returns the byte offset of the grapheme at or just past `target_col`.
/// If the line is shorter than `target_col`, returns `line_text.len()`.
pub fn display_col_to_byte(line_text: &str, target_col: u16) -> usize {
    let mut col: u16 = 0;
    for (gi, g) in line_text.grapheme_indices(true) {
        if col >= target_col {
            return gi;
        }
        col += UnicodeWidthStr::width(g).max(1) as u16;
    }
    line_text.len()
}

/// A rectangular region defined by line range and display column range.
#[derive(Debug, Clone, Copy)]
pub struct RectRegion {
    /// First line (0-based, inclusive).
    pub start_line: usize,
    /// Last line (0-based, inclusive).
    pub end_line: usize,
    /// Left display column (inclusive).
    pub left_col: u16,
    /// Right display column (exclusive).
    pub right_col: u16,
}

impl RectRegion {
    /// Compute a rectangle from the mark and cursor byte offsets.
    pub fn from_mark_cursor(
        editor: &Editor,
        buffer_id: BufferId,
        mark_byte: usize,
        cursor_byte: usize,
    ) -> Option<Self> {
        let buffer = editor.buffers().get(buffer_id)?;
        let rope = buffer.rope();
        let mark_line = rope.byte_to_line(mark_byte);
        let cursor_line = rope.byte_to_line(cursor_byte);
        let start_line = mark_line.min(cursor_line);
        let end_line = mark_line.max(cursor_line);

        // Get display columns.
        let mark_line_start = rope.line_to_byte(mark_line);
        let cursor_line_start = rope.line_to_byte(cursor_line);

        let mark_line_text = line_text_no_newline(editor, buffer_id, mark_line)?;
        let cursor_line_text = line_text_no_newline(editor, buffer_id, cursor_line)?;

        let mark_col =
            byte_to_display_col(&mark_line_text, mark_byte.saturating_sub(mark_line_start));
        let cursor_col = byte_to_display_col(
            &cursor_line_text,
            cursor_byte.saturating_sub(cursor_line_start),
        );

        let left_col = mark_col.min(cursor_col);
        let right_col = mark_col.max(cursor_col);

        Some(RectRegion {
            start_line,
            end_line,
            left_col,
            right_col,
        })
    }
}

/// Extract a line's text without the trailing newline.
fn line_text_no_newline(editor: &Editor, buffer_id: BufferId, line: usize) -> Option<String> {
    let buffer = editor.buffers().get(buffer_id)?;
    let rope = buffer.rope();
    let start = rope.line_to_byte(line);
    let end = if line + 1 >= rope.len_lines() {
        rope.len_bytes()
    } else {
        rope.line_to_byte(line + 1).saturating_sub(1)
    };
    Some(rope.slice_to_string(start..end))
}

/// Kill (delete) a rectangular region from the buffer. Returns the
/// killed text as a vector of strings (one per line), suitable for
/// storing in the kill ring as `KilledText::Rectangular`.
///
/// Edits are applied bottom-to-top to preserve byte offsets.
pub fn kill_rectangle(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    rect: &RectRegion,
) -> Vec<String> {
    let mut killed_lines = Vec::new();

    // First pass: extract the text to kill.
    for line_idx in rect.start_line..=rect.end_line {
        let text = line_text_no_newline(editor, buffer_id, line_idx).unwrap_or_default();
        let byte_start = display_col_to_byte(&text, rect.left_col);
        let byte_end = display_col_to_byte(&text, rect.right_col);
        killed_lines.push(text[byte_start..byte_end].to_owned());
    }

    // Second pass: delete bottom-to-top.
    for line_idx in (rect.start_line..=rect.end_line).rev() {
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            break;
        };
        let line_start = buffer.rope().line_to_byte(line_idx);
        let text = line_text_no_newline(editor, buffer_id, line_idx).unwrap_or_default();
        let byte_start = display_col_to_byte(&text, rect.left_col);
        let byte_end = display_col_to_byte(&text, rect.right_col);
        if byte_start < byte_end {
            let abs_start = line_start + byte_start;
            let abs_end = line_start + byte_end;
            crate::stock::user_edit(
                editor,
                window_id,
                buffer_id,
                abs_start..abs_end,
                "",
                abs_start,
                abs_start,
            );
        }
    }

    killed_lines
}

/// Open (insert blank space into) a rectangular region, shifting text
/// right. Inserts `(right_col - left_col)` spaces at `left_col` on
/// each line.
pub fn open_rectangle(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    rect: &RectRegion,
) {
    let width = (rect.right_col.saturating_sub(rect.left_col)) as usize;
    if width == 0 {
        return;
    }
    let spaces: String = " ".repeat(width);

    for line_idx in (rect.start_line..=rect.end_line).rev() {
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            break;
        };
        let line_start = buffer.rope().line_to_byte(line_idx);
        let text = line_text_no_newline(editor, buffer_id, line_idx).unwrap_or_default();
        let byte_at_col = display_col_to_byte(&text, rect.left_col);

        // If the line is shorter than left_col, pad with spaces first.
        let line_display_width = byte_to_display_col(&text, text.len());
        let padding = if line_display_width < rect.left_col {
            " ".repeat((rect.left_col - line_display_width) as usize)
        } else {
            String::new()
        };

        let insert_text = format!("{padding}{spaces}");
        let abs_pos = line_start + byte_at_col;
        crate::stock::user_edit(
            editor,
            window_id,
            buffer_id,
            abs_pos..abs_pos,
            &insert_text,
            abs_pos,
            abs_pos + insert_text.len(),
        );
    }
}

/// Yank (paste) a rectangular kill at the cursor's current position.
/// Each line of the rectangle is inserted at the cursor's display
/// column on successive lines.
pub fn yank_rectangle(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    lines: &[String],
) {
    let Some(window) = editor.windows().get(window_id) else {
        return;
    };
    let cursor = window.cursor_byte;
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return;
    };
    let cursor_line = buffer.rope().byte_to_line(cursor);
    let cursor_line_start = buffer.rope().line_to_byte(cursor_line);
    let line_text = line_text_no_newline(editor, buffer_id, cursor_line).unwrap_or_default();
    let target_col = byte_to_display_col(&line_text, cursor - cursor_line_start);

    let total_lines = editor
        .buffers()
        .get(buffer_id)
        .map_or(0, |b| b.rope().len_lines());

    // Insert bottom-to-top so byte offsets remain valid.
    for (i, rect_line) in lines.iter().enumerate().rev() {
        let target_line = cursor_line + i;
        if target_line >= total_lines {
            // Need to add a newline at the end of the buffer first.
            let Some(buf) = editor.buffers().get(buffer_id) else {
                break;
            };
            let end = buf.rope().len_bytes();
            crate::stock::user_edit(
                editor,
                window_id,
                buffer_id,
                end..end,
                "\n",
                end,
                end + 1,
            );
        }
        let Some(buf) = editor.buffers().get(buffer_id) else {
            break;
        };
        let line_start = buf.rope().line_to_byte(target_line);
        let text = line_text_no_newline(editor, buffer_id, target_line).unwrap_or_default();

        // Pad the line if it's shorter than target_col.
        let line_width = byte_to_display_col(&text, text.len());
        let padding = if line_width < target_col {
            " ".repeat((target_col - line_width) as usize)
        } else {
            String::new()
        };

        let byte_at_col = display_col_to_byte(&text, target_col);
        let insert_text = format!("{padding}{rect_line}");
        let abs_pos = line_start + byte_at_col;
        crate::stock::user_edit(
            editor,
            window_id,
            buffer_id,
            abs_pos..abs_pos,
            &insert_text,
            abs_pos,
            abs_pos + insert_text.len(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_to_col_ascii() {
        assert_eq!(byte_to_display_col("hello", 0), 0);
        assert_eq!(byte_to_display_col("hello", 3), 3);
        assert_eq!(byte_to_display_col("hello", 5), 5);
    }

    #[test]
    fn col_to_byte_ascii() {
        assert_eq!(display_col_to_byte("hello", 0), 0);
        assert_eq!(display_col_to_byte("hello", 3), 3);
        assert_eq!(display_col_to_byte("hello", 10), 5); // past end
    }

    #[test]
    fn col_conversion_roundtrip() {
        let text = "abc";
        for byte in 0..=text.len() {
            let col = byte_to_display_col(text, byte);
            let back = display_col_to_byte(text, col);
            assert_eq!(back, byte, "roundtrip failed for byte {byte}");
        }
    }
}
