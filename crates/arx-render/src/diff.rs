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

/// Produce a full-repaint `DiffOp` stream for an initial frame. Equivalent
/// to `diff(&RenderTree::blank(new.width, new.height, 0), new)`.
pub fn initial_paint(new: &RenderTree) -> Vec<DiffOp> {
    let blank = RenderTree::blank(new.cells.width(), new.cells.height(), 0);
    diff(&blank, new)
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
    fn initial_paint_emits_every_non_blank_cell_and_cursor() {
        let mut b = tree(2, 2);
        b.cells.set(0, 0, cell("X"));
        b.cells.set(1, 1, cell("Y"));
        b.cursors = smallvec![CursorRender {
            col: 0,
            row: 0,
            style: CursorStyle::Block,
        }];
        let ops = initial_paint(&b);
        // Only 2 non-blank cells, but the blank-vs-blank path also
        // produces *no* ops for unchanged cells, so expect 2 SetCell ops
        // plus a MoveCursor.
        let setcells: Vec<_> = ops
            .iter()
            .filter(|op| matches!(op, DiffOp::SetCell { .. }))
            .collect();
        assert_eq!(setcells.len(), 2);
        assert!(ops.iter().any(|op| matches!(op, DiffOp::MoveCursor(_))));
    }
}

