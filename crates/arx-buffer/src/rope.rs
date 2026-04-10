//! Persistent copy-on-write rope.
//!
//! The rope is modelled as a balanced binary tree of UTF-8 leaves. Every
//! subtree is wrapped in an [`Arc`] so that cloning the rope, splitting it,
//! and taking snapshots share structure rather than copying text. Each node
//! caches a [`TextSummary`] (bytes, chars, line-break count) so that offset,
//! line, and character conversions are `O(log n)`.
//!
//! The invariants maintained at the boundary of every public operation are:
//!
//! * Every leaf chunk contains at most [`MAX_LEAF_BYTES`] bytes. Small leaves
//!   produced by concatenation are greedily merged so that most leaves end up
//!   near [`TARGET_LEAF_BYTES`].
//! * Every leaf split occurs on a UTF-8 character boundary.
//! * The tree is self-balancing: whenever a root grows deeper than
//!   [`REBALANCE_THRESHOLD`], it is rebuilt in-order from its leaves.

use std::fmt;
use std::iter::FusedIterator;
use std::ops::{Add, AddAssign, Range};
use std::sync::Arc;

/// A byte-indexed half-open range `[start, end)` inside a rope.
pub type ByteRange = Range<usize>;

/// Target size of a freshly-built leaf chunk in bytes.
pub const TARGET_LEAF_BYTES: usize = 256;
/// Maximum number of bytes allowed in a single leaf chunk.
pub const MAX_LEAF_BYTES: usize = 512;
/// Depth at which a rope is rebuilt in-place to rebalance.
pub const REBALANCE_THRESHOLD: u16 = 32;

// ---------------------------------------------------------------------------
// Summaries
// ---------------------------------------------------------------------------

/// Metadata aggregated per subtree. Combining summaries is associative so
/// internal nodes can cache it cheaply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextSummary {
    /// Number of UTF-8 bytes in the subtree.
    pub bytes: usize,
    /// Number of Unicode scalar values in the subtree.
    pub chars: usize,
    /// Number of line break (`\n`) bytes in the subtree.
    pub line_breaks: usize,
}

impl TextSummary {
    /// Summarise a contiguous UTF-8 slice.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let mut chars = 0;
        let mut line_breaks = 0;
        for byte in s.as_bytes() {
            // UTF-8 continuation bytes have the top bits `10xxxxxx`.
            // Every other byte starts a new scalar value.
            if (*byte & 0xC0) != 0x80 {
                chars += 1;
            }
            if *byte == b'\n' {
                line_breaks += 1;
            }
        }
        Self {
            bytes: s.len(),
            chars,
            line_breaks,
        }
    }
}

impl Add for TextSummary {
    type Output = Self;
    fn add(mut self, rhs: Self) -> Self {
        self += rhs;
        self
    }
}

impl AddAssign for TextSummary {
    fn add_assign(&mut self, rhs: Self) {
        self.bytes += rhs.bytes;
        self.chars += rhs.chars;
        self.line_breaks += rhs.line_breaks;
    }
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Node {
    Leaf(Leaf),
    Branch(Branch),
}

#[derive(Debug)]
struct Leaf {
    text: String,
    summary: TextSummary,
}

#[derive(Debug)]
struct Branch {
    left: Arc<Node>,
    right: Arc<Node>,
    summary: TextSummary,
    depth: u16,
}

impl Node {
    fn summary(&self) -> &TextSummary {
        match self {
            Node::Leaf(l) => &l.summary,
            Node::Branch(b) => &b.summary,
        }
    }

    fn bytes(&self) -> usize {
        self.summary().bytes
    }

    fn depth(&self) -> u16 {
        match self {
            Node::Leaf(_) => 0,
            Node::Branch(b) => b.depth,
        }
    }

    fn is_empty(&self) -> bool {
        self.bytes() == 0
    }

    fn leaf_from_string(text: String) -> Self {
        let summary = TextSummary::from_str(&text);
        Node::Leaf(Leaf { text, summary })
    }

    fn leaf_from_str(text: &str) -> Self {
        Self::leaf_from_string(text.to_string())
    }

