//! Arx editor core: event loop, command bus, and editor state.
//!
//! This crate sits between [`arx_buffer`] (the persistent rope + buffer
//! substrate) and the rest of the editor (rendering, agents, daemon, SDK).
//! It provides:
//!
//! * [`Editor`] — the in-process state container. Owns the [`BufferManager`]
//!   today; will grow to own windows, layouts, agents, and the keymap as
//!   later Phase-1 milestones land. Lives entirely on the event loop's task,
//!   so we never need locks for editor state.
//!
//! * [`BufferManager`] — owns every open [`arx_buffer::Buffer`], publishes
//!   immutable [`arx_buffer::BufferSnapshot`]s through a per-buffer
//!   [`tokio::sync::watch`] channel so any number of readers (renderers,
//!   agents, background analysis) can observe edits without locks. This
//!   matches the model in `docs/spec.md` §3.4.
//!
//! * [`CommandBus`] — a cheap-to-clone, `Send + Sync` handle for dispatching
//!   work onto the editor. Producers can be key handlers, agents, file
//!   watchers, network clients, timers — anything that runs in a tokio task.
//!
//! * [`EventLoop`] — the single-writer dispatch loop. Drains the bus and
//!   runs each command against the editor in arrival order, on its own task.
//!   When the last [`CommandBus`] handle is dropped the loop exits cleanly.
//!
//! ## Concurrency model
//!
//! See `docs/spec.md` §2.1. Single writer, many lock-free readers:
//!
//! ```text
//!     producers (any task)
//!         │
//!         ▼
//!     CommandBus  ──mpsc──▶  EventLoop  ──&mut──▶  Editor
//!                                                    │
//!                                                    │ snapshot
//!                                                    ▼
//!     readers (any task)  ◀──watch──  BufferManager
//! ```
//!
//! ## Example
//!
//! ```no_run
//! use arx_core::{EventLoop, BufferManager};
//! use arx_buffer::EditOrigin;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let (event_loop, bus) = EventLoop::new();
//!
//! // Spawn the dispatcher onto the current runtime.
//! let driver = tokio::spawn(event_loop.run());
//!
//! // From any other task / context, dispatch work against the editor.
//! let id = bus
//!     .invoke(|editor| editor.buffers_mut().create_from_text("hello", None))
//!     .await?;
//!
//! bus.invoke(move |editor| {
//!     editor
//!         .buffers_mut()
//!         .edit(id, 5..5, " world", EditOrigin::User);
//! })
//! .await?;
//!
//! // Drop the bus → loop drains → returns the final Editor state.
//! drop(bus);
//! let final_editor = driver.await?;
//! assert_eq!(
//!     final_editor.buffers().get(id).unwrap().text(),
//!     "hello world"
//! );
//! # Ok(())
//! # }
//! ```

pub mod command;
pub mod editor;
pub mod event_loop;
pub mod file;
pub mod palette;
pub mod registry;
pub mod session;
pub mod stock;
pub mod window;

pub use command::{CommandBus, CommandFn, DispatchError};
pub use editor::{BufferManager, Editor, KeyHandled};
pub use event_loop::{DEFAULT_BUS_CAPACITY, EventLoop};
pub use file::{OpenFileError, SaveFileError, open_file, save_file, save_file_as};
pub use palette::{CommandPalette, PaletteMatch};
pub use registry::{Command, CommandContext, CommandRegistry};
pub use session::{SerializedBuffer, SerializedWindow, Session, SessionFile};
pub use window::{Layout, SplitAxis, WindowData, WindowId, WindowManager};
