//! Per-buffer tree-sitter parser, tree, and highlight query.

use std::sync::Arc;

use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit, Parser, Point, Query, QueryCursor, Tree};

use arx_buffer::{Edit, Interval, PropertyValue, Rope, StickyBehavior};

use crate::language::LanguageConfig;
use crate::theme::Theme;

/// A highlighter for one buffer. Owns the parser, the current parse
/// tree, and the compiled highlight query. All three are tied to the
/// same [`LanguageConfig`].
pub struct Highlighter {
    parser: Parser,
    tree: Option<Tree>,
    query: Query,
    #[allow(dead_code)]
    language_config: Arc<LanguageConfig>,
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("language", &self.language_config.name)
            .field("has_tree", &self.tree.is_some())
            .finish_non_exhaustive()
    }
}

impl Highlighter {
    /// Create a new highlighter for `config`. The parser is ready to
    /// parse; no tree exists yet (call [`Highlighter::parse_full`] to
    /// produce one).
    pub fn new(config: Arc<LanguageConfig>) -> Result<Self, HighlightError> {
        let mut parser = Parser::new();
        parser
            .set_language(&config.language)
            .map_err(|e| HighlightError::Language(e.to_string()))?;
        let query = Query::new(&config.language, config.highlights_query)
            .map_err(|e| HighlightError::Query(e.to_string()))?;
        Ok(Self {
            parser,
            tree: None,
            query,
            language_config: config,
        })
    }

    /// Parse the entire buffer from scratch and store the resulting
    /// tree. Call once when attaching a highlighter to a buffer.
    pub fn parse_full(&mut self, rope: &Rope) {
        let tree = parse_rope(&mut self.parser, rope, None);
        self.tree = tree;
    }

    /// Notify the tree of an edit and re-parse incrementally. Much
    /// cheaper than [`Highlighter::parse_full`] for single-keystroke
    /// edits (typically sub-millisecond).
    pub fn apply_edit(&mut self, edit: &Edit, rope: &Rope) {
        if let Some(tree) = &mut self.tree {
            let input_edit = edit_to_input_edit(edit, rope);
            tree.edit(&input_edit);
        }
        let old_tree = self.tree.as_ref();
        let new_tree = parse_rope(&mut self.parser, rope, old_tree);
        self.tree = new_tree;
    }

    /// Run the highlights query over `range` and return a list of
    /// `Interval` entries resolved against `theme`.
    pub fn highlight_range(
        &self,
        rope: &Rope,
        range: std::ops::Range<usize>,
        theme: &Theme,
    ) -> Vec<Interval> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let capture_names = self.query.capture_names();
        let mut intervals = Vec::new();

        // Materialize the text for the query. For large files a
        // streaming approach would be better, but for correctness
        // and simplicity this works. The text is already in memory
        // in the rope.
        let text = rope.text();
        let text_bytes = text.as_bytes();

        let mut matches = cursor.matches(&self.query, tree.root_node(), text_bytes);
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let node = capture.node;
                let byte_range = node.byte_range();
                if byte_range.start >= byte_range.end {
                    continue;
                }
                let name = &capture_names[capture.index as usize];
                if let Some(face) = theme.face_for_capture(name) {
                    intervals.push(Interval::new(
                        byte_range,
                        PropertyValue::Decoration(face),
                        StickyBehavior::Shrink,
                    ));
                }
            }
        }
        intervals
    }

    /// Convenience: highlight the entire buffer.
    pub fn highlight_all(
        &self,
        rope: &Rope,
        theme: &Theme,
    ) -> Vec<Interval> {
        self.highlight_range(rope, 0..rope.len_bytes(), theme)
    }
}

/// Parse `rope` via `parser`, optionally using `old_tree` for
/// incrementality. Uses ropey's chunk iterator to feed text without
/// materialising the full buffer into a contiguous allocation.
fn parse_rope(
    parser: &mut Parser,
    rope: &Rope,
    old_tree: Option<&Tree>,
) -> Option<Tree> {
    #[allow(deprecated)] // parse_with_options not stable yet in 0.25
    parser.parse_with(
        &mut |byte, _position| -> &[u8] {
            if byte >= rope.len_bytes() {
                return &[];
            }
            let inner = rope.inner();
            let (chunk, chunk_byte_start, _, _) = inner.chunk_at_byte(byte);
            let offset_in_chunk = byte - chunk_byte_start;
            &chunk.as_bytes()[offset_in_chunk..]
        },
        old_tree,
    )
}

/// Translate an [`arx_buffer::Edit`] into a [`tree_sitter::InputEdit`].
fn edit_to_input_edit(edit: &Edit, rope: &Rope) -> InputEdit {
    let start_byte = edit.offset;
    let old_end_byte = edit.offset + edit.old_len;
    let new_end_byte = edit.offset + edit.new_len;

    let start_position = byte_to_point(rope, start_byte);
    let old_end_position = byte_to_point_clamped(rope, old_end_byte);
    let new_end_position = byte_to_point(rope, new_end_byte);

    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position,
        old_end_position,
        new_end_position,
    }
}

fn byte_to_point(rope: &Rope, byte: usize) -> Point {
    let byte = byte.min(rope.len_bytes());
    let row = rope.byte_to_line(byte);
    let line_start = rope.line_to_byte(row);
    let column = byte - line_start;
    Point::new(row, column)
}

fn byte_to_point_clamped(rope: &Rope, byte: usize) -> Point {
    byte_to_point(rope, byte.min(rope.len_bytes()))
}

#[derive(Debug)]
pub enum HighlightError {
    Language(String),
    Query(String),
}

impl std::fmt::Display for HighlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HighlightError::Language(e) => write!(f, "language error: {e}"),
            HighlightError::Query(e) => write!(f, "query error: {e}"),
        }
    }
}

impl std::error::Error for HighlightError {}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_buffer::{Buffer, BufferId, EditOrigin};
    use crate::language::LanguageRegistry;

    fn rust_highlighter() -> (Highlighter, Arc<LanguageConfig>) {
        let reg = LanguageRegistry::new();
        let config = reg.config_for_extension("rs").unwrap();
        let hl = Highlighter::new(Arc::clone(&config)).unwrap();
        (hl, config)
    }

    #[test]
    fn parse_full_produces_a_tree() {
        let (mut hl, _) = rust_highlighter();
        let buf = Buffer::from_str(BufferId(1), "fn main() {}");
        hl.parse_full(buf.rope());
        assert!(hl.tree.is_some());
    }

    #[test]
    fn highlight_detects_keyword() {
        let (mut hl, _) = rust_highlighter();
        let buf = Buffer::from_str(BufferId(1), "fn main() {}");
        hl.parse_full(buf.rope());
        let theme = Theme::default_dark();
        let intervals = hl.highlight_all(buf.rope(), &theme);
        // `fn` should be captured as a keyword. Check that at least
        // one interval covers byte 0..2.
        let has_fn = intervals.iter().any(|iv| {
            iv.range.start == 0 && iv.range.end == 2
        });
        assert!(has_fn, "expected an interval for `fn`, got: {intervals:?}");
    }

    #[test]
    fn incremental_reparse_updates_tree() {
        let (mut hl, _) = rust_highlighter();
        let mut buf = Buffer::from_str(BufferId(1), "fn f() {}");
        hl.parse_full(buf.rope());
        // Insert "oo" after the 'f' in the function name (byte 4).
        let edit = buf.edit(4..4, "oo", EditOrigin::User);
        hl.apply_edit(&edit, buf.rope());
        assert!(hl.tree.is_some());
        assert_eq!(buf.text(), "fn foo() {}");
    }
}
