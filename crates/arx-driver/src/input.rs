//! Input task: translate terminal events into editor commands.
//!
//! Reads from a [`crossterm::event::EventStream`] (or any async stream of
//! [`Event`]) and turns each event into a dispatch onto the
//! [`arx_core::CommandBus`]. For Phase 1 the keybindings are hardcoded:
//!
//! | Event | Action |
//! |---|---|
//! | `Ctrl+Q`, `Ctrl+C`, `Esc` | request shutdown |
//! | printable char | insert at cursor |
//! | `Enter` | insert `"\n"` |
//! | `Backspace` | delete the grapheme before the cursor |
//! | `Delete` | delete the grapheme at the cursor |
//! | `Left` / `Right` | move cursor one grapheme |
//! | `Up` / `Down` | move cursor one line |
//! | `Home` / `End` | move to start / end of line |
//! | `PageUp` / `PageDown` | scroll 10 lines |
//! | `Resize(w, h)` | update the shared terminal size |
//!
//! A real keymap system lands in the next milestone (spec §15); this
//! file is deliberately a flat `match` so swapping it out is trivial.
//!
//! The handlers are written as free functions taking an owned
//! [`CommandBus`] clone so that the input task's async state machine
//! never holds `&self` across an `.await`, which keeps it `Send` under
//! tokio's multi-threaded runtime even when the event stream type is
//! not `Sync`.

use std::ops::ControlFlow;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
use tracing::{debug, trace};
use unicode_segmentation::UnicodeSegmentation;

use arx_buffer::{BufferId, ByteRange, EditOrigin};
use arx_core::{CommandBus, Editor, WindowId};

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
    /// Drive the input loop until a shutdown key is seen, the event
    /// stream ends, or the [`CommandBus`] is closed.
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
        _ => ControlFlow::Continue(()),
    }
}

async fn handle_key(key: KeyEvent, bus: &CommandBus) -> ControlFlow<()> {
    let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if matches!(key.code, KeyCode::Esc) || (is_ctrl && matches!(key.code, KeyCode::Char('q' | 'c')))
    {
        return ControlFlow::Break(());
    }

    let dispatched = match key.code {
        KeyCode::Char(ch) if !is_ctrl => dispatch_insert(bus, ch.to_string()).await,
        KeyCode::Enter => dispatch_insert(bus, "\n".into()).await,
        KeyCode::Backspace => dispatch_backspace(bus).await,
        KeyCode::Delete => dispatch_delete_forward(bus).await,
        KeyCode::Left => dispatch_move_left(bus).await,
        KeyCode::Right => dispatch_move_right(bus).await,
        KeyCode::Up => dispatch_move_vertical(bus, -1).await,
        KeyCode::Down => dispatch_move_vertical(bus, 1).await,
        KeyCode::Home => dispatch_move_home(bus).await,
        KeyCode::End => dispatch_move_end(bus).await,
        KeyCode::PageUp => dispatch_scroll(bus, -10).await,
        KeyCode::PageDown => dispatch_scroll(bus, 10).await,
        _ => Ok(()),
    };
    if dispatched.is_err() {
        return ControlFlow::Break(());
    }
    ControlFlow::Continue(())
}

/// Resolve the active window's `(window_id, buffer_id, cursor_byte)` triple.
fn active_window_cursor(editor: &Editor) -> Option<(WindowId, BufferId, usize)> {
    let id = editor.windows().active()?;
    let data = editor.windows().get(id)?;
    Some((id, data.buffer_id, data.cursor_byte))
}

