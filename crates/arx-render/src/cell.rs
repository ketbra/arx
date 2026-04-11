//! Terminal cells and the 2D grid they live in.
//!
//! A [`Cell`] is one renderable position in the terminal grid. It owns its
//! grapheme cluster (as a [`CompactString`] so tiny graphemes don't
//! allocate), its resolved visual face, and a set of [`CellFlags`] bits
//! that the differ / backend can inspect for fast-paths (cursors, wide
//! continuation, diagnostics hint, etc.).
//!
//! A [`CellGrid`] is a dense row-major array of cells. It's the payload of
//! the [`crate::RenderTree`] and what the differ compares.

use bitflags::bitflags;
use compact_str::CompactString;

use crate::face::ResolvedFace;

bitflags! {
    /// Per-cell flags. Mirrors spec §4.3.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CellFlags: u8 {
        /// This cell is the second half of a wide grapheme; its
        /// `grapheme` is empty and it must not be rendered independently.
        const WIDE_CONTINUATION = 0b0000_0001;
        /// This cell marks a soft-wrap point for the line it's on.
        const WRAP_POINT        = 0b0000_0010;
        /// The primary cursor sits on this cell.
        const CURSOR_PRIMARY    = 0b0000_0100;
        /// A secondary (multi-cursor) cursor sits on this cell.
        const CURSOR_SECONDARY  = 0b0000_1000;
        /// This cell carries a diagnostic hint marker.
        const DIAGNOSTIC_HINT   = 0b0001_0000;
        /// This cell is inside an active search match.
        const SEARCH_MATCH      = 0b0010_0000;
    }
}

/// One renderable cell of the terminal grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The grapheme cluster displayed at this cell. Empty for
    /// [`CellFlags::WIDE_CONTINUATION`] cells.
    pub grapheme: CompactString,
    /// The resolved face for this cell.
    pub face: ResolvedFace,
    /// Non-visual flags.
    pub flags: CellFlags,
}

impl Cell {
    /// A blank cell painted with the default face. Used as the initial
    /// value when allocating a grid and as the fill for trailing space.
    pub fn blank() -> Self {
        Self {
            grapheme: CompactString::const_new(" "),
            face: ResolvedFace::DEFAULT,
            flags: CellFlags::empty(),
        }
    }

    /// An explicit wide-continuation cell. Sits to the right of a
    /// wide grapheme (like a CJK character or an emoji).
    pub fn wide_continuation(face: ResolvedFace) -> Self {
        Self {
            grapheme: CompactString::const_new(""),
            face,
            flags: CellFlags::WIDE_CONTINUATION,
        }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank()
    }
}

/// A dense row-major grid of [`Cell`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellGrid {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl CellGrid {
    /// Allocate a `width × height` grid of [`Cell::blank`] cells.
    pub fn new(width: u16, height: u16) -> Self {
        let size = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            cells: vec![Cell::blank(); size],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Whether the grid has zero area.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// `(x, y) → linear index`, or `None` if out of bounds.
    pub fn index(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some((y as usize) * (self.width as usize) + (x as usize))
    }

    /// Read-only access to a cell. Returns `None` if out of bounds.
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.index(x, y).map(|i| &self.cells[i])
    }

    /// Mutable access to a cell. Returns `None` if out of bounds.
    pub fn get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        let idx = self.index(x, y)?;
        Some(&mut self.cells[idx])
    }

    /// Write `cell` at `(x, y)`. Silently ignores out-of-bounds writes so
    /// render code doesn't have to branch at every cell.
    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(idx) = self.index(x, y) {
            self.cells[idx] = cell;
        }
    }

    /// Iterate over `(x, y, &Cell)` in row-major order.
    pub fn iter(&self) -> impl Iterator<Item = (u16, u16, &Cell)> {
        let w = self.width;
        self.cells
            .iter()
            .enumerate()
            .map(move |(i, c)| ((i as u16) % w, (i as u16) / w, c))
    }

    /// Overwrite the entire grid with [`Cell::blank`].
    pub fn clear(&mut self) {
        for c in &mut self.cells {
            *c = Cell::blank();
        }
    }

    /// Render the grid as a plain-text string for tests / debugging. One
    /// row per line; wide-continuation cells are omitted.
    pub fn to_debug_text(&self) -> String {
        let mut out = String::with_capacity(self.cells.len() + self.height as usize);
        for y in 0..self.height {
            for x in 0..self.width {
                let cell = self.get(x, y).unwrap();
                if cell.flags.contains(CellFlags::WIDE_CONTINUATION) {
                    continue;
                }
                out.push_str(&cell.grapheme);
            }
            if y + 1 < self.height {
                out.push('\n');
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_grid_has_spaces() {
        let g = CellGrid::new(3, 2);
        assert_eq!(g.width(), 3);
        assert_eq!(g.height(), 2);
        assert_eq!(g.to_debug_text(), "   \n   ");
    }

    #[test]
    fn set_and_get_roundtrip() {
        let mut g = CellGrid::new(4, 2);
        let c = Cell {
            grapheme: CompactString::new("X"),
            face: ResolvedFace::DEFAULT,
            flags: CellFlags::empty(),
        };
        g.set(2, 1, c.clone());
        assert_eq!(g.get(2, 1), Some(&c));
        assert_eq!(g.get(0, 0), Some(&Cell::blank()));
    }

    #[test]
    fn out_of_bounds_writes_are_silent() {
        let mut g = CellGrid::new(2, 2);
        g.set(99, 99, Cell::blank());
        assert_eq!(g.to_debug_text(), "  \n  ");
    }

    #[test]
    fn cellflags_default_is_empty() {
        let f = CellFlags::default();
        assert!(f.is_empty());
        assert!(!f.contains(CellFlags::WIDE_CONTINUATION));
    }

    #[test]
    fn wide_continuation_cells_skipped_in_debug() {
        let mut g = CellGrid::new(3, 1);
        g.set(
            0,
            0,
            Cell {
                grapheme: CompactString::new("中"),
                face: ResolvedFace::DEFAULT,
                flags: CellFlags::empty(),
            },
        );
        g.set(1, 0, Cell::wide_continuation(ResolvedFace::DEFAULT));
        // Third cell stays blank.
        assert_eq!(g.to_debug_text(), "中 ");
    }
}
