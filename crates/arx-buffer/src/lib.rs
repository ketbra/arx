//! Persistent rope-based buffer with text properties.
//!
//! This crate is the Phase-1 storage substrate for the Arx editor. It provides:
//!
//! * [`Rope`] — an immutable, copy-on-write rope backed by a balanced binary
//!   tree of UTF-8 chunks. Edits are `O(log n)` and snapshots are `O(1)`
//!   (an `Arc` clone), enabling any number of concurrent readers (agents,
//!   background jobs, renderers) to hold consistent views of a buffer without
//!   blocking writers.
//! * [`Buffer`] — a mutable wrapper around a [`Rope`] that tracks a monotonic
//!   version counter and a [`PropertyMap`] of layered text annotations.
//! * [`BufferSnapshot`] — an `O(1)`-cloned immutable view of a buffer at a
//!   particular version, safe to send across threads.
//! * [`PropertyMap`] / [`PropertyLayer`] — a persistent per-layer interval
//!   tree of [`PropertyValue`]s with configurable
//!   [`AdjustmentPolicy`] on edits (track, invalidate, or static).
//!
//! See `docs/spec.md` §3 for the full design notes.
//!
//! # Example
//!
//! ```
//! use arx_buffer::{Buffer, BufferId, EditOrigin};
//!
//! let mut buf = Buffer::from_str(BufferId(1), "hello world");
//! assert_eq!(buf.version(), 0);
//!
//! let snap_a = buf.snapshot();
//! buf.edit(6..11, "rope!", EditOrigin::User);
//!
//! // The old snapshot still observes the pre-edit text.
//! assert_eq!(snap_a.text(), "hello world");
//! assert_eq!(buf.text(), "hello rope!");
//! assert_eq!(buf.version(), 1);
//! ```

pub mod buffer;
pub mod interval_tree;
pub mod properties;
pub mod rope;

pub use buffer::{Buffer, BufferId, BufferSnapshot, Edit, EditOrigin};
pub use interval_tree::{Interval, IntervalTree};
pub use properties::{
    AdjustmentPolicy, AgentId, Diagnostic, Face, LayerId, PropertyFlags, PropertyLayer,
    PropertyMap, PropertyValue, Severity, StickyBehavior, StyledRun, UnderlineStyle,
};
pub use rope::{ByteRange, Rope, TextSummary};
