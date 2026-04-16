//! Arx GUI frontend.
//!
//! This crate is the "minimal spike" described in
//! `docs/spec.md` §4.8: a second [`arx_render::Backend`] implementation
//! that paints via `wgpu` + `glyphon` (which wraps `cosmic-text`) into
//! a `winit` window.
//!
//! ### Threading model
//!
//! `winit`'s event loop must own the process's main thread on every
//! platform we care about, and it is synchronous. `arx-driver` in
//! contrast wants to sit inside a `tokio` multi-thread runtime and
//! drive the editor's event loop asynchronously. We bridge the two:
//!
//! 1. The main thread creates a `winit::event_loop::EventLoop` and
//!    builds [`app::GuiApp`] as its `ApplicationHandler`.
//! 2. A dedicated background thread starts a tokio multi-thread
//!    runtime and spawns [`arx_driver::Driver::run_with`] inside it.
//!    That driver owns everything: editor, event-loop task, input
//!    task, render task, command bus.
//! 3. The two sides communicate over channels:
//!    * **winit → driver**: a `tokio::sync::mpsc` of
//!      `io::Result<crossterm::event::Event>` that stands in for the
//!      terminal's `EventStream`. Winit key/resize events are
//!      translated in-place ([`input::translate_key`]) and pushed
//!      into it.
//!    * **driver → winit**: a `std::sync::mpsc` of [`BackendFrame`]s.
//!      The driver's [`GpuBackend`] flushes each `apply()` batch into
//!      it, then pokes the winit event loop via an
//!      [`winit::event_loop::EventLoopProxy`] so the window can
//!      request a redraw.
//!
//! ### Scope for the spike
//!
//! * Monospace-only rendering. One bundled system font (whichever
//!   `cosmic-text` finds first — on Linux that's typically
//!   `DejaVu Sans Mono`).
//! * Per-cell foreground color via cosmic-text attrs. Backgrounds are
//!   drawn as coloured rects in a prior pass.
//! * Cursor is a single additional rect.
//! * No underlines / italics / selection dragging / IME / clipboard
//!   / mouse in v0 — follow-up work.
//! * Linux is the only verified platform.

pub mod app;
pub mod backend;
pub mod input;
pub mod renderer;

pub use app::{GuiError, run_gui};
pub use backend::{BackendFrame, GpuBackend};
