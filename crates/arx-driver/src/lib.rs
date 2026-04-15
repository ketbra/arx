//! Arx editor driver: wires [`arx_core`]'s event loop, [`arx_render`]'s
//! pipeline, and a terminal backend (typically [`arx_render::CrosstermBackend`])
//! together into a running editor.
//!
//! The composition looks like this:
//!
//! ```text
//!     stdin                                      stdout
//!       │                                          ▲
//!       │ crossterm::EventStream                   │ crossterm queued writes
//!       ▼                                          │
//!   ┌───────────┐   commands   ┌────────────┐      │
//!   │ InputTask │──────────────▶ CommandBus │      │
//!   └───────────┘              └─────┬──────┘      │
//!                                    │             │
//!                                    ▼             │
//!                              ┌────────────┐      │
//!                              │ EventLoop  │──┐   │
//!                              │  +Editor   │  │   │
//!                              └────────────┘  │   │
//!                                notify_one()  │   │
//!                                    │         │   │
//!                                    ▼         │   │
//!                              ┌────────────┐  │   │
//!                              │ RenderTask │──┘   │
//!                              │  Backend   │──────┘
//!                              └────────────┘
//! ```
//!
//! * **`InputTask`** reads `crossterm` events, maps them to commands, and
//!   dispatches onto the `CommandBus`. On `Ctrl-Q`/`Ctrl-C`/`Esc` it fires
//!   the shutdown signal so every other task wakes up and unwinds.
//!
//! * **`EventLoop`** is the single-writer dispatcher from `arx-core`. It
//!   drains the bus serially against `&mut Editor` and pings a redraw
//!   notify after every command that called `editor.mark_dirty()`.
//!
//! * **`RenderTask`** waits on that redraw notify, `invoke`s the bus for a
//!   fresh `ViewState`, calls `arx_render::render` + `diff`, and applies
//!   the resulting [`arx_render::DiffOp`]s to the configured backend.
//!
//! [`Driver::run`] is the convenience for running against a real terminal;
//! [`Driver::run_with`] is the generic entry used by tests with a
//! scripted event stream and an in-memory backend.

pub mod daemon;
pub mod driver;
pub mod ext_host;
pub mod ext_watcher;
pub mod input;
pub mod lsp;
pub mod remote_backend;
pub mod render;
pub mod state;
pub mod suspend;

pub use daemon::{DaemonClient, DaemonError, DaemonServer};
pub use driver::{Driver, DriverError};
pub use ext_host::{ExtHostError, ExtensionHost};
pub use ext_watcher::{ExtensionWatcher, ExtensionWatcherError};
pub use input::InputTask;
pub use remote_backend::RemoteBackend;
pub use render::RenderTask;
pub use state::{SharedTerminalSize, Shutdown};
