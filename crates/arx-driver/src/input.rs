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

use arx_core::{CommandBus, Editor, KeyHandled};
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
        use tokio::time::{Duration, sleep};

        let InputTask {
            mut events,
            bus,
            size,
            shutdown,
        } = self;
        let mut pending_prefix = false;
        let which_key_delay = Duration::from_millis(500);

        loop {
            if pending_prefix {
                // A prefix key is pending. Race the next event against
                // a timeout. If the timeout wins, show the which-key
                // overlay.
                tokio::select! {
                    biased;
                    event = events.next() => {
                        match event {
                            Some(Ok(ev)) => {
                                let (flow, is_pending) = handle_with_pending(ev, &bus, &size).await;
                                pending_prefix = is_pending;
                                if flow.is_break() {
                                    debug!("shutdown requested from input");
                                    shutdown.fire();
                                    break;
                                }
                            }
                            Some(Err(err)) => {
                                tracing::warn!(%err, "input event error");
                                break;
                            }
                            None => break,
                        }
                    }
                    () = sleep(which_key_delay) => {
                        // Timeout: show which-key overlay.
                        let _ = bus.dispatch(|editor| {
                            editor.show_which_key();
                        }).await;
                    }
                }
            } else {
                // Normal path: wait for the next event with no timeout.
                match events.next().await {
                    Some(Ok(ev)) => {
                        trace!(?ev, "input event");
                        let (flow, is_pending) = handle_with_pending(ev, &bus, &size).await;
                        pending_prefix = is_pending;
                        if flow.is_break() {
                            debug!("shutdown requested from input");
                            shutdown.fire();
                            break;
                        }
                    }
                    Some(Err(err)) => {
                        tracing::warn!(%err, "input event error");
                        break;
                    }
                    None => break,
                }
            }
        }
    }
}

/// Returns `(control_flow, is_pending)` — the second element is true
/// when the keymap is waiting for more chords in a prefix sequence.
async fn handle_with_pending(
    event: Event,
    bus: &CommandBus,
    size: &SharedTerminalSize,
) -> (ControlFlow<()>, bool) {
    match event {
        Event::Key(key) => handle_key(key, bus).await,
        Event::Resize(cols, rows) => {
            size.set(cols, rows);
            let _ = bus.dispatch(Editor::mark_dirty).await;
            (ControlFlow::Continue(()), false)
        }
        _ => (ControlFlow::Continue(()), false),
    }
}

async fn handle_key(
    key: crossterm::event::KeyEvent,
    bus: &CommandBus,
) -> (ControlFlow<()>, bool) {
    let chord = KeyChord::from(&key);

    // Always route through the editor keymap first. Editor-level
    // bindings (window management, palette, search, etc.) take
    // priority even when a terminal pane is focused. Only truly
    // unbound keys are forwarded to the PTY.
    let bus_clone = bus.clone();
    let result = bus
        .invoke(move |editor| {
            let outcome = editor.handle_key(&bus_clone, chord);
            (outcome, editor.quit_requested())
        })
        .await;
    let Ok((outcome, quit)) = result else {
        return (ControlFlow::Break(()), false);
    };

    if quit {
        return (ControlFlow::Break(()), false);
    }

    let is_pending = outcome == KeyHandled::Pending;

    match outcome {
        KeyHandled::Unbound {
            printable_fallback: Some(ch),
        } => {
            // Printable unbound key: self-insert, or forward to the
            // terminal PTY if the active pane is a terminal.
            if bus
                .dispatch(move |editor| editor.handle_printable_fallback(ch))
                .await
                .is_err()
            {
                return (ControlFlow::Break(()), false);
            }
        }
        KeyHandled::Unbound {
            printable_fallback: None,
        } => {
            // Non-printable unbound key (arrows, Enter, Backspace,
            // etc.): forward to the PTY if the active pane is a
            // terminal; otherwise silently drop.
            let _ = bus
                .dispatch(move |editor| {
                    let Some(active) = editor.windows().active() else {
                        return;
                    };
                    if let Some(term) = editor.terminal(active) {
                        if let Some(bytes) = key_to_pty_bytes(&key) {
                            term.write(bytes);
                        }
                        editor.mark_dirty();
                    }
                })
                .await;
        }
        _ => {}
    }
    (ControlFlow::Continue(()), is_pending)
}

/// Convert a crossterm `KeyEvent` to the byte sequence a PTY
/// expects. Returns `None` for keys that don't produce output
/// (modifiers alone, etc.).
fn key_to_pty_bytes(key: &crossterm::event::KeyEvent) -> Option<Vec<u8>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char(c) => {
            if ctrl && c.is_ascii_alphabetic() {
                // Ctrl+A = 0x01, Ctrl+B = 0x02, etc.
                Some(vec![(c.to_ascii_lowercase() as u8) - b'a' + 1])
            } else {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                Some(s.as_bytes().to_vec())
            }
        }
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => Some(b"\t".to_vec()),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        _ => None,
    }
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
