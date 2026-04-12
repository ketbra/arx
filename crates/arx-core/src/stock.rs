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
    reg.register(CursorWordForward);
    reg.register(CursorWordBackward);
    reg.register(CursorBufferStart);
    reg.register(CursorBufferEnd);
    reg.register(BufferNewline);
    reg.register(BufferDeleteBackward);
    reg.register(BufferDeleteForward);
    reg.register(ScrollPageUp);
    reg.register(ScrollPageDown);
    reg.register(BufferSave);
    reg.register(EditorQuit);
    reg.register(ModeEnterInsert);
    reg.register(ModeLeaveInsert);
    reg.register(CommandPaletteOpen);
    reg.register(CommandPaletteClose);
    reg.register(CommandPaletteExecute);
    reg.register(CommandPaletteNext);
    reg.register(CommandPalettePrev);
    reg.register(CommandPaletteBackspace);
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
            fn description(&self) -> &str {
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
        let delta = -(cx.count.max(1) as i32);
        move_cursor_vertical_by(cx.editor, delta);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(CursorDown, CURSOR_DOWN, "Move the cursor down one line");
impl CursorDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let delta = cx.count.max(1) as i32;
        move_cursor_vertical_by(cx.editor, delta);
        cx.editor.mark_dirty();
    }
}

