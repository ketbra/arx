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
use std::time::{Duration as StdDuration, Instant};

use crossterm::event::Event;
use futures_util::StreamExt;
use tracing::{debug, trace};

use arx_core::{CommandBus, Editor, KeyHandled};
use arx_keymap::KeyChord;

use crate::state::{SharedTerminalSize, Shutdown};

/// Maximum time between consecutive clicks to count as a multi-click.
const MULTI_CLICK_INTERVAL: StdDuration = StdDuration::from_millis(400);

/// State for tracking double/triple clicks.
#[derive(Debug, Clone, Copy, Default)]
struct ClickState {
    /// When the last left-mouse-down occurred.
    last_time: Option<Instant>,
    /// Screen (x, y) of the last click.
    last_pos: (u16, u16),
    /// How many consecutive clicks have landed at roughly the same
    /// position within `MULTI_CLICK_INTERVAL`. `1` = single click,
    /// `2` = double click, `3` = triple click. Caps at 3.
    count: u8,
}

impl ClickState {
    /// Record a new left-click at `(x, y)`. Returns the click count
    /// (1, 2, or 3) after this click.
    fn record(&mut self, x: u16, y: u16) -> u8 {
        let now = Instant::now();
        let is_continuation = self.last_time.is_some_and(|t| {
            now.duration_since(t) <= MULTI_CLICK_INTERVAL
                && self.last_pos == (x, y)
        });
        if is_continuation {
            self.count = (self.count + 1).min(3);
        } else {
            self.count = 1;
        }
        self.last_time = Some(now);
        self.last_pos = (x, y);
        self.count
    }
}

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
        let mut click_state = ClickState::default();
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
                                let (flow, is_pending) = handle_with_pending(ev, &bus, &size, &mut click_state).await;
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
                        let (flow, is_pending) = handle_with_pending(ev, &bus, &size, &mut click_state).await;
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
    click_state: &mut ClickState,
) -> (ControlFlow<()>, bool) {
    match event {
        Event::Key(key) => handle_key(key, bus).await,
        Event::Resize(cols, rows) => {
            size.set(cols, rows);
            let _ = bus.dispatch(Editor::mark_dirty).await;
            (ControlFlow::Continue(()), false)
        }
        Event::Mouse(mev) => {
            handle_mouse(mev, bus, size, click_state).await;
            (ControlFlow::Continue(()), false)
        }
        _ => (ControlFlow::Continue(()), false),
    }
}

/// Handle a mouse event.
///
/// - Left-click (count=1): moves the cursor, clears selection. If
///   Shift is held, extends the selection instead (keeps the mark).
/// - Left-click (count=2): selects the word at the click.
/// - Left-click (count=3): selects the entire line.
/// - Left-drag: updates the cursor with the mark anchored at the
///   click position, creating a selection.
/// - Wheel up/down: scrolls the pane under the cursor.
async fn handle_mouse(
    mev: crossterm::event::MouseEvent,
    bus: &CommandBus,
    size: &SharedTerminalSize,
    click_state: &mut ClickState,
) {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    let (cols, rows) = size.get();
    let x = mev.column;
    let y = mev.row;
    let shift = mev.modifiers.contains(KeyModifiers::SHIFT);
    match mev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let count = click_state.record(x, y);
            let kind = match count {
                2 => crate::render::ClickKind::DoubleClick,
                3 => crate::render::ClickKind::TripleClick,
                _ if shift => crate::render::ClickKind::ShiftClick,
                _ => crate::render::ClickKind::Single,
            };
            let _ = bus
                .dispatch(move |editor| {
                    crate::render::hit_test_and_click(editor, cols, rows, x, y, kind);
                })
                .await;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let _ = bus
                .dispatch(move |editor| {
                    crate::render::hit_test_and_click(
                        editor,
                        cols,
                        rows,
                        x,
                        y,
                        crate::render::ClickKind::Drag,
                    );
                })
                .await;
        }
        MouseEventKind::ScrollUp => {
            let _ = bus
                .dispatch(move |editor| {
                    crate::render::mouse_scroll(editor, cols, rows, x, y, -3);
                })
                .await;
        }
        MouseEventKind::ScrollDown => {
            let _ = bus
                .dispatch(move |editor| {
                    crate::render::mouse_scroll(editor, cols, rows, x, y, 3);
                })
                .await;
        }
        _ => {}
    }
}

async fn handle_key(
    key: crossterm::event::KeyEvent,
    bus: &CommandBus,
) -> (ControlFlow<()>, bool) {
    use crossterm::event::KeyEventKind;
    // With the Kitty keyboard protocol enabled some terminals report
    // both press and release events. We only want to act on press/repeat —
    // releases would double-trigger bindings.
    if matches!(key.kind, KeyEventKind::Release) {
        return (ControlFlow::Continue(()), false);
    }

    let chord = KeyChord::from(&key);
    // Log the raw event and the derived chord. Users debugging
    // key-binding issues (especially around Ctrl+/ / Ctrl+_ / M-<
    // that terminals report inconsistently) can run with
    // `RUST_LOG=arx_driver::input=debug` to see exactly what the
    // terminal is sending.
    debug!(?key, %chord, "key event");

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
