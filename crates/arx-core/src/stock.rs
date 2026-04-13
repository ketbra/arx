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
use std::time::SystemTime;

use arx_buffer::{BufferId, ByteRange, EditOrigin, EditRecord};
use arx_keymap::{Layer, LayerId, commands as names};
use unicode_segmentation::UnicodeSegmentation;

use crate::editor::Editor;
use crate::registry::{Command, CommandContext, CommandRegistry};
use crate::window::SplitAxis;
use crate::WindowId;

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
    reg.register(ScrollRecenter);
    reg.register(BufferKillLine);
    reg.register(BufferKillWord);
    reg.register(BufferKillWordBackward);
    reg.register(BufferKillRegion);
    reg.register(BufferCopyRegion);
    reg.register(BufferYank);
    reg.register(BufferSetMark);
    reg.register(BufferFindFile);
    reg.register(BufferClose);
    reg.register(BufferSwitch);
    reg.register(BufferOpenLine);
    reg.register(BufferTransposeChars);
    reg.register(BufferSave);
    reg.register(EditorQuit);
    reg.register(EditorCancel);
    reg.register(EditorDescribeKey);
    reg.register(ModeEnterInsert);
    reg.register(ModeLeaveInsert);
    reg.register(CommandPaletteOpen);
    reg.register(CommandPaletteClose);
    reg.register(CommandPaletteExecute);
    reg.register(CommandPaletteNext);
    reg.register(CommandPalettePrev);
    reg.register(CommandPaletteBackspace);
    reg.register(CommandPaletteHistoryPrev);
    reg.register(CommandPaletteHistoryNext);
    reg.register(WindowSplitHorizontal);
    reg.register(WindowSplitVertical);
    reg.register(WindowClose);
    reg.register(WindowDeleteOther);
    reg.register(WindowFocusNext);
    reg.register(WindowFocusPrev);
    reg.register(BufferUndo);
    reg.register(BufferRedo);
    reg.register(BufferUndoBranchNext);
    reg.register(BufferUndoBranchPrev);
    reg.register(LspHover);
    reg.register(LspNextDiagnostic);
    reg.register(LspPrevDiagnostic);
    reg.register(CompletionTrigger);
    reg.register(TerminalOpen);
    reg.register(SearchOpen);
    reg.register(SearchClose);
    reg.register(SearchExecute);
    reg.register(SearchNext);
    reg.register(SearchPrev);
    reg.register(SearchPageDown);
    reg.register(SearchPageUp);
    reg.register(SearchToggleMode);
    reg.register(SearchBackspace);
    reg.register(SearchHistoryPrev);
    reg.register(SearchHistoryNext);
    reg.register(RectKill);
    reg.register(RectCopy);
    reg.register(RectYank);
    reg.register(RectOpen);
    reg.register(ModeEnterVisualBlock);
    reg.register(ModeLeaveVisualBlock);
    reg.register(CompletionAccept);
    reg.register(CompletionDismiss);
    reg.register(CompletionNext);
    reg.register(CompletionPrev);
    reg.register(CompletionPageDown);
    reg.register(CompletionPageUp);
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

/// Apply a user edit to `buffer_id` *and* push a matching
/// [`EditRecord`] to the buffer's undo tree, then set the invoking
/// window's cursor to `cursor_after`. This is the single path every
/// user-initiated stock edit routes through, so undo / redo can
/// round-trip the full editor state (content + cursor) against one
/// record per logical edit.
///
/// `Io`- and `System`-origin edits (file reload, undo application
/// itself, agent merges) go directly through
/// [`crate::BufferManager::edit`] and deliberately *do not* show up
/// in the undo tree — the user didn't type them.
pub(crate) fn user_edit(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    range: ByteRange,
    text: &str,
    cursor_before: usize,
    cursor_after: usize,
) -> bool {
    // Capture the bytes that will be removed BEFORE we apply the
    // edit, so the undo tree gets the pre-edit content.
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return false;
    };
    let removed = buffer.rope().slice_to_string(range.clone());
    let offset = range.start;
    let inserted_text = text.to_owned();

    // Apply the edit and update syntax highlights in one step.
    let applied = editor
        .edit_with_highlight(buffer_id, range, text, EditOrigin::User)
        .is_some();
    if !applied {
        return false;
    }

    if let Some(window) = editor.windows_mut().get_mut(window_id) {
        window.cursor_byte = cursor_after;
    }
    if let Some(buffer) = editor.buffers_mut().get_mut(buffer_id) {
        buffer.undo_tree_mut().push(EditRecord {
            offset,
            removed,
            inserted: inserted_text,
            cursor_before,
            cursor_after,
            timestamp: SystemTime::now(),
        });
    }
    // Notify the LSP manager of the content change.
    #[cfg(feature = "lsp")]
    if let Some(buffer) = editor.buffers().get(buffer_id) {
        editor.notify_lsp(arx_lsp::LspEvent::BufferEdited {
            buffer_id,
            new_text: buffer.text(),
        });
    }
    editor.mark_dirty();
    true
}

/// Free-function helper: insert the given text at the cursor, advance
/// the cursor, mark dirty, record in the undo tree. Used by the
/// keymap fallback path in `arx-driver` for self-insert characters —
/// exposed because the input task can't go through the command
/// registry for literal text input without a dedicated command
/// binding.
pub fn insert_at_cursor(editor: &mut Editor, text: &str) {
    let Some((window_id, buffer_id, cursor)) = active(editor) else {
        return;
    };
    let cursor_after = cursor + text.len();
    user_edit(
        editor,
        window_id,
        buffer_id,
        cursor..cursor,
        text,
        cursor,
        cursor_after,
    );
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
            user_edit(
                cx.editor, window_id, buffer_id, range, "", cursor, start,
            );
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
            let end_in_tail = tail
                .grapheme_indices(true)
                .nth(1)
                .map_or(tail.len(), |(idx, _)| idx);
            let range: ByteRange = cursor..cursor + end_in_tail;
            user_edit(
                cx.editor, window_id, buffer_id, range, "", cursor, cursor,
            );
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
// Recenter
// ---------------------------------------------------------------------------

stock_cmd!(
    ScrollRecenter,
    SCROLL_RECENTER,
    "Scroll the window to center the cursor vertically"
);
impl ScrollRecenter {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some(window_id) = cx.editor.windows().active() else {
            return;
        };
        let Some(data) = cx.editor.windows().get(window_id) else {
            return;
        };
        let visible = data.visible_rows as usize;
        if visible == 0 {
            return;
        }
        let buffer_id = data.buffer_id;
        let cursor = data.cursor_byte;
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let cursor_line = buffer.rope().byte_to_line(cursor);
        let new_top = cursor_line.saturating_sub(visible / 2);
        if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
            window.scroll_top_line = new_top;
        }
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Kill / yank / mark
// ---------------------------------------------------------------------------

stock_cmd!(
    BufferKillLine,
    BUFFER_KILL_LINE,
    "Kill from the cursor to the end of the line"
);
impl BufferKillLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let line_end = if line + 1 < rope.len_lines() {
            rope.line_to_byte(line + 1).saturating_sub(1)
        } else {
            rope.len_bytes()
        };
        // If cursor is already at line end, kill the newline.
        let end = if cursor == line_end && line + 1 < rope.len_lines() {
            line_end + 1
        } else {
            line_end
        };
        if cursor >= end {
            return;
        }
        let killed = rope.slice_to_string(cursor..end);
        let range: ByteRange = cursor..end;
        user_edit(cx.editor, window_id, buffer_id, range, "", cursor, cursor);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
    }
}

