//! Arx editor rendering pipeline.
//!
//! This crate implements the shape described in `docs/spec.md` §4:
//!
//! ```text
//!  Editor Core
//!       │
//!       │  publishes ViewState (snapshot + layout + cursors + mode)
//!       ▼
//!  ┌─────────────┐
//!  │  View Layer  │  Pure function: ViewState → RenderTree
//!  └──────┬──────┘
//!         │  RenderTree (frame N)
//!         ▼
//!  ┌─────────────┐      ┌──────────────────┐
//!  │  Differ      │◄────│ RenderTree (N-1) │
//!  └──────┬──────┘      └──────────────────┘
//!         │  DiffOps (only what changed)
//!         ▼
//!  ┌─────────────┐
//!  │  Backend    │  TestBackend  ·  CrosstermBackend  ·  (future) GpuBackend
//!  └─────────────┘
//! ```
//!
//! * [`view::render`] is a pure function from [`ViewState`] to
//!   [`RenderTree`]. It walks the buffer with grapheme-cluster awareness
//!   (via [`unicode_segmentation`]), computes display widths (via
//!   [`unicode_width`]) so CJK / emoji occupy the right number of cells,
//!   applies the buffer's [`PropertyMap`] styled runs on top of a theme
//!   default face, and paints a [`CellGrid`].
//!
//! * [`diff::diff`] compares two [`RenderTree`]s and emits a minimal
//!   stream of [`DiffOp`]s. Phase 1 is cell-granular; later commits can
//!   coalesce horizontal runs / detect scroll.
//!
//! * [`backend::Backend`] is the trait every backend implements.
//!   [`backend::TestBackend`] is an in-memory implementation used for
//!   tests (no TTY required); [`backend::CrosstermBackend`] drives real
//!   terminals via `crossterm`.
//!
//! Nothing in this crate touches async or tokio — the renderer is a
//! synchronous pipeline invoked by `arx-core`'s event loop. Keeping it
//! runtime-agnostic means the same pipeline serves the daemon's TUI
//! clients, test rigs, and (eventually) a wgpu backend.
//!
//! [`PropertyMap`]: arx_buffer::PropertyMap
//! [`CellGrid`]: crate::cell::CellGrid
//! [`ViewState`]: crate::view_state::ViewState
//! [`RenderTree`]: crate::render_tree::RenderTree
//! [`DiffOp`]: crate::diff::DiffOp

pub mod backend;
pub mod cell;
pub mod diff;
pub mod face;
pub mod render_tree;
pub mod view;
pub mod view_state;

pub use backend::{Backend, CrosstermBackend, TestBackend};
pub use cell::{Cell, CellFlags, CellGrid};
pub use diff::{DiffOp, diff, initial_paint};
pub use face::{Color, ResolvedFace};
pub use render_tree::{CursorRender, CursorStyle, RenderTree};
pub use view::render;
pub use view_state::{
    CompletionEntry, CompletionView, Cursor, GlobalState, GutterConfig, LayoutTree, PaletteEntry,
    PaletteView, Rect, ScrollPosition, SplitDirection, TerminalSize, ViewState, WindowId,
    WindowState,
};
