//! Input task: translate terminal events into editor commands.
//!
//! Reads from a [`crossterm::event::EventStream`] (or any async stream of
//! [`Event`]) and hands each keystroke to the editor's [`KeymapEngine`]
//! via [`arx_core::Editor::handle_key`]. The keymap resolves the chord
//! against the active mode stack, invokes the registered command if any,
//! and reports back whether the key was handled, pending, or unbound.
//!
//! Unbound printable characters fall through to a crate-local self-insert
//! so plain typing works even without explicit bindings. Non-printable
//! unbound keys are silently dropped.
//!
//! The task shuts down when it sees the editor's `quit_requested()` flag
//! flip (set by the `editor.quit` stock command) or when the event
//! stream drains.

use std::ops::ControlFlow;

use crossterm::event::Event;
use futures_util::StreamExt;
use tracing::{debug, trace};

use arx_core::{CommandBus, KeyHandled};
use arx_keymap::KeyChord;

use crate::state::{SharedTerminalSize, Shutdown};

/// Everything the input task needs to run.
pub struct InputTask<S>
where
    S: futures_util::Stream<Item = std::io::Result<Event>> + Unpin + Send,
{
    pub events: S,
    pub bus: CommandBus,
    pub size: SharedTerminalSize,
    pub shutdown: Shutdown,
}

impl<S> std::fmt::Debug for InputTask<S>
where
    S: futures_util::Stream<Item = std::io::Result<Event>> + Unpin + Send,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputTask")
            .field("size", &self.size)
            .field("bus", &self.bus)
            .finish_non_exhaustive()
    }
}

impl<S> InputTask<S>
where
    S: futures_util::Stream<Item = std::io::Result<Event>> + Unpin + Send + 'static,
{
    /// Drive the input loop until a quit is requested, the event stream
    /// ends, or the command bus closes.
    pub async fn run(self) {
        let InputTask {
            mut events,
            bus,
            size,
            shutdown,
        } = self;

        while let Some(event) = events.next().await {
            match event {
                Ok(ev) => {
                    trace!(?ev, "input event");
                    if handle(ev, &bus, &size).await.is_break() {
                        debug!("shutdown requested from input");
                        shutdown.fire();
                        break;
                    }
                }
                Err(err) => {
                    tracing::warn!(%err, "input event error");
                    break;
                }
            }
        }
    }
}

async fn handle(event: Event, bus: &CommandBus, size: &SharedTerminalSize) -> ControlFlow<()> {
    match event {
        Event::Key(key) => handle_key(key, bus).await,
        Event::Resize(cols, rows) => {
            size.set(cols, rows);
            ControlFlow::Continue(())
        }
        // Mouse / paste / focus events are ignored for Phase 1.
        _ => ControlFlow::Continue(()),
    }
}