    fn empty() -> Self {
        Node::Leaf(Leaf {
            text: String::new(),
            summary: TextSummary::default(),
        })
    }
}

fn branch_of(left: Arc<Node>, right: Arc<Node>) -> Arc<Node> {
    let summary = *left.summary() + *right.summary();
    let depth = left.depth().max(right.depth()) + 1;
    Arc::new(Node::Branch(Branch {
        left,
        right,
        summary,
        depth,
    }))
}

/// Join two subtrees. If both are tiny leaves they are merged into one.
fn concat_nodes(left: Arc<Node>, right: Arc<Node>) -> Arc<Node> {
    if left.is_empty() {
        return right;
    }
    if right.is_empty() {
        return left;
    }
    if let (Node::Leaf(l), Node::Leaf(r)) = (&*left, &*right) {
        if l.text.len() + r.text.len() <= MAX_LEAF_BYTES {
            let mut combined = String::with_capacity(l.text.len() + r.text.len());
            combined.push_str(&l.text);
            combined.push_str(&r.text);
            return Arc::new(Node::leaf_from_string(combined));
        }
    }
    branch_of(left, right)
}

/// Split a subtree at a byte offset. Both halves preserve structural
/// sharing with the original where possible.
fn split_node(node: &Arc<Node>, offset: usize) -> (Arc<Node>, Arc<Node>) {
    debug_assert!(offset <= node.bytes(), "split offset out of bounds");
    match &**node {
        Node::Leaf(l) => {
            if offset == 0 {
                return (Arc::new(Node::empty()), node.clone());
            }
            if offset == l.text.len() {
                return (node.clone(), Arc::new(Node::empty()));
            }
            assert!(
                l.text.is_char_boundary(offset),
                "split offset {offset} is not on a UTF-8 char boundary"
            );
            let (a, b) = l.text.split_at(offset);
            (
                Arc::new(Node::leaf_from_str(a)),
                Arc::new(Node::leaf_from_str(b)),
            )
        }
        Node::Branch(b) => {
            let left_bytes = b.left.bytes();
            match offset.cmp(&left_bytes) {
                std::cmp::Ordering::Less => {
                    let (ll, lr) = split_node(&b.left, offset);
                    let right = concat_nodes(lr, b.right.clone());
                    (ll, right)
                }
                std::cmp::Ordering::Equal => (b.left.clone(), b.right.clone()),
                std::cmp::Ordering::Greater => {
                    let (rl, rr) = split_node(&b.right, offset - left_bytes);
                    let left = concat_nodes(b.left.clone(), rl);
                    (left, rr)
                }
            }
        }
    }
}

/// Walk to the character at a given byte offset. Used for character counting
/// queries against an arbitrary offset.
fn byte_to_char(node: &Node, offset: usize) -> usize {
    match node {
        Node::Leaf(l) => {
            assert!(
                l.text.is_char_boundary(offset),
                "byte_to_char: offset {offset} not on char boundary"
            );
            l.text[..offset].chars().count()
        }
        Node::Branch(b) => {
            let left_bytes = b.left.bytes();
            if offset <= left_bytes {
                byte_to_char(&b.left, offset)
            } else {
                b.left.summary().chars + byte_to_char(&b.right, offset - left_bytes)
            }
        }
    }
}

/// Walk to the line break count at a given byte offset, i.e. the 0-indexed
/// line number containing that byte (line N means "after N newlines").
fn byte_to_line(node: &Node, offset: usize) -> usize {
    match node {
        Node::Leaf(l) => l.text[..offset].bytes().filter(|&b| b == b'\n').count(),
        Node::Branch(b) => {
            let left_bytes = b.left.bytes();
            if offset <= left_bytes {
                byte_to_line(&b.left, offset)
            } else {
                b.left.summary().line_breaks + byte_to_line(&b.right, offset - left_bytes)
            }
        }
    }
}

/// Inverse of [`byte_to_line`]: return the byte offset where `line` starts.
fn line_to_byte(node: &Node, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    match node {
        Node::Leaf(l) => {
            let mut seen = 0;
            for (i, b) in l.text.bytes().enumerate() {
                if b == b'\n' {
                    seen += 1;
                    if seen == line {
                        return i + 1;
                    }
                }
            }
            // Requested line beyond the leaf: return total length (clamped).
            l.text.len()
        }
        Node::Branch(b) => {
            let left_breaks = b.left.summary().line_breaks;
            if line <= left_breaks {
                line_to_byte(&b.left, line)
            } else {
                b.left.bytes() + line_to_byte(&b.right, line - left_breaks)
            }
        }
    }
}

fn collect_leaves(node: &Arc<Node>, out: &mut Vec<Arc<Node>>) {
    match &**node {
        Node::Leaf(l) => {
            if !l.text.is_empty() {
                out.push(node.clone());
            }
        }
        Node::Branch(b) => {
            collect_leaves(&b.left, out);
            collect_leaves(&b.right, out);
        }
    }
}

/// Build a balanced binary tree from an ordered list of leaves.
fn build_balanced(mut leaves: Vec<Arc<Node>>) -> Arc<Node> {
    if leaves.is_empty() {
        return Arc::new(Node::empty());
    }
    while leaves.len() > 1 {
        let mut next = Vec::with_capacity(leaves.len().div_ceil(2));
        let mut iter = leaves.into_iter();
        while let Some(left) = iter.next() {
            if let Some(right) = iter.next() {
                next.push(branch_of(left, right));
            } else {
                next.push(left);
            }
        }
        leaves = next;
    }
    leaves.into_iter().next().unwrap()
}

/// Break a string into char-boundary-aligned chunks sized near
/// [`TARGET_LEAF_BYTES`].
fn chunk_string(text: &str) -> Vec<Arc<Node>> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::with_capacity(text.len() / TARGET_LEAF_BYTES + 1);
    let bytes = text.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let mut end = (start + TARGET_LEAF_BYTES).min(bytes.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        // A single scalar value is at most 4 bytes, so `end > start` must hold.
        debug_assert!(end > start, "chunk_string: failed to advance");
        chunks.push(Arc::new(Node::leaf_from_str(&text[start..end])));
        start = end;
    }
    chunks
}

