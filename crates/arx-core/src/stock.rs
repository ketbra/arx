//! Stock command catalogue.
//!
//! Registers every command name declared in [`arx_keymap::commands`]
//! against concrete implementations that mutate an [`Editor`] through
//! a [`CommandContext`]. These are the commands the Emacs, Vim, and
//! KEDIT profiles bind by default.
//!
//! Shipping them in `arx-core` (rather than in `arx-driver`) keeps the
//! editor's behaviour independent of how keys are delivered — headless
//! test code, a daemon RPC, and the interactive TUI all run the same
//! code paths through the registry.

use std::sync::Arc;

use arx_buffer::{BufferId, ByteRange, EditOrigin};
use arx_keymap::{Layer, LayerId, commands as names};
use unicode_segmentation::UnicodeSegmentation;

use crate::editor::Editor;
use crate::registry::{Command, CommandContext, CommandRegistry};

/// Register every stock command into `reg`. Call once at editor start.
pub fn register_stock(reg: &mut CommandRegistry) {
    reg.register(CursorLeft);
    reg.register(CursorRight);
    reg.register(CursorUp);
    reg.register(CursorDown);
    reg.register(CursorLineStart);
    reg.register(CursorLineEnd);
    reg.register(BufferNewline);
    reg.register(BufferDeleteBackward);
    reg.register(BufferDeleteForward);
    reg.register(ScrollPageUp);
    reg.register(ScrollPageDown);
    reg.register(BufferSave);
    reg.register(EditorQuit);
    reg.register(ModeEnterInsert);
    reg.register(ModeLeaveInsert);
}

// ---------------------------------------------------------------------------
// Cursor movement
// ---------------------------------------------------------------------------

/// Resolve the active window's `(window_id, buffer_id, cursor_byte)`.
fn active(editor: &Editor) -> Option<(crate::WindowId, BufferId, usize)> {
    let id = editor.windows().active()?;
    let data = editor.windows().get(id)?;
    Some((id, data.buffer_id, data.cursor_byte))
}

macro_rules! stock_cmd {
    ($ty:ident, $name:ident, $desc:literal) => {
        struct $ty;
        impl Command for $ty {
            fn name(&self) -> &str {
                names::$name
            }
            fn description(&self) -> &'static str {
                $desc
            }
            fn run(&self, cx: &mut CommandContext<'_>) {
                Self::run_impl(cx);
            }
        }
    };
}

stock_cmd!(CursorLeft, CURSOR_LEFT, "Move the cursor one grapheme left");
impl CursorLeft {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let n = cx.count.max(1);
        for _ in 0..n {
            let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
                return;
            };
            if cursor == 0 {
                break;
            }
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                return;
            };
            let text = buffer.rope().slice_to_string(0..cursor);
            let start = text
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(idx, _)| idx);
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = start;
            }
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(CursorRight, CURSOR_RIGHT, "Move the cursor one grapheme right");
impl CursorRight {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let n = cx.count.max(1);
        for _ in 0..n {
            let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
                return;
            };
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                return;
            };
            let len = buffer.len_bytes();
            if cursor >= len {
                break;
            }
            let tail = buffer.rope().slice_to_string(cursor..len);
            let advance = tail
                .grapheme_indices(true)
                .nth(1)
                .map_or(tail.len(), |(idx, _)| idx);
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = cursor + advance;
            }
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(CursorUp, CURSOR_UP, "Move the cursor up one line");
impl CursorUp {
    fn run_impl(cx: &mut CommandContext<'_>) {
        move_vertical(cx, -1);
    }
}

stock_cmd!(CursorDown, CURSOR_DOWN, "Move the cursor down one line");
impl CursorDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        move_vertical(cx, 1);
    }
}

fn move_vertical(cx: &mut CommandContext<'_>, direction: i32) {
    let n = cx.count.max(1) as i32;
    let delta = direction * n;
    let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
        return;
    };
    let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
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
    if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
        window.cursor_byte = new_cursor;
    }
    cx.editor.mark_dirty();
}

