//! Arx daemon ↔ client IPC protocol.
//!
//! The daemon runs the full editor state and renders frames. The client
//! is a "thin" process that owns the terminal and nothing else:
//!
//! * It forwards every [`KeyChord`](arx_keymap::KeyChord) (and resize
//!   event) to the daemon.
//! * It receives [`DiffOp`](arx_render::DiffOp) batches and applies them
//!   to the terminal via [`CrosstermBackend`](arx_render::CrosstermBackend).
//! * It holds no buffers, no keymap, no commands.
//!
//! This crate contains the wire-level types:
//!
//! * [`ClientMessage`] — client → daemon.
//! * [`DaemonMessage`] — daemon → client.
//! * [`codec`] — length-prefixed framing over any `AsyncRead`/`AsyncWrite`.
//!
//! The on-the-wire encoding is [`postcard`] — compact, schema-stable, and
//! designed for local IPC. Frames are a `u32` big-endian length prefix
//! followed by the postcard-encoded body.
//!
//! See `docs/spec.md` §7 for the architectural motivation.

pub mod codec;
pub mod message;

pub use codec::{FrameError, MAX_FRAME_BYTES, read_frame, write_frame};
pub use message::{ClientMessage, DaemonMessage, HelloInfo, ShutdownReason, PROTOCOL_VERSION};
