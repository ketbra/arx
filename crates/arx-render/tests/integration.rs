//! End-to-end tests for the rendering pipeline:
//! `Buffer` → `ViewState` → `render` → `diff` → `TestBackend`.
//!
//! These verify that the whole pipeline composes correctly, including
//! property-layer face propagation and cell-level diff application.

use arx_buffer::{
    AdjustmentPolicy, Buffer, BufferId, Face as SparseFace, Interval, PropertyValue,
    StickyBehavior,
};
use arx_render::{
    Backend, CellFlags, Color, Cursor, CursorStyle, GlobalState, GutterConfig, LayoutTree,
    ResolvedFace, ScrollPosition, TerminalSize, TestBackend, ViewState, WindowId, WindowState,
    diff, initial_paint, render,
};
use smallvec::smallvec;

fn window(text: &str) -> WindowState {
    let buf = Buffer::from_str(BufferId(1), text);
    WindowState {
        id: WindowId(1),
        buffer: buf.snapshot(),
        cursors: smallvec![Cursor::at(0)],
        scroll: ScrollPosition::default(),
        gutter: GutterConfig::default(),
        selection: None,
        excluded_lines: std::collections::BTreeSet::new(),
    }
}

fn state_with(window: WindowState, cols: u16, rows: u16) -> ViewState {
    let id = window.id;
    ViewState {
        size: TerminalSize::new(cols, rows),
        layout: LayoutTree::Single(id),
        windows: vec![window],
        terminal_panes: vec![],
        active_window: Some(id),
        global: GlobalState::default(),
    }
}

#[test]
fn initial_paint_reaches_the_backend() {
    let w = window("hello\nworld");
    let state = state_with(w, 20, 4);
    let tree = render(&state, 0);

    let mut backend = TestBackend::new(20, 4);
    backend.apply(&initial_paint(&tree)).unwrap();

    // The gutter + text rows should show our content.
    let text = backend.grid().to_debug_text();
    let rows: Vec<&str> = text.split('\n').collect();
    assert!(rows[0].contains("hello"), "{:?}", rows[0]);
    assert!(rows[1].contains("world"), "{:?}", rows[1]);
    // Cursor visible at (gutter_end, 0).
    let cursor = backend.cursor().expect("cursor present");
    assert_eq!(cursor.row, 0);
    assert_eq!(cursor.style, CursorStyle::Block);
}

#[test]
fn typed_character_diffs_to_two_setcell_ops() {
    // Frame N: buffer is "abc". Frame N+1: buffer is "aXbc".
    // Because every character after the insertion moves, we expect
    // SetCell ops for positions `col[X], col[b], col[c]` plus one
    // MoveCursor.
    let mut buf = Buffer::from_str(BufferId(1), "abc");
    let w0 = WindowState {
        id: WindowId(1),
        buffer: buf.snapshot(),
        cursors: smallvec![Cursor::at(1)],
        scroll: ScrollPosition::default(),
        gutter: GutterConfig::default(),
        selection: None,
        excluded_lines: std::collections::BTreeSet::new(),
    };
    let state0 = state_with(w0, 20, 3);
    let tree0 = render(&state0, 0);

    buf.edit(1..1, "X", arx_buffer::EditOrigin::User);
    let w1 = WindowState {
        id: WindowId(1),
        buffer: buf.snapshot(),
        cursors: smallvec![Cursor::at(2)],
        scroll: ScrollPosition::default(),
        gutter: GutterConfig::default(),
        selection: None,
        excluded_lines: std::collections::BTreeSet::new(),
    };
    let state1 = state_with(w1, 20, 3);
    let tree1 = render(&state1, 1);

    let ops = diff(&tree0, &tree1);
    let setcells = ops
        .iter()
        .filter(|op| matches!(op, arx_render::DiffOp::SetCell { .. }))
        .count();
    // The cell that previously held the cursor highlight is also updated
    // (CURSOR_PRIMARY flag cleared), so we expect at least three SetCell
    // ops plus one MoveCursor.
    assert!(setcells >= 3, "got {setcells} SetCell ops");
    assert!(
        ops.iter()
            .any(|op| matches!(op, arx_render::DiffOp::MoveCursor(_)))
    );
}

#[test]
fn property_decoration_flows_into_cell_face() {
    let mut buf = Buffer::from_str(BufferId(1), "hello world");
    // Paint the word "hello" red via a decoration property layer.
    buf.properties_mut()
        .ensure_layer("decor", AdjustmentPolicy::TrackEdits)
        .insert(Interval::new(
            0..5,
            PropertyValue::Decoration(SparseFace {
                fg: Some(0xff_00_00),
                priority: 10,
                ..Default::default()
            }),
            StickyBehavior::RearSticky,
        ));

    let w = WindowState {
        id: WindowId(1),
        buffer: buf.snapshot(),
        cursors: smallvec![Cursor::at(0)],
        scroll: ScrollPosition::default(),
        gutter: GutterConfig::default(),
        selection: None,
        excluded_lines: std::collections::BTreeSet::new(),
    };
    let state = state_with(w, 20, 3);
    let tree = render(&state, 0);

    // Find the 'h' cell and check its foreground.
    let grid = &tree.cells;
    let h_cell = (0..grid.width())
        .find_map(|x| {
            let c = grid.get(x, 0).unwrap();
            if c.grapheme.as_str() == "h" { Some(c) } else { None }
        })
        .expect("'h' cell present");
    assert_eq!(h_cell.face.fg, Color(0xff_00_00));
    // A cell past the decoration should fall back to the default face.
    let space_cell = grid.get(grid.width() - 1, 0).unwrap();
    assert_eq!(space_cell.face.fg, ResolvedFace::DEFAULT.fg);
}

#[test]
fn resize_triggers_backend_resize_then_repaint() {
    let w = window("abc");
    let state_small = state_with(w.clone(), 10, 3);
    let state_big = state_with(w, 20, 4);

    let tree_small = render(&state_small, 0);
    let tree_big = render(&state_big, 1);

    let mut backend = TestBackend::new(10, 3);
    backend.apply(&initial_paint(&tree_small)).unwrap();
    assert_eq!(backend.size(), (10, 3));

    let ops = diff(&tree_small, &tree_big);
    assert!(
        ops.iter()
            .any(|op| matches!(op, arx_render::DiffOp::Resize { .. }))
    );
    backend.apply(&ops).unwrap();
    assert_eq!(backend.size(), (20, 4));
    // Still renders the buffer content after the resize.
    let text = backend.grid().to_debug_text();
    assert!(text.contains("abc"));
}

#[test]
fn cursor_cell_carries_cursor_primary_flag() {
    let mut w = window("hello");
    w.cursors = smallvec![Cursor::at(2)]; // between 'e' and 'l'
    let state = state_with(w, 20, 3);
    let tree = render(&state, 0);

    let cr = tree.cursors.first().expect("primary cursor present");
    let cell = tree.cells.get(cr.col, cr.row).unwrap();
    assert!(cell.flags.contains(CellFlags::CURSOR_PRIMARY));
}

#[test]
fn second_frame_with_no_changes_produces_no_ops() {
    let w = window("idempotent");
    let state = state_with(w, 30, 3);
    let tree_a = render(&state, 0);
    let tree_b = render(&state, 1); // same state, different frame_id
    let ops = diff(&tree_a, &tree_b);
    assert!(
        ops.is_empty(),
        "expected no-op diff, got {} ops: {ops:#?}",
        ops.len()
    );
}
