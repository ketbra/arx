//! Persistent text rope.
//!
//! Phase 1 wraps the [`ropey`] crate. Ropey is a battle-tested B-tree rope
//! used by Helix and other production editors; it gives us O(log n) edits,
//! O(1) snapshots (via `Clone` — Ropey is internally `Arc`-backed and
//! copy-on-write), correct UTF-8 handling, multi-EOL line counting, UTF-16
//! code unit indexing for LSP, and grapheme-aware iteration. Reimplementing
//! all of that ourselves was the path of the previous version of this
//! module; the review pass found multiple correctness bugs and the
//! maintenance burden was unnecessary.
//!
//! This module is a thin newtype wrapper. The public API is byte-indexed
//! and intentionally narrow so that:
//!
//! 1. Callers stay decoupled from `ropey::Rope` directly. We can swap the
//!    backing store later (e.g. for a Zed-style `sum_tree` if/when we adopt
//!    full CRDT support, see `docs/spec.md` §19 q2).
//! 2. The byte-range vocabulary matches our [`crate::IntervalTree`] and
//!    [`crate::Edit`] types — we don't have to teach the rest of the crate
//!    about Ropey's char-indexed mutation API.
//!
//! Callers that need direct access to Ropey's full feature set can reach
//! through [`Rope::inner`] for the underlying [`ropey::Rope`].
//!
//! ## `TODO(phase-2)`: introduce `sum_tree` for non-text sequences
//!
//! Once we wire up rendering, multi-cursor selections, undo history, and
//! anchors that need to survive concurrent edits without manual offset
//! shifting, those data structures should live in a Zed-style generic
//! B-tree (`sum_tree`, published as a standalone crate). Ropey stays the
//! text store; `sum_tree` handles position-bearing metadata.

use std::fmt;
use std::ops::Range;

/// A byte-indexed half-open range `[start, end)` inside a rope.
pub type ByteRange = Range<usize>;

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

/// Aggregated counters for a rope. Synthesised on demand from Ropey's
/// totals — kept as a stable struct so callers don't have to learn the
/// underlying API.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextSummary {
    /// Total number of UTF-8 bytes.
    pub bytes: usize,
    /// Total number of Unicode scalar values.
    pub chars: usize,
    /// Number of `\n` line break bytes (so the *line count* is `line_breaks + 1`).
    pub line_breaks: usize,
    /// Number of UTF-16 code units, useful for LSP byte/char/utf-16 round-trips.
    pub utf16_code_units: usize,
}

impl TextSummary {
    fn from_rope(r: &ropey::Rope) -> Self {
        Self {
            bytes: r.len_bytes(),
            chars: r.len_chars(),
            line_breaks: r.len_lines().saturating_sub(1),
            utf16_code_units: r.len_utf16_cu(),
        }
    }
}

// ---------------------------------------------------------------------------
// Rope
// ---------------------------------------------------------------------------

/// An immutable, copy-on-write text rope.
///
/// Cloning is `O(1)` — Ropey shares its internal B-tree via `Arc`. All
/// mutating operations return a new rope that shares structure with the
/// original.
#[derive(Clone, Default)]
pub struct Rope {
    inner: ropey::Rope,
}

impl Rope {
    // ---------- construction ----------