stock_cmd!(
    BufferKillWord,
    BUFFER_KILL_WORD,
    "Kill the word after the cursor"
);
impl BufferKillWord {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let text = buffer.rope().slice_to_string(0..buffer.len_bytes());
        let end = next_word_boundary(&text, cursor);
        if end <= cursor {
            return;
        }
        let killed = text[cursor..end].to_owned();
        let range: ByteRange = cursor..end;
        user_edit(cx.editor, window_id, buffer_id, range, "", cursor, cursor);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
    }
}

stock_cmd!(
    BufferKillWordBackward,
    BUFFER_KILL_WORD_BACKWARD,
    "Kill the word before the cursor"
);
impl BufferKillWordBackward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let text = buffer.rope().slice_to_string(0..buffer.len_bytes());
        let start = prev_word_boundary(&text, cursor);
        if start >= cursor {
            return;
        }
        let killed = text[start..cursor].to_owned();
        let range: ByteRange = start..cursor;
        user_edit(cx.editor, window_id, buffer_id, range, "", cursor, start);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
    }
}

stock_cmd!(
    BufferSetMark,
    BUFFER_SET_MARK,
    "Set the mark at the cursor (start a selection)"
);
impl BufferSetMark {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, _, cursor)) = active(cx.editor) else {
            return;
        };
        cx.editor.set_mark(window_id, cursor);
        cx.editor.set_status("Mark set");
    }
}

stock_cmd!(
    BufferKillRegion,
    BUFFER_KILL_REGION,
    "Kill (cut) the region between mark and cursor"
);
impl BufferKillRegion {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No mark set");
            return;
        };
        let start = mark.min(cursor);
        let end = mark.max(cursor);
        if start == end {
            return;
        }
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let killed = buffer.rope().slice_to_string(start..end);
        let range: ByteRange = start..end;
        user_edit(cx.editor, window_id, buffer_id, range, "", cursor, start);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
        cx.editor.clear_mark(window_id);
    }
}

stock_cmd!(
    BufferCopyRegion,
    BUFFER_COPY_REGION,
    "Copy the region between mark and cursor"
);
impl BufferCopyRegion {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No mark set");
            return;
        };
        let start = mark.min(cursor);
        let end = mark.max(cursor);
        if start == end {
            return;
        }
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let copied = buffer.rope().slice_to_string(start..end);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(copied));
        cx.editor.clear_mark(window_id);
        cx.editor.set_status("Region copied");
    }
}

stock_cmd!(BufferYank, BUFFER_YANK, "Yank (paste) the most recently killed text");
impl BufferYank {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(entry) = cx.editor.kill_ring_top().cloned() else {
            cx.editor.set_status("Kill ring empty");
            return;
        };
        match entry {
            crate::editor::KilledText::Linear(text) => {
                let len = text.len();
                user_edit(
                    cx.editor,
                    window_id,
                    buffer_id,
                    cursor..cursor,
                    &text,
                    cursor,
                    cursor + len,
                );
            }
            crate::editor::KilledText::Rectangular(lines) => {
                crate::column::yank_rectangle(cx.editor, window_id, buffer_id, &lines);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Buffer management
// ---------------------------------------------------------------------------

stock_cmd!(
    BufferFindFile,
    BUFFER_FIND_FILE,
    "Open a file by path"
);
impl BufferFindFile {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.palette_mut().open_find_file();
        ensure_palette_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    BufferClose,
    BUFFER_CLOSE,
    "Close the active buffer"
);
impl BufferClose {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        // Don't close the last window — use editor.quit for that.
        let leaf_count = cx
            .editor
            .windows()
            .layout()
            .map_or(0, |l| l.leaves().len());
        if leaf_count <= 1 {
            cx.editor.set_status("Last window — use C-x C-c to quit");
            return;
        }
        cx.editor.buffers_mut().close(buffer_id);
        cx.editor.windows_mut().close(window_id);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    BufferSwitch,
    BUFFER_SWITCH,
    "Switch to a different open buffer"
);
impl BufferSwitch {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Open the palette in switch-buffer mode. The description
        // carries the buffer id as a parseable string.
        let buffers: Vec<(String, String)> = cx
            .editor
            .buffers()
            .ids()
            .map(|id| {
                let label = cx
                    .editor
                    .buffers()
                    .path(id)
                    .and_then(|p| p.file_name())
                    .map_or_else(
                        || format!("*scratch-{}*", id.0),
                        |n| n.to_string_lossy().into_owned(),
                    );
                (label, id.0.to_string())
            })
            .collect();
        cx.editor.palette_mut().open_switch_buffer(buffers);
        ensure_palette_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Utility editing
// ---------------------------------------------------------------------------

stock_cmd!(
    BufferOpenLine,
    BUFFER_OPEN_LINE,
    "Insert a newline at the cursor without moving the cursor"
);
impl BufferOpenLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        // Insert a newline but keep the cursor at the current position.
        user_edit(
            cx.editor,
            window_id,
            buffer_id,
            cursor..cursor,
            "\n",
            cursor,
            cursor,
        );
    }
}

stock_cmd!(
    BufferTransposeChars,
    BUFFER_TRANSPOSE_CHARS,
    "Swap the character at the cursor with the one before it"
);
impl BufferTransposeChars {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        if cursor == 0 {
            return;
        }
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let len = buffer.len_bytes();
        if cursor >= len {
            return;
        }
        let text = buffer.rope().slice_to_string(0..len);
        // Find the grapheme before and at the cursor.
        let before = text[..cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(i, g)| (i, g.to_owned()));
        let at = text[cursor..]
            .grapheme_indices(true)
            .next()
            .map(|(_, g)| g.to_owned());
        let Some((before_start, before_g)) = before else {
            return;
        };
        let Some(at_g) = at else {
            return;
        };
        let end = cursor + at_g.len();
        let swapped = format!("{at_g}{before_g}");
        let range: ByteRange = before_start..end;
        user_edit(
            cx.editor,
            window_id,
            buffer_id,
            range,
            &swapped,
            cursor,
            end,
        );
    }
}

stock_cmd!(
    EditorCancel,
    EDITOR_CANCEL,
    "Cancel the current operation"
);
impl EditorCancel {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Close any open overlay (palette, completion).
        if cx.editor.palette().is_open() {
            cx.editor.palette_mut().close();
            leave_palette_layer(cx.editor);
        }
        if cx.editor.completion().is_open() {
            cx.editor.completion_mut().dismiss();
            leave_completion_layer(cx.editor);
        }
        // Clear the mark.
        if let Some(id) = cx.editor.windows().active() {
            cx.editor.clear_mark(id);
        }
        cx.editor.set_status("Quit");
    }
}

