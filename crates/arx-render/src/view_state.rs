//! Immutable snapshot of what the editor wants to display.
//!
//! [`ViewState`] is a pure-data description of a frame: which buffers are
//! visible, where each cursor sits, how the screen is partitioned. The
//! view layer ([`crate::view::render`]) is a *pure function* from a
//! [`ViewState`] to a [`crate::RenderTree`], which makes the whole
//! rendering pipeline testable without a terminal.
//!
//! Spec §4.2. Phase 2 adds split layouts: [`LayoutTree::Split`] is now
//! actually rendered and [`LayoutTree::walk_pane_rects`] is the single
//! place that decides how a bounding rect is partitioned into
//! per-window rectangles. Both the pure view renderer and the driver's
//! viewport-size writeback call it so there's no way for them to
//! disagree about how much space each pane gets.

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
    /// Buffer window states referenced by the layout tree.
    pub windows: Vec<WindowState>,
    /// Terminal pane states referenced by the layout tree.
    pub terminal_panes: Vec<TerminalViewState>,
    /// Which window is the focused one. The view layer paints the
    /// terminal cursor for this window only, so inactive panes show
    /// their text without a cursor. `None` means no window is
    /// focused — common during bootstrap before a buffer is opened.
    pub active_window: Option<WindowId>,
    /// Global editor state (active mode, status text, etc.).
    pub global: GlobalState,
}

/// A rectangle on the terminal grid. `width` / `height` of 0 mean the
/// pane has been squeezed out of visibility and should not be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Is this rect degenerate (zero width or zero height)?
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
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
/// The layout is a binary tree whose leaves are [`WindowId`]s. Phase 2
/// wired this up so [`LayoutTree::Split`] is actually rendered — the
/// walkers below are the single source of truth for how a bounding
/// rectangle is divided across the tree.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutTree {
    /// A single window fills the entire viewport.
    Single(WindowId),
    /// A split of two child layouts along one axis. The `ratio` is the
    /// fraction of the parent region allocated to `first`; the remainder
    /// (minus one cell for the divider) goes to `second`.
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<LayoutTree>,
        second: Box<LayoutTree>,
    },
}

/// Orientation of a [`LayoutTree::Split`].
///
/// Follows Vim terminology:
///
/// * [`SplitDirection::Horizontal`] — horizontal divider line; children
///   stacked **top-to-bottom**. Vim `:split`.
/// * [`SplitDirection::Vertical`] — vertical divider line; children
///   placed **side-by-side**. Vim `:vsplit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

impl LayoutTree {
    /// Invoke `f` once per leaf with the `(WindowId, Rect)` describing
    /// where that window should be painted inside `rect`.
    ///
    /// The walker is the shared source of truth for pane geometry: both
    /// [`crate::view::render`] and the driver's viewport writeback call
    /// it so there's no way for them to disagree about how much space a
    /// pane gets, even under odd ratios or very small terminals.
    pub fn walk_pane_rects(&self, rect: Rect, f: &mut impl FnMut(WindowId, Rect)) {
        if rect.is_empty() {
            return;
        }
        match self {
            LayoutTree::Single(id) => f(*id, rect),
            LayoutTree::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (r1, r2, _divider) = split_rect(rect, *direction, *ratio);
                first.walk_pane_rects(r1, f);
                second.walk_pane_rects(r2, f);
            }
        }
    }

    /// Invoke `f` once per internal [`LayoutTree::Split`] with the
    /// divider's rectangle (a 1-cell-wide vertical strip for a vertical
    /// split, or 1-cell-high horizontal strip for a horizontal split).
    ///
    /// Used by the view layer to paint separator glyphs between panes.
    pub fn walk_divider_rects(
        &self,
        rect: Rect,
        f: &mut impl FnMut(Rect, SplitDirection),
    ) {
        if rect.is_empty() {
            return;
        }
        match self {
            LayoutTree::Single(_) => {}
            LayoutTree::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (r1, r2, divider) = split_rect(rect, *direction, *ratio);
                if let Some(d) = divider {
                    f(d, *direction);
                }
                first.walk_divider_rects(r1, f);
                second.walk_divider_rects(r2, f);
            }
        }
    }
}

