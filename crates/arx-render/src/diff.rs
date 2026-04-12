//! Minimal cell-level diff between two [`RenderTree`]s.
//!
//! The view layer produces a fresh [`RenderTree`] every frame. The differ
//! walks both trees in parallel and emits a sequence of [`DiffOp`]s that
//! describe just the cells the backend has to touch. For a keystroke that
//! updates one line, that's on the order of a few dozen ops instead of
//! repainting the whole screen.
//!
//! Phase 1 is deliberately dumb: one [`DiffOp::SetCell`] per changed cell
//! plus cursor / resize ops. Later milestones can spot horizontal runs
//! and switch to `WriteRun` / `Scroll` ops.

use crate::cell::Cell;
use crate::render_tree::{CursorRender, RenderTree};

/// A single low-level terminal update.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DiffOp {
    /// The terminal was resized. Backends should clear state before
    /// applying the subsequent ops.
    Resize { width: u16, height: u16 },
    /// Overwrite the cell at `(x, y)`.
    SetCell { x: u16, y: u16, cell: Cell },
    /// Move the cursor and update its style.
    MoveCursor(CursorRender),
    /// No cursors are visible this frame — the backend should hide it.
    HideCursor,
}

/// Produce a minimal sequence of [`DiffOp`]s that turns `old` into `new`.
///
/// * If the grids have different sizes we emit a [`DiffOp::Resize`] and
///   then a full repaint of the new grid.
/// * Otherwise we scan cell-by-cell and emit a [`DiffOp::SetCell`] for
///   every difference.
/// * We always emit cursor ops if either the cursor position or cursor
///   visibility differ.
pub fn diff(old: &RenderTree, new: &RenderTree) -> Vec<DiffOp> {
    let mut ops = Vec::new();
    let same_size = old.cells.width() == new.cells.width()
        && old.cells.height() == new.cells.height();
    if same_size {
        for y in 0..new.cells.height() {
            for x in 0..new.cells.width() {
                let a = old.cells.get(x, y).unwrap();
                let b = new.cells.get(x, y).unwrap();
                if a != b {
                    ops.push(DiffOp::SetCell {
                        x,
                        y,
                        cell: b.clone(),
                    });
                }
            }
        }
    } else {
        ops.push(DiffOp::Resize {
            width: new.cells.width(),
            height: new.cells.height(),
        });
        // Repaint every cell of the new frame.
        for y in 0..new.cells.height() {
            for x in 0..new.cells.width() {
                let cell = new.cells.get(x, y).unwrap();
                ops.push(DiffOp::SetCell {
                    x,
                    y,
                    cell: cell.clone(),
                });
            }
        }
    }

    // Cursor diff — independent of cell diff because the terminal cursor
    // is its own piece of state.
    match (old.cursors.first(), new.cursors.first()) {
        (Some(a), Some(b)) if a == b => {}
        (_, Some(b)) => ops.push(DiffOp::MoveCursor(*b)),
        (Some(_), None) => ops.push(DiffOp::HideCursor),
        (None, None) => {}
    }

    ops
}

