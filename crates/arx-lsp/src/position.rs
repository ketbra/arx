//! Translate between LSP positions (line + UTF-16 character offset)
//! and Arx byte offsets.
//!
//! LSP specifies positions as `{ line: u32, character: u32 }` where
//! `character` counts UTF-16 code units from the start of the line.
//! Arx uses byte offsets into a UTF-8 rope. The helpers here bridge
//! the two coordinate systems using ropey's character-level iterators.

use arx_buffer::Rope;

/// Convert an LSP `Position` to a byte offset in `rope`.
///
/// Returns `None` if the line is out of range. If `character` extends
/// past the end of the line, the offset is clamped to the line's end
/// (before its newline, if any) — this is defensive against servers
/// that sometimes report positions one past EOL.
pub fn lsp_position_to_byte(rope: &Rope, line: u32, character: u32) -> Option<usize> {
    let line = line as usize;
    if line >= rope.len_lines() {
        return None;
    }
    let line_start_byte = rope.line_to_byte(line);
    let line_end_byte = if line + 1 < rope.len_lines() {
        rope.line_to_byte(line + 1)
    } else {
        rope.len_bytes()
    };

    // Walk the line's characters, accumulating UTF-16 code units.
    let line_text = rope.slice_to_string(line_start_byte..line_end_byte);
    let target_utf16 = character as usize;
    let mut utf16_offset: usize = 0;
    let mut byte_offset: usize = 0;
    for ch in line_text.chars() {
        if utf16_offset >= target_utf16 {
            break;
        }
        utf16_offset += ch.len_utf16();
        byte_offset += ch.len_utf8();
    }
    Some(line_start_byte + byte_offset)
}

/// Convert a byte offset in `rope` to an LSP `Position`.
pub fn byte_to_lsp_position(rope: &Rope, byte: usize) -> lsp_types::Position {
    let byte = byte.min(rope.len_bytes());
    let line = rope.byte_to_line(byte);
    let line_start = rope.line_to_byte(line);
    let prefix = rope.slice_to_string(line_start..byte);
    let character: u32 = prefix.chars().map(|c| c.len_utf16() as u32).sum();
    lsp_types::Position::new(line as u32, character)
}

/// Convert an LSP `Range` to a byte range in `rope`. Returns `None`
/// if either endpoint is out of range.
pub fn lsp_range_to_bytes(
    rope: &Rope,
    range: &lsp_types::Range,
) -> Option<std::ops::Range<usize>> {
    let start = lsp_position_to_byte(rope, range.start.line, range.start.character)?;
    let end = lsp_position_to_byte(rope, range.end.line, range.end.character)?;
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_buffer::{Buffer, BufferId};

    fn rope(text: &str) -> Rope {
        Buffer::from_str(BufferId(1), text).rope().clone()
    }

    #[test]
    fn ascii_round_trip() {
        let r = rope("hello\nworld\n");
        // 'w' is at line 1, character 0, byte 6.
        assert_eq!(lsp_position_to_byte(&r, 1, 0), Some(6));
        assert_eq!(byte_to_lsp_position(&r, 6), lsp_types::Position::new(1, 0));
    }

    #[test]
    fn multibyte_utf8_vs_utf16() {
        // 'e' with acute accent = U+00E9, 1 UTF-16 code unit, 2 UTF-8 bytes.
        let r = rope("caf\u{00E9}\n");
        // Character 4 (after 'c','a','f','e-acute') should be byte 5
        // (c=1, a=1, f=1, e-acute=2).
        assert_eq!(lsp_position_to_byte(&r, 0, 4), Some(5));
        assert_eq!(
            byte_to_lsp_position(&r, 5),
            lsp_types::Position::new(0, 4),
        );
    }

    #[test]
    fn supplementary_plane_counts_two_utf16_units() {
        // U+1F600 (grinning face) = 4 UTF-8 bytes, 2 UTF-16 code units.
        let r = rope("a\u{1F600}b\n");
        // After 'a' (1 UTF-16 cu) + emoji (2 UTF-16 cu) = character 3
        // → byte offset should be 1 + 4 = 5.
        assert_eq!(lsp_position_to_byte(&r, 0, 3), Some(5));
        assert_eq!(
            byte_to_lsp_position(&r, 5),
            lsp_types::Position::new(0, 3),
        );
    }

    #[test]
    fn out_of_range_line_returns_none() {
        let r = rope("one line");
        assert!(lsp_position_to_byte(&r, 5, 0).is_none());
    }

    #[test]
    fn character_past_eol_clamps() {
        let r = rope("hi\n");
        // Character 999 on line 0 → clamps to end of "hi" = byte 2.
        let byte = lsp_position_to_byte(&r, 0, 999).unwrap();
        assert!(byte <= 3); // at most the newline position
    }

    #[test]
    fn range_conversion() {
        let r = rope("hello\nworld\n");
        let range = lsp_types::Range::new(
            lsp_types::Position::new(0, 1),
            lsp_types::Position::new(0, 4),
        );
        assert_eq!(lsp_range_to_bytes(&r, &range), Some(1..4));
    }
}