stock_cmd!(
    EditorDescribeKey,
    EDITOR_DESCRIBE_KEY,
    "Describe what a key sequence is bound to"
);
impl EditorDescribeKey {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.enter_describe_key_mode();
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
    #[allow(clippy::too_many_lines)]
    fn run_impl(cx: &mut CommandContext<'_>) {
        let mode = cx.editor.palette().mode();
        match mode {
            crate::palette::PaletteMode::FindFile => {
                // Use the selected match (full path from the listing)
                // if one exists, otherwise fall back to the raw query.
                let path = cx
                    .editor
                    .palette()
                    .selected_match()
                    .map_or_else(
                        || cx.editor.palette().query().to_owned(),
                        |m| m.name.clone(),
                    );
                if path.is_empty() {
                    cx.editor.palette_mut().close();
                    leave_palette_layer(cx.editor);
                    return;
                }
                // If it's a directory, navigate into it instead of
                // opening. Replace the query and re-list.
                if path.ends_with('/') {
                    cx.editor.palette_mut().set_query(path);
                    cx.editor.palette_mut().refresh_find_file_pub();
                    cx.editor.mark_dirty();
                    return;
                }
                cx.editor.palette_mut().push_history(
                    path.clone(),
                    crate::palette::PaletteMode::FindFile,
                );
                cx.editor.palette_mut().close();
                leave_palette_layer(cx.editor);
                cx.editor.mark_dirty();
                let bus = cx.bus.clone();
                let path = std::path::PathBuf::from(path);
                tokio::spawn(async move {
                    match crate::open_file(&bus, path.clone()).await {
                        Ok(_) => {
                            tracing::info!(path = %path.display(), "opened file");
                            let _ = bus.dispatch(Editor::mark_dirty).await;
                        }
                        Err(err) => {
                            tracing::warn!(%err, "find-file failed");
                            let msg = format!("Error: {err}");
                            let _ = bus
                                .dispatch(move |editor| editor.set_status(msg))
                                .await;
                        }
                    }
                });
            }
            crate::palette::PaletteMode::SwitchBuffer => {
                // The selected match's description is the buffer id.
                let selected = cx
                    .editor
                    .palette()
                    .selected_match()
                    .and_then(|m| m.description.parse::<u64>().ok())
                    .map(arx_buffer::BufferId);
                cx.editor.palette_mut().close();
                leave_palette_layer(cx.editor);
                cx.editor.mark_dirty();

                if let Some(buffer_id) = selected {
                    // Switch the active window to show this buffer.
                    let Some(window_id) = cx.editor.windows().active() else {
                        return;
                    };
                    if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                        window.buffer_id = buffer_id;
                        window.cursor_byte = 0;
                        window.scroll_top_line = 0;
                        window.scroll_left_col = 0;
                    }
                    // Re-attach syntax highlighting for the new buffer.
                    let ext = cx
                        .editor
                        .buffers()
                        .path(buffer_id)
                        .and_then(|p| p.extension())
                        .and_then(|e| e.to_str())
                        .map(str::to_owned);
                    cx.editor.attach_highlight(buffer_id, ext.as_deref());
                    cx.editor.mark_dirty();
                }
            }
            crate::palette::PaletteMode::Command => {
                // Snapshot the selected command name.
                let selected_name = cx
                    .editor
                    .palette()
                    .selected_match()
                    .map(|m| m.name.clone());
                if let Some(ref name) = selected_name {
                    cx.editor.palette_mut().push_history(
                        name.clone(),
                        crate::palette::PaletteMode::Command,
                    );
                }
                cx.editor.palette_mut().close();
                leave_palette_layer(cx.editor);
                cx.editor.mark_dirty();

                let Some(name) = selected_name else {
                    return;
                };
                let Some(command) = cx.editor.commands().get(&name) else {
                    tracing::warn!(%name, "palette: command vanished");
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

stock_cmd!(
    CommandPaletteHistoryPrev,
    COMMAND_PALETTE_HISTORY_PREV,
    "Navigate to the previous palette history entry"
);
impl CommandPaletteHistoryPrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.palette_mut().history_prev();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CommandPaletteHistoryNext,
    COMMAND_PALETTE_HISTORY_NEXT,
    "Navigate to the next palette history entry"
);
impl CommandPaletteHistoryNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.palette_mut().history_next();
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Window splits
// ---------------------------------------------------------------------------
//
// The split commands all go through `WindowManager::split_active` /
// `focus_next` / `focus_prev` / `close`, which own the layout tree and
// handle the tricky cases (collapsing a split back into its sibling
// when a pane closes, picking a fresh active window if the closed one
// was active, etc.). The stock commands here are thin wrappers that
// also mark the editor dirty so the render task wakes up.
//
// A new pane always opens on the *same buffer* as the pane that's
// being split, so splitting gives you two views of the same content
// by default. Switching one of them to a different buffer is follow-up
// work (a buffer-switcher or an `:edit` command).

fn split_active_into(editor: &mut Editor, axis: SplitAxis) {
    let Some(active) = editor.windows().active() else {
        return;
    };
    let Some(buffer_id) = editor.windows().get(active).map(|d| d.buffer_id) else {
        return;
    };
    if editor.windows_mut().split_active(axis, buffer_id).is_some() {
        editor.mark_dirty();
    }
}

stock_cmd!(
    WindowSplitHorizontal,
    WINDOW_SPLIT_HORIZONTAL,
    "Split the active window horizontally (new pane below)"
);
impl WindowSplitHorizontal {
    fn run_impl(cx: &mut CommandContext<'_>) {
        split_active_into(cx.editor, SplitAxis::Horizontal);
    }
}

stock_cmd!(
    WindowSplitVertical,
    WINDOW_SPLIT_VERTICAL,
    "Split the active window vertically (new pane to the right)"
);
impl WindowSplitVertical {
    fn run_impl(cx: &mut CommandContext<'_>) {
        split_active_into(cx.editor, SplitAxis::Vertical);
    }
}

stock_cmd!(
    WindowClose,
    WINDOW_CLOSE,
    "Close the active window, collapsing its split into the surviving sibling"
);
impl WindowClose {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some(active) = cx.editor.windows().active() else {
            return;
        };
        // Refuse to close the last visible pane — otherwise the render
        // task has nothing to draw and commands like cursor motions
        // silently no-op. `editor.quit` is the command for exiting.
        let leaf_count = cx
            .editor
            .windows()
            .layout()
            .map_or(0, |l| l.leaves().len());
        if leaf_count <= 1 {
            return;
        }
        if cx.editor.windows_mut().close(active) {
            cx.editor.mark_dirty();
            cx.editor.ensure_active_cursor_visible();
        }
    }
}

stock_cmd!(
    WindowDeleteOther,
    WINDOW_DELETE_OTHER,
    "Close all windows except the active one"
);
impl WindowDeleteOther {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if cx.editor.windows_mut().delete_other() {
            cx.editor.mark_dirty();
            cx.editor.ensure_active_cursor_visible();
        }
    }
}

stock_cmd!(
    WindowFocusNext,
    WINDOW_FOCUS_NEXT,
    "Cycle focus to the next window in the layout"
);
impl WindowFocusNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if cx.editor.windows_mut().focus_next().is_some() {
            cx.editor.mark_dirty();
            cx.editor.ensure_active_cursor_visible();
        }
    }
}