/// Move the active window's cursor up (`delta < 0`) or down (`delta > 0`)
/// by that many lines, preserving the byte-offset-within-line so long
/// lines don't snap to column 0.
///
/// Used by [`CursorUp`] / [`CursorDown`] *and* by the page-scroll
/// commands — moving the cursor by a page's worth of lines and letting
/// [`Editor::ensure_active_cursor_visible`] chase it is how page-up /
/// page-down end up scrolling in this editor.
fn move_cursor_vertical_by(editor: &mut Editor, delta: i32) {
    let Some(window_id) = editor.windows().active() else {
        return;
    };
    let Some(data) = editor.windows().get(window_id) else {
        return;
    };
    let buffer_id = data.buffer_id;
    let cursor = data.cursor_byte;
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

/// A character is a "word character" for the purposes of our stock
/// `M-f` / `M-b` / Vim `w` / `b` if it's alphanumeric or an underscore.
/// This matches Emacs's default `[[:word:]]` character class closely
/// enough for source-code editing without pulling in a regex crate.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Advance `cursor` forward to the start of the next word, or to the
/// end of the buffer if there is no next word. "Next word" means: skip
/// any run of non-word characters, then skip the run of word characters
/// that follows. Emacs `M-f` semantics.
fn next_word_boundary(text: &str, cursor: usize) -> usize {
    // Walk grapheme-by-grapheme so multi-byte characters don't land
    // mid-codepoint. The word-char test looks at the first char of the
    // grapheme, which is good enough for the identifier-ish characters
    // we care about.
    let tail = &text[cursor..];
    let mut idx = cursor;
    let mut saw_word = false;
    for (i, g) in tail.grapheme_indices(true) {
        let first = g.chars().next().unwrap_or(' ');
        let is_w = is_word_char(first);
        if !saw_word {
            if is_w {
                saw_word = true;
            }
        } else if !is_w {
            return cursor + i;
        }
        idx = cursor + i + g.len();
    }
    idx
}

/// Walk backward from `cursor` to the start of the previous word, or
/// to the start of the buffer if there isn't one. Emacs `M-b`.
fn prev_word_boundary(text: &str, cursor: usize) -> usize {
    let head = &text[..cursor];
    // Collect grapheme offsets once so we can walk them in reverse.
    let graphemes: Vec<(usize, &str)> = head.grapheme_indices(true).collect();
    let mut saw_word = false;
    let mut result = 0;
    for (i, g) in graphemes.into_iter().rev() {
        let first = g.chars().next().unwrap_or(' ');
        let is_w = is_word_char(first);
        if !saw_word {
            if is_w {
                saw_word = true;
                result = i;
            }
        } else if is_w {
            result = i;
        } else {
            return result;
        }
    }
    result
}

stock_cmd!(
    CursorWordForward,
    CURSOR_WORD_FORWARD,
    "Move the cursor forward one word"
);
impl CursorWordForward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let n = cx.count.max(1);
        for _ in 0..n {
            let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
                return;
            };
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                return;
            };
            let text = buffer.rope().slice_to_string(0..buffer.len_bytes());
            let next = next_word_boundary(&text, cursor);
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = next;
            }
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CursorWordBackward,
    CURSOR_WORD_BACKWARD,
    "Move the cursor backward one word"
);
impl CursorWordBackward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let n = cx.count.max(1);
        for _ in 0..n {
            let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
                return;
            };
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                return;
            };
            let text = buffer.rope().slice_to_string(0..buffer.len_bytes());
            let prev = prev_word_boundary(&text, cursor);
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = prev;
            }
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CursorBufferStart,
    CURSOR_BUFFER_START,
    "Move the cursor to the start of the buffer"
);
impl CursorBufferStart {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, _, _)) = active(cx.editor) else {
            return;
        };
        if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = 0;
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CursorBufferEnd,
    CURSOR_BUFFER_END,
    "Move the cursor to the end of the buffer"
);
impl CursorBufferEnd {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let end = buffer.len_bytes();
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

/// Compute a page size for vertical scroll commands from the active
/// window's cached viewport height. Emacs convention is to leave two
/// lines of overlap between pages so the eye can track position, which
/// works out to `visible_rows - 2`. Falls back to a conservative
/// default when the window hasn't been rendered yet.
fn active_page_size(editor: &Editor) -> i32 {
    let visible = editor
        .windows()
        .active_data()
        .map_or(0, |d| d.visible_rows);
    if visible >= 3 {
        i32::from(visible - 2)
    } else {
        // Window not rendered yet (or absurdly small). Pick something
        // better than 0 so the command isn't a no-op.
        10
    }
}

stock_cmd!(ScrollPageUp, SCROLL_PAGE_UP, "Scroll the view up one page");
impl ScrollPageUp {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let page = active_page_size(cx.editor);
        let n = cx.count.max(1) as i32 * page;
        move_cursor_vertical_by(cx.editor, -n);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    ScrollPageDown,
    SCROLL_PAGE_DOWN,
    "Scroll the view down one page"
);
impl ScrollPageDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let page = active_page_size(cx.editor);
        let n = cx.count.max(1) as i32 * page;
        move_cursor_vertical_by(cx.editor, n);
        cx.editor.mark_dirty();
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

// ---------------------------------------------------------------------------
// Command palette
// ---------------------------------------------------------------------------
//
// The command palette is opened by [`CommandPaletteOpen`] (bound to
// `M-x` in Emacs). Opening pushes a dedicated `palette` keymap layer
// so all subsequent keystrokes route to palette-control commands
// instead of the usual editor bindings — `<Up>`/`<Down>` move the
// selection, `<Enter>` executes, `<Esc>` closes, `<Backspace>` edits
// the query, and every printable key falls through the unbound path
// so [`crate::Editor::handle_printable_fallback`] appends it to the
// query.
//
// [`CommandPaletteExecute`] snapshots the selected command name,
// closes the palette (popping the layer and resetting state), and
// then invokes the captured command. Doing the close *before* the
// invocation means the executed command runs against a normal editor
// state — so `M-x buffer.save` doesn't leave the palette layer on
// top of the stack if the saved command happens to check the layer
// for some reason.

fn ensure_palette_layer(editor: &mut Editor) {
    if editor.keymap().has_layer("palette") {
        return;
    }
    editor.keymap_mut().push_layer(Layer::new(
        LayerId::from("palette"),
        Arc::new(arx_keymap::profiles::palette_layer()),
    ));
    // Disable count prefixes while the palette is open — a digit
    // typed into the search query shouldn't also multiply the next
    // command.
    editor
        .keymap_mut()
        .set_count_mode(arx_keymap::CountMode::Reject);
}

fn leave_palette_layer(editor: &mut Editor) {
    if editor.keymap().has_layer("palette") {
        editor.keymap_mut().pop_layer();
        // Restore the count mode set by the active profile. We don't
        // know which profile, so pick the safe default: both Emacs
        // and Vim's outer layers start with count-accept/reject
        // configured at `Editor::with_profile` time, and a push/pop
        // doesn't disturb that — but the palette layer changed it,
        // so reinstate the inverse here. Emacs has Reject; Vim has
        // Accept. We assume Accept as the "enabled" default and let
        // the next keystroke through either way.
        editor
            .keymap_mut()
            .set_count_mode(arx_keymap::CountMode::Accept);
    }
}

stock_cmd!(
    CommandPaletteOpen,
    COMMAND_PALETTE_OPEN,
    "Open the command palette for fuzzy command search"
);
impl CommandPaletteOpen {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // `open` snapshots the registry, so mutate it through a
        // split borrow to release the registry ref before calling
        // `open`. Simpler: clone the list out first.
        let registry_snapshot: Vec<(String, String)> = cx
            .editor
            .commands()
            .iter()
            .map(|(n, d)| (n.to_owned(), d.to_owned()))
            .collect();
        let palette = cx.editor.palette_mut();
        palette.open_with_entries(registry_snapshot);
        ensure_palette_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CommandPaletteClose,
    COMMAND_PALETTE_CLOSE,
    "Close the command palette without executing"
);
impl CommandPaletteClose {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.palette_mut().close();
        leave_palette_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CommandPaletteExecute,
    COMMAND_PALETTE_EXECUTE,
    "Execute the selected command in the palette"
);
impl CommandPaletteExecute {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Snapshot the selected command name (so we can drop the
        // palette borrow before invoking anything).
        let selected_name = cx
            .editor
            .palette()
            .selected_match()
            .map(|m| m.name.clone());
        // Close the palette BEFORE running the target command so the
        // executed command sees normal state.
        cx.editor.palette_mut().close();
        leave_palette_layer(cx.editor);
        cx.editor.mark_dirty();

        let Some(name) = selected_name else {
            return;
        };
        let Some(command) = cx.editor.commands().get(&name) else {
            tracing::warn!(%name, "palette: selected command vanished before execute");
            return;
        };
        let mut inner = CommandContext {
            editor: cx.editor,
            bus: cx.bus.clone(),
            count: 1,
        };
        command.run(&mut inner);
    }
}

stock_cmd!(
    CommandPaletteNext,
    COMMAND_PALETTE_NEXT,
    "Move the palette selection down one row"
);
impl CommandPaletteNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.palette_mut().select_next();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CommandPalettePrev,
    COMMAND_PALETTE_PREV,
    "Move the palette selection up one row"
);
impl CommandPalettePrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.palette_mut().select_prev();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CommandPaletteBackspace,
    COMMAND_PALETTE_BACKSPACE,
    "Delete the last character from the palette query"
);
impl CommandPaletteBackspace {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.palette_mut().backspace();
        cx.editor.mark_dirty();
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