stock_cmd!(
    CursorLineStart,
    CURSOR_LINE_START,
    "Move the cursor to the start of its line"
);
impl CursorLineStart {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let line = buffer.rope().byte_to_line(cursor);
        let start = buffer.rope().line_to_byte(line);
        if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = start;
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CursorLineEnd,
    CURSOR_LINE_END,
    "Move the cursor to the end of its line"
);
impl CursorLineEnd {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let end = if line + 1 < rope.len_lines() {
            rope.line_to_byte(line + 1).saturating_sub(1)
        } else {
            rope.len_bytes()
        };
        if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = end;
        }
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------

/// Free-function helper: insert the given text at the cursor, advance
/// the cursor, mark dirty. Used by the keymap fallback path in
/// `arx-driver` for self-insert characters — exposed because the input
/// task can't go through the command registry for literal text input
/// without a dedicated command binding.
pub fn insert_at_cursor(editor: &mut Editor, text: &str) {
    let Some((window_id, buffer_id, cursor)) = active(editor) else {
        return;
    };
    let inserted = editor
        .buffers_mut()
        .edit(buffer_id, cursor..cursor, text, EditOrigin::User);
    if inserted.is_some() {
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = cursor + text.len();
        }
        editor.mark_dirty();
    }
}

stock_cmd!(
    BufferNewline,
    BUFFER_NEWLINE,
    "Insert a newline at the cursor"
);
impl BufferNewline {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let n = cx.count.max(1);
        for _ in 0..n {
            insert_at_cursor(cx.editor, "\n");
        }
    }
}

stock_cmd!(
    BufferDeleteBackward,
    BUFFER_DELETE_BACKWARD,
    "Delete the grapheme before the cursor"
);
impl BufferDeleteBackward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let n = cx.count.max(1);
        for _ in 0..n {
            let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
                return;
            };
            if cursor == 0 {
                break;
            }
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                return;
            };
            let text = buffer.rope().slice_to_string(0..cursor);
            let start = text
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(idx, _)| idx);
            let range: ByteRange = start..cursor;
            cx.editor
                .buffers_mut()
                .edit(buffer_id, range, "", EditOrigin::User);
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = start;
            }
            cx.editor.mark_dirty();
        }
    }
}

stock_cmd!(
    BufferDeleteForward,
    BUFFER_DELETE_FORWARD,
    "Delete the grapheme at the cursor"
);
impl BufferDeleteForward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let n = cx.count.max(1);
        for _ in 0..n {
            let Some((_, buffer_id, cursor)) = active(cx.editor) else {
                return;
            };
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                return;
            };
            let len = buffer.len_bytes();
            if cursor >= len {
                break;
            }
            let tail = buffer.rope().slice_to_string(cursor..len);
            let end_in_tail = tail
                .grapheme_indices(true)
                .nth(1)
                .map_or(tail.len(), |(idx, _)| idx);
            let range: ByteRange = cursor..cursor + end_in_tail;
            cx.editor
                .buffers_mut()
                .edit(buffer_id, range, "", EditOrigin::User);
            cx.editor.mark_dirty();
        }
    }
}

// ---------------------------------------------------------------------------
// Scrolling
// ---------------------------------------------------------------------------

fn scroll_by(cx: &mut CommandContext<'_>, lines: i32) {
    let n = cx.count.max(1) as i32 * lines;
    let Some(window_id) = cx.editor.windows().active() else {
        return;
    };
    let Some(data) = cx.editor.windows().get(window_id) else {
        return;
    };
    let new_top = if n >= 0 {
        data.scroll_top_line.saturating_add(n as usize)
    } else {
        data.scroll_top_line.saturating_sub((-n) as usize)
    };
    if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
        window.scroll_top_line = new_top;
    }
    cx.editor.mark_dirty();
}

stock_cmd!(ScrollPageUp, SCROLL_PAGE_UP, "Scroll the view up one page");
impl ScrollPageUp {
    fn run_impl(cx: &mut CommandContext<'_>) {
        scroll_by(cx, -10);
    }
}

