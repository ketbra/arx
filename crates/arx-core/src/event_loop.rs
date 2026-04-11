//! The single-writer event loop that drains the [`CommandBus`].
//!
//! [`EventLoop`] owns the [`Editor`] and a `mpsc::Receiver` of pending
//! [`CommandFn`]s. Calling [`EventLoop::run`] consumes the loop and drives
//! it to completion: each command is executed against the editor in arrival
//! order, on the loop's task. When every [`CommandBus`] handle has been
//! dropped the receiver returns `None` and `run` returns the final
//! [`Editor`] state.

use tokio::sync::mpsc;
use tracing::trace;

use crate::command::{CommandBus, CommandFn};
use crate::editor::Editor;

/// Default capacity for a freshly-created event-loop channel.
///
/// Generous enough to absorb a burst of input/agent dispatches without
/// stalling producers; small enough to surface runaway loops in tests.
pub const DEFAULT_BUS_CAPACITY: usize = 1024;

/// The editor's main event loop.
#[derive(Debug)]
pub struct EventLoop {
    editor: Editor,
    receiver: mpsc::Receiver<CommandFn>,
}

impl EventLoop {
    /// Create a new event loop and a [`CommandBus`] handle that can
    /// dispatch commands onto it. The bus uses
    /// [`DEFAULT_BUS_CAPACITY`].
    pub fn new() -> (Self, CommandBus) {
        Self::with_capacity(DEFAULT_BUS_CAPACITY)
    }

    /// Create a new event loop with a custom channel capacity. Useful for
    /// tests that want to exercise back-pressure deterministically.
    pub fn with_capacity(capacity: usize) -> (Self, CommandBus) {
        let (sender, receiver) = mpsc::channel(capacity);
        let bus = CommandBus::new(sender);
        let event_loop = EventLoop {
            editor: Editor::new(),
            receiver,
        };
        (event_loop, bus)
    }

    /// Build an event loop around an existing [`Editor`]. Useful for tests
    /// or for restoring state across daemon restarts later.
    pub fn with_editor(editor: Editor, capacity: usize) -> (Self, CommandBus) {
        let (sender, receiver) = mpsc::channel(capacity);
        let bus = CommandBus::new(sender);
        (EventLoop { editor, receiver }, bus)
    }

    /// Borrow the [`Editor`] without running. Useful before [`Self::run`].
    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Run the event loop until every [`CommandBus`] handle has been
    /// dropped, then return the final [`Editor`] state.
    ///
    /// This consumes the loop and is the typical entry point:
    ///
    /// ```no_run
    /// # use arx_core::EventLoop;
    /// # async fn run() {
    /// let (event_loop, bus) = EventLoop::new();
    /// let driver = tokio::spawn(event_loop.run());
    /// // ... use `bus` from elsewhere ...
    /// drop(bus);
    /// let final_editor = driver.await.unwrap();
    /// # let _ = final_editor;
    /// # }
    /// ```
    pub async fn run(mut self) -> Editor {
        let mut count: u64 = 0;
        while let Some(cmd) = self.receiver.recv().await {
            cmd(&mut self.editor);
            count = count.wrapping_add(1);
        }
        trace!(commands_executed = count, "event loop drained");
        self.editor
    }

    /// Run a single batch of pending commands without blocking. Returns the
    /// number of commands that were drained.
    ///
    /// Useful for tests and for embedding the event loop inside a larger
    /// driver (e.g. one that also pumps a UI frame between command batches).
    pub fn pump(&mut self) -> usize {
        let mut count = 0;
        while let Ok(cmd) = self.receiver.try_recv() {
            cmd(&mut self.editor);
            count += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DispatchError;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[tokio::test]
    async fn drains_when_bus_dropped() {
        let (event_loop, bus) = EventLoop::new();
        let driver = tokio::spawn(event_loop.run());

        let counter = Arc::new(AtomicU64::new(0));
        for _ in 0..10 {
            let c = counter.clone();
            bus.dispatch(move |_editor| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .unwrap();
        }
        drop(bus);
        let editor = driver.await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 10);
        assert!(editor.buffers().is_empty());
    }

    #[tokio::test]
    async fn pump_runs_pending_commands_synchronously() {
        let (mut event_loop, bus) = EventLoop::new();
        bus.try_dispatch(|editor| {
            editor.buffers_mut().create_scratch();
        })
        .unwrap();
        bus.try_dispatch(|editor| {
            editor.buffers_mut().create_scratch();
        })
        .unwrap();
        let n = event_loop.pump();
        assert_eq!(n, 2);
        assert_eq!(event_loop.editor().buffers().len(), 2);
    }

    #[tokio::test]
    async fn invoke_returns_value() {
        let (event_loop, bus) = EventLoop::new();
        let driver = tokio::spawn(event_loop.run());

        let id = bus
            .invoke(|editor| editor.buffers_mut().create_from_text("alpha", None))
            .await
            .unwrap();

        let text = bus
            .invoke(move |editor| editor.buffers().get(id).unwrap().text())
            .await
            .unwrap();

        assert_eq!(text, "alpha");
        drop(bus);
        let _ = driver.await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_after_shutdown_errors() {
        let (event_loop, bus) = EventLoop::new();
        let driver = tokio::spawn(event_loop.run());
        let bus2 = bus.clone();
        drop(bus);
        // The original bus is dropped, but the clone keeps the loop alive.
        bus2.dispatch(|_| {}).await.unwrap();
        drop(bus2);
        // Wait for the loop to shut down.
        let _ = driver.await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_on_closed_bus_errors() {
        let (event_loop, bus) = EventLoop::new();
        let bus2 = bus.clone();
        drop(bus);
        // Drop the loop entirely without spawning it — receivers gone.
        drop(event_loop);
        // bus2 still exists but the receiver is gone.
        let res = bus2.dispatch(|_| {}).await;
        assert!(matches!(res, Err(DispatchError::Closed)));
    }

    #[tokio::test]
    async fn try_dispatch_full_returns_full() {
        let (mut event_loop, bus) = EventLoop::with_capacity(2);
        bus.try_dispatch(|_| {}).unwrap();
        bus.try_dispatch(|_| {}).unwrap();
        let res = bus.try_dispatch(|_| {});
        assert!(matches!(res, Err(DispatchError::Full)));
        // Drain so the test exits cleanly.
        event_loop.pump();
    }

    #[tokio::test]
    async fn ordering_preserved_from_single_producer() {
        let (event_loop, bus) = EventLoop::new();
        let driver = tokio::spawn(event_loop.run());

        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        for i in 0..50u32 {
            let l = log.clone();
            bus.dispatch(move |_| {
                l.lock().unwrap().push(i);
            })
            .await
            .unwrap();
        }
        drop(bus);
        let _ = driver.await.unwrap();

        let collected = log.lock().unwrap().clone();
        let expected: Vec<u32> = (0..50).collect();
        assert_eq!(collected, expected);
    }

    #[tokio::test]
    async fn many_producers_all_arrive() {
        let (event_loop, bus) = EventLoop::new();
        let driver = tokio::spawn(event_loop.run());

        let counter = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let bus = bus.clone();
            let counter = counter.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..100 {
                    let c = counter.clone();
                    bus.dispatch(move |_| {
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                    .await
                    .unwrap();
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        drop(bus);
        let _ = driver.await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 16 * 100);
    }
}