    /// Create an empty rope.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a rope from a string slice.
    ///
    /// Named `from_str` to match the spec; intentionally not implementing
    /// [`std::str::FromStr`] because construction is infallible and
    /// returning `Result<Self, Infallible>` would just add noise.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        Self {
            inner: ropey::Rope::from_str(text),
        }
    }

    /// Borrow the underlying [`ropey::Rope`] for callers who want the
    /// crate's full feature set (`RopeSlice`, `byte_slice`, UTF-16 indexing,
    /// `chunks_at_byte`, etc.).
    pub fn inner(&self) -> &ropey::Rope {
        &self.inner
    }

    /// Consume this wrapper and return the underlying [`ropey::Rope`].
    pub fn into_inner(self) -> ropey::Rope {
        self.inner
    }

    // ---------- size queries ----------

    /// Total byte length.
    pub fn len_bytes(&self) -> usize {
        self.inner.len_bytes()
    }

    /// Total number of Unicode scalar values.
    pub fn len_chars(&self) -> usize {
        self.inner.len_chars()
    }

    /// Total number of lines. An empty rope has one line.
    pub fn len_lines(&self) -> usize {
        self.inner.len_lines()
    }

    /// Total number of UTF-16 code units. Used by LSP integrations that
    /// need to translate between UTF-8 and UTF-16 positions.
    pub fn len_utf16_cu(&self) -> usize {
        self.inner.len_utf16_cu()
    }

    /// Cached counter aggregate for the entire rope.
    pub fn summary(&self) -> TextSummary {
        TextSummary::from_rope(&self.inner)
    }

    /// Return true if the rope contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.inner.len_bytes() == 0
    }

    // ---------- coordinate conversions ----------

    /// Byte offset → 0-indexed line number.
    ///
    /// `byte` may be any value in `0..=len_bytes()`. The byte offset does
    /// not need to land on a UTF-8 character boundary because line counting
    /// is byte-level (`\n` is ASCII).
    pub fn byte_to_line(&self, byte: usize) -> usize {
        self.inner.byte_to_line(byte)
    }

    /// Byte offset → character index. Panics if `byte` is not on a UTF-8
    /// character boundary.
    pub fn byte_to_char(&self, byte: usize) -> usize {
        self.inner.byte_to_char(byte)
    }

    /// Character index → byte offset.
    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.inner.char_to_byte(char_idx)
    }

    /// Line number → byte offset of the line's first byte.
    ///
    /// For `line == len_lines()` this returns [`Rope::len_bytes`]. Requests
    /// past that saturate at the end of the rope.
    pub fn line_to_byte(&self, line: usize) -> usize {
        if line >= self.inner.len_lines() {
            return self.inner.len_bytes();
        }
        self.inner.line_to_byte(line)
    }

    /// Return whether `byte_offset` lies on a UTF-8 character boundary.
    /// 0 and [`Rope::len_bytes`] are always boundaries.
    ///
    /// Implementation note: Ropey's `byte_to_char` clamps mid-character
    /// offsets to the char they're inside (it does not signal "not a
    /// boundary"), so we round-trip the conversion: a true boundary
    /// satisfies `char_to_byte(byte_to_char(b)) == b`.
    pub fn is_char_boundary(&self, byte_offset: usize) -> bool {
        let len = self.inner.len_bytes();
        if byte_offset == 0 || byte_offset == len {
            return true;
        }
        if byte_offset > len {
            return false;
        }
        let char_idx = self.inner.byte_to_char(byte_offset);
        self.inner.char_to_byte(char_idx) == byte_offset
    }

    // ---------- editing ----------

    /// Replace the bytes in `range` with `text`, returning a new rope that
    /// shares structure with `self`.
    ///
    /// Panics if `range` is inverted, out of bounds, or does not fall on
    /// UTF-8 character boundaries.
    pub fn edit(&self, range: ByteRange, text: &str) -> Rope {
        let len = self.inner.len_bytes();
        assert!(range.start <= range.end, "inverted edit range {range:?}");
        assert!(
            range.end <= len,
            "edit range {range:?} out of bounds (len={len})"
        );
        assert!(
            self.is_char_boundary(range.start),
            "edit range start {} is not on a UTF-8 character boundary",
            range.start
        );
        assert!(
            self.is_char_boundary(range.end),
            "edit range end {} is not on a UTF-8 character boundary",
            range.end
        );

        // Clone is O(1) — Ropey shares the internal B-tree via Arc and
        // copies-on-write on mutation, so the original snapshot is unchanged.
        let mut next = self.inner.clone();
        let char_start = next.byte_to_char(range.start);
        let char_end = next.byte_to_char(range.end);
        if char_end > char_start {
            next.remove(char_start..char_end);
        }
        if !text.is_empty() {
            next.insert(char_start, text);
        }
        Rope { inner: next }
    }

    /// Split the rope at a byte offset. Both halves share structure with
    /// `self`. Panics if `byte_offset` is not on a character boundary.
    pub fn split(&self, byte_offset: usize) -> (Rope, Rope) {
        let len = self.inner.len_bytes();
        assert!(byte_offset <= len, "split offset {byte_offset} > len {len}");
        assert!(
            self.is_char_boundary(byte_offset),
            "split offset {byte_offset} is not on a UTF-8 character boundary"
        );
        let mut left = self.inner.clone();
        let char_idx = left.byte_to_char(byte_offset);
        let right = left.split_off(char_idx);
        (Rope { inner: left }, Rope { inner: right })
    }

    /// Concatenate `other` to the end of `self`.
    pub fn concat(self, other: Rope) -> Rope {
        let mut left = self.inner;
        left.append(other.inner);
        Rope { inner: left }
    }

    // ---------- materialisation ----------

    /// Materialise the entire rope as a newly-allocated `String`.
    ///
    /// `O(n)` and pre-sized. Prefer [`Rope::chunks`] / [`Rope::bytes`] /
    /// [`Rope::chars`] for incremental access — they don't allocate.
    pub fn text(&self) -> String {
        String::from(&self.inner)
    }

    /// Extract a byte range as a newly-allocated `String`.
    ///
    /// Walks the tree directly via Ropey's `byte_slice` (no intermediate
    /// nodes are constructed). Panics if the range is out of bounds or off
    /// UTF-8 character boundaries.
    pub fn slice_to_string(&self, range: ByteRange) -> String {
        let len = self.inner.len_bytes();
        assert!(range.start <= range.end, "inverted slice range");
        assert!(range.end <= len, "slice range out of bounds");
        assert!(
            self.is_char_boundary(range.start) && self.is_char_boundary(range.end),
            "slice range {range:?} is not on UTF-8 character boundaries"
        );
        String::from(self.inner.byte_slice(range))
    }

    // ---------- iteration ----------

    /// Iterate over leaf chunks (`&str`) in order. Zero allocations.
    pub fn chunks(&self) -> impl Iterator<Item = &str> + '_ {
        self.inner.chunks()
    }

    /// Byte-wise iterator. Zero allocations.
    pub fn bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.inner.bytes()
    }

    /// Iterator over Unicode scalar values. Zero allocations.
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.inner.chars()
    }

    /// Iterator over 0-indexed lines as owned `String`s. Each yielded line
    /// retains its trailing newline if present.
    ///
    /// For zero-copy access prefer `rope.inner().lines()` and operate on
    /// the resulting [`ropey::RopeSlice`] directly.
    pub fn lines(&self) -> impl Iterator<Item = String> + '_ {
        self.inner.lines().map(String::from)
    }
}