/// Produce a full-repaint `DiffOp` stream for an initial frame.
///
/// Unlike `diff(&RenderTree::blank(..), new)`, this emits a
/// [`DiffOp::SetCell`] for **every** position in the grid — even cells
/// that happen to equal [`Cell::blank`]. We can't skip blank-to-blank
/// matches on the first frame because we have no idea what the
/// terminal's starting state actually is: a cell we don't explicitly
/// paint ends up showing whatever background the terminal decided to
/// use for its "erased" state, which is rarely our
/// [`crate::face::ResolvedFace::DEFAULT`]. The visible symptom of the
/// old shortcut was that text cells got our black background but
/// single-space cells between words (and whole empty rows) showed
/// through to the terminal's own default bg, producing a mottled
/// light/dark pattern.
///
/// The extra bytes on the wire (every cell in an 80×24 terminal, ~2000
/// ops) are a one-shot cost at session startup — not a concern for a
/// local IPC link.
pub fn initial_paint(new: &RenderTree) -> Vec<DiffOp> {
    let width = new.cells.width();
    let height = new.cells.height();
    let mut ops = Vec::with_capacity((width as usize) * (height as usize) + 2);
    for y in 0..height {
        for x in 0..width {
            let cell = new.cells.get(x, y).unwrap();
            ops.push(DiffOp::SetCell {
                x,
                y,
                cell: cell.clone(),
            });
        }
    }
    if let Some(cursor) = new.cursors.first() {
        ops.push(DiffOp::MoveCursor(*cursor));
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::CellFlags;
    use crate::face::ResolvedFace;
    use crate::render_tree::CursorStyle;
    use compact_str::CompactString;
    use smallvec::smallvec;

    fn cell(c: &str) -> Cell {
        Cell {
            grapheme: CompactString::new(c),
            face: ResolvedFace::DEFAULT,
            flags: CellFlags::empty(),
        }
    }

    fn tree(width: u16, height: u16) -> RenderTree {
        RenderTree::blank(width, height, 0)
    }

    #[test]
    fn identical_trees_produce_no_ops() {
        let a = tree(4, 2);
        let b = tree(4, 2);
        assert_eq!(diff(&a, &b), Vec::<DiffOp>::new());
    }

    #[test]
    fn single_cell_change_emits_one_setcell() {
        let a = tree(4, 2);
        let mut b = tree(4, 2);
        b.cells.set(1, 0, cell("X"));
        let ops = diff(&a, &b);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            DiffOp::SetCell { x, y, cell: c } => {
                assert_eq!((*x, *y), (1, 0));
                assert_eq!(c.grapheme.as_str(), "X");
            }
            op => panic!("unexpected op: {op:?}"),
        }
    }

    #[test]
    fn resize_triggers_full_repaint() {
        let a = tree(2, 2);
        let mut b = tree(3, 2);
        b.cells.set(0, 0, cell("A"));
        let ops = diff(&a, &b);
        // First op: Resize; followed by one SetCell per cell of the new grid.
        assert!(matches!(ops[0], DiffOp::Resize { width: 3, height: 2 }));
        let setcell_count = ops
            .iter()
            .filter(|op| matches!(op, DiffOp::SetCell { .. }))
            .count();
        assert_eq!(setcell_count, 3 * 2);
    }

    #[test]
    fn cursor_change_emits_move_cursor() {
        let a = tree(4, 2);
        let mut b = tree(4, 2);
        b.cursors = smallvec![CursorRender {
            col: 2,
            row: 1,
            style: CursorStyle::Block,
        }];
        let ops = diff(&a, &b);
        assert!(ops.iter().any(|op| matches!(op, DiffOp::MoveCursor(_))));
    }

    #[test]
    fn cursor_disappearing_hides_it() {
        let mut a = tree(4, 2);
        a.cursors = smallvec![CursorRender {
            col: 0,
            row: 0,
            style: CursorStyle::Block,
        }];
        let b = tree(4, 2);
        let ops = diff(&a, &b);
        assert_eq!(ops, vec![DiffOp::HideCursor]);
    }

    #[test]
    fn equal_cursors_produce_no_cursor_op() {
        let mut a = tree(4, 2);
        let mut b = tree(4, 2);
        let cr = CursorRender {
            col: 1,
            row: 1,
            style: CursorStyle::Block,
        };
        a.cursors = smallvec![cr];
        b.cursors = smallvec![cr];
        let ops = diff(&a, &b);
        assert!(ops.is_empty());
    }

    #[test]
    fn initial_paint_emits_every_cell_and_cursor() {
        let mut b = tree(2, 2);
        b.cells.set(0, 0, cell("X"));
        b.cells.set(1, 1, cell("Y"));
        b.cursors = smallvec![CursorRender {
            col: 0,
            row: 0,
            style: CursorStyle::Block,
        }];
        let ops = initial_paint(&b);
        // 2×2 grid = 4 SetCell ops, regardless of how many were blanks.
        let setcells: Vec<_> = ops
            .iter()
            .filter(|op| matches!(op, DiffOp::SetCell { .. }))
            .collect();
        assert_eq!(setcells.len(), 4);
        assert!(ops.iter().any(|op| matches!(op, DiffOp::MoveCursor(_))));
    }

    /// Regression test for the "different background colours on reopen"
    /// bug: `initial_paint` used to diff against a synthetic blank
    /// reference and skipped any cell that matched it. In-line spaces
    /// and empty rows stayed un-painted, so the terminal's own default
    /// background leaked through wherever the new frame didn't happen
    /// to have non-blank content — producing a mottled light/dark
    /// pattern on fresh sessions.
    ///
    /// Every cell, including the ones still equal to `Cell::blank`,
    /// must be represented in the op stream so the terminal is told
    /// exactly what colour every position has.
    #[test]
    fn initial_paint_explicitly_paints_blank_cells() {
        // 3×2 grid with one painted cell. The other five positions are
        // still Cell::blank() — they MUST all appear in the op stream.
        let mut b = tree(3, 2);
        b.cells.set(1, 0, cell("X"));
        let ops = initial_paint(&b);
        let setcells: Vec<(u16, u16, String)> = ops
            .iter()
            .filter_map(|op| match op {
                DiffOp::SetCell { x, y, cell } => {
                    Some((*x, *y, cell.grapheme.as_str().to_owned()))
                }
                _ => None,
            })
            .collect();
        // 3 × 2 = 6 positions, all present.
        assert_eq!(setcells.len(), 6);
        assert!(setcells.contains(&(1, 0, "X".into())));
        // Every blank position must also appear, so the terminal is
        // told what colour to use for them (not left showing through
        // to its own default).
        assert!(setcells.contains(&(0, 0, " ".into())));
        assert!(setcells.contains(&(2, 0, " ".into())));
        assert!(setcells.contains(&(0, 1, " ".into())));
        assert!(setcells.contains(&(1, 1, " ".into())));
        assert!(setcells.contains(&(2, 1, " ".into())));
    }
}