async fn handle_key(
    key: crossterm::event::KeyEvent,
    bus: &CommandBus,
) -> ControlFlow<()> {
    let chord = KeyChord::from(&key);

    // One round-trip to the event loop: feed the chord through the
    // keymap engine, let the matching command run, and ask whether a
    // quit was requested. This keeps the single-writer invariant: only
    // the event-loop task ever touches `Editor`.
    let bus_clone = bus.clone();
    let result = bus
        .invoke(move |editor| {
            let outcome = editor.handle_key(&bus_clone, chord);
            (outcome, editor.quit_requested())
        })
        .await;
    let Ok((outcome, quit)) = result else {
        return ControlFlow::Break(());
    };

    if quit {
        return ControlFlow::Break(());
    }

    if let KeyHandled::Unbound {
        printable_fallback: Some(ch),
    } = outcome
    {
        // Printable fallback: let the editor decide what to do.
        // `handle_printable_fallback` self-inserts into the active
        // buffer under normal conditions, or routes into the command
        // palette query when the palette layer is on the stack.
        if bus
            .dispatch(move |editor| editor.handle_printable_fallback(ch))
            .await
            .is_err()
        {
            return ControlFlow::Break(());
        }
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_buffer::BufferId;
    use arx_core::{Editor, EventLoop, WindowId};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use futures_util::stream;
    use tokio::task::JoinHandle;

    /// Spawn an event loop and seed it with a buffer+window. Returns
    /// the join handle plus the seed ids.
    async fn seeded_editor() -> (JoinHandle<Editor>, CommandBus, WindowId, BufferId) {
        let (event_loop, bus) = EventLoop::new();
        let loop_handle = tokio::spawn(event_loop.run());
        let (window_id, buffer_id) = bus
            .invoke(|editor| {
                let buf = editor.buffers_mut().create_from_text("hello", None);
                let win = editor.windows_mut().open(buf);
                (win, buf)
            })
            .await
            .unwrap();
        (loop_handle, bus, window_id, buffer_id)
    }

    #[allow(clippy::unnecessary_wraps)]
    fn key(code: KeyCode) -> std::io::Result<Event> {
        Ok(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    #[allow(clippy::unnecessary_wraps)]
    fn ctrl_key(ch: char) -> std::io::Result<Event> {
        Ok(Event::Key(KeyEvent::new(
            KeyCode::Char(ch),
            KeyModifiers::CONTROL,
        )))
    }

    #[tokio::test]
    async fn typing_a_character_inserts_via_self_insert_fallback() {
        let (loop_handle, bus, window_id, buffer_id) = seeded_editor().await;

        // Emacs keymap: 'X' is unbound, so this exercises the
        // printable-fallback path.
        let events = stream::iter(vec![key(KeyCode::Char('X')), ctrl_key('q')]);
        let task = InputTask {
            events,
            bus: bus.clone(),
            size: SharedTerminalSize::new(80, 24),
            shutdown: Shutdown::new(),
        };
        task.run().await;

        let (text, cursor) = bus
            .invoke(move |editor| {
                (
                    editor.buffers().get(buffer_id).unwrap().text(),
                    editor.windows().get(window_id).unwrap().cursor_byte,
                )
            })
            .await
            .unwrap();
        assert_eq!(text, "Xhello");
        assert_eq!(cursor, 1);

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn ctrl_f_moves_cursor_right_via_emacs_binding() {
        let (loop_handle, bus, window_id, _) = seeded_editor().await;

        let events = stream::iter(vec![ctrl_key('f'), ctrl_key('f'), ctrl_key('q')]);
        let task = InputTask {
            events,
            bus: bus.clone(),
            size: SharedTerminalSize::new(80, 24),
            shutdown: Shutdown::new(),
        };
        task.run().await;

        let cursor = bus
            .invoke(move |editor| editor.windows().get(window_id).unwrap().cursor_byte)
            .await
            .unwrap();
        assert_eq!(cursor, 2);

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn ctrl_x_ctrl_c_requests_quit_via_keymap() {
        let (loop_handle, bus, _, _) = seeded_editor().await;

        let shutdown = Shutdown::new();
        let events = stream::iter(vec![ctrl_key('x'), ctrl_key('c')]);
        let task = InputTask {
            events,
            bus: bus.clone(),
            size: SharedTerminalSize::new(80, 24),
            shutdown: shutdown.clone(),
        };
        task.run().await;

        // Input task fired shutdown because the keymap resolved
        // `C-x C-c` → `editor.quit` → `Editor::request_quit()`.
        assert!(shutdown.is_fired());

        // The event loop has exited (quit_requested drained it), so we
        // can't invoke the bus any more. Drop it and inspect the
        // returned editor instead.
        drop(bus);
        let editor = loop_handle.await.unwrap();
        assert!(editor.quit_requested());
    }

    #[tokio::test]
    async fn enter_emits_newline() {
        let (loop_handle, bus, _, buffer_id) = seeded_editor().await;

        let events = stream::iter(vec![key(KeyCode::Enter), ctrl_key('x'), ctrl_key('c')]);
        let task = InputTask {
            events,
            bus: bus.clone(),
            size: SharedTerminalSize::new(80, 24),
            shutdown: Shutdown::new(),
        };
        task.run().await;

        // Same as above: event loop has exited due to the quit, so we
        // read state from the returned Editor.
        drop(bus);
        let editor = loop_handle.await.unwrap();
        let text = editor.buffers().get(buffer_id).unwrap().text();
        assert_eq!(text, "\nhello");
    }

    #[tokio::test]
    async fn resize_updates_shared_size() {
        let (loop_handle, bus, _, _) = seeded_editor().await;

        let size = SharedTerminalSize::new(40, 10);
        let events = stream::iter(vec![Ok(Event::Resize(120, 32)), ctrl_key('x'), ctrl_key('c')]);
        let task = InputTask {
            events,
            bus: bus.clone(),
            size: size.clone(),
            shutdown: Shutdown::new(),
        };
        task.run().await;

        assert_eq!(size.get(), (120, 32));

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }
}