/// Partition `rect` into two child rects with a 1-cell divider between
/// them. Returns `(first, second, divider)`; `divider` is `None` when
/// there isn't room for one (a degenerately small parent rect), in
/// which case the caller should skip painting a separator glyph.
///
/// The ratio is clamped to `[0.05, 0.95]` so neither child is crushed
/// to zero. When the parent is smaller than three cells along the
/// split axis, the divider is dropped and the children share the
/// remaining space as best they can.
#[must_use]
fn split_rect(
    rect: Rect,
    direction: SplitDirection,
    ratio: f32,
) -> (Rect, Rect, Option<Rect>) {
    let clamped = ratio.clamp(0.05, 0.95);
    match direction {
        SplitDirection::Vertical => {
            // Panes side-by-side; divider is one column wide.
            let total = rect.width;
            if total == 0 {
                return (
                    Rect::new(rect.x, rect.y, 0, rect.height),
                    Rect::new(rect.x, rect.y, 0, rect.height),
                    None,
                );
            }
            if total < 3 {
                // No room for a divider; give first the left half,
                // second the rest.
                let left_w = total / 2;
                let right_w = total - left_w;
                return (
                    Rect::new(rect.x, rect.y, left_w, rect.height),
                    Rect::new(rect.x + left_w, rect.y, right_w, rect.height),
                    None,
                );
            }
            let available = total - 1;
            let mut left_w = ((f32::from(available)) * clamped).round() as u16;
            left_w = left_w.clamp(1, available - 1);
            let right_w = available - left_w;
            let divider = Rect::new(rect.x + left_w, rect.y, 1, rect.height);
            (
                Rect::new(rect.x, rect.y, left_w, rect.height),
                Rect::new(rect.x + left_w + 1, rect.y, right_w, rect.height),
                Some(divider),
            )
        }
        SplitDirection::Horizontal => {
            // Panes stacked; divider is one row tall.
            let total = rect.height;
            if total == 0 {
                return (
                    Rect::new(rect.x, rect.y, rect.width, 0),
                    Rect::new(rect.x, rect.y, rect.width, 0),
                    None,
                );
            }
            if total < 3 {
                let top_h = total / 2;
                let bottom_h = total - top_h;
                return (
                    Rect::new(rect.x, rect.y, rect.width, top_h),
                    Rect::new(rect.x, rect.y + top_h, rect.width, bottom_h),
                    None,
                );
            }
            let available = total - 1;
            let mut top_h = ((f32::from(available)) * clamped).round() as u16;
            top_h = top_h.clamp(1, available - 1);
            let bottom_h = available - top_h;
            let divider = Rect::new(rect.x, rect.y + top_h, rect.width, 1);
            (
                Rect::new(rect.x, rect.y, rect.width, top_h),
                Rect::new(rect.x, rect.y + top_h + 1, rect.width, bottom_h),
                Some(divider),
            )
        }
    }
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
    /// Selection region (mark..cursor or cursor..mark) as a byte
    /// range in the buffer. `None` when no mark is set.
    pub selection: Option<std::ops::Range<usize>>,
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
    /// Command palette overlay state. `None` when the palette is
    /// closed (the common case); `Some(...)` means the view layer
    /// should paint it as a bottom overlay and the caller should
    /// reserve the corresponding rows before drawing the primary
    /// window.
    pub palette: Option<PaletteView>,
    /// Completion popup overlay state. `None` when no completion is
    /// in progress. `Some(...)` means the view layer should paint a
    /// popup near the cursor.
    pub completion: Option<CompletionView>,
    /// Which-key overlay: shows available completions for a pending
    /// key prefix. `None` when no prefix is pending or the timeout
    /// hasn't fired yet.
    pub which_key: Option<Vec<WhichKeyEntry>>,
    /// Interactive buffer search overlay. `None` when search is not
    /// active; `Some(...)` means the view layer should paint a bottom
    /// overlay similar to the palette.
    pub search: Option<SearchView>,
}

/// One entry in the which-key overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhichKeyEntry {
    /// The next key to press (e.g. `"C-s"`).
    pub key: String,
    /// The command it resolves to (e.g. `"buffer.save"`) or
    /// `"+prefix"` if it's another prefix level.
    pub command: String,
}

/// What the view layer needs to draw a completion popup. A direct
/// projection of `arx_core::CompletionPopup` flattened into
/// display-friendly types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionView {
    /// The completion items.
    pub items: Vec<CompletionEntry>,
    /// Index into `items` for the highlighted row.
    pub selected: usize,
    /// Maximum number of rows to draw.
    pub max_rows: u16,
    /// Column where the popup should be anchored (typically the
    /// cursor column at the time completion was triggered).
    pub anchor_col: u16,
    /// Row where the popup should start (typically just below the
    /// cursor row).
    pub anchor_row: u16,
}

/// One row in the completion popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEntry {
    /// The label displayed in the popup.
    pub label: String,
    /// Optional detail shown beside the label.
    pub detail: String,
    /// Kind indicator (`"fn"`, `"var"`, ...).
    pub kind: String,
}

/// What the view layer needs to know to draw a command-palette
/// overlay. A direct projection of `arx_core::CommandPalette`
/// flattened into display-friendly types so `arx-render` doesn't
/// depend on `arx-core`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteView {
    /// Prompt text shown before the query (e.g. `"M-x "` or
    /// `"Find file: "`).
    pub prompt: String,
    /// Current query text shown in the prompt line.
    pub query: String,
    /// Filtered, scored, sorted match list.
    pub matches: Vec<PaletteEntry>,
    /// Index into `matches` for the highlighted row.
    pub selected: usize,
    /// Maximum number of match rows to draw. The render layer caps
    /// `matches` at this figure when the list is longer; the caller
    /// supplies the cap so it can reserve the matching number of
    /// viewport rows for the overlay.
    pub max_rows: u16,
}