    // ---- Word boundary helpers ----

    #[test]
    fn next_word_skips_non_word_then_word() {
        // Matches Emacs `forward-word` / `M-f`: finishes the current
        // word if we're inside one, then skips the gap and the next
        // word to land at its end.
        let text = "foo  bar baz";
        assert_eq!(next_word_boundary(text, 2), 3); // inside "foo" → end of "foo"
        assert_eq!(next_word_boundary(text, 3), 8); // from the gap → end of "bar"
        assert_eq!(next_word_boundary(text, 8), text.len()); // last word → EOB
    }

    #[test]
    fn prev_word_walks_back() {
        let text = "foo  bar baz";
        // From end of text, previous word start = "baz" at idx 9.
        assert_eq!(prev_word_boundary(text, text.len()), 9);
        // From inside "baz" we step back to its start.
        assert_eq!(prev_word_boundary(text, 11), 9);
        // From start of "bar" we step back to start of "foo".
        assert_eq!(prev_word_boundary(text, 5), 0);
    }

    #[tokio::test]
    async fn word_forward_and_backward_commands_move_cursor() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor
                .buffers_mut()
                .create_from_text("alpha beta gamma", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();

        let bus_clone = bus.clone();
        let after_forward = bus
            .invoke(move |editor| {
                let cmd = editor
                    .commands()
                    .get(names::CURSOR_WORD_FORWARD)
                    .unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                cx.editor.windows().active_data().unwrap().cursor_byte
            })
            .await
            .unwrap();
        // Forward-word from column 0 jumps past "alpha" and lands at
        // the start of " beta", i.e. right after "alpha".
        assert_eq!(after_forward, "alpha".len());

        let bus_clone = bus.clone();
        let after_backward = bus
            .invoke(move |editor| {
                let cmd = editor
                    .commands()
                    .get(names::CURSOR_WORD_BACKWARD)
                    .unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                cx.editor.windows().active_data().unwrap().cursor_byte
            })
            .await
            .unwrap();
        // Stepping back from the end of "alpha" lands at the start of
        // "alpha".
        assert_eq!(after_backward, 0);

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn buffer_start_and_end_commands_jump_to_edges() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello world", None);
            editor.windows_mut().open(buf);
            // Start the cursor somewhere in the middle so we can
            // observe the jumps.
            editor.windows_mut().active_data_mut().unwrap().cursor_byte = 4;
        })
        .await
        .unwrap();

        let bus_clone = bus.clone();
        let at_end = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get(names::CURSOR_BUFFER_END).unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                cx.editor.windows().active_data().unwrap().cursor_byte
            })
            .await
            .unwrap();
        assert_eq!(at_end, "hello world".len());

        let bus_clone = bus.clone();
        let at_start = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get(names::CURSOR_BUFFER_START).unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                cx.editor.windows().active_data().unwrap().cursor_byte
            })
            .await
            .unwrap();
        assert_eq!(at_start, 0);

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn page_down_uses_visible_rows_when_available() {
        // Set up a 50-line buffer with a 12-row visible area. Page
        // size should be `12 - 2 = 10`. Starting at line 0, a single
        // page-down places the cursor on line 10 (and the ensure-visible
        // step pulls scroll along with it).
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let text = (0..50)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n");
            let buf = editor.buffers_mut().create_from_text(&text, None);
            let id = editor.windows_mut().open(buf);
            let data = editor.windows_mut().get_mut(id).unwrap();
            data.visible_rows = 12;
            data.visible_cols = 40;
        })
        .await
        .unwrap();

        let bus_clone = bus.clone();
        let cursor_line = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get(names::SCROLL_PAGE_DOWN).unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                let data = cx.editor.windows().active_data().unwrap();
                let buffer = cx.editor.buffers().get(data.buffer_id).unwrap();
                buffer.rope().byte_to_line(data.cursor_byte)
            })
            .await
            .unwrap();
        assert_eq!(cursor_line, 10);

        drop(bus);
        let _ = handle.await.unwrap();
    }

    // ---- Command palette end-to-end ----

    /// Helper: run a named command against a fresh `CommandContext`.
    async fn run_named(bus: &crate::CommandBus, name: &'static str) {
        let bus_clone = bus.clone();
        bus.invoke(move |editor| {
            let cmd = editor
                .commands()
                .get(name)
                .unwrap_or_else(|| panic!("command {name:?} not registered"));
            let mut cx = CommandContext {
                editor,
                bus: bus_clone,
                count: 1,
            };
            cmd.run(&mut cx);
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn palette_open_pushes_layer_and_populates_matches() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();

        run_named(&bus, names::COMMAND_PALETTE_OPEN).await;

        let (is_open, top_layer, match_count) = bus
            .invoke(|editor| {
                (
                    editor.palette().is_open(),
                    editor.keymap().top_layer().to_string(),
                    editor.palette().matches().len(),
                )
            })
            .await
            .unwrap();
        assert!(is_open);
        assert_eq!(top_layer, "palette");
        // Every stock command should show up when the query is empty.
        assert!(match_count >= 20, "got {match_count} matches");

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn palette_printable_fallback_appends_to_query() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();

        run_named(&bus, names::COMMAND_PALETTE_OPEN).await;

        // Pump four characters through handle_printable_fallback —
        // that's what the driver's input task does for unbound
        // printable keys.
        bus.invoke(|editor| {
            for ch in "curs".chars() {
                editor.handle_printable_fallback(ch);
            }
        })
        .await
        .unwrap();

        let (query, top_name) = bus
            .invoke(|editor| {
                (
                    editor.palette().query().to_owned(),
                    editor
                        .palette()
                        .selected_match()
                        .map(|m| m.name.clone()),
                )
            })
            .await
            .unwrap();
        assert_eq!(query, "curs");
        // Every `cursor.*` command starts with "curs" — so the top
        // match had better start with it too.
        let name = top_name.unwrap();
        assert!(name.starts_with("cursor."), "top match was {name:?}");

        // The buffer itself must NOT have changed: "curs" went to the
        // palette query, not the active buffer.
        let text = bus
            .invoke(|editor| {
                let id = editor.windows().active().unwrap();
                let buf = editor.windows().get(id).unwrap().buffer_id;
                editor.buffers().get(buf).unwrap().text()
            })
            .await
            .unwrap();
        assert_eq!(text, "hello");

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn palette_execute_runs_selected_command_and_closes() {
        // Full round trip: open, narrow to `cursor.right`, execute,
        // verify the cursor moved AND the palette is closed.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();

        run_named(&bus, names::COMMAND_PALETTE_OPEN).await;
        bus.invoke(|editor| {
            // Query that uniquely matches cursor.right as the top hit.
            for ch in "cursor.righ".chars() {
                editor.handle_printable_fallback(ch);
            }
        })
        .await
        .unwrap();

        let top = bus
            .invoke(|editor| {
                editor
                    .palette()
                    .selected_match()
                    .map(|m| m.name.clone())
                    .unwrap()
            })
            .await
            .unwrap();
        assert_eq!(top, "cursor.right");

        run_named(&bus, names::COMMAND_PALETTE_EXECUTE).await;

        let (cursor, is_open, top_layer) = bus
            .invoke(|editor| {
                let id = editor.windows().active().unwrap();
                (
                    editor.windows().get(id).unwrap().cursor_byte,
                    editor.palette().is_open(),
                    editor.keymap().top_layer().to_string(),
                )
            })
            .await
            .unwrap();
        assert_eq!(cursor, 1, "cursor.right should have advanced one grapheme");
        assert!(!is_open);
        assert_ne!(top_layer, "palette");

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn palette_close_resets_without_executing() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();

        run_named(&bus, names::COMMAND_PALETTE_OPEN).await;
        bus.invoke(|editor| editor.handle_printable_fallback('c'))
            .await
            .unwrap();
        run_named(&bus, names::COMMAND_PALETTE_CLOSE).await;

        let (is_open, top_layer, cursor) = bus
            .invoke(|editor| {
                let id = editor.windows().active().unwrap();
                (
                    editor.palette().is_open(),
                    editor.keymap().top_layer().to_string(),
                    editor.windows().get(id).unwrap().cursor_byte,
                )
            })
            .await
            .unwrap();
        assert!(!is_open);
        assert_ne!(top_layer, "palette");
        // No command ran, so the cursor stayed at 0.
        assert_eq!(cursor, 0);

        drop(bus);
        let _ = handle.await.unwrap();
    }
}