// ---------------------------------------------------------------------------
// Public rope
// ---------------------------------------------------------------------------

/// An immutable, copy-on-write rope.
///
/// Cloning a rope is `O(1)` (a single `Arc` bump). All mutating operations
/// return a new rope that shares most of its structure with the original.
#[derive(Clone)]
pub struct Rope {
    root: Arc<Node>,
}

impl Rope {
    /// Create an empty rope.
    pub fn new() -> Self {
        Self {
            root: Arc::new(Node::empty()),
        }
    }

    /// Create a balanced rope from a string slice.
    ///
    /// Named `from_str` to match the spec's pseudocode. It is infallible and
    /// therefore intentionally does *not* implement [`std::str::FromStr`],
    /// which would force a `Result` return type for no reason.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        if text.is_empty() {
            return Self::new();
        }
        let leaves = chunk_string(text);
        Self {
            root: build_balanced(leaves),
        }
    }

    /// Total byte length.
    pub fn len_bytes(&self) -> usize {
        self.root.bytes()
    }

    /// Total number of Unicode scalar values.
    pub fn len_chars(&self) -> usize {
        self.root.summary().chars
    }

    /// Total number of lines. An empty rope has one line.
    pub fn len_lines(&self) -> usize {
        self.root.summary().line_breaks + 1
    }

    /// Return the cached summary of the entire rope.
    pub fn summary(&self) -> TextSummary {
        *self.root.summary()
    }

    /// Return true if the rope contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }

    /// Split the rope at a byte offset. Both halves share structure with
    /// `self`.
    pub fn split(&self, byte_offset: usize) -> (Rope, Rope) {
        assert!(
            byte_offset <= self.len_bytes(),
            "split offset out of bounds"
        );
        let (l, r) = split_node(&self.root, byte_offset);
        (
            Rope { root: l }.maybe_rebalance(),
            Rope { root: r }.maybe_rebalance(),
        )
    }

    /// Concatenate another rope to the end of this one.
    pub fn concat(self, other: Rope) -> Rope {
        Rope {
            root: concat_nodes(self.root, other.root),
        }
        .maybe_rebalance()
    }

    /// Replace the bytes in `range` with `text`, returning a new rope.
    pub fn edit(&self, range: ByteRange, text: &str) -> Rope {
        let len = self.len_bytes();
        assert!(range.start <= range.end, "inverted edit range");
        assert!(range.end <= len, "edit range out of bounds");
        let (left, rest) = self.split(range.start);
        let (_, right) = rest.split(range.end - range.start);
        left.concat(Rope::from_str(text)).concat(right)
    }

    /// Byte offset → line number (0-indexed).
    pub fn byte_to_line(&self, byte: usize) -> usize {
        assert!(byte <= self.len_bytes(), "byte offset out of bounds");
        byte_to_line(&self.root, byte)
    }

    /// Byte offset → character index.
    pub fn byte_to_char(&self, byte: usize) -> usize {
        assert!(byte <= self.len_bytes(), "byte offset out of bounds");
        byte_to_char(&self.root, byte)
    }

    /// Line number → byte offset of the line's first byte.
    ///
    /// For `line == len_lines()` this returns [`Rope::len_bytes`]. Requests
    /// past that saturate at the end of the rope.
    pub fn line_to_byte(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        if line >= self.len_lines() {
            return self.len_bytes();
        }
        line_to_byte(&self.root, line)
    }

    /// Return the full rope as a newly-allocated `String`.
    pub fn as_string(&self) -> String {
        let mut out = String::with_capacity(self.len_bytes());
        for chunk in self.chunks() {
            out.push_str(chunk);
        }
        out
    }

    /// Extract a byte range as a newly-allocated `String`.
    pub fn slice_to_string(&self, range: ByteRange) -> String {
        assert!(
            range.start <= range.end && range.end <= self.len_bytes(),
            "slice range out of bounds"
        );
        let (_, rest) = self.split(range.start);
        let (mid, _) = rest.split(range.end - range.start);
        mid.as_string()
    }

    /// Iterator over leaf chunks in order.
    pub fn chunks(&self) -> Chunks<'_> {
        Chunks::new(&self.root)
    }

    /// Iterator over bytes.
    pub fn bytes(&self) -> Bytes<'_> {
        Bytes {
            chunks: self.chunks(),
            current: "".as_bytes(),
            idx: 0,
        }
    }

    /// Iterator over Unicode scalar values.
    pub fn chars(&self) -> Chars<'_> {
        Chars {
            chunks: self.chunks(),
            iter: "".chars(),
        }
    }

    /// Iterator over 0-indexed lines. Line separators are included in each
    /// yielded line string.
    pub fn lines(&self) -> Lines<'_> {
        Lines {
            rope: self,
            next_byte: 0,
            total: self.len_bytes(),
        }
    }

    /// Tree depth — useful for tests and diagnostics.
    pub fn depth(&self) -> u16 {
        self.root.depth()
    }

    fn maybe_rebalance(self) -> Rope {
        if self.root.depth() > REBALANCE_THRESHOLD {
            self.rebalance()
        } else {
            self
        }
    }

    /// Rebuild the rope as a fully-balanced tree.
    pub fn rebalance(self) -> Rope {
        let mut leaves = Vec::new();
        collect_leaves(&self.root, &mut leaves);
        // Greedily merge tiny adjacent leaves so balanced rebuilds tighten
        // the tree even further.
        let mut merged: Vec<Arc<Node>> = Vec::with_capacity(leaves.len());
        for leaf in leaves {
            if let Some(back) = merged.last_mut() {
                if let (Node::Leaf(a), Node::Leaf(b)) = (&**back, &*leaf) {
                    if a.text.len() + b.text.len() <= TARGET_LEAF_BYTES {
                        let mut combined = String::with_capacity(a.text.len() + b.text.len());
                        combined.push_str(&a.text);
                        combined.push_str(&b.text);
                        *back = Arc::new(Node::leaf_from_string(combined));
                        continue;
                    }
                }
            }
            merged.push(leaf);
        }
        Rope {
            root: build_balanced(merged),
        }
    }
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
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