/// What the view layer needs to draw the interactive buffer search
/// overlay. Similar to [`PaletteView`] but with line-number metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchView {
    /// Prompt text: e.g. `"Search (fuzzy): "`.
    pub prompt: String,
    /// Current query text.
    pub query: String,
    /// Matching lines from the buffer.
    pub matches: Vec<SearchEntry>,
    /// Index into `matches` for the highlighted row.
    pub selected: usize,
    /// Maximum number of match rows to draw.
    pub max_rows: u16,
    /// Total number of matches (may be larger than `matches.len()`
    /// if capped). Displayed as "N matches" in the prompt area.
    pub total_matches: usize,
}

/// One entry in the search overlay's match list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEntry {
    /// 0-based line number in the buffer.
    pub line_number: usize,
    /// The line text (may be truncated for display).
    pub line_text: String,
}

// ---------------------------------------------------------------------------
// Terminal pane view state
// ---------------------------------------------------------------------------

/// View-layer snapshot of an embedded terminal pane.
#[derive(Debug, Clone)]
pub struct TerminalViewState {
    /// Window id this terminal occupies.
    pub id: WindowId,
    /// Row-major grid of cells.
    pub cells: Vec<Vec<TerminalViewCell>>,
    /// Cursor position (col, row), or `None` if hidden.
    pub cursor: Option<(u16, u16)>,
    pub cols: u16,
    pub rows: u16,
}

/// A single cell in a terminal pane's grid.
#[derive(Debug, Clone)]
pub struct TerminalViewCell {
    pub c: String,
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// One row in the palette's match list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    /// Command name (`"cursor.word-forward"`).
    pub name: String,
    /// Human description, possibly empty.
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: u64) -> LayoutTree {
        LayoutTree::Single(WindowId(id))
    }

    fn split(
        direction: SplitDirection,
        ratio: f32,
        first: LayoutTree,
        second: LayoutTree,
    ) -> LayoutTree {
        LayoutTree::Split {
            direction,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    #[test]
    fn vertical_split_divides_cols_with_one_cell_divider() {
        let layout = split(SplitDirection::Vertical, 0.5, leaf(1), leaf(2));
        let mut rects: Vec<(WindowId, Rect)> = Vec::new();
        layout.walk_pane_rects(Rect::new(0, 0, 21, 10), &mut |id, rect| {
            rects.push((id, rect));
        });
        assert_eq!(rects.len(), 2);
        // Available after divider = 20, half = 10.
        assert_eq!(rects[0].1, Rect::new(0, 0, 10, 10));
        assert_eq!(rects[1].1, Rect::new(11, 0, 10, 10));

        let mut dividers: Vec<(Rect, SplitDirection)> = Vec::new();
        layout.walk_divider_rects(Rect::new(0, 0, 21, 10), &mut |r, d| {
            dividers.push((r, d));
        });
        assert_eq!(dividers.len(), 1);
        assert_eq!(dividers[0].0, Rect::new(10, 0, 1, 10));
        assert_eq!(dividers[0].1, SplitDirection::Vertical);
    }

    #[test]
    fn horizontal_split_divides_rows_with_one_cell_divider() {
        let layout = split(SplitDirection::Horizontal, 0.5, leaf(1), leaf(2));
        let mut rects: Vec<(WindowId, Rect)> = Vec::new();
        layout.walk_pane_rects(Rect::new(0, 0, 20, 11), &mut |id, rect| {
            rects.push((id, rect));
        });
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].1, Rect::new(0, 0, 20, 5));
        assert_eq!(rects[1].1, Rect::new(0, 6, 20, 5));
    }

    #[test]
    fn nested_splits_cover_every_cell() {
        let layout = split(
            SplitDirection::Vertical,
            0.5,
            leaf(1),
            split(SplitDirection::Horizontal, 0.5, leaf(2), leaf(3)),
        );
        let rect = Rect::new(0, 0, 21, 11);
        let mut total_area: u32 = 0;
        layout.walk_pane_rects(rect, &mut |_, r| {
            total_area += u32::from(r.width) * u32::from(r.height);
        });
        let mut divider_area: u32 = 0;
        layout.walk_divider_rects(rect, &mut |r, _| {
            divider_area += u32::from(r.width) * u32::from(r.height);
        });
        assert_eq!(total_area + divider_area, 21 * 11);
    }

    #[test]
    fn tiny_rect_collapses_without_divider() {
        let layout = split(SplitDirection::Vertical, 0.5, leaf(1), leaf(2));
        let mut panes = 0;
        layout.walk_pane_rects(Rect::new(0, 0, 2, 5), &mut |_, _| panes += 1);
        assert_eq!(panes, 2);

        let mut dividers = 0;
        layout.walk_divider_rects(Rect::new(0, 0, 2, 5), &mut |_, _| dividers += 1);
        assert_eq!(dividers, 0);
    }

    #[test]
    fn walk_skips_empty_rect() {
        let layout = split(SplitDirection::Vertical, 0.5, leaf(1), leaf(2));
        let mut any = false;
        layout.walk_pane_rects(Rect::new(0, 0, 0, 10), &mut |_, _| any = true);
        assert!(!any);
    }
}
