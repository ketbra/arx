//! Immutable snapshot of what the editor wants to display.
//!
//! [`ViewState`] is a pure-data description of a frame: which buffers are
//! visible, where each cursor sits, how the screen is partitioned. The
//! view layer ([`crate::view::render`]) is a *pure function* from a
//! [`ViewState`] to a [`crate::RenderTree`], which makes the whole
//! rendering pipeline testable without a terminal.
//!
//! Spec §4.2. For Phase 1 we only model a single-window layout — splits,
//! overlays, floating panels, modelines with rich widgets all land in
//! later milestones.

use arx_buffer::BufferSnapshot;
use smallvec::SmallVec;

// ---------------------------------------------------------------------------
// ViewState
// ---------------------------------------------------------------------------

/// Immutable description of a frame to render.
#[derive(Debug, Clone)]
pub struct ViewState {
    /// Overall terminal size in cells.
    pub size: TerminalSize,
    /// Layout tree describing how `size` is partitioned.
    pub layout: LayoutTree,
    /// The actual window states referenced by the layout tree.
    pub windows: Vec<WindowState>,
    /// Global editor state (active mode, status text, etc.).
    pub global: GlobalState,
}

/// Terminal size in cells. `cols × rows`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSize {
    pub const fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// How the terminal is partitioned into windows.
///
/// Phase 1 only supports the single-window case. `Split` is sketched here
/// so follow-up commits can drop in multi-pane layouts without changing
/// the view-layer API.
#[derive(Debug, Clone)]
pub enum LayoutTree {
    /// A single window fills the entire viewport.
    Single(WindowId),
    /// A split of two child layouts (sketch — not rendered by
    /// [`crate::view::render`] yet).
    #[allow(dead_code)]
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<LayoutTree>,
        second: Box<LayoutTree>,
    },
}

/// Orientation of a [`LayoutTree::Split`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// Stable identifier for a window inside a [`ViewState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// State for a single window.
#[derive(Debug, Clone)]
pub struct WindowState {
    pub id: WindowId,
    pub buffer: BufferSnapshot,
    /// Cursors for this window. At least one; the first is the primary.
    pub cursors: SmallVec<[Cursor; 1]>,
    pub scroll: ScrollPosition,
    pub gutter: GutterConfig,
}

impl WindowState {
    /// Convenience: the primary (first) cursor.
    pub fn primary_cursor(&self) -> &Cursor {
        &self.cursors[0]
    }
}

/// A cursor in a buffer. `byte_offset` is the anchor; the optional
/// `selection_anchor` gives the other end of an active selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub byte_offset: usize,
    pub selection_anchor: Option<usize>,
}

impl Cursor {
    pub const fn at(byte_offset: usize) -> Self {
        Self {
            byte_offset,
            selection_anchor: None,
        }
    }
}

/// Scroll position expressed in buffer coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollPosition {
    /// First *line* visible in the window (0-indexed into the buffer).
    pub top_line: usize,
    /// First *column* visible in the window (character column, 0-indexed).
    /// Phase 1 only supports horizontal scroll by clipping.
    pub left_col: u16,
}

/// What the line gutter renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GutterConfig {
    /// Show the line number beside each row.
    pub line_numbers: bool,
    /// Minimum width reserved for the gutter, in cells (not counting the
    /// single-cell spacer between gutter and text).
    pub min_width: u16,
}

impl Default for GutterConfig {
    fn default() -> Self {
        Self {
            line_numbers: true,
            min_width: 4,
        }
    }
}

// ---------------------------------------------------------------------------
// GlobalState
// ---------------------------------------------------------------------------

/// Editor-wide state visible in the frame (modeline text, current mode).
#[derive(Debug, Clone, Default)]
pub struct GlobalState {
    /// Left-aligned modeline text (Phase 1: simple single-line footer).
    pub modeline_left: String,
    /// Right-aligned modeline text.
    pub modeline_right: String,
}