stock_cmd!(
    WindowFocusPrev,
    WINDOW_FOCUS_PREV,
    "Cycle focus to the previous window in the layout"
);
impl WindowFocusPrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if cx.editor.windows_mut().focus_prev().is_some() {
            cx.editor.mark_dirty();
            cx.editor.ensure_active_cursor_visible();
        }
    }
}

// ---------------------------------------------------------------------------
// Undo / redo
// ---------------------------------------------------------------------------
//
// The undo tree lives on [`arx_buffer::Buffer`] as pure bookkeeping;
// `user_edit` above pushes a record after every user-visible edit.
// `buffer.undo` and `buffer.redo` pop a record off the tree and
// apply it back against the buffer — undo inverts (replace inserted
// bytes with removed bytes); redo replays forward. Both origin the
// resulting edit as `EditOrigin::System` so the undo-application
// itself doesn't re-enter the tree.
//
// After the edit is applied the invoking window's cursor is set to
// the position recorded in the record (`cursor_before` for undo,
// `cursor_after` for redo). Any *other* windows that happen to be
// viewing the same buffer get their cursors clamped to the buffer's
// new byte length so a shortened buffer can't leave them pointing
// into hyperspace — they keep their existing position otherwise.

fn clamp_cursors_to_buffer_end(editor: &mut Editor, buffer_id: BufferId) {
    let Some(len) = editor
        .buffers()
        .get(buffer_id)
        .map(arx_buffer::Buffer::len_bytes)
    else {
        return;
    };
    let affected: Vec<WindowId> = editor
        .windows()
        .iter()
        .filter_map(|(id, data)| {
            if data.buffer_id == buffer_id && data.cursor_byte > len {
                Some(id)
            } else {
                None
            }
        })
        .collect();
    for id in affected {
        if let Some(window) = editor.windows_mut().get_mut(id) {
            window.cursor_byte = len;
        }
    }
}

fn apply_undo_record(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    record: &EditRecord,
) {
    // Inverting the edit means: at `offset`, replace the `inserted`
    // span with the `removed` bytes.
    let invert_range = record.offset..record.offset + record.inserted.len();
    editor.buffers_mut().edit(
        buffer_id,
        invert_range,
        &record.removed,
        EditOrigin::System,
    );
    if let Some(window) = editor.windows_mut().get_mut(window_id) {
        window.cursor_byte = record.cursor_before;
    }
    clamp_cursors_to_buffer_end(editor, buffer_id);
    editor.mark_dirty();
    editor.ensure_active_cursor_visible();
}

fn apply_redo_record(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    record: &EditRecord,
) {
    let redo_range = record.offset..record.offset + record.removed.len();
    editor.buffers_mut().edit(
        buffer_id,
        redo_range,
        &record.inserted,
        EditOrigin::System,
    );
    if let Some(window) = editor.windows_mut().get_mut(window_id) {
        window.cursor_byte = record.cursor_after;
    }
    clamp_cursors_to_buffer_end(editor, buffer_id);
    editor.mark_dirty();
    editor.ensure_active_cursor_visible();
}

stock_cmd!(BufferUndo, BUFFER_UNDO, "Undo the last user edit");
impl BufferUndo {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        let n = cx.count.max(1);
        for _ in 0..n {
            let record = cx
                .editor
                .buffers_mut()
                .get_mut(buffer_id)
                .and_then(|b| b.undo_tree_mut().undo());
            let Some(record) = record else {
                break;
            };
            apply_undo_record(cx.editor, window_id, buffer_id, &record);
        }
    }
}

stock_cmd!(BufferRedo, BUFFER_REDO, "Redo the last undone edit");
impl BufferRedo {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        let n = cx.count.max(1);
        for _ in 0..n {
            let record = cx
                .editor
                .buffers_mut()
                .get_mut(buffer_id)
                .and_then(|b| b.undo_tree_mut().redo());
            let Some(record) = record else {
                break;
            };
            apply_redo_record(cx.editor, window_id, buffer_id, &record);
        }
    }
}

stock_cmd!(
    BufferUndoBranchNext,
    BUFFER_UNDO_BRANCH_NEXT,
    "Switch to the next undo branch"
);
impl BufferUndoBranchNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        let switched = cx
            .editor
            .buffers_mut()
            .get_mut(buffer_id)
            .is_some_and(|b| b.undo_tree_mut().branch_next());
        if switched {
            cx.editor.set_status("Undo branch: next");
        }
    }
}

