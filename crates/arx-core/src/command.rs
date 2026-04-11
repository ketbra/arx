//! Command bus: typed dispatch onto the editor's main task.
//!
//! A "command" here is just a `Send + 'static` closure that takes
//! `&mut Editor`. The bus is a [`tokio::sync::mpsc`] channel that the
//! [`crate::EventLoop`] drains in arrival order. Producers can be anything
//! running on the tokio runtime — input handlers, agent adapters, file
//! watchers, network clients — and they all share a single
//! [`CommandBus`] handle (which is `Clone + Send + Sync`).
//!
//! The closure-based shape gives us:
//!
//! * **Single writer, no locks.** Only the event-loop task touches `Editor`.
//! * **Type safety without an enum.** Each producer can build its own
//!   strongly-typed command without registering it in a central enum.
//! * **Reply support.** [`CommandBus::invoke`] composes the bus with a
//!   one-shot channel so a caller can `await` the command's return value.
//! * **A clean evolution path** to a registered/named command system later
//!   (for the command palette, keymaps, and the SDK) without changing how
//!   the underlying transport works.
//!
//! # Example
//!
//! ```no_run
//! # use arx_core::{EventLoop, CommandBus};
//! # async fn run(bus: CommandBus) -> Result<(), Box<dyn std::error::Error>> {
//! // Fire-and-forget.
//! bus.dispatch(|editor| {
//!     editor.buffers_mut().create_scratch();
//! })
//! .await?;
//!
//! // Dispatch and wait for a return value.
//! let id = bus
//!     .invoke(|editor| editor.buffers_mut().create_scratch())
//!     .await?;
//! # let _ = id;
//! # Ok(())
//! # }
//! ```

use std::fmt;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::editor::Editor;

/// A type-erased function that runs against the editor's mutable state.
///
/// Bodies must not block — long-running work should spawn a tokio task that
/// re-enters the bus when it's done with a follow-up command.
pub type CommandFn = Box<dyn FnOnce(&mut Editor) + Send + 'static>;

/// Error returned when a dispatch can't be delivered.
#[derive(Debug, Error)]
pub enum DispatchError {
    /// The event loop has shut down — every receiver is gone.
    #[error("editor command bus is closed")]
    Closed,
    /// The bus is at capacity (only returned by [`CommandBus::try_dispatch`]).
    #[error("editor command bus is full")]
    Full,
    /// The dispatched command's reply channel was dropped before the
    /// command finished — typically because the command panicked or the
    /// loop shut down mid-execution.
    #[error("command did not produce a reply")]
    NoReply,
}

/// Cloneable handle for dispatching commands onto the editor's command bus.
#[derive(Clone)]
pub struct CommandBus {
    sender: mpsc::Sender<CommandFn>,
}

impl CommandBus {
    pub(crate) fn new(sender: mpsc::Sender<CommandFn>) -> Self {
        Self { sender }
    }

    /// Dispatch a command, awaiting backpressure if the bus is at capacity.
    pub async fn dispatch<F>(&self, f: F) -> Result<(), DispatchError>
    where
        F: FnOnce(&mut Editor) + Send + 'static,
    {
        self.sender
            .send(Box::new(f))
            .await
            .map_err(|_| DispatchError::Closed)
    }

    /// Try to dispatch a command without blocking. Returns immediately if
    /// the bus is full or closed.
    pub fn try_dispatch<F>(&self, f: F) -> Result<(), DispatchError>
    where
        F: FnOnce(&mut Editor) + Send + 'static,
    {
        self.sender
            .try_send(Box::new(f))
            .map_err(|e| match e {
                mpsc::error::TrySendError::Closed(_) => DispatchError::Closed,
                mpsc::error::TrySendError::Full(_) => DispatchError::Full,
            })
    }

    /// Dispatch a command and await its return value.
    ///
    /// This is the "ask" pattern: the closure runs on the event loop with
    /// `&mut Editor`, returns a value, and the value is sent back to the
    /// caller through a one-shot channel.
    ///
    /// Returns [`DispatchError::Closed`] if the loop has shut down before
    /// the command runs, or [`DispatchError::NoReply`] if the command was
    /// dropped before producing a value (e.g. the loop shut down between
    /// dispatch and execution, or the command panicked).
    pub async fn invoke<F, T>(&self, f: F) -> Result<T, DispatchError>
    where
        F: FnOnce(&mut Editor) -> T + Send + 'static,
        T: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.dispatch(move |editor| {
            let result = f(editor);
            // If the receiver was dropped (caller cancelled `invoke`), the
            // result is silently discarded.
            let _ = reply_tx.send(result);
        })
        .await?;
        reply_rx.await.map_err(|_| DispatchError::NoReply)
    }

    /// Whether the underlying channel is closed (i.e. the event loop has
    /// shut down). Useful for cooperative shutdown of producer tasks.
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    /// Current free capacity of the underlying channel. Producers can use
    /// this for back-pressure-aware throttling.
    pub fn capacity(&self) -> usize {
        self.sender.capacity()
    }
}

impl fmt::Debug for CommandBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandBus")
            .field("closed", &self.is_closed())
            .field("capacity", &self.capacity())
            .finish()
    }
}
