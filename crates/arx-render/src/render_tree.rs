//! Output of the view layer — the thing the differ and backend consume.
//!
//! A [`RenderTree`] is just a [`CellGrid`] plus cursor metadata and a
//! monotonic frame id. The spec (§4.3) also includes inline content
//! (images, widgets) and floating panels — those slot in here later.

use smallvec::SmallVec;

use crate::cell::CellGrid;

/// What the backend needs to draw the current frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderTree {
    /// The cell grid to paint.
    pub cells: CellGrid,
    /// Cursors the backend should position / style.
    pub cursors: SmallVec<[CursorRender; 1]>,
    /// Monotonic frame identifier. Backends may use this to correlate
    /// frames with input events; the differ ignores it.
    pub frame_id: u64,
}

impl RenderTree {
    pub fn new(cells: CellGrid, cursors: SmallVec<[CursorRender; 1]>, frame_id: u64) -> Self {
        Self {
            cells,
            cursors,
            frame_id,
        }
    }

    /// An empty `width × height` render tree with the default blank grid.
    pub fn blank(width: u16, height: u16, frame_id: u64) -> Self {
        Self {
            cells: CellGrid::new(width, height),
            cursors: SmallVec::new(),
            frame_id,
        }
    }
}

/// Where and how to render a cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CursorRender {
    /// Column (0-indexed from the left of the terminal).
    pub col: u16,
    /// Row (0-indexed from the top of the terminal).
    pub row: u16,
    /// Visual style the backend should apply.
    pub style: CursorStyle,
}

/// Cursor visual style.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub enum CursorStyle {
    /// Solid block (default for normal/insert modes).
    #[default]
    Block,
    /// Thin vertical bar (a convention for insert mode in vim-like modes).
    Bar,
    /// Underscore.
    Underline,
}