stock_cmd!(
    BufferUndoBranchPrev,
    BUFFER_UNDO_BRANCH_PREV,
    "Switch to the previous undo branch"
);
impl BufferUndoBranchPrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        let switched = cx
            .editor
            .buffers_mut()
            .get_mut(buffer_id)
            .is_some_and(|b| b.undo_tree_mut().branch_prev());
        if switched {
            cx.editor.set_status("Undo branch: prev");
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostic navigation
// ---------------------------------------------------------------------------

stock_cmd!(
    LspHover,
    LSP_HOVER,
    "Show hover info (diagnostic or type) at the cursor"
);
impl LspHover {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        // Check the diagnostics layer for a diagnostic at the cursor.
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let Some(layer) = buffer.properties().layer("diagnostics") else {
            cx.editor.set_status("No diagnostics");
            return;
        };
        let at_cursor: Vec<_> = layer
            .overlapping(cursor..cursor + 1)
            .filter_map(|iv| {
                if let arx_buffer::PropertyValue::Diagnostic(d) = &iv.value {
                    Some(d.clone())
                } else {
                    None
                }
            })
            .collect();
        if at_cursor.is_empty() {
            cx.editor.set_status("No info at cursor");
        } else {
            // Show the first diagnostic's message.
            let msg = &at_cursor[0].message;
            let severity = match at_cursor[0].severity {
                arx_buffer::Severity::Error => "error",
                arx_buffer::Severity::Warning => "warning",
                arx_buffer::Severity::Info => "info",
                arx_buffer::Severity::Hint => "hint",
            };
            cx.editor
                .set_status(format!("[{severity}] {msg}"));
        }
    }
}

/// Collect the start-byte of every diagnostic interval in the given
/// buffer's `"diagnostics"` property layer, sorted and deduped.
fn diagnostic_offsets(editor: &Editor, buffer_id: BufferId) -> Vec<usize> {
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return Vec::new();
    };
    let Some(layer) = buffer.properties().layer("diagnostics") else {
        return Vec::new();
    };
    let mut offsets: Vec<usize> = layer
        .tree()
        .iter()
        .filter(|iv| matches!(iv.value, arx_buffer::PropertyValue::Diagnostic(_)))
        .map(|iv| iv.range.start)
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

stock_cmd!(
    LspNextDiagnostic,
    LSP_NEXT_DIAGNOSTIC,
    "Jump to the next diagnostic in the buffer"
);
impl LspNextDiagnostic {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let offsets = diagnostic_offsets(cx.editor, buffer_id);
        // Find the first offset strictly after the cursor.
        let next = offsets.iter().find(|&&o| o > cursor).copied();
        // Wrap around if nothing after cursor.
        let target = next.or_else(|| offsets.first().copied());
        if let Some(byte) = target {
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = byte;
            }
            cx.editor.mark_dirty();
        }
    }
}

stock_cmd!(
    LspPrevDiagnostic,
    LSP_PREV_DIAGNOSTIC,
    "Jump to the previous diagnostic in the buffer"
);
impl LspPrevDiagnostic {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let offsets = diagnostic_offsets(cx.editor, buffer_id);
        // Find the last offset strictly before the cursor.
        let prev = offsets.iter().rev().find(|&&o| o < cursor).copied();
        // Wrap around.
        let target = prev.or_else(|| offsets.last().copied());
        if let Some(byte) = target {
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = byte;
            }
            cx.editor.mark_dirty();
        }
    }
}

// ---------------------------------------------------------------------------
// Embedded terminal
// ---------------------------------------------------------------------------

stock_cmd!(
    TerminalOpen,
    TERMINAL_OPEN,
    "Open an embedded terminal in a split pane"
);
impl TerminalOpen {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.open_terminal(SplitAxis::Horizontal);
    }
}

// ---------------------------------------------------------------------------
// Interactive buffer search
// ---------------------------------------------------------------------------
//
// Swiper / telescope-style line search. The user types a query, sees
// matching lines in a bottom overlay, and navigates between them with
// the buffer scrolling in real time. Enter accepts (cursor stays at
// the match); Escape cancels (cursor returns to original position).

fn ensure_search_layer(editor: &mut Editor) {
    if editor.keymap().has_layer("search") {
        return;
    }
    editor.keymap_mut().push_layer(Layer::new(
        LayerId::from("search"),
        Arc::new(arx_keymap::profiles::search_layer()),
    ));
    editor
        .keymap_mut()
        .set_count_mode(arx_keymap::CountMode::Reject);
}

fn leave_search_layer(editor: &mut Editor) {
    if editor.keymap().has_layer("search") {
        editor.keymap_mut().pop_layer();
        editor
            .keymap_mut()
            .set_count_mode(arx_keymap::CountMode::Accept);
    }
}

/// Jump the active window's cursor to the currently selected search
/// match. Called after every navigation action so the buffer scrolls
/// in real time.
fn apply_search_preview(editor: &mut Editor) {
    let Some(m) = editor.search().selected_match().cloned() else {
        return;
    };
    let Some(window_id) = editor.windows().active() else {
        return;
    };
    if let Some(window) = editor.windows_mut().get_mut(window_id) {
        window.cursor_byte = m.byte_start + m.match_offset;
    }
    editor.ensure_active_cursor_visible();
}

/// Write search-match highlights into the buffer's `"search"` property
/// layer so the render pipeline paints them. Clears previous highlights
/// first. The selected match gets a brighter face; other matches get a
/// dimmer one.
fn apply_search_highlights(editor: &mut Editor) {
    use arx_buffer::{AdjustmentPolicy, Face, Interval, PropertyValue, StickyBehavior};

    let Some(window_id) = editor.windows().active() else {
        return;
    };
    let Some(buffer_id) = editor.windows().get(window_id).map(|d| d.buffer_id) else {
        return;
    };
    let selected_idx = editor.search().selected_index();
    let matches: Vec<(usize, usize, bool)> = editor
        .search()
        .matches()
        .iter()
        .enumerate()
        .filter(|(_, m)| m.match_len > 0)
        .map(|(i, m)| {
            let start = m.byte_start + m.match_offset;
            let end = start + m.match_len;
            (start, end, i == selected_idx)
        })
        .collect();

    let Some(buf) = editor.buffers_mut().get_mut(buffer_id) else {
        return;
    };
    let layer = buf
        .properties_mut()
        .ensure_layer("search", AdjustmentPolicy::Static);
    layer.clear();

    // Dim yellow background for non-selected matches.
    let other_face = Face {
        bg: Some(0x50_50_00),
        priority: 50,
        ..Face::default()
    };
    // Bright yellow background + black text for the selected match.
    let selected_face = Face {
        fg: Some(0x00_00_00),
        bg: Some(0xE6_C8_3C),
        bold: Some(true),
        priority: 60,
        ..Face::default()
    };

    for (start, end, is_selected) in &matches {
        if start >= end {
            continue;
        }
        let face = if *is_selected {
            selected_face.clone()
        } else {
            other_face.clone()
        };
        layer.insert(Interval::new(
            *start..*end,
            PropertyValue::Decoration(face),
            StickyBehavior::Shrink,
        ));
    }
}