stock_cmd!(
    ScrollPageDown,
    SCROLL_PAGE_DOWN,
    "Scroll the view down one page"
);
impl ScrollPageDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        scroll_by(cx, 10);
    }
}

// ---------------------------------------------------------------------------
// File & editor
// ---------------------------------------------------------------------------

stock_cmd!(
    BufferSave,
    BUFFER_SAVE,
    "Write the active buffer to its associated path"
);
impl BufferSave {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some(buffer_id) = cx.editor.windows().active_data().map(|d| d.buffer_id) else {
            return;
        };
        let bus = cx.bus.clone();
        tokio::spawn(async move {
            match crate::file::save_file(&bus, buffer_id).await {
                Ok(path) => tracing::info!(path = %path.display(), "saved"),
                Err(err) => tracing::warn!(%err, "save failed"),
            }
            let _ = bus.dispatch(Editor::mark_dirty).await;
        });
    }
}

stock_cmd!(EditorQuit, EDITOR_QUIT, "Request editor shutdown");
impl EditorQuit {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.request_quit();
    }
}

// ---------------------------------------------------------------------------
// Mode switches
// ---------------------------------------------------------------------------

stock_cmd!(
    ModeEnterInsert,
    MODE_ENTER_INSERT,
    "Enter Vim-style insert mode"
);
impl ModeEnterInsert {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Push an empty insert layer. Keys that aren't bound here fall
        // through to the global layer (arrow keys, Backspace, Esc) and
        // printable characters become self-inserts.
        cx.editor.keymap_mut().push_layer(Layer::new(
            LayerId::from("insert"),
            Arc::new(arx_keymap::Keymap::named("vim.insert")),
        ));
        cx.editor
            .keymap_mut()
            .set_count_mode(arx_keymap::CountMode::Reject);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    ModeLeaveInsert,
    MODE_LEAVE_INSERT,
    "Leave Vim-style insert mode"
);
impl ModeLeaveInsert {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if cx.editor.keymap().has_layer("insert") {
            cx.editor.keymap_mut().pop_layer();
            // Re-enable count mode for normal.
            cx.editor
                .keymap_mut()
                .set_count_mode(arx_keymap::CountMode::Accept);
            cx.editor.mark_dirty();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventLoop;

    #[tokio::test]
    async fn cursor_right_advances_via_stock_command() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());

        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();

        let bus_clone = bus.clone();
        let cursor_after = bus
            .invoke(move |editor| {
                // Look up the command *before* constructing the context,
                // so the borrow of `editor.commands()` is released by
                // the time we borrow `editor` mutably.
                let cmd = editor.commands().get(names::CURSOR_RIGHT).unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 3,
                };
                cmd.run(&mut cx);
                cx.editor.windows().active_data().unwrap().cursor_byte
            })
            .await
            .unwrap();
        assert_eq!(cursor_after, 3);

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn mode_enter_and_leave_insert_toggles_layer() {
        use arx_keymap::{Keymap, Layer as L, LayerId as Lid};

        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());

        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_scratch();
            editor.windows_mut().open(buf);
            // Start with a fake vim.normal layer on top of the default
            // Emacs global so we can observe push/pop.
            editor.keymap_mut().push_layer(L::new(
                Lid::from("vim.normal"),
                std::sync::Arc::new(Keymap::named("vim.normal")),
            ));
        })
        .await
        .unwrap();

        let bus_clone = bus.clone();
        let top_after = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get(names::MODE_ENTER_INSERT).unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                cx.editor.keymap().top_layer().to_string()
            })
            .await
            .unwrap();
        assert_eq!(top_after, "insert");

        let bus_clone = bus.clone();
        let top_after_leave = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get(names::MODE_LEAVE_INSERT).unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                cx.editor.keymap().top_layer().to_string()
            })
            .await
            .unwrap();
        assert_eq!(top_after_leave, "vim.normal");

        drop(bus);
        let _ = handle.await.unwrap();
    }
}