impl fmt::Debug for Rope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rope")
            .field("bytes", &self.len_bytes())
            .field("lines", &self.len_lines())
            .field("depth", &self.root.depth())
            .finish()
    }
}

impl PartialEq for Rope {
    fn eq(&self, other: &Self) -> bool {
        if self.len_bytes() != other.len_bytes() {
            return false;
        }
        let mut a = self.bytes();
        let mut b = other.bytes();
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) if x == y => {}
                _ => return false,
            }
        }
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

// ---------------------------------------------------------------------------
// Iterators
// ---------------------------------------------------------------------------

/// In-order iterator over leaf chunks.
#[derive(Debug, Clone)]
pub struct Chunks<'a> {
    stack: Vec<&'a Node>,
}

impl<'a> Chunks<'a> {
    fn new(root: &'a Arc<Node>) -> Self {
        let mut stack = Vec::new();
        if !root.is_empty() {
            stack.push(&**root);
        }
        Self { stack }
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        while let Some(node) = self.stack.pop() {
            match node {
                Node::Leaf(l) => {
                    if !l.text.is_empty() {
                        return Some(&l.text);
                    }
                }
                Node::Branch(b) => {
                    // Push right first so left is visited first.
                    self.stack.push(&b.right);
                    self.stack.push(&b.left);
                }
            }
        }
        None
    }
}

impl FusedIterator for Chunks<'_> {}

/// Byte-wise iterator.
#[derive(Debug, Clone)]
pub struct Bytes<'a> {
    chunks: Chunks<'a>,
    current: &'a [u8],
    idx: usize,
}

impl Iterator for Bytes<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        loop {
            if self.idx < self.current.len() {
                let b = self.current[self.idx];
                self.idx += 1;
                return Some(b);
            }
            let next = self.chunks.next()?;
            self.current = next.as_bytes();
            self.idx = 0;
        }
    }
}