/// Clear search highlights from the buffer's property map.
fn clear_search_highlights(editor: &mut Editor) {
    let Some(window_id) = editor.windows().active() else {
        return;
    };
    let Some(buffer_id) = editor.windows().get(window_id).map(|d| d.buffer_id) else {
        return;
    };
    if let Some(buf) = editor.buffers_mut().get_mut(buffer_id) {
        buf.properties_mut().remove_layer("search");
    }
}

stock_cmd!(SearchOpen, SEARCH_OPEN, "Open interactive buffer search");
impl SearchOpen {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if cx.editor.search().is_open() {
            return;
        }
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let scroll = cx
            .editor
            .windows()
            .get(window_id)
            .map_or(0, |d| d.scroll_top_line);
        let text = cx
            .editor
            .buffers()
            .get(buffer_id)
            .map_or_else(String::new, arx_buffer::Buffer::text);
        cx.editor.search_mut().open(&text, cursor, scroll);
        ensure_search_layer(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchClose,
    SEARCH_CLOSE,
    "Close search and restore cursor to original position"
);
impl SearchClose {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Restore saved position.
        let saved_cursor = cx.editor.search().saved_cursor();
        let saved_scroll = cx.editor.search().saved_scroll();
        if let Some(window_id) = cx.editor.windows().active() {
            if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = saved_cursor;
                window.scroll_top_line = saved_scroll;
            }
        }
        clear_search_highlights(cx.editor);
        cx.editor.search_mut().close();
        leave_search_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchExecute,
    SEARCH_EXECUTE,
    "Accept the selected search match and jump to it"
);
impl SearchExecute {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Record history before closing.
        let query = cx.editor.search().query().to_owned();
        if !query.is_empty() {
            cx.editor.search_mut().push_history(query);
        }
        // Jump cursor to the match (already there from preview,
        // but make sure).
        apply_search_preview(cx.editor);
        clear_search_highlights(cx.editor);
        cx.editor.search_mut().close();
        leave_search_layer(cx.editor);
        cx.editor.mark_dirty();
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(
    SearchNext,
    SEARCH_NEXT,
    "Move the search selection down one row"
);
impl SearchNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().select_next();
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchPrev,
    SEARCH_PREV,
    "Move the search selection up one row"
);
impl SearchPrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().select_prev();
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

const SEARCH_PAGE_SIZE: usize = 8;

stock_cmd!(
    SearchPageDown,
    SEARCH_PAGE_DOWN,
    "Move the search selection down one page"
);
impl SearchPageDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().select_next_n(SEARCH_PAGE_SIZE);
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchPageUp,
    SEARCH_PAGE_UP,
    "Move the search selection up one page"
);
impl SearchPageUp {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().select_prev_n(SEARCH_PAGE_SIZE);
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchToggleMode,
    SEARCH_TOGGLE_MODE,
    "Cycle search mode: fuzzy / literal / regex"
);
impl SearchToggleMode {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().toggle_mode();
        let label = cx.editor.search().mode().label();
        cx.editor.set_status(format!("Search mode: {label}"));
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchBackspace,
    SEARCH_BACKSPACE,
    "Delete the last character from the search query"
);
impl SearchBackspace {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().backspace();
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchHistoryPrev,
    SEARCH_HISTORY_PREV,
    "Navigate to the previous search history entry"
);
impl SearchHistoryPrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().history_prev();
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    SearchHistoryNext,
    SEARCH_HISTORY_NEXT,
    "Navigate to the next search history entry"
);
impl SearchHistoryNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.search_mut().history_next();
        apply_search_preview(cx.editor);
        apply_search_highlights(cx.editor);
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Rectangle (column block) operations
// ---------------------------------------------------------------------------

stock_cmd!(RectKill, RECT_KILL, "Kill (delete) the rectangular region between mark and cursor");
impl RectKill {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No mark set");
            return;
        };
        let Some(rect) = crate::column::RectRegion::from_mark_cursor(
            cx.editor, buffer_id, mark, cursor,
        ) else {
            return;
        };
        let killed = crate::column::kill_rectangle(cx.editor, window_id, buffer_id, &rect);
        cx.editor.kill_ring_push(crate::editor::KilledText::Rectangular(killed));
        cx.editor.clear_mark(window_id);
        // Leave visual-block mode if active.
        leave_visual_block_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(RectCopy, RECT_COPY, "Copy the rectangular region between mark and cursor");
impl RectCopy {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No mark set");
            return;
        };
        let Some(rect) = crate::column::RectRegion::from_mark_cursor(
            cx.editor, buffer_id, mark, cursor,
        ) else {
            return;
        };
        // Extract text without deleting.
        let mut copied_lines = Vec::new();
        for line_idx in rect.start_line..=rect.end_line {
            let text = cx.editor.buffers().get(buffer_id)
                .map(|b| {
                    let rope = b.rope();
                    let start = rope.line_to_byte(line_idx);
                    let end = if line_idx + 1 >= rope.len_lines() {
                        rope.len_bytes()
                    } else {
                        rope.line_to_byte(line_idx + 1).saturating_sub(1)
                    };
                    rope.slice_to_string(start..end)
                })
                .unwrap_or_default();
            let byte_start = crate::column::display_col_to_byte(&text, rect.left_col);
            let byte_end = crate::column::display_col_to_byte(&text, rect.right_col);
            copied_lines.push(text[byte_start..byte_end].to_owned());
        }
        cx.editor.kill_ring_push(crate::editor::KilledText::Rectangular(copied_lines));
        cx.editor.clear_mark(window_id);
        leave_visual_block_layer(cx.editor);
        cx.editor.set_status("Rectangle copied");
        cx.editor.mark_dirty();
    }
}

stock_cmd!(RectYank, RECT_YANK, "Yank (paste) the most recent rectangular kill");
impl RectYank {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _cursor)) = active(cx.editor) else {
            return;
        };
        let Some(entry) = cx.editor.kill_ring_top().cloned() else {
            cx.editor.set_status("Kill ring empty");
            return;
        };
        match entry {
            crate::editor::KilledText::Rectangular(lines) => {
                crate::column::yank_rectangle(cx.editor, window_id, buffer_id, &lines);
            }
            crate::editor::KilledText::Linear(text) => {
                // Fall back to normal yank for linear text.
                let cursor = cx.editor.windows().get(window_id)
                    .map_or(0, |d| d.cursor_byte);
                let len = text.len();
                user_edit(
                    cx.editor, window_id, buffer_id,
                    cursor..cursor, &text, cursor, cursor + len,
                );
            }
        }
    }
}