impl fmt::Debug for Rope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rope")
            .field("bytes", &self.len_bytes())
            .field("lines", &self.len_lines())
            .finish()
    }
}

impl fmt::Display for Rope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl PartialEq for Rope {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for Rope {}

impl From<&str> for Rope {
    fn from(s: &str) -> Self {
        Rope::from_str(s)
    }
}

impl From<String> for Rope {
    fn from(s: String) -> Self {
        Rope::from_str(&s)
    }
}

impl From<ropey::Rope> for Rope {
    fn from(inner: ropey::Rope) -> Self {
        Rope { inner }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rope() {
        let r = Rope::new();
        assert!(r.is_empty());
        assert_eq!(r.len_bytes(), 0);
        assert_eq!(r.len_chars(), 0);
        assert_eq!(r.len_lines(), 1);
        assert_eq!(r.text(), "");
    }

    #[test]
    fn small_from_str() {
        let r = Rope::from_str("hello");
        assert_eq!(r.len_bytes(), 5);
        assert_eq!(r.len_chars(), 5);
        assert_eq!(r.len_lines(), 1);
        assert_eq!(r.text(), "hello");
    }

    #[test]
    fn multi_byte_chars() {
        let r = Rope::from_str("héllo 🦀");
        assert_eq!(r.text(), "héllo 🦀");
        assert_eq!(r.len_bytes(), "héllo 🦀".len());
        assert_eq!(r.len_chars(), 7);
    }

    #[test]
    fn line_queries() {
        let r = Rope::from_str("a\nbb\nccc\ndddd");
        assert_eq!(r.len_lines(), 4);
        assert_eq!(r.line_to_byte(0), 0);
        assert_eq!(r.line_to_byte(1), 2);
        assert_eq!(r.line_to_byte(2), 5);
        assert_eq!(r.line_to_byte(3), 9);
        assert_eq!(r.line_to_byte(4), 13);
        assert_eq!(r.byte_to_line(0), 0);
        assert_eq!(r.byte_to_line(1), 0);
        assert_eq!(r.byte_to_line(2), 1);
        assert_eq!(r.byte_to_line(5), 2);
        assert_eq!(r.byte_to_line(13), 3);
    }

    #[test]
    fn byte_to_char_mixed() {
        let r = Rope::from_str("a🦀b");
        assert_eq!(r.byte_to_char(0), 0);
        assert_eq!(r.byte_to_char(1), 1);
        assert_eq!(r.byte_to_char(5), 2);
        assert_eq!(r.byte_to_char(6), 3);
    }

    #[test]
    fn edit_replace_middle() {
        let r = Rope::from_str("hello world");
        let r2 = r.edit(6..11, "rope!");
        assert_eq!(r.text(), "hello world");
        assert_eq!(r2.text(), "hello rope!");
    }

    #[test]
    fn edit_insert() {
        let r = Rope::from_str("ac");
        let r2 = r.edit(1..1, "b");
        assert_eq!(r2.text(), "abc");
    }

    #[test]
    fn edit_delete() {
        let r = Rope::from_str("abcdef");
        let r2 = r.edit(2..4, "");
        assert_eq!(r2.text(), "abef");
    }

    #[test]
    fn edit_empty_rope() {
        let r = Rope::new();
        let r2 = r.edit(0..0, "hello");
        assert_eq!(r2.text(), "hello");
    }

    #[test]
    fn large_rope_roundtrip() {
        use std::fmt::Write;
        let mut text = String::new();
        for i in 0..5_000 {
            writeln!(text, "line {i}").unwrap();
        }
        let r = Rope::from_str(&text);
        assert_eq!(r.len_bytes(), text.len());
        assert_eq!(r.len_lines(), text.matches('\n').count() + 1);
        assert_eq!(r.text(), text);

        let mid = text.len() / 2;
        // Walk to the nearest char boundary at or before `mid`.
        let mut split_at = mid;
        while !r.is_char_boundary(split_at) {
            split_at -= 1;
        }
        let r2 = r.edit(split_at..split_at, "INSERT ");
        assert_eq!(r2.len_bytes(), text.len() + "INSERT ".len());
        assert!(r2.text().contains("INSERT "));
    }

    #[test]
    fn many_appends_share_structure() {
        let mut r = Rope::from_str("start");
        for i in 0..2_000 {
            let len = r.len_bytes();
            r = r.edit(len..len, &format!("[{i}]"));
        }
        assert!(r.text().starts_with("start[0][1]"));
        assert!(r.text().ends_with("[1999]"));
    }

    #[test]
    fn split_and_concat_roundtrip() {
        let text = "the quick brown fox jumps over the lazy dog";
        let r = Rope::from_str(text);
        for i in 0..=text.len() {
            let (a, b) = r.split(i);
            let joined = a.concat(b).text();
            assert_eq!(joined, text, "split/concat mismatch at {i}");
        }
    }

    #[test]
    fn snapshot_independence() {
        let r1 = Rope::from_str("abc");
        let r2 = r1.edit(1..1, "XYZ");
        assert_eq!(r1.text(), "abc");
        assert_eq!(r2.text(), "aXYZbc");
    }

    #[test]
    fn iterators_consistent() {
        let text = "hello\nworld\nfoo bar";
        let r = Rope::from_str(text);
        let collected: String = r.chunks().collect();
        assert_eq!(collected, text);

        let from_bytes: Vec<u8> = r.bytes().collect();
        assert_eq!(from_bytes, text.as_bytes());

        let from_chars: String = r.chars().collect();
        assert_eq!(from_chars, text);
    }

    #[test]
    fn lines_iterator() {
        let r = Rope::from_str("a\nbb\nccc");
        let lines: Vec<String> = r.lines().collect();
        assert_eq!(lines, vec!["a\n", "bb\n", "ccc"]);

        let r2 = Rope::from_str("trailing\n");
        let lines2: Vec<String> = r2.lines().collect();
        assert_eq!(lines2, vec!["trailing\n", ""]);

        let r3 = Rope::new();
        let lines3: Vec<String> = r3.lines().collect();
        assert_eq!(lines3, vec![String::new()]);
    }

    #[test]
    fn slice_to_string_range() {
        let r = Rope::from_str("hello world");
        assert_eq!(r.slice_to_string(0..5), "hello");
        assert_eq!(r.slice_to_string(6..11), "world");
        assert_eq!(r.slice_to_string(11..11), "");
    }

    // ---- char boundary / UTF-8 safety ----

    #[test]
    fn is_char_boundary_matches_str() {
        let text = "a🦀b한ç";
        let r = Rope::from_str(text);
        for i in 0..=text.len() {
            assert_eq!(
                r.is_char_boundary(i),
                text.is_char_boundary(i),
                "disagreement at offset {i}"
            );
        }
    }

    #[test]
    fn multi_byte_split_and_concat_roundtrip() {
        let text = "héllo 🦀 世界";
        let r = Rope::from_str(text);
        for i in 0..=text.len() {
            if !text.is_char_boundary(i) {
                continue;
            }
            let (a, b) = r.split(i);
            assert_eq!(a.concat(b).text(), text, "split at {i}");
        }
    }

    #[test]
    fn multi_byte_edit() {
        // "a🦀b" is 6 bytes: 'a' (1), '🦀' (4 @ 1..5), 'b' (1 @ 5).
        let r = Rope::from_str("a🦀b");
        let r2 = r.edit(1..5, "X");
        assert_eq!(r2.text(), "aXb");
        let r3 = r.edit(6..6, "🌊");
        assert_eq!(r3.text(), "a🦀b🌊");
        let r4 = r.edit(1..5, "🦊");
        assert_eq!(r4.text(), "a🦊b");
    }

    #[test]
    #[should_panic(expected = "not on a UTF-8 character boundary")]
    fn edit_off_boundary_panics_clearly() {
        let r = Rope::from_str("🦀");
        let _ = r.edit(1..2, "X");
    }

    #[test]
    fn byte_to_line_accepts_non_boundary_offsets() {
        let r = Rope::from_str("a🦀\nb");
        // 🦀 spans bytes 1..5. Offset 2 falls inside but byte_to_line should
        // still answer (no newlines before offset 2).
        assert_eq!(r.byte_to_line(2), 0);
        assert_eq!(r.byte_to_line(5), 0);
        assert_eq!(r.byte_to_line(6), 1);
    }

    // ---- Lines iterator correctness & performance ----

    #[test]
    fn lines_over_many_lines_is_linear() {
        use std::fmt::Write;
        let line_count = 100_000;
        let mut text = String::with_capacity(line_count * 8);
        for i in 0..line_count {
            writeln!(text, "l{i}").unwrap();
        }
        let r = Rope::from_str(&text);
        let count = r.lines().count();
        assert_eq!(count, line_count + 1); // trailing empty line
    }

    // ---- summary + utf-16 ----

    #[test]
    fn summary_includes_utf16_units() {
        let r = Rope::from_str("a🦀b");
        let s = r.summary();
        assert_eq!(s.bytes, 6);
        assert_eq!(s.chars, 3);
        // 🦀 is a supplementary plane char → 2 UTF-16 code units, plus 'a' and 'b'.
        assert_eq!(s.utf16_code_units, 4);
        assert_eq!(s.line_breaks, 0);
    }

    // ---- Thread-safety assertions ----

    #[test]
    fn rope_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<Rope>();
        assert_sync::<Rope>();
    }
}