async fn dispatch_insert(bus: &CommandBus, text: String) -> Result<(), ()> {
    bus.dispatch(move |editor| {
        if let Some((window_id, buffer_id, cursor)) = active_window_cursor(editor) {
            let edit =
                editor
                    .buffers_mut()
                    .edit(buffer_id, cursor..cursor, &text, EditOrigin::User);
            if edit.is_some() {
                if let Some(window) = editor.windows_mut().get_mut(window_id) {
                    window.cursor_byte = cursor + text.len();
                }
                editor.mark_dirty();
            }
        }
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_backspace(bus: &CommandBus) -> Result<(), ()> {
    bus.dispatch(|editor| {
        let Some((window_id, buffer_id, cursor)) = active_window_cursor(editor) else {
            return;
        };
        if cursor == 0 {
            return;
        }
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let text = buffer.rope().slice_to_string(0..cursor);
        let start = text
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(idx, _)| idx);
        let range: ByteRange = start..cursor;
        editor
            .buffers_mut()
            .edit(buffer_id, range, "", EditOrigin::User);
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = start;
        }
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_delete_forward(bus: &CommandBus) -> Result<(), ()> {
    bus.dispatch(|editor| {
        let Some((_, buffer_id, cursor)) = active_window_cursor(editor) else {
            return;
        };
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let len = buffer.len_bytes();
        if cursor >= len {
            return;
        }
        let tail = buffer.rope().slice_to_string(cursor..len);
        let end_in_tail = tail
            .grapheme_indices(true)
            .nth(1)
            .map_or(tail.len(), |(idx, _)| idx);
        let range: ByteRange = cursor..cursor + end_in_tail;
        editor
            .buffers_mut()
            .edit(buffer_id, range, "", EditOrigin::User);
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_move_left(bus: &CommandBus) -> Result<(), ()> {
    bus.dispatch(|editor| {
        let Some((window_id, buffer_id, cursor)) = active_window_cursor(editor) else {
            return;
        };
        if cursor == 0 {
            return;
        }
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let text = buffer.rope().slice_to_string(0..cursor);
        let start = text
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(idx, _)| idx);
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = start;
        }
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_move_right(bus: &CommandBus) -> Result<(), ()> {
    bus.dispatch(|editor| {
        let Some((window_id, buffer_id, cursor)) = active_window_cursor(editor) else {
            return;
        };
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let len = buffer.len_bytes();
        if cursor >= len {
            return;
        }
        let tail = buffer.rope().slice_to_string(cursor..len);
        let advance = tail
            .grapheme_indices(true)
            .nth(1)
            .map_or(tail.len(), |(idx, _)| idx);
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = cursor + advance;
        }
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_move_vertical(bus: &CommandBus, delta: i32) -> Result<(), ()> {
    bus.dispatch(move |editor| {
        let Some((window_id, buffer_id, cursor)) = active_window_cursor(editor) else {
            return;
        };
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let rope = buffer.rope();
        let current_line = rope.byte_to_line(cursor);
        let target_line = current_line
            .saturating_add_signed(delta as isize)
            .min(rope.len_lines().saturating_sub(1));
        let line_start = rope.line_to_byte(current_line);
        let col = cursor - line_start;
        let new_line_start = rope.line_to_byte(target_line);
        let new_line_end = if target_line + 1 < rope.len_lines() {
            rope.line_to_byte(target_line + 1).saturating_sub(1)
        } else {
            rope.len_bytes()
        };
        let new_cursor = (new_line_start + col).min(new_line_end);
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = new_cursor;
        }
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_move_home(bus: &CommandBus) -> Result<(), ()> {
    bus.dispatch(|editor| {
        let Some((window_id, buffer_id, cursor)) = active_window_cursor(editor) else {
            return;
        };
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let line = buffer.rope().byte_to_line(cursor);
        let start = buffer.rope().line_to_byte(line);
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = start;
        }
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_move_end(bus: &CommandBus) -> Result<(), ()> {
    bus.dispatch(|editor| {
        let Some((window_id, buffer_id, cursor)) = active_window_cursor(editor) else {
            return;
        };
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let end = if line + 1 < rope.len_lines() {
            rope.line_to_byte(line + 1).saturating_sub(1)
        } else {
            rope.len_bytes()
        };
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = end;
        }
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

async fn dispatch_scroll(bus: &CommandBus, delta: i32) -> Result<(), ()> {
    bus.dispatch(move |editor| {
        let Some(window_id) = editor.windows().active() else {
            return;
        };
        let Some(data) = editor.windows().get(window_id) else {
            return;
        };
        let new_top = if delta >= 0 {
            data.scroll_top_line.saturating_add(delta as usize)
        } else {
            data.scroll_top_line.saturating_sub((-delta) as usize)
        };
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.scroll_top_line = new_top;
        }
        editor.mark_dirty();
    })
    .await
    .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_buffer::BufferId;
    use arx_core::{Editor, EventLoop};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use futures_util::stream;
    use tokio::task::JoinHandle;

    /// Spawn an event loop, seed it with a fresh buffer + window, and
    /// return the join handle plus the seed ids. Callers drop the bus
    /// and `await` the handle to shut the loop down cleanly.
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

    // Wrap events in `io::Result` to match what the real crossterm
    // `EventStream` yields, so our tests exercise the same code path the
    // production driver goes through. Clippy's `unnecessary_wraps` lint
    // wants us to flatten these, but that would diverge from the real
    // item type. Silence it narrowly.
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
    async fn typing_a_character_inserts_and_advances_cursor() {
        let (loop_handle, bus, window_id, buffer_id) = seeded_editor().await;

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
                let text = editor.buffers().get(buffer_id).unwrap().text();
                let cursor = editor.windows().get(window_id).unwrap().cursor_byte;
                (text, cursor)
            })
            .await
            .unwrap();
        assert_eq!(text, "Xhello");
        assert_eq!(cursor, 1);

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn backspace_deletes_previous_grapheme() {
        let (loop_handle, bus, _, buffer_id) = seeded_editor().await;

        let events = stream::iter(vec![
            key(KeyCode::End),
            key(KeyCode::Backspace),
            key(KeyCode::Backspace),
            ctrl_key('q'),
        ]);
        let task = InputTask {
            events,
            bus: bus.clone(),
            size: SharedTerminalSize::new(80, 24),
            shutdown: Shutdown::new(),
        };
        task.run().await;

        let text = bus
            .invoke(move |editor| editor.buffers().get(buffer_id).unwrap().text())
            .await
            .unwrap();
        assert_eq!(text, "hel");

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn arrow_keys_move_cursor() {
        let (loop_handle, bus, window_id, _) = seeded_editor().await;

        let events = stream::iter(vec![
            key(KeyCode::End),
            key(KeyCode::Left),
            key(KeyCode::Left),
            ctrl_key('q'),
        ]);
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
        assert_eq!(cursor, 3);

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn resize_updates_shared_size() {
        let (loop_handle, bus, _, _) = seeded_editor().await;

        let size = SharedTerminalSize::new(40, 10);
        let events = stream::iter(vec![Ok(Event::Resize(120, 32)), ctrl_key('q')]);
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

    #[tokio::test]
    async fn ctrl_q_fires_shutdown_notify() {
        let (loop_handle, bus, _, _) = seeded_editor().await;

        let shutdown = Shutdown::new();
        let events = stream::iter(vec![ctrl_key('q')]);
        let task = InputTask {
            events,
            bus: bus.clone(),
            size: SharedTerminalSize::new(80, 24),
            shutdown: shutdown.clone(),
        };

        task.run().await;
        // `Shutdown` is sticky, so we can check after the fact without a race.
        assert!(shutdown.is_fired());
        tokio::time::timeout(std::time::Duration::from_millis(100), shutdown.wait())
            .await
            .expect("late wait should resolve immediately");

        drop(bus);
        let _ = loop_handle.await.unwrap();
    }
}