stock_cmd!(RectOpen, RECT_OPEN, "Insert blank space into the rectangular region");
impl RectOpen {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No mark set");
            return;
        };
        let Some(rect) = crate::column::RectRegion::from_mark_cursor(
            cx.editor, buffer_id, mark, cursor,
        ) else {
            return;
        };
        crate::column::open_rectangle(cx.editor, window_id, buffer_id, &rect);
        cx.editor.clear_mark(window_id);
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Vim visual-block mode
// ---------------------------------------------------------------------------

fn leave_visual_block_layer(editor: &mut Editor) {
    if editor.keymap().has_layer("vim.visual-block") {
        editor.keymap_mut().pop_layer();
    }
}

stock_cmd!(
    ModeEnterVisualBlock,
    MODE_ENTER_VISUAL_BLOCK,
    "Enter Vim visual-block selection mode"
);
impl ModeEnterVisualBlock {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, _buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        cx.editor.set_mark_with_mode(
            window_id,
            cursor,
            crate::editor::SelectionMode::Rectangle,
        );
        if !cx.editor.keymap().has_layer("vim.visual-block") {
            cx.editor.keymap_mut().push_layer(Layer::new(
                LayerId::from("vim.visual-block"),
                Arc::new(arx_keymap::profiles::visual_block_layer()),
            ));
        }
        cx.editor.set_status("-- VISUAL BLOCK --");
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    ModeLeaveVisualBlock,
    MODE_LEAVE_VISUAL_BLOCK,
    "Leave Vim visual-block selection mode"
);
impl ModeLeaveVisualBlock {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let window_id = cx.editor.windows().active();
        if let Some(wid) = window_id {
            cx.editor.clear_mark(wid);
        }
        leave_visual_block_layer(cx.editor);
        cx.editor.clear_status();
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Completion popup
// ---------------------------------------------------------------------------
//
// `completion.trigger` collects word-boundary context around the
// cursor and opens the popup with a placeholder list. The actual
// LSP `textDocument/completion` request is async and happens via
// the driver's LspManager; this command just opens the popup UI.
// For MVP, the command also pushes the `completion` keymap layer
// so subsequent keystrokes route to popup navigation.

fn leave_completion_layer(editor: &mut Editor) {
    if editor.keymap().has_layer("completion") {
        editor.keymap_mut().pop_layer();
    }
}

/// Walk backward from `cursor` to find the start of the current
/// word (the "completion prefix"). This is the `anchor` that
/// `completion.accept` will replace from.
fn completion_anchor(text: &str, cursor: usize) -> usize {
    let head = &text[..cursor];
    head.rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map_or(0, |i| {
            // `i` is the byte index of the non-word char; anchor is
            // one past it.
            i + head[i..].chars().next().map_or(1, char::len_utf8)
        })
}

stock_cmd!(
    CompletionTrigger,
    COMPLETION_TRIGGER,
    "Trigger code completion at the cursor"
);
impl CompletionTrigger {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // If the completion popup is already open, do nothing.
        if cx.editor.completion().is_open() {
            return;
        }
        let Some((_window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            return;
        };
        let text = buffer.text();
        let anchor = completion_anchor(&text, cursor);
        let prefix = &text[anchor..cursor];

        // Collect simple word completions from the buffer itself as
        // a baseline. This works even without an LSP server.
        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();
        for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if word.len() < 2 || !word.starts_with(prefix) || word == prefix {
                continue;
            }
            if seen.insert(word.to_owned()) {
                items.push(crate::completion::CompletionItem {
                    insert_text: word.to_owned(),
                    label: word.to_owned(),
                    detail: None,
                    kind: None,
                });
            }
            if items.len() >= 50 {
                break;
            }
        }

        if items.is_empty() {
            return;
        }

        cx.editor.completion_mut().show(items, anchor);

        // Push the completion keymap layer.
        if !cx.editor.keymap().has_layer("completion") {
            cx.editor.keymap_mut().push_layer(Layer::new(
                LayerId::from("completion"),
                Arc::new(arx_keymap::profiles::completion_layer()),
            ));
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CompletionAccept,
    COMPLETION_ACCEPT,
    "Accept the selected completion item"
);
impl CompletionAccept {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        if !cx.editor.completion().is_open() {
            return;
        }
        let anchor = cx.editor.completion().anchor();
        let item = cx.editor.completion().selected_item().cloned();
        cx.editor.completion_mut().dismiss();
        leave_completion_layer(cx.editor);
        let Some(item) = item else {
            return;
        };
        // Replace anchor..cursor with the insert text.
        let range = anchor..cursor;
        user_edit(
            cx.editor,
            window_id,
            buffer_id,
            range,
            &item.insert_text,
            anchor,
            anchor + item.insert_text.len(),
        );
    }
}

stock_cmd!(
    CompletionDismiss,
    COMPLETION_DISMISS,
    "Dismiss the completion popup"
);
impl CompletionDismiss {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.completion_mut().dismiss();
        leave_completion_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CompletionNext,
    COMPLETION_NEXT,
    "Move the completion selection down one row"
);
impl CompletionNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.completion_mut().select_next();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CompletionPrev,
    COMPLETION_PREV,
    "Move the completion selection up one row"
);
impl CompletionPrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.completion_mut().select_prev();
        cx.editor.mark_dirty();
    }
}

/// Page size for the completion popup — matches the max visible rows
/// the renderer shows (8).
const COMPLETION_PAGE_SIZE: usize = 8;

stock_cmd!(
    CompletionPageDown,
    COMPLETION_PAGE_DOWN,
    "Move the completion selection down one page"
);
impl CompletionPageDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor
            .completion_mut()
            .select_next_n(COMPLETION_PAGE_SIZE);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    CompletionPageUp,
    COMPLETION_PAGE_UP,
    "Move the completion selection up one page"
);
impl CompletionPageUp {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor
            .completion_mut()
            .select_prev_n(COMPLETION_PAGE_SIZE);
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

    // ---- Undo / redo ----