impl FusedIterator for Bytes<'_> {}

/// Char-wise iterator.
#[derive(Debug, Clone)]
pub struct Chars<'a> {
    chunks: Chunks<'a>,
    iter: std::str::Chars<'a>,
}

impl Iterator for Chars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            if let Some(c) = self.iter.next() {
                return Some(c);
            }
            self.iter = self.chunks.next()?.chars();
        }
    }
}

impl FusedIterator for Chars<'_> {}

/// Line iterator returning owned `String`s. Each yielded line keeps its
/// trailing newline unless it is the final line of the rope without one.
#[derive(Debug)]
pub struct Lines<'a> {
    rope: &'a Rope,
    next_byte: usize,
    total: usize,
}

impl Iterator for Lines<'_> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        if self.next_byte > self.total {
            return None;
        }
        if self.next_byte == self.total {
            // Emit the (possibly empty) tail exactly once.
            self.next_byte = self.total + 1;
            return Some(String::new());
        }
        // Find the next newline starting at `next_byte`.
        let start = self.next_byte;
        let mut cursor = start;
        let mut found_newline = false;
        for b in self.rope.bytes().skip(start) {
            cursor += 1;
            if b == b'\n' {
                found_newline = true;
                break;
            }
        }
        let end = cursor;
        let line = self.rope.slice_to_string(start..end);
        if found_newline {
            self.next_byte = end;
        } else {
            // No newline means we just consumed the final partial line;
            // mark iteration finished.
            self.next_byte = self.total + 1;
        }
        Some(line)
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
        assert_eq!(r.to_string(), "");
    }

    #[test]
    fn small_from_str() {
        let r = Rope::from_str("hello");
        assert_eq!(r.len_bytes(), 5);
        assert_eq!(r.len_chars(), 5);
        assert_eq!(r.len_lines(), 1);
        assert_eq!(r.to_string(), "hello");
    }

    #[test]
    fn multi_byte_chars() {
        let r = Rope::from_str("héllo 🦀");
        assert_eq!(r.to_string(), "héllo 🦀");
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
        assert_eq!(r.to_string(), "hello world");
        assert_eq!(r2.to_string(), "hello rope!");
    }

    #[test]
    fn edit_insert() {
        let r = Rope::from_str("ac");
        let r2 = r.edit(1..1, "b");
        assert_eq!(r2.to_string(), "abc");
    }

    #[test]
    fn edit_delete() {
        let r = Rope::from_str("abcdef");
        let r2 = r.edit(2..4, "");
        assert_eq!(r2.to_string(), "abef");
    }

    #[test]
    fn edit_empty_rope() {
        let r = Rope::new();
        let r2 = r.edit(0..0, "hello");
        assert_eq!(r2.to_string(), "hello");
    }

    #[test]
    fn large_rope_roundtrip() {
        let text: String = (0..5_000).map(|i| format!("line {i}\n")).collect();
        let r = Rope::from_str(&text);
        assert_eq!(r.len_bytes(), text.len());
        assert_eq!(r.len_lines(), text.matches('\n').count() + 1);
        assert_eq!(r.to_string(), text);

        // Random-ish edit.
        let mid = text.len() / 2;
        let r2 = r.edit(mid..mid, "INSERT ");
        assert_eq!(r2.len_bytes(), text.len() + "INSERT ".len());
        let s = r2.to_string();
        assert!(s.contains("INSERT "));
    }

    #[test]
    fn many_edits_rebalance() {
        let mut r = Rope::from_str("start");
        for i in 0..2_000 {
            let len = r.len_bytes();
            r = r.edit(len..len, &format!("[{i}]"));
        }
        assert!(r.to_string().starts_with("start[0][1]"));
        assert!(r.to_string().ends_with("[1999]"));
        // Rebalance threshold keeps depth bounded.
        assert!(r.depth() <= REBALANCE_THRESHOLD + 1);
    }

    #[test]
    fn split_and_concat_roundtrip() {
        let text = "the quick brown fox jumps over the lazy dog";
        let r = Rope::from_str(text);
        for i in 0..=text.len() {
            let (a, b) = r.split(i);
            let joined = a.concat(b).to_string();
            assert_eq!(joined, text, "split/concat mismatch at {i}");
        }
    }

    #[test]
    fn snapshot_independence() {
        let r1 = Rope::from_str("abc");
        let r2 = r1.edit(1..1, "XYZ");
        assert_eq!(r1.to_string(), "abc");
        assert_eq!(r2.to_string(), "aXYZbc");
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
}