    /// Spin up an editor with a single window over `text` and return
    /// the bus + event-loop join handle. Caller is responsible for
    /// dropping the bus and awaiting the handle.
    async fn editor_with_text(
        text: &str,
    ) -> (crate::CommandBus, tokio::task::JoinHandle<Editor>) {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        let owned = text.to_owned();
        bus.invoke(move |editor| {
            let buf = editor.buffers_mut().create_from_text(&owned, None);
            let win = editor.windows_mut().open(buf);
            // Seed a cached viewport size so ensure_active_cursor_visible
            // doesn't early-out during undo.
            let data = editor.windows_mut().get_mut(win).unwrap();
            data.visible_rows = 10;
            data.visible_cols = 40;
        })
        .await
        .unwrap();
        (bus, handle)
    }

    async fn active_text_and_cursor(bus: &crate::CommandBus) -> (String, usize) {
        bus.invoke(|editor| {
            let win = editor.windows().active().unwrap();
            let data = editor.windows().get(win).unwrap();
            let text = editor.buffers().get(data.buffer_id).unwrap().text();
            (text, data.cursor_byte)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn user_edit_records_to_undo_tree() {
        let (bus, handle) = editor_with_text("hello").await;
        // Self-insert path goes through `insert_at_cursor`, which
        // routes through `user_edit` and pushes a record.
        bus.invoke(|editor| {
            // Cursor is at 0 from the fresh editor.
            insert_at_cursor(editor, "X");
        })
        .await
        .unwrap();
        let tree_len = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let buf_id = editor.windows().get(win).unwrap().buffer_id;
                editor.buffers().get(buf_id).unwrap().undo_tree().len()
            })
            .await
            .unwrap();
        // Root + one edit = 2 nodes.
        assert_eq!(tree_len, 2);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn undo_command_reverts_an_insert() {
        let (bus, handle) = editor_with_text("hi").await;
        // Move to end, insert "!", confirm, undo.
        run_named(&bus, names::CURSOR_BUFFER_END).await;
        bus.invoke(|editor| insert_at_cursor(editor, "!"))
            .await
            .unwrap();
        let (text, cursor) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "hi!");
        assert_eq!(cursor, 3);

        run_named(&bus, names::BUFFER_UNDO).await;
        let (text, cursor) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "hi");
        // Cursor should land where it was before the insert.
        assert_eq!(cursor, 2);

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn redo_replays_the_undone_edit() {
        let (bus, handle) = editor_with_text("ab").await;
        run_named(&bus, names::CURSOR_BUFFER_END).await;
        bus.invoke(|editor| insert_at_cursor(editor, "c"))
            .await
            .unwrap();
        run_named(&bus, names::BUFFER_UNDO).await;
        let (text, _) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "ab");

        run_named(&bus, names::BUFFER_REDO).await;
        let (text, cursor) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "abc");
        assert_eq!(cursor, 3);

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn undo_past_root_is_a_noop() {
        let (bus, handle) = editor_with_text("x").await;
        // No user edits yet. Undo should do nothing.
        run_named(&bus, names::BUFFER_UNDO).await;
        let (text, cursor) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "x");
        assert_eq!(cursor, 0);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn new_edit_after_undo_creates_a_branch() {
        // Type 'a', type 'b', undo once (buffer back to "a"), type
        // 'c' → new sibling branch under the 'a' node. The old 'b'
        // branch is unreachable by plain redo but still in the tree.
        let (bus, handle) = editor_with_text("").await;
        bus.invoke(|editor| insert_at_cursor(editor, "a"))
            .await
            .unwrap();
        bus.invoke(|editor| insert_at_cursor(editor, "b"))
            .await
            .unwrap();
        run_named(&bus, names::BUFFER_UNDO).await;
        // Buffer should be "a" again.
        assert_eq!(active_text_and_cursor(&bus).await.0, "a");

        bus.invoke(|editor| insert_at_cursor(editor, "c"))
            .await
            .unwrap();
        let (text, _) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "ac");

        // Tree now has root + {a, b, c} = 4 nodes. Both b and c are
        // children of a.
        let (node_count, a_children) = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let buf_id = editor.windows().get(win).unwrap().buffer_id;
                let tree = editor.buffers().get(buf_id).unwrap().undo_tree();
                let nodes = tree.len();
                // After pushing 'c', current = c. Undo once → we're
                // back at 'a'; a's children count tells us how many
                // siblings c has.
                let mut tree_clone_ops = (nodes, 0usize);
                // Borrow-immutable only: walk by undoing via peek.
                // The real shape check happens in the next invoke.
                tree_clone_ops.1 = 0;
                tree_clone_ops
            })
            .await
            .unwrap();
        assert_eq!(node_count, 4);
        let _ = a_children;

        // Undo once more → back to "a". The 'a' node now has two
        // children; last_active_child is 'c' (just pushed), so a
        // second redo here replays 'c', not 'b'.
        run_named(&bus, names::BUFFER_UNDO).await;
        assert_eq!(active_text_and_cursor(&bus).await.0, "a");
        run_named(&bus, names::BUFFER_REDO).await;
        assert_eq!(active_text_and_cursor(&bus).await.0, "ac");

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn multi_step_undo_via_count_prefix() {
        // Three separate inserts; one undo command with count = 3
        // should roll them all back.
        let (bus, handle) = editor_with_text("").await;
        bus.invoke(|editor| insert_at_cursor(editor, "a"))
            .await
            .unwrap();
        bus.invoke(|editor| insert_at_cursor(editor, "b"))
            .await
            .unwrap();
        bus.invoke(|editor| insert_at_cursor(editor, "c"))
            .await
            .unwrap();

        let bus_clone = bus.clone();
        bus.invoke(move |editor| {
            let cmd = editor.commands().get(names::BUFFER_UNDO).unwrap();
            let mut cx = CommandContext {
                editor,
                bus: bus_clone,
                count: 3,
            };
            cmd.run(&mut cx);
        })
        .await
        .unwrap();
        let (text, cursor) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "");
        assert_eq!(cursor, 0);

        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn undo_of_delete_restores_bytes() {
        // Delete-backward should also be undoable.
        let (bus, handle) = editor_with_text("hello").await;
        run_named(&bus, names::CURSOR_BUFFER_END).await;
        run_named(&bus, names::BUFFER_DELETE_BACKWARD).await;
        assert_eq!(active_text_and_cursor(&bus).await.0, "hell");

        run_named(&bus, names::BUFFER_UNDO).await;
        let (text, cursor) = active_text_and_cursor(&bus).await;
        assert_eq!(text, "hello");
        // Cursor should be back at the end (where it was before the
        // delete).
        assert_eq!(cursor, 5);

        drop(bus);
        let _ = handle.await.unwrap();
    }
}
