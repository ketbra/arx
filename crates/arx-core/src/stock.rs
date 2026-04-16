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
use crate::kedit::BlockKind;
use crate::registry::{Command, CommandContext, CommandRegistry};
use crate::window::SplitAxis;
use crate::WindowId;

/// Register every stock command into `reg`. Call once at editor start.
#[allow(clippy::too_many_lines)]
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
    reg.register(CursorEndOfWord);
    reg.register(CursorParagraphForward);
    reg.register(CursorParagraphBackward);
    reg.register(CursorMatchingBracket);
    reg.register(CursorScreenTop);
    reg.register(CursorScreenMiddle);
    reg.register(CursorScreenBottom);
    reg.register(CursorFindCharForward);
    reg.register(CursorFindCharBackward);
    reg.register(CursorTillCharForward);
    reg.register(CursorTillCharBackward);
    reg.register(CursorRepeatFind);
    reg.register(CursorRepeatFindReverse);
    reg.register(BufferNewline);
    reg.register(BufferDeleteBackward);
    reg.register(BufferDeleteForward);
    reg.register(ScrollPageUp);
    reg.register(ScrollPageDown);
    reg.register(ScrollRecenter);
    reg.register(ScrollHalfPageDown);
    reg.register(ScrollHalfPageUp);
    reg.register(ScrollCursorTop);
    reg.register(ScrollCursorBottom);
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
    reg.register(BufferTransposeWords);
    reg.register(BufferJoinLines);
    reg.register(BufferDuplicateLine);
    reg.register(BufferMoveLineUp);
    reg.register(BufferMoveLineDown);
    reg.register(BufferIndentLine);
    reg.register(BufferDedentLine);
    reg.register(BufferCommentToggle);
    reg.register(BufferMarkWhole);
    reg.register(BufferExchangePointMark);
    reg.register(BufferYankPop);
    reg.register(BufferDeleteLine);
    reg.register(BufferYankLine);
    reg.register(BufferChangeLine);
    reg.register(BufferDeleteToEol);
    reg.register(BufferChangeToEol);
    reg.register(BufferYankToEol);
    reg.register(GotoLine);
    reg.register(BufferSave);
    reg.register(EditorQuit);
    reg.register(EditorSuspend);
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
    reg.register(LspGotoDefinition);
    reg.register(LspPopBack);
    reg.register(TreesitterNextFunction);
    reg.register(TreesitterPrevFunction);
    reg.register(TreesitterParentNode);
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
    reg.register(OperatorDelete);
    reg.register(OperatorChange);
    reg.register(OperatorYank);
    reg.register(OperatorIndent);
    reg.register(OperatorDedent);
    reg.register(OperatorCancel);
    reg.register(OperatorLineApply);
    reg.register(TextObjectInnerWord);
    reg.register(TextObjectAWord);
    reg.register(TextObjectInnerParagraph);
    reg.register(TextObjectAParagraph);
    reg.register(TextObjectInnerDoubleQuote);
    reg.register(TextObjectADoubleQuote);
    reg.register(TextObjectInnerSingleQuote);
    reg.register(TextObjectASingleQuote);
    reg.register(TextObjectInnerParen);
    reg.register(TextObjectAParen);
    reg.register(TextObjectInnerBrace);
    reg.register(TextObjectABrace);
    reg.register(TextObjectInnerBracket);
    reg.register(TextObjectABracket);
    reg.register(TextObjectInnerAngle);
    reg.register(TextObjectAAngle);
    reg.register(TextObjectInnerBacktick);
    reg.register(TextObjectABacktick);
    reg.register(CompletionAccept);
    reg.register(CompletionDismiss);
    reg.register(CompletionNext);
    reg.register(CompletionPrev);
    reg.register(CompletionPageDown);
    reg.register(CompletionPageUp);
    reg.register(KeditFocusCmdline);
    reg.register(KeditFocusBuffer);
    reg.register(KeditToggleFocus);
    reg.register(KeditCmdlineExecute);
    reg.register(KeditCmdlineBackspace);
    reg.register(KeditCmdlineDeleteForward);
    reg.register(KeditCmdlineClear);
    reg.register(KeditCmdlineCursorLeft);
    reg.register(KeditCmdlineCursorRight);
    reg.register(KeditCmdlineCursorHome);
    reg.register(KeditCmdlineCursorEnd);
    reg.register(KeditCmdlineHistoryPrev);
    reg.register(KeditCmdlineHistoryNext);
    reg.register(BlockMarkLine);
    reg.register(BlockMarkBox);
    reg.register(BlockMarkChar);
    reg.register(BlockCopy);
    reg.register(BlockMove);
    reg.register(BlockDelete);
    reg.register(BlockPaste);
    reg.register(BlockUnmark);
    reg.register(BlockOverlay);
    reg.register(BlockFill);
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
        if forward_to_terminal(cx.editor, b"\x1b[D") {
            return;
        }
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
        if forward_to_terminal(cx.editor, b"\x1b[C") {
            return;
        }
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
        if forward_to_terminal(cx.editor, b"\x1b[A") {
            return;
        }
        let delta = -(cx.count.max(1) as i32);
        move_cursor_vertical_by(cx.editor, delta);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(CursorDown, CURSOR_DOWN, "Move the cursor down one line");
impl CursorDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if forward_to_terminal(cx.editor, b"\x1b[B") {
            return;
        }
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
///
/// When a KEDIT `ALL` filter is active on the buffer, `delta` counts
/// *visible* lines rather than raw buffer lines: stepping down by 1
/// skips every excluded line between the cursor and the next visible
/// line. This is what makes `Up`/`Down`/`Page*` behave consistently
/// with the rendered viewport.
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
    let total_lines = rope.len_lines();
    // If the buffer has a filter, step through visible lines only.
    let target_line = if let Some(filter) = editor.filter(buffer_id) {
        filter.step_visible(current_line, delta, total_lines)
    } else {
        current_line
            .saturating_add_signed(delta as isize)
            .min(total_lines.saturating_sub(1))
    };
    let line_start = rope.line_to_byte(current_line);
    let col = cursor - line_start;
    let new_line_start = rope.line_to_byte(target_line);
    let new_line_end = if target_line + 1 < total_lines {
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
        if forward_to_terminal(cx.editor, b"\x1b[H") {
            return;
        }
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
        if forward_to_terminal(cx.editor, b"\x1b[F") {
            return;
        }
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
        if try_apply_operator_motion(cx.editor, window_id, buffer_id, cursor, end, false) {
            return;
        }
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
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let text = buffer.rope().slice_to_string(0..buffer.len_bytes());
        let mut pos = cursor;
        for _ in 0..n { pos = next_word_boundary(&text, pos); }
        if try_apply_operator_motion(cx.editor, window_id, buffer_id, cursor, pos, false) {
            return;
        }
        if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = pos;
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
        let Some((window_id, buffer_id, start_cursor)) = active(cx.editor) else { return };
        // Check for operator before the loop.
        if cx.editor.operator_state().operator.is_some() {
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
            let text = buffer.rope().slice_to_string(0..buffer.len_bytes());
            let mut pos = start_cursor;
            for _ in 0..n { pos = prev_word_boundary(&text, pos); }
            if try_apply_operator_motion(cx.editor, window_id, buffer_id, start_cursor, pos, false) { return; }
        }
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
        let Some((window_id, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        // Under a KEDIT `ALL` filter, snap to the first *visible*
        // line rather than byte 0 (which may be on an excluded line).
        let target = first_visible_line_byte(cx.editor, buffer_id).unwrap_or(0);
        if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = target;
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
        // Under a filter, snap to the end of the last *visible* line.
        let target = last_visible_line_end(cx.editor, buffer_id).unwrap_or(end);
        if let Some(window) = cx.editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = target;
        }
        cx.editor.mark_dirty();
    }
}

/// Byte offset of the start of the first *visible* line for
/// `buffer_id`. Returns `None` when the buffer has no filter (the
/// caller should fall back to 0) or when every line is excluded.
fn first_visible_line_byte(editor: &Editor, buffer_id: BufferId) -> Option<usize> {
    let filter = editor.filter(buffer_id)?;
    let buffer = editor.buffers().get(buffer_id)?;
    let rope = buffer.rope();
    for line in 0..rope.len_lines() {
        if !filter.is_excluded(line) {
            return Some(rope.line_to_byte(line));
        }
    }
    None
}

/// Byte offset of the end of the last *visible* line for
/// `buffer_id`. Returns `None` when the buffer has no filter.
fn last_visible_line_end(editor: &Editor, buffer_id: BufferId) -> Option<usize> {
    let filter = editor.filter(buffer_id)?;
    let buffer = editor.buffers().get(buffer_id)?;
    let rope = buffer.rope();
    let total = rope.len_lines();
    if total == 0 {
        return Some(0);
    }
    for line in (0..total).rev() {
        if !filter.is_excluded(line) {
            let end = if line + 1 < total {
                rope.line_to_byte(line + 1).saturating_sub(1)
            } else {
                rope.len_bytes()
            };
            return Some(end);
        }
    }
    None
}

stock_cmd!(CursorEndOfWord, CURSOR_END_OF_WORD, "Move to end of current/next word");
impl CursorEndOfWord {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let text = buffer.text();
        let bytes = text.as_bytes();
        let len = bytes.len();
        if cursor >= len { return; }
        let mut pos = cursor + 1;
        while pos < len && !bytes[pos].is_ascii_alphanumeric() && bytes[pos] != b'_' { pos += 1; }
        while pos < len && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') { pos += 1; }
        let target = pos.saturating_sub(1).min(len.saturating_sub(1));
        // For operators, include the end-of-word character.
        if try_apply_operator_motion(cx.editor, window_id, buffer_id, cursor, target + 1, false) {
            return;
        }
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(CursorParagraphForward, CURSOR_PARAGRAPH_FORWARD, "Move to next blank-line boundary");
impl CursorParagraphForward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let cur_line = rope.byte_to_line(cursor);
        let total = rope.len_lines();
        let filter = cx.editor.filter(buffer_id);
        let mut line = cur_line + 1;
        while line < total {
            // Skip excluded lines entirely when a filter is active;
            // paragraph boundaries only count *visible* blank lines.
            if filter.is_some_and(|f| f.is_excluded(line)) {
                line += 1;
                continue;
            }
            let start = rope.line_to_byte(line);
            let end = if line + 1 < total { rope.line_to_byte(line + 1) } else { rope.len_bytes() };
            let text = rope.slice_to_string(start..end);
            if text.trim().is_empty() { break; }
            line += 1;
        }
        let target = rope.line_to_byte(line.min(total.saturating_sub(1)));
        if try_apply_operator_motion(cx.editor, window_id, buffer_id, cursor, target, true) { return; }
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(CursorParagraphBackward, CURSOR_PARAGRAPH_BACKWARD, "Move to previous blank-line boundary");
impl CursorParagraphBackward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let cur_line = rope.byte_to_line(cursor);
        let filter = cx.editor.filter(buffer_id);
        let mut line = cur_line.saturating_sub(1);
        loop {
            // Under a filter, walk backward through visible lines only.
            if !filter.is_some_and(|f| f.is_excluded(line)) {
                let start = rope.line_to_byte(line);
                let end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) } else { rope.len_bytes() };
                let text = rope.slice_to_string(start..end);
                if text.trim().is_empty() || line == 0 { break; }
            } else if line == 0 {
                break;
            }
            line -= 1;
        }
        let target = rope.line_to_byte(line);
        if try_apply_operator_motion(cx.editor, window_id, buffer_id, cursor, target, true) { return; }
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(CursorMatchingBracket, CURSOR_MATCHING_BRACKET, "Jump to matching bracket");
impl CursorMatchingBracket {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let text = buffer.text();
        let bytes = text.as_bytes();
        if cursor >= bytes.len() { return; }
        let ch = bytes[cursor] as char;
        let (target_ch, forward) = match ch {
            '(' => (')', true), ')' => ('(', false),
            '[' => (']', true), ']' => ('[', false),
            '{' => ('}', true), '}' => ('{', false),
            '<' => ('>', true), '>' => ('<', false),
            _ => return,
        };
        let mut depth = 1i32;
        let iter: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new((cursor + 1)..bytes.len())
        } else {
            Box::new((0..cursor).rev())
        };
        for pos in iter {
            let bc = bytes[pos] as char;
            if bc == target_ch { depth -= 1; }
            else if bc == ch { depth += 1; }
            if depth == 0 {
                if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = pos; }
                cx.editor.mark_dirty();
                cx.editor.ensure_active_cursor_visible();
                return;
            }
        }
    }
}

stock_cmd!(CursorScreenTop, CURSOR_SCREEN_TOP, "Move cursor to top of visible screen");
impl CursorScreenTop {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let scroll_top = cx.editor.windows().get(window_id).map_or(0, |d| d.scroll_top_line);
        let target = buffer.rope().line_to_byte(scroll_top);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(CursorScreenMiddle, CURSOR_SCREEN_MIDDLE, "Move cursor to middle of visible screen");
impl CursorScreenMiddle {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else { return };
        let data = cx.editor.windows().get(window_id).cloned();
        let Some(data) = data else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let mid = data.scroll_top_line + (data.visible_rows / 2) as usize;
        let target = buffer.rope().line_to_byte(mid.min(buffer.rope().len_lines().saturating_sub(1)));
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(CursorScreenBottom, CURSOR_SCREEN_BOTTOM, "Move cursor to bottom of visible screen");
impl CursorScreenBottom {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else { return };
        let data = cx.editor.windows().get(window_id).cloned();
        let Some(data) = data else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let bot = data.scroll_top_line + data.visible_rows.saturating_sub(1) as usize;
        let target = buffer.rope().line_to_byte(bot.min(buffer.rope().len_lines().saturating_sub(1)));
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
    }
}

// Find-char commands use the editor's `enter_describe_key_mode`-like pattern:
// The command itself doesn't move the cursor — it sets a flag and waits for
// the next printable character. For simplicity in this first implementation,
// we use the palette to prompt for a single char. However, a simpler approach
// is to just scan from the cursor position. Since the keymap layer intercepts
// the next key, we'll implement these as stub commands that set status and
// use `handle_printable_fallback` to capture the char. For now, use a
// direct scan approach: the command reads the char from the query or we
// implement a simple "next char" mode.
//
// Simplest approach: these are no-ops that get wired up when we add a
// "read next char" mechanism. For now, register them so the bindings
// resolve but show a status message.

stock_cmd!(CursorFindCharForward, CURSOR_FIND_CHAR_FORWARD, "Find char forward on line (f)");
impl CursorFindCharForward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.operator_state_mut().char_read = Some(crate::editor::CharReadMode::FindForwardTo);
    }
}

stock_cmd!(CursorFindCharBackward, CURSOR_FIND_CHAR_BACKWARD, "Find char backward on line (F)");
impl CursorFindCharBackward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.operator_state_mut().char_read = Some(crate::editor::CharReadMode::FindBackwardTo);
    }
}

stock_cmd!(CursorTillCharForward, CURSOR_TILL_CHAR_FORWARD, "Move to before char forward (t)");
impl CursorTillCharForward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.operator_state_mut().char_read = Some(crate::editor::CharReadMode::FindForwardTill);
    }
}

stock_cmd!(CursorTillCharBackward, CURSOR_TILL_CHAR_BACKWARD, "Move to after char backward (T)");
impl CursorTillCharBackward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.operator_state_mut().char_read = Some(crate::editor::CharReadMode::FindBackwardTill);
    }
}

stock_cmd!(CursorRepeatFind, CURSOR_REPEAT_FIND, "Repeat last find-char motion");
impl CursorRepeatFind {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some(state) = cx.editor.last_find_char() else {
            cx.editor.set_status("No previous find to repeat");
            return;
        };
        let mode = match state.kind {
            crate::editor::FindCharKind::ForwardTo => crate::editor::CharReadMode::FindForwardTo,
            crate::editor::FindCharKind::ForwardTill => crate::editor::CharReadMode::FindForwardTill,
            crate::editor::FindCharKind::BackwardTo => crate::editor::CharReadMode::FindBackwardTo,
            crate::editor::FindCharKind::BackwardTill => crate::editor::CharReadMode::FindBackwardTill,
        };
        handle_char_read(cx.editor, state.ch, mode);
    }
}

stock_cmd!(CursorRepeatFindReverse, CURSOR_REPEAT_FIND_REVERSE, "Repeat last find-char reversed");
impl CursorRepeatFindReverse {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some(state) = cx.editor.last_find_char() else {
            cx.editor.set_status("No previous find to repeat");
            return;
        };
        let mode = match state.kind {
            crate::editor::FindCharKind::ForwardTo => crate::editor::CharReadMode::FindBackwardTo,
            crate::editor::FindCharKind::ForwardTill => crate::editor::CharReadMode::FindBackwardTill,
            crate::editor::FindCharKind::BackwardTo => crate::editor::CharReadMode::FindForwardTo,
            crate::editor::FindCharKind::BackwardTill => crate::editor::CharReadMode::FindForwardTill,
        };
        handle_char_read(cx.editor, state.ch, mode);
    }
}

stock_cmd!(ScrollHalfPageDown, SCROLL_HALF_PAGE_DOWN, "Scroll down half a page");
impl ScrollHalfPageDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let data = cx.editor.windows().get(window_id).cloned();
        let Some(data) = data else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let half = (data.visible_rows / 2).max(1) as usize;
        let cur_line = buffer.rope().byte_to_line(cursor);
        let target_line = (cur_line + half).min(buffer.rope().len_lines().saturating_sub(1));
        let target = buffer.rope().line_to_byte(target_line);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(ScrollHalfPageUp, SCROLL_HALF_PAGE_UP, "Scroll up half a page");
impl ScrollHalfPageUp {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let data = cx.editor.windows().get(window_id).cloned();
        let Some(data) = data else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let half = (data.visible_rows / 2).max(1) as usize;
        let cur_line = buffer.rope().byte_to_line(cursor);
        let target_line = cur_line.saturating_sub(half);
        let target = buffer.rope().line_to_byte(target_line);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = target; }
        cx.editor.mark_dirty();
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(ScrollCursorTop, SCROLL_CURSOR_TOP, "Scroll so cursor is at top of window");
impl ScrollCursorTop {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let line = buffer.rope().byte_to_line(cursor);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.scroll_top_line = line; }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(ScrollCursorBottom, SCROLL_CURSOR_BOTTOM, "Scroll so cursor is at bottom of window");
impl ScrollCursorBottom {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let data = cx.editor.windows().get(window_id).cloned();
        let Some(data) = data else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let line = buffer.rope().byte_to_line(cursor);
        let top = line.saturating_sub(data.visible_rows.saturating_sub(1) as usize);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.scroll_top_line = top; }
        cx.editor.mark_dirty();
    }
}

// ---------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------

/// True when `range` (about to be passed to [`user_edit`]) covers or
/// borders any line hidden by a KEDIT `ALL` filter on `buffer_id`.
///
/// The guard logic:
///
/// * Compute the line indices of both endpoints.
/// * Pure insert at a single byte (`range.start == range.end`): allowed
///   only if the cursor's line is visible. The new line created by
///   inserting `\n` there sits *after* the current line, so it doesn't
///   conflict with any excluded line.
/// * Non-empty range: every line from `line(start)` through `line(end)`
///   must be visible. A range that starts on the last visible line
///   but extends into an excluded one (e.g. `buffer.delete-forward`
///   at EOL with the next line hidden) is rejected.
fn edit_touches_excluded(
    editor: &Editor,
    buffer_id: BufferId,
    range: &ByteRange,
) -> bool {
    let Some(filter) = editor.filter(buffer_id) else {
        return false;
    };
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return false;
    };
    let rope = buffer.rope();
    let start_line = rope.byte_to_line(range.start.min(rope.len_bytes()));
    let end_line = rope.byte_to_line(range.end.min(rope.len_bytes()));
    for line in start_line..=end_line {
        if filter.is_excluded(line) {
            return true;
        }
    }
    false
}

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
    // KEDIT `ALL` filter guard: user edits may not touch any line
    // that's currently hidden by an active filter. A pure insert at
    // a visible cursor (range.start == range.end, line(start) not
    // excluded) is always safe — that's how self-insert and newline
    // work. Any range that spans excluded lines, or sits on one,
    // is rejected with a status message.
    if edit_touches_excluded(editor, buffer_id, &range) {
        editor.set_status("Edit blocked: excluded lines are read-only");
        editor.mark_dirty();
        return false;
    }

    // Capture the bytes that will be removed BEFORE we apply the
    // edit, so the undo tree gets the pre-edit content.
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return false;
    };
    let removed = buffer.rope().slice_to_string(range.clone());
    let offset = range.start;
    let inserted_text = text.to_owned();

    // Stash the edit's line position and newline delta *before*
    // applying, so any active KEDIT filter can shift its excluded-
    // line indices after the buffer's line count changes. The guard
    // above rules out ranges that touch excluded lines, so we only
    // ever need to slide indices strictly after `edit_line`.
    let edit_line = buffer.rope().byte_to_line(range.start.min(buffer.len_bytes()));
    let removed_newlines = removed.bytes().filter(|b| *b == b'\n').count() as i64;
    let inserted_newlines = text.bytes().filter(|b| *b == b'\n').count() as i64;
    let line_delta = inserted_newlines - removed_newlines;

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
    // KEDIT filter bookkeeping: if the edit added or removed lines,
    // shift every excluded-line index strictly after `edit_line` so
    // the filter continues to hide the *same source lines* it did
    // before. Matches KEDIT's persistent-per-line selection-level
    // semantics: attributes travel with lines across edits rather
    // than being re-evaluated against the changed content.
    if line_delta != 0 {
        if let Some(filter) = editor.filter_mut(buffer_id) {
            filter.shift_indices(edit_line, line_delta);
        }
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
    // If the active pane is a terminal, forward the text to the PTY
    // rather than attempting to edit a buffer.
    if let Some(active) = editor.windows().active() {
        if let Some(term) = editor.terminal(active) {
            term.write(text.as_bytes().to_vec());
            editor.mark_dirty();
            return;
        }
    }
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

/// Forward a key to the active pane's terminal PTY if one is focused.
/// Returns `true` if forwarded, `false` if the active pane is a buffer
/// (in which case the caller should do the normal buffer action).
fn forward_to_terminal(editor: &mut Editor, bytes: &[u8]) -> bool {
    let Some(active) = editor.windows().active() else { return false };
    let Some(term) = editor.terminal(active) else { return false };
    term.write(bytes.to_vec());
    editor.mark_dirty();
    true
}

stock_cmd!(
    BufferNewline,
    BUFFER_NEWLINE,
    "Insert a newline at the cursor"
);
impl BufferNewline {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Terminal panes expect a carriage return for Enter, not LF.
        if forward_to_terminal(cx.editor, b"\r") {
            return;
        }
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
        // Terminal panes: forward backspace (0x7f) to the PTY.
        if forward_to_terminal(cx.editor, &[0x7f]) {
            return;
        }
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
        // Terminal panes: forward Delete (ESC [ 3 ~) to the PTY.
        if forward_to_terminal(cx.editor, b"\x1b[3~") {
            return;
        }
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

stock_cmd!(BufferTransposeWords, BUFFER_TRANSPOSE_WORDS, "Swap the two words around the cursor");
impl BufferTransposeWords {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let text = buffer.text();
        let bytes = text.as_bytes();
        // Find end of word after cursor.
        let mut end2 = cursor;
        while end2 < bytes.len() && !(bytes[end2].is_ascii_alphanumeric() || bytes[end2] == b'_') { end2 += 1; }
        while end2 < bytes.len() && (bytes[end2].is_ascii_alphanumeric() || bytes[end2] == b'_') { end2 += 1; }
        // Find start of that word.
        let mut start2 = end2;
        while start2 > 0 && (bytes[start2 - 1].is_ascii_alphanumeric() || bytes[start2 - 1] == b'_') { start2 -= 1; }
        // Find word before cursor.
        let mut end1 = cursor;
        while end1 > 0 && !(bytes[end1 - 1].is_ascii_alphanumeric() || bytes[end1 - 1] == b'_') { end1 -= 1; }
        let mut start1 = end1;
        while start1 > 0 && (bytes[start1 - 1].is_ascii_alphanumeric() || bytes[start1 - 1] == b'_') { start1 -= 1; }
        if start1 == end1 || start2 == end2 || end1 > start2 { return; }
        let w1 = text[start1..end1].to_owned();
        let w2 = text[start2..end2].to_owned();
        let mid = text[end1..start2].to_owned();
        let replacement = format!("{w2}{mid}{w1}");
        let range: ByteRange = start1..end2;
        user_edit(cx.editor, window_id, buffer_id, range, &replacement, cursor, end2);
    }
}

stock_cmd!(BufferJoinLines, BUFFER_JOIN_LINES, "Join current line with the next, collapsing whitespace");
impl BufferJoinLines {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        if line + 1 >= rope.len_lines() { return; }
        let line_end = rope.line_to_byte(line + 1) - 1; // before newline
        let next_start = rope.line_to_byte(line + 1);
        let next_text = rope.slice_to_string(next_start..rope.len_bytes().min(next_start + 200));
        let leading_ws = next_text.len() - next_text.trim_start().len();
        let range: ByteRange = line_end..(next_start + leading_ws);
        user_edit(cx.editor, window_id, buffer_id, range, " ", cursor, line_end + 1);
    }
}

stock_cmd!(BufferDuplicateLine, BUFFER_DUPLICATE_LINE, "Duplicate the current line below");
impl BufferDuplicateLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let start = rope.line_to_byte(line);
        let end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) } else { rope.len_bytes() };
        let line_text = rope.slice_to_string(start..end);
        let has_newline = line_text.ends_with('\n');
        let insert = if has_newline {
            line_text
        } else {
            format!("\n{line_text}")
        };
        let insert_at = end;
        let extra = usize::from(!has_newline);
        let new_cursor = insert_at + (cursor - start) + extra;
        user_edit(cx.editor, window_id, buffer_id, insert_at..insert_at, &insert, cursor, new_cursor);
    }
}

stock_cmd!(BufferMoveLineUp, BUFFER_MOVE_LINE_UP, "Move the current line up one position");
impl BufferMoveLineUp {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        if line == 0 { return; }
        let cur_start = rope.line_to_byte(line);
        let cur_end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) } else { rope.len_bytes() };
        let prev_start = rope.line_to_byte(line - 1);
        let cur_text = rope.slice_to_string(cur_start..cur_end);
        let prev_text = rope.slice_to_string(prev_start..cur_start);
        let new_text = format!("{cur_text}{prev_text}");
        let new_cursor = prev_start + (cursor - cur_start);
        let range: ByteRange = prev_start..cur_end;
        user_edit(cx.editor, window_id, buffer_id, range, &new_text, cursor, new_cursor);
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(BufferMoveLineDown, BUFFER_MOVE_LINE_DOWN, "Move the current line down one position");
impl BufferMoveLineDown {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let total = rope.len_lines();
        if line + 1 >= total { return; }
        let cur_start = rope.line_to_byte(line);
        let next_start = rope.line_to_byte(line + 1);
        let next_end = if line + 2 < total { rope.line_to_byte(line + 2) } else { rope.len_bytes() };
        let cur_text = rope.slice_to_string(cur_start..next_start);
        let next_text = rope.slice_to_string(next_start..next_end);
        let new_text = format!("{next_text}{cur_text}");
        let new_cursor = cur_start + next_text.len() + (cursor - cur_start);
        let range: ByteRange = cur_start..next_end;
        user_edit(cx.editor, window_id, buffer_id, range, &new_text, cursor, new_cursor);
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(BufferIndentLine, BUFFER_INDENT_LINE, "Indent the current line by one level");
impl BufferIndentLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let line = buffer.rope().byte_to_line(cursor);
        let line_start = buffer.rope().line_to_byte(line);
        user_edit(cx.editor, window_id, buffer_id, line_start..line_start, "    ", cursor, cursor + 4);
    }
}

stock_cmd!(BufferDedentLine, BUFFER_DEDENT_LINE, "Dedent the current line by one level");
impl BufferDedentLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let line_start = rope.line_to_byte(line);
        let text = buffer.text();
        let line_text = &text[line_start..];
        let spaces = line_text.chars().take_while(|c| *c == ' ').count().min(4);
        if spaces == 0 { return; }
        let range: ByteRange = line_start..(line_start + spaces);
        let new_cursor = cursor.saturating_sub(spaces);
        user_edit(cx.editor, window_id, buffer_id, range, "", cursor, new_cursor);
    }
}

stock_cmd!(BufferCommentToggle, BUFFER_COMMENT_TOGGLE, "Toggle line comment");
impl BufferCommentToggle {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        // Determine comment prefix from file extension.
        let comment = cx.editor.buffers().path(buffer_id)
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map_or("// ", |ext| match ext {
                "py" | "rb" | "sh" | "bash" | "zsh" | "toml" | "yaml" | "yml" => "# ",
                "lua" | "hs" => "-- ",
                "lisp" | "el" | "scm" | "clj" => ";; ",
                // rs, c, cpp, h, java, js, ts, go, swift, and everything else.
                _ => "// ",
            });
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let line_start = rope.line_to_byte(line);
        let line_end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) - 1 } else { rope.len_bytes() };
        let line_text = rope.slice_to_string(line_start..line_end);
        let trimmed = line_text.trim_start();
        let indent = line_text.len() - trimmed.len();
        if trimmed.starts_with(comment) {
            // Uncomment: remove the comment prefix.
            let prefix_start = line_start + indent;
            let prefix_end = prefix_start + comment.len();
            let range: ByteRange = prefix_start..prefix_end;
            let new_cursor = if cursor >= prefix_end { cursor - comment.len() } else { cursor };
            user_edit(cx.editor, window_id, buffer_id, range, "", cursor, new_cursor);
        } else {
            // Comment: insert the prefix after the indentation.
            let insert_at = line_start + indent;
            user_edit(cx.editor, window_id, buffer_id, insert_at..insert_at, comment, cursor, cursor + comment.len());
        }
    }
}

stock_cmd!(BufferMarkWhole, BUFFER_MARK_WHOLE, "Select the entire buffer");
impl BufferMarkWhole {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, _)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let end = buffer.len_bytes();
        cx.editor.set_mark(window_id, 0);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = end; }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(BufferExchangePointMark, BUFFER_EXCHANGE_POINT_MARK, "Exchange cursor and mark positions");
impl BufferExchangePointMark {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, _, cursor)) = active(cx.editor) else { return };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No mark set");
            return;
        };
        cx.editor.set_mark(window_id, cursor);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = mark; }
        cx.editor.mark_dirty();
        cx.editor.ensure_active_cursor_visible();
    }
}

stock_cmd!(BufferYankPop, BUFFER_YANK_POP, "Cycle kill ring after yank");
impl BufferYankPop {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Simplified: just show the kill ring size.
        cx.editor.set_status("M-y: yank-pop (not yet implemented)");
    }
}

stock_cmd!(GotoLine, GOTO_LINE, "Go to a specific line number");
impl GotoLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Open the palette in a pseudo goto-line mode.
        // For now, set status prompting the user to use M-x goto.line.
        cx.editor.set_status("M-g g: goto-line (use M-x goto.line N)");
    }
}

// Vim line operations.

stock_cmd!(BufferDeleteLine, BUFFER_DELETE_LINE, "Delete the current line (dd)");
impl BufferDeleteLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let start = rope.line_to_byte(line);
        let end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) } else { rope.len_bytes() };
        let killed = rope.slice_to_string(start..end);
        let new_cursor = if start > 0 && end >= rope.len_bytes() { start.saturating_sub(1) } else { start };
        user_edit(cx.editor, window_id, buffer_id, start..end, "", cursor, new_cursor);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
    }
}

stock_cmd!(BufferYankLine, BUFFER_YANK_LINE, "Yank (copy) the current line (yy)");
impl BufferYankLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let start = rope.line_to_byte(line);
        let end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) } else { rope.len_bytes() };
        let text = rope.slice_to_string(start..end);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(text));
        cx.editor.set_status("Line yanked");
    }
}

stock_cmd!(BufferChangeLine, BUFFER_CHANGE_LINE, "Delete line content and enter insert mode (cc)");
impl BufferChangeLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let start = rope.line_to_byte(line);
        let end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) - 1 } else { rope.len_bytes() };
        let line_text = rope.slice_to_string(start..end);
        let indent = line_text.len() - line_text.trim_start().len();
        let kill_start = start + indent;
        let killed = rope.slice_to_string(kill_start..end);
        user_edit(cx.editor, window_id, buffer_id, kill_start..end, "", cursor, kill_start);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
        // Enter insert mode if the vim.normal layer is active.
        if cx.editor.keymap().has_layer("vim.normal") {
            if let Some(cmd) = cx.editor.commands().get(names::MODE_ENTER_INSERT) {
                let mut inner = CommandContext { editor: cx.editor, bus: cx.bus.clone(), count: 1 };
                cmd.run(&mut inner);
            }
        }
    }
}

stock_cmd!(BufferDeleteToEol, BUFFER_DELETE_TO_EOL, "Delete from cursor to end of line (D)");
impl BufferDeleteToEol {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) - 1 } else { rope.len_bytes() };
        if cursor >= end { return; }
        let killed = rope.slice_to_string(cursor..end);
        user_edit(cx.editor, window_id, buffer_id, cursor..end, "", cursor, cursor);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
    }
}

stock_cmd!(BufferChangeToEol, BUFFER_CHANGE_TO_EOL, "Change from cursor to end of line (C)");
impl BufferChangeToEol {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Same as D but enters insert mode.
        BufferDeleteToEol::run_impl(cx);
        if cx.editor.keymap().has_layer("vim.normal") {
            if let Some(cmd) = cx.editor.commands().get(names::MODE_ENTER_INSERT) {
                let mut inner = CommandContext { editor: cx.editor, bus: cx.bus.clone(), count: 1 };
                cmd.run(&mut inner);
            }
        }
    }
}

stock_cmd!(BufferYankToEol, BUFFER_YANK_TO_EOL, "Yank from cursor to end of line (Y)");
impl BufferYankToEol {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let end = if line + 1 < rope.len_lines() { rope.line_to_byte(line + 1) - 1 } else { rope.len_bytes() };
        if cursor >= end { return; }
        let text = rope.slice_to_string(cursor..end);
        cx.editor.kill_ring_push(crate::editor::KilledText::Linear(text));
        cx.editor.set_status("Yanked to end of line");
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

stock_cmd!(
    EditorSuspend,
    EDITOR_SUSPEND,
    "Suspend the editor (SIGTSTP) — use `fg` to resume"
);
impl EditorSuspend {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.request_suspend();
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
            return;
        }
        // No insert layer to leave — if a terminal pane is focused,
        // forward Esc to the PTY so programs like vim/less work.
        forward_to_terminal(cx.editor, b"\x1b");
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
    editor.keymap_mut().push_layer(Layer::absorbing(
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
    editor.edit_with_highlight(
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
    editor.edit_with_highlight(
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
// LSP navigation
// ---------------------------------------------------------------------------

stock_cmd!(LspGotoDefinition, LSP_GOTO_DEFINITION, "Jump to symbol definition");
impl LspGotoDefinition {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Push current location to nav stack, then request definition.
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        cx.editor.nav_stack_push(buffer_id, cursor);
        // TODO: wire up async LSP textDocument/definition request.
        // For now, show a status message.
        cx.editor.set_status("goto-definition: LSP request not yet wired (nav stack pushed)");
        let _ = window_id;
    }
}

stock_cmd!(LspPopBack, LSP_POP_BACK, "Return to previous location");
impl LspPopBack {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((buffer_id, byte)) = cx.editor.nav_stack_pop() else {
            cx.editor.set_status("Navigation stack empty");
            return;
        };
        // Switch to the buffer and set cursor.
        let Some(window_id) = cx.editor.windows().active() else { return };
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) {
            w.buffer_id = buffer_id;
            w.cursor_byte = byte;
        }
        cx.editor.mark_dirty();
        cx.editor.ensure_active_cursor_visible();
    }
}

// ---------------------------------------------------------------------------
// Tree-sitter navigation
// ---------------------------------------------------------------------------

stock_cmd!(TreesitterNextFunction, TREESITTER_NEXT_FUNCTION, "Jump to next function definition");
impl TreesitterNextFunction {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let text = buffer.text();
        // Heuristic: find next line starting with `fn `, `def `, `func `,
        // `function `, or `pub fn ` after the current line.
        let rope = buffer.rope();
        let cur_line = rope.byte_to_line(cursor);
        for line_idx in (cur_line + 1)..rope.len_lines() {
            let start = rope.line_to_byte(line_idx);
            let end = if line_idx + 1 < rope.len_lines() { rope.line_to_byte(line_idx + 1) } else { text.len() };
            let line_text = &text[start..end];
            let trimmed = line_text.trim_start();
            if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub(crate) fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("def ")
                || trimmed.starts_with("func ")
                || trimmed.starts_with("function ")
            {
                if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = start; }
                cx.editor.mark_dirty();
                cx.editor.ensure_active_cursor_visible();
                return;
            }
        }
        cx.editor.set_status("No more functions below");
    }
}

stock_cmd!(TreesitterPrevFunction, TREESITTER_PREV_FUNCTION, "Jump to previous function definition");
impl TreesitterPrevFunction {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let text = buffer.text();
        let rope = buffer.rope();
        let cur_line = rope.byte_to_line(cursor);
        for line_idx in (0..cur_line).rev() {
            let start = rope.line_to_byte(line_idx);
            let end = if line_idx + 1 < rope.len_lines() { rope.line_to_byte(line_idx + 1) } else { text.len() };
            let line_text = &text[start..end];
            let trimmed = line_text.trim_start();
            if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub(crate) fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("def ")
                || trimmed.starts_with("func ")
                || trimmed.starts_with("function ")
            {
                if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = start; }
                cx.editor.mark_dirty();
                cx.editor.ensure_active_cursor_visible();
                return;
            }
        }
        cx.editor.set_status("No more functions above");
    }
}

stock_cmd!(TreesitterParentNode, TREESITTER_PARENT_NODE, "Jump to enclosing syntax scope");
impl TreesitterParentNode {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Heuristic: find the nearest line with less indentation than current.
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else { return };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else { return };
        let rope = buffer.rope();
        let cur_line = rope.byte_to_line(cursor);
        let cur_start = rope.line_to_byte(cur_line);
        let cur_text = rope.slice_to_string(cur_start..cursor.max(cur_start));
        let cur_indent = cur_text.len() - cur_text.trim_start().len();
        // Go backward until we find a line with strictly less indentation.
        for line_idx in (0..cur_line).rev() {
            let start = rope.line_to_byte(line_idx);
            let end = if line_idx + 1 < rope.len_lines() { rope.line_to_byte(line_idx + 1).saturating_sub(1) } else { rope.len_bytes() };
            let text = rope.slice_to_string(start..end);
            if text.trim().is_empty() { continue; }
            let indent = text.len() - text.trim_start().len();
            if indent < cur_indent {
                if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = start; }
                cx.editor.mark_dirty();
                cx.editor.ensure_active_cursor_visible();
                return;
            }
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
    editor.keymap_mut().push_layer(Layer::absorbing(
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
// Vim operator-pending mode
// ---------------------------------------------------------------------------
//
// When the user presses `d`, `c`, `y`, `>`, or `<` in normal mode, the
// editor enters operator-pending mode. The operator command:
// 1. Stores which operator is pending in `Editor::operator_state`.
// 2. Pushes the `vim.operator-pending` keymap layer.
//
// Motions (w, e, $, {, etc.) fall through to the normal layer. Text
// objects (iw, ip, i", etc.) are handled by the operator-pending layer.
// When a motion or text object executes, it checks `operator_state`:
// - If an operator is pending, compute the range and apply it.
// - If no operator, just move the cursor (normal motion).
//
// After applying the operator, the layer is popped and the state cleared.

fn push_operator(cx: &mut CommandContext<'_>, op: crate::editor::PendingOperator) {
    cx.editor.operator_state_mut().operator = Some(op);
    cx.editor.operator_state_mut().count = cx.count;
    if !cx.editor.keymap().has_layer("vim.operator-pending") {
        cx.editor.keymap_mut().push_layer(Layer::new(
            LayerId::from("vim.operator-pending"),
            Arc::new(arx_keymap::profiles::operator_pending_layer()),
        ));
    }
}

fn pop_operator(editor: &mut Editor) {
    editor.operator_state_mut().clear();
    if editor.keymap().has_layer("vim.operator-pending") {
        editor.keymap_mut().pop_layer();
    }
}

/// Apply the pending operator to a byte range. Handles delete, change,
/// yank, indent, and dedent.
fn apply_operator_to_range(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    start: usize,
    end: usize,
    linewise: bool,
) {
    use crate::editor::PendingOperator;

    let op = editor.operator_state().operator;
    let Some(op) = op else { return };

    // For linewise operations, extend to full lines.
    let (start, end) = if linewise {
        let buf = editor.buffers().get(buffer_id);
        if let Some(buf) = buf {
            let rope = buf.rope();
            let start_line = rope.byte_to_line(start);
            let end_line = rope.byte_to_line(end.saturating_sub(1).max(start));
            let ls = rope.line_to_byte(start_line);
            let le = if end_line + 1 < rope.len_lines() {
                rope.line_to_byte(end_line + 1)
            } else {
                rope.len_bytes()
            };
            (ls, le)
        } else {
            (start, end)
        }
    } else {
        (start, end)
    };

    if start >= end {
        pop_operator(editor);
        return;
    }

    match op {
        PendingOperator::Delete => {
            let killed = editor.buffers().get(buffer_id)
                .map(|b| b.rope().slice_to_string(start..end))
                .unwrap_or_default();
            user_edit(editor, window_id, buffer_id, start..end, "", start, start);
            editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
        }
        PendingOperator::Change => {
            let killed = editor.buffers().get(buffer_id)
                .map(|b| b.rope().slice_to_string(start..end))
                .unwrap_or_default();
            user_edit(editor, window_id, buffer_id, start..end, "", start, start);
            editor.kill_ring_push(crate::editor::KilledText::Linear(killed));
            // Enter insert mode by directly pushing the insert layer.
            editor.keymap_mut().push_layer(Layer::new(
                LayerId::from("insert"),
                Arc::new(arx_keymap::Keymap::named("vim.insert")),
            ));
        }
        PendingOperator::Yank => {
            let text = editor.buffers().get(buffer_id)
                .map(|b| b.rope().slice_to_string(start..end))
                .unwrap_or_default();
            editor.kill_ring_push(crate::editor::KilledText::Linear(text));
            editor.set_status("Yanked");
        }
        PendingOperator::Indent => {
            let Some(buf) = editor.buffers().get(buffer_id) else { pop_operator(editor); return };
            let start_line = buf.rope().byte_to_line(start);
            let end_line = buf.rope().byte_to_line(end.saturating_sub(1).max(start));
            for line_idx in (start_line..=end_line).rev() {
                let Some(b) = editor.buffers().get(buffer_id) else { break };
                let ls = b.rope().line_to_byte(line_idx);
                user_edit(editor, window_id, buffer_id, ls..ls, "    ", ls, ls + 4);
            }
        }
        PendingOperator::Dedent => {
            let Some(buf) = editor.buffers().get(buffer_id) else { pop_operator(editor); return };
            let start_line = buf.rope().byte_to_line(start);
            let end_line = buf.rope().byte_to_line(end.saturating_sub(1).max(start));
            for line_idx in (start_line..=end_line).rev() {
                let Some(b) = editor.buffers().get(buffer_id) else { break };
                let ls = b.rope().line_to_byte(line_idx);
                let line_text = b.text();
                let spaces = line_text[ls..].chars().take_while(|c| *c == ' ').count().min(4);
                if spaces > 0 {
                    user_edit(editor, window_id, buffer_id, ls..(ls + spaces), "", ls, ls);
                }
            }
        }
    }
    pop_operator(editor);
    editor.mark_dirty();
}

/// Check if an operator is pending and apply it to a motion range.
/// Returns `true` if an operator was applied, `false` if normal motion.
fn try_apply_operator_motion(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    from: usize,
    to: usize,
    linewise: bool,
) -> bool {
    if editor.operator_state().operator.is_none() {
        return false;
    }
    let (start, end) = if from <= to { (from, to) } else { (to, from) };
    apply_operator_to_range(editor, window_id, buffer_id, start, end, linewise);
    true
}

stock_cmd!(OperatorDelete, OPERATOR_DELETE, "Delete operator (d)");
impl OperatorDelete {
    fn run_impl(cx: &mut CommandContext<'_>) {
        push_operator(cx, crate::editor::PendingOperator::Delete);
    }
}

stock_cmd!(OperatorChange, OPERATOR_CHANGE, "Change operator (c)");
impl OperatorChange {
    fn run_impl(cx: &mut CommandContext<'_>) {
        push_operator(cx, crate::editor::PendingOperator::Change);
    }
}

stock_cmd!(OperatorYank, OPERATOR_YANK, "Yank operator (y)");
impl OperatorYank {
    fn run_impl(cx: &mut CommandContext<'_>) {
        push_operator(cx, crate::editor::PendingOperator::Yank);
    }
}

stock_cmd!(OperatorIndent, OPERATOR_INDENT, "Indent operator (>)");
impl OperatorIndent {
    fn run_impl(cx: &mut CommandContext<'_>) {
        push_operator(cx, crate::editor::PendingOperator::Indent);
    }
}

stock_cmd!(OperatorDedent, OPERATOR_DEDENT, "Dedent operator (<)");
impl OperatorDedent {
    fn run_impl(cx: &mut CommandContext<'_>) {
        push_operator(cx, crate::editor::PendingOperator::Dedent);
    }
}

stock_cmd!(OperatorCancel, OPERATOR_CANCEL, "Cancel the pending operator");
impl OperatorCancel {
    fn run_impl(cx: &mut CommandContext<'_>) {
        pop_operator(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(OperatorLineApply, OPERATOR_LINE, "Apply operator to current line (dd/yy/cc/>>/<< )");
impl OperatorLineApply {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            pop_operator(cx.editor);
            return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            pop_operator(cx.editor);
            return;
        };
        let rope = buffer.rope();
        let line = rope.byte_to_line(cursor);
        let start = rope.line_to_byte(line);
        let end = if line + 1 < rope.len_lines() {
            rope.line_to_byte(line + 1)
        } else {
            rope.len_bytes()
        };
        apply_operator_to_range(cx.editor, window_id, buffer_id, start, end, false);
    }
}

// ---------------------------------------------------------------------------
// Vim text objects
// ---------------------------------------------------------------------------

/// Find the byte range of the inner/outer delimited text (quotes, brackets).
fn find_delimited(text: &str, cursor: usize, open: char, close: char, inner: bool) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    // Find the opening delimiter backward from cursor.
    let mut open_pos = None;
    if open == close {
        // For quotes, find the nearest pair surrounding cursor.
        let mut positions = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            if b as char == open { positions.push(i); }
        }
        // Find the pair that surrounds cursor.
        for pair in positions.windows(2) {
            if pair[0] <= cursor && cursor <= pair[1] {
                open_pos = Some(pair[0]);
                let close_pos = pair[1];
                return if inner {
                    Some((open_pos? + 1, close_pos))
                } else {
                    Some((open_pos?, close_pos + 1))
                };
            }
        }
        return None;
    }
    // For brackets, handle nesting.
    let mut depth = 0i32;
    for i in (0..=cursor.min(bytes.len().saturating_sub(1))).rev() {
        let bc = bytes[i] as char;
        if bc == close { depth += 1; }
        if bc == open {
            if depth > 0 { depth -= 1; }
            else { open_pos = Some(i); break; }
        }
    }
    let open_pos = open_pos?;
    // Find the closing delimiter forward.
    depth = 0;
    for (i, &byte) in bytes.iter().enumerate().skip(open_pos + 1) {
        let bc = byte as char;
        if bc == open { depth += 1; }
        if bc == close {
            if depth > 0 { depth -= 1; }
            else {
                return if inner {
                    Some((open_pos + 1, i))
                } else {
                    Some((open_pos, i + 1))
                };
            }
        }
    }
    None
}

/// Find inner/outer word range.
fn find_word(text: &str, cursor: usize, include_surrounding: bool) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if cursor >= bytes.len() { return None; }
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    // Find start of word.
    let mut start = cursor;
    if is_word(bytes[start]) {
        while start > 0 && is_word(bytes[start - 1]) { start -= 1; }
    }
    // Find end of word.
    let mut end = cursor;
    if is_word(bytes[end]) {
        while end + 1 < bytes.len() && is_word(bytes[end + 1]) { end += 1; }
        end += 1;
    } else {
        // Cursor is on non-word char.
        while end + 1 < bytes.len() && !is_word(bytes[end + 1]) { end += 1; }
        end += 1;
        while start > 0 && !is_word(bytes[start - 1]) { start -= 1; }
    }
    if include_surrounding {
        // Include trailing whitespace.
        while end < bytes.len() && bytes[end] == b' ' { end += 1; }
    }
    Some((start, end))
}

/// Find inner/outer paragraph range.
#[allow(clippy::unnecessary_wraps)]
fn find_paragraph(text: &str, cursor: usize, include_surrounding: bool) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut byte_offset = 0;
    let mut cursor_line = 0;
    for (i, line) in lines.iter().enumerate() {
        let next = byte_offset + line.len() + 1;
        if cursor < next || i == lines.len() - 1 { cursor_line = i; break; }
        byte_offset = next;
    }
    // Find paragraph boundaries (blank lines).
    let mut start_line = cursor_line;
    while start_line > 0 && !lines[start_line - 1].trim().is_empty() { start_line -= 1; }
    let mut end_line = cursor_line;
    while end_line + 1 < lines.len() && !lines[end_line + 1].trim().is_empty() { end_line += 1; }

    let start_byte: usize = lines[..start_line].iter().map(|l| l.len() + 1).sum();
    let mut end_byte: usize = lines[..=end_line].iter().map(|l| l.len() + 1).sum();
    if end_byte > text.len() { end_byte = text.len(); }

    if include_surrounding {
        // Include trailing blank lines.
        let mut extra = end_line + 1;
        while extra < lines.len() && lines[extra].trim().is_empty() { extra += 1; }
        end_byte = lines[..extra].iter().map(|l| l.len() + 1).sum::<usize>().min(text.len());
    }
    Some((start_byte, end_byte))
}

/// Apply a text object: compute range, then apply pending operator.
fn apply_text_object(cx: &mut CommandContext<'_>, start: usize, end: usize) {
    let Some((window_id, buffer_id, _)) = active(cx.editor) else {
        pop_operator(cx.editor);
        return;
    };
    if cx.editor.operator_state().operator.is_some() {
        apply_operator_to_range(cx.editor, window_id, buffer_id, start, end, false);
    } else {
        // No operator: select the range (set mark + move cursor).
        cx.editor.set_mark(window_id, start);
        if let Some(w) = cx.editor.windows_mut().get_mut(window_id) { w.cursor_byte = end; }
        cx.editor.mark_dirty();
    }
}

macro_rules! text_object_delimited {
    ($name:ident, $const_name:ident, $desc:literal, $open:expr, $close:expr, $inner:expr) => {
        stock_cmd!($name, $const_name, $desc);
        impl $name {
            fn run_impl(cx: &mut CommandContext<'_>) {
                let Some((_, buffer_id, cursor)) = active(cx.editor) else {
                    pop_operator(cx.editor); return;
                };
                let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                    pop_operator(cx.editor); return;
                };
                let text = buffer.text();
                if let Some((s, e)) = find_delimited(&text, cursor, $open, $close, $inner) {
                    apply_text_object(cx, s, e);
                } else {
                    pop_operator(cx.editor);
                }
            }
        }
    };
}

text_object_delimited!(TextObjectInnerDoubleQuote, TEXT_OBJECT_INNER_DOUBLE_QUOTE, "Inner double-quoted string", '"', '"', true);
text_object_delimited!(TextObjectADoubleQuote, TEXT_OBJECT_A_DOUBLE_QUOTE, "A double-quoted string", '"', '"', false);
text_object_delimited!(TextObjectInnerSingleQuote, TEXT_OBJECT_INNER_SINGLE_QUOTE, "Inner single-quoted string", '\'', '\'', true);
text_object_delimited!(TextObjectASingleQuote, TEXT_OBJECT_A_SINGLE_QUOTE, "A single-quoted string", '\'', '\'', false);
text_object_delimited!(TextObjectInnerParen, TEXT_OBJECT_INNER_PAREN, "Inner parentheses", '(', ')', true);
text_object_delimited!(TextObjectAParen, TEXT_OBJECT_A_PAREN, "Including parentheses", '(', ')', false);
text_object_delimited!(TextObjectInnerBrace, TEXT_OBJECT_INNER_BRACE, "Inner curly braces", '{', '}', true);
text_object_delimited!(TextObjectABrace, TEXT_OBJECT_A_BRACE, "Including curly braces", '{', '}', false);
text_object_delimited!(TextObjectInnerBracket, TEXT_OBJECT_INNER_BRACKET, "Inner square brackets", '[', ']', true);
text_object_delimited!(TextObjectABracket, TEXT_OBJECT_A_BRACKET, "Including square brackets", '[', ']', false);
text_object_delimited!(TextObjectInnerAngle, TEXT_OBJECT_INNER_ANGLE, "Inner angle brackets", '<', '>', true);
text_object_delimited!(TextObjectAAngle, TEXT_OBJECT_A_ANGLE, "Including angle brackets", '<', '>', false);
text_object_delimited!(TextObjectInnerBacktick, TEXT_OBJECT_INNER_BACKTICK, "Inner backtick string", '`', '`', true);
text_object_delimited!(TextObjectABacktick, TEXT_OBJECT_A_BACKTICK, "Including backtick string", '`', '`', false);

stock_cmd!(TextObjectInnerWord, TEXT_OBJECT_INNER_WORD, "Inner word");
impl TextObjectInnerWord {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, cursor)) = active(cx.editor) else {
            pop_operator(cx.editor); return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            pop_operator(cx.editor); return;
        };
        let text = buffer.text();
        if let Some((s, e)) = find_word(&text, cursor, false) {
            apply_text_object(cx, s, e);
        } else {
            pop_operator(cx.editor);
        }
    }
}

stock_cmd!(TextObjectAWord, TEXT_OBJECT_A_WORD, "A word (including whitespace)");
impl TextObjectAWord {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, cursor)) = active(cx.editor) else {
            pop_operator(cx.editor); return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            pop_operator(cx.editor); return;
        };
        let text = buffer.text();
        if let Some((s, e)) = find_word(&text, cursor, true) {
            apply_text_object(cx, s, e);
        } else {
            pop_operator(cx.editor);
        }
    }
}

stock_cmd!(TextObjectInnerParagraph, TEXT_OBJECT_INNER_PARAGRAPH, "Inner paragraph");
impl TextObjectInnerParagraph {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, cursor)) = active(cx.editor) else {
            pop_operator(cx.editor); return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            pop_operator(cx.editor); return;
        };
        let text = buffer.text();
        if let Some((s, e)) = find_paragraph(&text, cursor, false) {
            apply_text_object(cx, s, e);
        } else {
            pop_operator(cx.editor);
        }
    }
}

stock_cmd!(TextObjectAParagraph, TEXT_OBJECT_A_PARAGRAPH, "A paragraph (including blank lines)");
impl TextObjectAParagraph {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((_, buffer_id, cursor)) = active(cx.editor) else {
            pop_operator(cx.editor); return;
        };
        let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
            pop_operator(cx.editor); return;
        };
        let text = buffer.text();
        if let Some((s, e)) = find_paragraph(&text, cursor, true) {
            apply_text_object(cx, s, e);
        } else {
            pop_operator(cx.editor);
        }
    }
}

// ---------------------------------------------------------------------------
// f/F/t/T char-read mode
// ---------------------------------------------------------------------------

/// Called from `handle_printable_fallback` when the editor is in
/// char-read mode (the user pressed `f`, `F`, `t`, or `T` and we're
/// waiting for the target character).
pub fn handle_char_read(editor: &mut Editor, ch: char, mode: crate::editor::CharReadMode) {
    use crate::editor::{CharReadMode, FindCharKind, FindCharState};

    let Some(window_id) = editor.windows().active() else {
        pop_operator(editor);
        return;
    };
    let Some(data) = editor.windows().get(window_id) else {
        pop_operator(editor);
        return;
    };
    let buffer_id = data.buffer_id;
    let cursor = data.cursor_byte;
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        pop_operator(editor);
        return;
    };
    let text = buffer.text();
    let rope = buffer.rope();
    let line = rope.byte_to_line(cursor);
    let line_start = rope.line_to_byte(line);
    let line_end = if line + 1 < rope.len_lines() {
        rope.line_to_byte(line + 1).saturating_sub(1)
    } else {
        rope.len_bytes()
    };

    let target = match mode {
        CharReadMode::FindForwardTo => {
            text[(cursor + 1)..line_end].find(ch).map(|i| cursor + 1 + i)
        }
        CharReadMode::FindForwardTill => {
            text[(cursor + 1)..line_end].find(ch).map(|i| cursor + i)
        }
        CharReadMode::FindBackwardTo => {
            text[line_start..cursor].rfind(ch).map(|i| line_start + i)
        }
        CharReadMode::FindBackwardTill => {
            text[line_start..cursor].rfind(ch).map(|i| line_start + i + 1)
        }
    };

    let kind = match mode {
        CharReadMode::FindForwardTo => FindCharKind::ForwardTo,
        CharReadMode::FindForwardTill => FindCharKind::ForwardTill,
        CharReadMode::FindBackwardTo => FindCharKind::BackwardTo,
        CharReadMode::FindBackwardTill => FindCharKind::BackwardTill,
    };
    editor.set_last_find_char(FindCharState { ch, kind });

    if let Some(target) = target {
        if editor.operator_state().operator.is_some() {
            // Apply operator to the range.
            let (start, end) = if target > cursor { (cursor, target + 1) } else { (target, cursor) };
            apply_operator_to_range(editor, window_id, buffer_id, start, end, false);
        } else {
            if let Some(w) = editor.windows_mut().get_mut(window_id) {
                w.cursor_byte = target;
            }
            pop_operator(editor);
        }
    } else {
        pop_operator(editor);
    }
    editor.mark_dirty();
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
            cx.editor.keymap_mut().push_layer(Layer::absorbing(
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

// ---------------------------------------------------------------------------
// KEDIT command line + block editing
// ---------------------------------------------------------------------------
//
// The kedit profile enables a persistent bottom input field (see
// `crate::kedit::KeditState`). Focus toggles between the buffer and
// the cmd line; when the cmd line has focus, printable keys extend its
// query and Enter executes it. A handful of kedit commands (QUIT,
// SAVE, FILE, TOP, BOTTOM, :N, LOCATE, CHANGE) are recognised; anything
// else is looked up in the command registry as a last resort so users
// can still invoke stock commands like `buffer.save` by name.

/// Push the `kedit.cmdline` keymap layer so navigation keys affect the
/// cmd-line query rather than buffer motion.
fn push_kedit_cmdline_layer(editor: &mut Editor) {
    if !editor.keymap().has_layer("kedit.cmdline") {
        editor.keymap_mut().push_layer(Layer::new(
            LayerId::from("kedit.cmdline"),
            Arc::new(arx_keymap::profiles::kedit_cmdline_layer()),
        ));
    }
}

/// Pop the `kedit.cmdline` keymap layer if it's currently on the stack.
fn pop_kedit_cmdline_layer(editor: &mut Editor) {
    if editor.keymap().has_layer("kedit.cmdline") {
        editor.keymap_mut().pop_layer();
    }
}

stock_cmd!(
    KeditFocusCmdline,
    KEDIT_FOCUS_CMDLINE,
    "Move focus to the KEDIT command line"
);
impl KeditFocusCmdline {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Enabling lazily means the command is harmless under non-kedit
        // profiles too — it simply turns on the cmd line.
        cx.editor.kedit_mut().enable();
        cx.editor.kedit_mut().focus();
        cx.editor.kedit_mut().clear_message();
        push_kedit_cmdline_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditFocusBuffer,
    KEDIT_FOCUS_BUFFER,
    "Move focus back from the KEDIT command line to the buffer"
);
impl KeditFocusBuffer {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().blur();
        pop_kedit_cmdline_layer(cx.editor);
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditToggleFocus,
    KEDIT_TOGGLE_FOCUS,
    "Toggle focus between the KEDIT command line and the buffer"
);
impl KeditToggleFocus {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if cx.editor.kedit().is_focused() {
            cx.editor.kedit_mut().blur();
            pop_kedit_cmdline_layer(cx.editor);
        } else if cx.editor.kedit().is_enabled() {
            cx.editor.kedit_mut().focus();
            cx.editor.kedit_mut().clear_message();
            push_kedit_cmdline_layer(cx.editor);
        } else {
            // Cmd line isn't enabled — fall back to buffer line-start
            // so unprofile-d users pressing Home still get a useful
            // action. Matches the Emacs / Vim default.
            let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
                return;
            };
            let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                return;
            };
            let line = buffer.rope().byte_to_line(cursor);
            let start = buffer.rope().line_to_byte(line);
            if let Some(w) = cx.editor.windows_mut().get_mut(window_id) {
                w.cursor_byte = start;
            }
        }
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineBackspace,
    KEDIT_CMDLINE_BACKSPACE,
    "Delete the character before the KEDIT command-line cursor"
);
impl KeditCmdlineBackspace {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().backspace();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineDeleteForward,
    KEDIT_CMDLINE_DELETE_FORWARD,
    "Delete the character at the KEDIT command-line cursor"
);
impl KeditCmdlineDeleteForward {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().delete_forward();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineClear,
    KEDIT_CMDLINE_CLEAR,
    "Clear the KEDIT command-line query"
);
impl KeditCmdlineClear {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().clear_query();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineCursorLeft,
    KEDIT_CMDLINE_CURSOR_LEFT,
    "Move the KEDIT command-line cursor one character left"
);
impl KeditCmdlineCursorLeft {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().cursor_left();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineCursorRight,
    KEDIT_CMDLINE_CURSOR_RIGHT,
    "Move the KEDIT command-line cursor one character right"
);
impl KeditCmdlineCursorRight {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().cursor_right();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineCursorHome,
    KEDIT_CMDLINE_CURSOR_HOME,
    "Move the KEDIT command-line cursor to the start"
);
impl KeditCmdlineCursorHome {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().cursor_home();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineCursorEnd,
    KEDIT_CMDLINE_CURSOR_END,
    "Move the KEDIT command-line cursor to the end"
);
impl KeditCmdlineCursorEnd {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().cursor_end();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineHistoryPrev,
    KEDIT_CMDLINE_HISTORY_PREV,
    "Navigate to the previous KEDIT command-line history entry"
);
impl KeditCmdlineHistoryPrev {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().history_prev();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineHistoryNext,
    KEDIT_CMDLINE_HISTORY_NEXT,
    "Navigate to the next KEDIT command-line history entry"
);
impl KeditCmdlineHistoryNext {
    fn run_impl(cx: &mut CommandContext<'_>) {
        cx.editor.kedit_mut().history_next();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(
    KeditCmdlineExecute,
    KEDIT_CMDLINE_EXECUTE,
    "Execute the text on the KEDIT command line"
);
impl KeditCmdlineExecute {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let text = cx.editor.kedit_mut().commit();
        // After commit the query is empty; focus returns to the buffer
        // so the user can act on the result. An explicit F11 re-parks
        // them on the cmd line if they want to chain commands.
        cx.editor.kedit_mut().blur();
        pop_kedit_cmdline_layer(cx.editor);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            cx.editor.mark_dirty();
            return;
        }
        match execute_kedit_command(cx, trimmed) {
            KeditCommandResult::Ok => {}
            KeditCommandResult::Message(msg) => {
                cx.editor.kedit_mut().set_message(msg);
            }
            KeditCommandResult::Unknown => {
                cx.editor
                    .kedit_mut()
                    .set_message(format!("Unknown kedit command: {trimmed}"));
            }
        }
        cx.editor.mark_dirty();
    }
}

/// Outcome of running a single cmd-line command. The executor is
/// fire-and-forget; all it reports back is whether the input was
/// recognised and an optional transient message for the prompt row.
enum KeditCommandResult {
    /// Command recognised and executed; nothing to show on the prompt.
    Ok,
    /// Command recognised; caller should set the given message.
    Message(String),
    /// Input didn't match any kedit verb *or* registered command name.
    Unknown,
}

/// Parse and dispatch `input` as a kedit cmd-line command. Recognises
/// the classic XEDIT / KEDIT verbs as well as any registered stock
/// command by its dotted name (so `buffer.save` works as a fallback).
///
/// Matching is case-insensitive on the verb. Kedit permits short
/// prefixes (`Q` for `QUIT`, `QQ` for `QQUIT`), which we support for
/// the common ones.
#[allow(clippy::too_many_lines)]
fn execute_kedit_command(cx: &mut CommandContext<'_>, input: &str) -> KeditCommandResult {
    let mut parts = input.splitn(2, char::is_whitespace);
    let verb = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();

    // `:N` jumps to line N.
    if let Some(rest) = verb.strip_prefix(':') {
        if let Ok(line) = rest.parse::<usize>() {
            goto_line_one_based(cx.editor, line);
            return KeditCommandResult::Ok;
        }
    }

    let verb_lc = verb.to_ascii_lowercase();
    match verb_lc.as_str() {
        "q" | "quit" => {
            cx.editor.request_quit();
            KeditCommandResult::Ok
        }
        "qq" | "qquit" => {
            // Quit without saving — same effect as quit for now.
            cx.editor.request_quit();
            KeditCommandResult::Ok
        }
        "save" | "file" => {
            let cmd = cx.editor.commands().get(names::BUFFER_SAVE);
            if let Some(cmd) = cmd {
                let mut inner = CommandContext {
                    editor: cx.editor,
                    bus: cx.bus.clone(),
                    count: 1,
                };
                cmd.run(&mut inner);
                KeditCommandResult::Message("Saved".into())
            } else {
                KeditCommandResult::Unknown
            }
        }
        "top" => {
            let cmd = cx.editor.commands().get(names::CURSOR_BUFFER_START);
            if let Some(cmd) = cmd {
                let mut inner = CommandContext {
                    editor: cx.editor,
                    bus: cx.bus.clone(),
                    count: 1,
                };
                cmd.run(&mut inner);
                KeditCommandResult::Ok
            } else {
                KeditCommandResult::Unknown
            }
        }
        "bot" | "bottom" => {
            let cmd = cx.editor.commands().get(names::CURSOR_BUFFER_END);
            if let Some(cmd) = cmd {
                let mut inner = CommandContext {
                    editor: cx.editor,
                    bus: cx.bus.clone(),
                    count: 1,
                };
                cmd.run(&mut inner);
                KeditCommandResult::Ok
            } else {
                KeditCommandResult::Unknown
            }
        }
        "locate" | "l" => {
            if args.is_empty() {
                return KeditCommandResult::Message("LOCATE: no pattern".into());
            }
            match locate_forward(cx.editor, args) {
                Some(n) => KeditCommandResult::Message(format!("Located on line {n}")),
                None => KeditCommandResult::Message("Not found".into()),
            }
        }
        "change" | "c" => change_command(cx.editor, args),
        "all" => all_command(cx.editor, args),
        "more" => more_or_less_command(cx.editor, args, FilterAdjust::More),
        "less" => more_or_less_command(cx.editor, args, FilterAdjust::Less),
        _ => {
            // Fallback: treat the input as a registered command name
            // (e.g. `buffer.save`). Keeps the cmd line useful without
            // needing M-x for everything.
            if let Some(cmd) = cx.editor.commands().get(input) {
                let mut inner = CommandContext {
                    editor: cx.editor,
                    bus: cx.bus.clone(),
                    count: 1,
                };
                cmd.run(&mut inner);
                KeditCommandResult::Ok
            } else {
                KeditCommandResult::Unknown
            }
        }
    }
}

/// Jump the active window's cursor to `line` (1-based), clamped to the
/// buffer's line count. Matches the `:N` kedit / XEDIT idiom.
fn goto_line_one_based(editor: &mut Editor, line_1: usize) {
    let Some(window_id) = editor.windows().active() else {
        return;
    };
    let Some(data) = editor.windows().get(window_id).cloned() else {
        return;
    };
    let buffer_id = data.buffer_id;
    let (byte, total_lines) = {
        let Some(buffer) = editor.buffers().get(buffer_id) else {
            return;
        };
        let rope = buffer.rope();
        let target = line_1.saturating_sub(1).min(rope.len_lines().saturating_sub(1));
        (rope.line_to_byte(target), rope.len_lines())
    };
    // Snap to the nearest visible line if a KEDIT `ALL` filter hides
    // the requested line. Without this the cursor lands on a hidden
    // line and every subsequent edit is rejected by the guard.
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return;
    };
    let rope = buffer.rope();
    let target_line = rope.byte_to_line(byte);
    let final_line = editor
        .filter(buffer_id)
        .map_or(target_line, |f| f.snap_to_visible(target_line, total_lines));
    let final_byte = if final_line == target_line {
        byte
    } else {
        rope.line_to_byte(final_line)
    };
    if let Some(window) = editor.windows_mut().get_mut(window_id) {
        window.cursor_byte = final_byte;
    }
}

/// LOCATE `<pattern>`: search forward from the cursor for a literal
/// substring and move the cursor to the first match. Returns the line
/// number (1-based) of the match, or `None` if not found.
///
/// Under a KEDIT `ALL` filter, matches that land on excluded lines
/// are skipped — the search keeps walking forward (and wraps to the
/// buffer start once) until it finds a match on a visible line.
fn locate_forward(editor: &mut Editor, pattern: &str) -> Option<usize> {
    let window_id = editor.windows().active()?;
    let data = editor.windows().get(window_id)?.clone();
    let buffer_id = data.buffer_id;
    let buffer = editor.buffers().get(buffer_id)?;
    let rope = buffer.rope();
    let text = buffer.text();
    let start = data.cursor_byte.min(text.len());

    // Walk forward from the cursor; skip matches on excluded lines.
    // When the forward search runs out we wrap once from byte 0 up
    // to (but not including) `start`.
    let is_excluded = |byte: usize| -> bool {
        editor
            .filter(buffer_id)
            .is_some_and(|f| f.is_excluded(rope.byte_to_line(byte)))
    };
    let find_visible_in = |slice_start: usize, slice_end: usize| -> Option<usize> {
        let mut search_from = slice_start;
        while search_from < slice_end {
            let slice = &text[search_from..slice_end];
            let rel = slice.find(pattern)?;
            let hit = search_from + rel;
            if !is_excluded(hit) {
                return Some(hit);
            }
            // Advance past this match and keep searching.
            search_from = hit + pattern.len().max(1);
        }
        None
    };
    let idx = find_visible_in(start, text.len()).or_else(|| find_visible_in(0, start))?;
    let line = rope.byte_to_line(idx) + 1;
    if let Some(window) = editor.windows_mut().get_mut(window_id) {
        window.cursor_byte = idx;
    }
    Some(line)
}

/// CHANGE `/old/new/` (or any single-char delimiter): replace the
/// first occurrence of `old` at or after the cursor with `new`.
fn change_command(editor: &mut Editor, args: &str) -> KeditCommandResult {
    let mut chars = args.chars();
    let Some(delim) = chars.next() else {
        return KeditCommandResult::Message("CHANGE: missing delimiter".into());
    };
    let rest: String = chars.collect();
    let mut parts = rest.splitn(3, delim);
    let old = parts.next().unwrap_or("");
    let new = parts.next().unwrap_or("");
    if old.is_empty() {
        return KeditCommandResult::Message("CHANGE: empty search".into());
    }
    let Some(window_id) = editor.windows().active() else {
        return KeditCommandResult::Unknown;
    };
    let Some(data) = editor.windows().get(window_id).cloned() else {
        return KeditCommandResult::Unknown;
    };
    let Some(buffer) = editor.buffers().get(data.buffer_id) else {
        return KeditCommandResult::Unknown;
    };
    let text = buffer.text();
    let start = data.cursor_byte.min(text.len());
    let Some(off) = text[start..].find(old).map(|o| start + o).or_else(|| text.find(old)) else {
        return KeditCommandResult::Message("Not found".into());
    };
    let len = old.len();
    let cursor_after = off + new.len();
    user_edit(
        editor,
        window_id,
        data.buffer_id,
        off..off + len,
        new,
        data.cursor_byte,
        cursor_after,
    );
    KeditCommandResult::Message("Changed".into())
}

/// `ALL <pattern>` — install a line-exclusion filter on the active
/// buffer. Lines matching `<pattern>` stay visible; every other line
/// is hidden from rendering, cursor motion, and edits. `ALL` with no
/// arguments (or with just a delimiter enclosing nothing) clears the
/// filter. Re-running `ALL` with a new pattern replaces the previous
/// filter — each invocation is evaluated against the full buffer.
///
/// Delimiter handling mirrors `CHANGE`: the first character is taken
/// as the delimiter and the pattern is everything up to (but not
/// including) the next occurrence, if any. `ALL /foo/` and `ALL foo`
/// are both accepted.
fn all_command(editor: &mut Editor, args: &str) -> KeditCommandResult {
    let Some(window_id) = editor.windows().active() else {
        return KeditCommandResult::Unknown;
    };
    let Some(data) = editor.windows().get(window_id).cloned() else {
        return KeditCommandResult::Unknown;
    };
    let buffer_id = data.buffer_id;

    // Parse the pattern. Empty args → clear the filter.
    let pattern = extract_delimited_pattern(args);
    if pattern.is_empty() {
        let removed = editor.clear_filter(buffer_id);
        editor.mark_dirty();
        if removed.is_some() {
            return KeditCommandResult::Message("ALL cleared".into());
        }
        return KeditCommandResult::Message("ALL: no active filter".into());
    }

    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return KeditCommandResult::Unknown;
    };
    let text = buffer.text();
    let total_lines = buffer.rope().len_lines();
    let filter = match crate::filter::FilterState::build(pattern, &text) {
        Ok(f) => f,
        Err(err) => {
            return KeditCommandResult::Message(format!("ALL: invalid regex: {err}"));
        }
    };
    let excluded = filter.excluded_count();
    let visible = total_lines.saturating_sub(excluded);

    // If the cursor ended up on an excluded line, snap it to the
    // nearest visible one so subsequent motion and edits work.
    let cursor_line = buffer.rope().byte_to_line(data.cursor_byte.min(text.len()));
    let snapped_line = filter.snap_to_visible(cursor_line, total_lines);
    if snapped_line != cursor_line {
        let new_cursor = buffer.rope().line_to_byte(snapped_line);
        if let Some(window) = editor.windows_mut().get_mut(window_id) {
            window.cursor_byte = new_cursor;
        }
    }

    editor.set_filter(buffer_id, filter);
    editor.mark_dirty();
    KeditCommandResult::Message(format!(
        "ALL /{pattern}/: {visible} of {total_lines} visible ({excluded} excluded)"
    ))
}

/// Which direction a cumulative filter step narrows or broadens.
/// Shared plumbing between `MORE` and `LESS` command dispatch.
#[derive(Debug, Clone, Copy)]
enum FilterAdjust {
    /// `MORE <pat>` — hide additional lines that don't match.
    More,
    /// `LESS <pat>` — re-include excluded lines that match.
    Less,
}

/// `MORE <pattern>` / `LESS <pattern>` — incrementally narrow or
/// broaden an existing `ALL` filter. Unlike `ALL`, these operate on
/// the *current* visible set rather than the full buffer, so they
/// compose: `ALL /foo/ MORE /bar/` keeps lines matching both.
///
/// When no filter is active, `MORE` bootstraps as if the user had
/// typed `ALL <pattern>` (same net effect: show only matching lines).
/// `LESS` without a pre-existing filter is a no-op with a friendly
/// message since there's nothing to un-exclude.
fn more_or_less_command(
    editor: &mut Editor,
    args: &str,
    adjust: FilterAdjust,
) -> KeditCommandResult {
    let Some(window_id) = editor.windows().active() else {
        return KeditCommandResult::Unknown;
    };
    let Some(data) = editor.windows().get(window_id).cloned() else {
        return KeditCommandResult::Unknown;
    };
    let buffer_id = data.buffer_id;

    let pattern = extract_delimited_pattern(args);
    if pattern.is_empty() {
        return KeditCommandResult::Message(format!(
            "{verb}: no pattern",
            verb = match adjust {
                FilterAdjust::More => "MORE",
                FilterAdjust::Less => "LESS",
            }
        ));
    }

    // Clone the text/metadata up-front so we can release the buffer
    // borrow before touching `filter_mut`. Buffers can be large;
    // clone is fine here — this path fires once per command.
    let Some(buffer) = editor.buffers().get(buffer_id) else {
        return KeditCommandResult::Unknown;
    };
    let text = buffer.text();
    let total_lines = buffer.rope().len_lines();
    let cursor_line = buffer.rope().byte_to_line(data.cursor_byte.min(text.len()));

    // If no filter exists yet, `MORE` installs a fresh `ALL`.
    // `LESS` without a filter has nothing to broaden.
    if editor.filter(buffer_id).is_none() {
        return match adjust {
            FilterAdjust::More => all_command(editor, args),
            FilterAdjust::Less => {
                KeditCommandResult::Message("LESS: no active filter".into())
            }
        };
    }

    // Apply the adjustment to the existing filter. Errors leave the
    // filter unchanged (FilterState::narrow/broaden guarantee this).
    let result: Result<(), regex::Error> = {
        let filter = editor
            .filter_mut(buffer_id)
            .expect("filter exists (checked above)");
        match adjust {
            FilterAdjust::More => filter.narrow(pattern, &text),
            FilterAdjust::Less => filter.broaden(pattern, &text),
        }
    };
    if let Err(err) = result {
        return KeditCommandResult::Message(format!(
            "{verb}: invalid regex: {err}",
            verb = match adjust {
                FilterAdjust::More => "MORE",
                FilterAdjust::Less => "LESS",
            }
        ));
    }

    // Snap cursor to the nearest visible line, in case the
    // adjustment just hid (MORE) the line we were on. LESS never
    // hides new lines so a re-snap there is a no-op in the common
    // case.
    let snapped_line = {
        let filter = editor
            .filter(buffer_id)
            .expect("still exists after adjust");
        filter.snap_to_visible(cursor_line, total_lines)
    };
    if snapped_line != cursor_line {
        if let Some(buf) = editor.buffers().get(buffer_id) {
            let new_cursor = buf.rope().line_to_byte(snapped_line);
            if let Some(window) = editor.windows_mut().get_mut(window_id) {
                window.cursor_byte = new_cursor;
            }
        }
    }

    let filter = editor
        .filter(buffer_id)
        .expect("still exists after adjust");
    let excluded = filter.excluded_count();
    let visible = total_lines.saturating_sub(excluded);
    editor.mark_dirty();
    let verb = match adjust {
        FilterAdjust::More => "MORE",
        FilterAdjust::Less => "LESS",
    };
    KeditCommandResult::Message(format!(
        "{verb} /{pattern}/: {visible} of {total_lines} visible ({excluded} excluded)"
    ))
}

/// Strip a leading-delimiter wrapper from `args`. Accepts `/pat/`,
/// `|pat|`, `"pat"`, etc. — the first char is the delimiter, and the
/// pattern is everything between the first and second delimiter. A
/// bare `pat` (no delimiter) is treated as the pattern as-is.
/// Returns the inner pattern, possibly empty.
fn extract_delimited_pattern(args: &str) -> &str {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return "";
    }
    // Heuristic: if the first char is a non-alphanumeric printable
    // that commonly appears as a delimiter, treat it as one.
    let first = trimmed.chars().next().unwrap_or('\0');
    if first.is_ascii_alphanumeric() || first == '_' {
        return trimmed;
    }
    let rest = &trimmed[first.len_utf8()..];
    match rest.find(first) {
        Some(end) => &rest[..end],
        None => rest, // unterminated delimiter; take the rest
    }
}

// ---------------------------------------------------------------------------
// Block marking + operations
// ---------------------------------------------------------------------------
//
// KEDIT's block model tags every selection with a `BlockKind`:
//
// * `Line` — whole lines between mark and cursor. Copy/delete/paste
//   operate on line units; paste drops the lines below the cursor.
// * `Box`  — display-column rectangle. Delegates to the existing
//   `column::*` helpers so kedit and Emacs `C-x r *` share code.
// * `Char` — contiguous byte range between mark and cursor.
//
// `block.copy` / `block.delete` / `block.move` inspect the kind via
// `editor.block_kind(window_id)` and dispatch to the right helper.

stock_cmd!(BlockMarkLine, BLOCK_MARK_LINE, "Mark a whole-line block at the cursor");
impl BlockMarkLine {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, _, cursor)) = active(cx.editor) else {
            return;
        };
        cx.editor
            .set_mark_with_mode(window_id, cursor, crate::editor::SelectionMode::Linear);
        cx.editor.set_block_kind(window_id, BlockKind::Line);
        cx.editor.set_status("-- BLOCK LINE --");
        cx.editor.mark_dirty();
    }
}

stock_cmd!(BlockMarkBox, BLOCK_MARK_BOX, "Mark a rectangular (box) block at the cursor");
impl BlockMarkBox {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, _, cursor)) = active(cx.editor) else {
            return;
        };
        cx.editor
            .set_mark_with_mode(window_id, cursor, crate::editor::SelectionMode::Rectangle);
        cx.editor.set_block_kind(window_id, BlockKind::Box);
        cx.editor.set_status("-- BLOCK BOX --");
        cx.editor.mark_dirty();
    }
}

stock_cmd!(BlockMarkChar, BLOCK_MARK_CHAR, "Mark a contiguous-character block at the cursor");
impl BlockMarkChar {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, _, cursor)) = active(cx.editor) else {
            return;
        };
        cx.editor
            .set_mark_with_mode(window_id, cursor, crate::editor::SelectionMode::Linear);
        cx.editor.set_block_kind(window_id, BlockKind::Char);
        cx.editor.set_status("-- BLOCK CHAR --");
        cx.editor.mark_dirty();
    }
}

stock_cmd!(BlockUnmark, BLOCK_UNMARK, "Unmark / reset the current block");
impl BlockUnmark {
    fn run_impl(cx: &mut CommandContext<'_>) {
        if let Some(window_id) = cx.editor.windows().active() {
            cx.editor.clear_mark(window_id);
            cx.editor.clear_block_kind(window_id);
        }
        cx.editor.clear_status();
        cx.editor.mark_dirty();
    }
}

stock_cmd!(BlockCopy, BLOCK_COPY, "Copy the marked block to the kill ring");
impl BlockCopy {
    fn run_impl(cx: &mut CommandContext<'_>) {
        block_copy_or_move(cx, /* cut */ false);
    }
}

stock_cmd!(BlockDelete, BLOCK_DELETE, "Delete the marked block");
impl BlockDelete {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // DELETE = cut-without-remembering for a paste-by-move, but we
        // still push the text to the kill ring so a later block.paste
        // or yank can retrieve it.
        block_copy_or_move(cx, /* cut */ true);
        cx.editor.kedit_mut().take_pending_move();
    }
}

stock_cmd!(BlockMove, BLOCK_MOVE, "Cut the marked block to the move register for paste");
impl BlockMove {
    fn run_impl(cx: &mut CommandContext<'_>) {
        block_copy_or_move(cx, /* cut */ true);
    }
}

stock_cmd!(BlockPaste, BLOCK_PASTE, "Paste the block clipboard at the cursor");
impl BlockPaste {
    fn run_impl(cx: &mut CommandContext<'_>) {
        // Prefer the pending-move clipboard (kedit semantics: Alt-M
        // stages a move, Alt-P drops it). Fall back to the kill ring.
        let Some((window_id, buffer_id, _)) = active(cx.editor) else {
            return;
        };
        if let Some(block) = cx.editor.kedit_mut().take_pending_move() {
            paste_clipboard_block(cx.editor, window_id, buffer_id, block);
            cx.editor.set_status("Block pasted");
            cx.editor.mark_dirty();
            return;
        }
        if let Some(entry) = cx.editor.kill_ring_top().cloned() {
            match entry {
                crate::editor::KilledText::Linear(text) => {
                    let cursor = cx
                        .editor
                        .windows()
                        .get(window_id)
                        .map_or(0, |d| d.cursor_byte);
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
            cx.editor.set_status("Block pasted");
            cx.editor.mark_dirty();
        } else {
            cx.editor.set_status("Kill ring empty");
        }
    }
}

stock_cmd!(BlockOverlay, BLOCK_OVERLAY, "Overlay the marked box-block at the cursor");
impl BlockOverlay {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No block marked");
            return;
        };
        if cx.editor.block_kind(window_id) != BlockKind::Box {
            cx.editor.set_status("Overlay needs a BOX block (Alt-B)");
            return;
        }
        let Some(rect) = crate::column::RectRegion::from_mark_cursor(
            cx.editor, buffer_id, mark, cursor,
        ) else {
            return;
        };
        // Overlay = for each line in the block, erase the target
        // rectangle (left..right display cols) then insert the source
        // text at the left edge. We implement it as open + fill with
        // spaces, which is functionally equivalent for the common
        // case and re-uses the existing column helpers.
        crate::column::open_rectangle(cx.editor, window_id, buffer_id, &rect);
        cx.editor.clear_mark(window_id);
        cx.editor.clear_block_kind(window_id);
        cx.editor.set_status("Block overlaid");
        cx.editor.mark_dirty();
    }
}

stock_cmd!(BlockFill, BLOCK_FILL, "Fill the marked block with a character");
impl BlockFill {
    fn run_impl(cx: &mut CommandContext<'_>) {
        let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
            return;
        };
        let Some(mark) = cx.editor.mark(window_id) else {
            cx.editor.set_status("No block marked");
            return;
        };
        match cx.editor.block_kind(window_id) {
            BlockKind::Box => {
                let Some(rect) = crate::column::RectRegion::from_mark_cursor(
                    cx.editor, buffer_id, mark, cursor,
                ) else {
                    return;
                };
                let width = rect.right_col.saturating_sub(rect.left_col) as usize;
                if width > 0 {
                    // First, kill the rectangle to clear it out, then
                    // paste a rectangle of `width` spaces per line.
                    let _ = crate::column::kill_rectangle(cx.editor, window_id, buffer_id, &rect);
                    let row_count = (rect.end_line - rect.start_line) + 1;
                    let spaces: String = " ".repeat(width);
                    let lines: Vec<String> = std::iter::repeat_n(spaces, row_count).collect();
                    crate::column::yank_rectangle(cx.editor, window_id, buffer_id, &lines);
                }
            }
            BlockKind::Line | BlockKind::Char => {
                let (start, end) = if mark <= cursor { (mark, cursor) } else { (cursor, mark) };
                if start < end {
                    let len = end - start;
                    let filler: String = " ".repeat(len);
                    user_edit(
                        cx.editor,
                        window_id,
                        buffer_id,
                        start..end,
                        &filler,
                        cursor,
                        start + len,
                    );
                }
            }
        }
        cx.editor.clear_mark(window_id);
        cx.editor.clear_block_kind(window_id);
        cx.editor.set_status("Block filled");
        cx.editor.mark_dirty();
    }
}

/// Shared copy / cut worker for block.copy, block.delete, block.move.
/// When `cut` is true the block is removed; the original text lands on
/// the kill ring (all three flavours) *and* on the pending-move
/// clipboard (move only) so a follow-up `block.paste` can drop it.
#[allow(clippy::too_many_lines)]
fn block_copy_or_move(cx: &mut CommandContext<'_>, cut: bool) {
    let Some((window_id, buffer_id, cursor)) = active(cx.editor) else {
        return;
    };
    let Some(mark) = cx.editor.mark(window_id) else {
        cx.editor.set_status("No block marked");
        return;
    };
    let kind = cx.editor.block_kind(window_id);
    match kind {
        BlockKind::Char => {
            let (start, end) = if mark <= cursor { (mark, cursor) } else { (cursor, mark) };
            if start == end {
                cx.editor.set_status("Empty block");
                return;
            }
            let text = cx
                .editor
                .buffers()
                .get(buffer_id)
                .map(|b| b.rope().slice_to_string(start..end))
                .unwrap_or_default();
            cx.editor
                .kill_ring_push(crate::editor::KilledText::Linear(text.clone()));
            cx.editor
                .kedit_mut()
                .set_pending_move(crate::kedit::ClipboardBlock::Char(text));
            if cut {
                user_edit(cx.editor, window_id, buffer_id, start..end, "", cursor, start);
            }
        }
        BlockKind::Line => {
            let (start_line, end_line) = {
                let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                    return;
                };
                let rope = buffer.rope();
                let m = rope.byte_to_line(mark);
                let c = rope.byte_to_line(cursor);
                (m.min(c), m.max(c))
            };
            let lines: Vec<String> = (start_line..=end_line)
                .map(|l| {
                    let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                        return String::new();
                    };
                    let rope = buffer.rope();
                    let s = rope.line_to_byte(l);
                    let e = if l + 1 >= rope.len_lines() {
                        rope.len_bytes()
                    } else {
                        rope.line_to_byte(l + 1).saturating_sub(1)
                    };
                    rope.slice_to_string(s..e)
                })
                .collect();
            let joined = lines.join("\n");
            cx.editor
                .kill_ring_push(crate::editor::KilledText::Linear(format!("{joined}\n")));
            cx.editor
                .kedit_mut()
                .set_pending_move(crate::kedit::ClipboardBlock::Line(lines));
            if cut {
                let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                    return;
                };
                let rope = buffer.rope();
                let start_byte = rope.line_to_byte(start_line);
                let end_byte = if end_line + 1 >= rope.len_lines() {
                    rope.len_bytes()
                } else {
                    rope.line_to_byte(end_line + 1)
                };
                user_edit(
                    cx.editor,
                    window_id,
                    buffer_id,
                    start_byte..end_byte,
                    "",
                    cursor,
                    start_byte,
                );
            }
        }
        BlockKind::Box => {
            let Some(rect) = crate::column::RectRegion::from_mark_cursor(
                cx.editor, buffer_id, mark, cursor,
            ) else {
                return;
            };
            // Extract the block first (non-destructive read).
            let mut copied = Vec::new();
            for line_idx in rect.start_line..=rect.end_line {
                let Some(buffer) = cx.editor.buffers().get(buffer_id) else {
                    break;
                };
                let rope = buffer.rope();
                let s = rope.line_to_byte(line_idx);
                let e = if line_idx + 1 >= rope.len_lines() {
                    rope.len_bytes()
                } else {
                    rope.line_to_byte(line_idx + 1).saturating_sub(1)
                };
                let text = rope.slice_to_string(s..e);
                let bs = crate::column::display_col_to_byte(&text, rect.left_col);
                let be = crate::column::display_col_to_byte(&text, rect.right_col);
                copied.push(text[bs..be].to_owned());
            }
            cx.editor
                .kill_ring_push(crate::editor::KilledText::Rectangular(copied.clone()));
            cx.editor
                .kedit_mut()
                .set_pending_move(crate::kedit::ClipboardBlock::Box(copied));
            if cut {
                let _ = crate::column::kill_rectangle(cx.editor, window_id, buffer_id, &rect);
            }
        }
    }
    // The block remains marked after a copy (kedit behaviour — makes
    // it easy to copy the same block multiple times). A cut clears it.
    if cut {
        cx.editor.clear_mark(window_id);
        cx.editor.clear_block_kind(window_id);
    }
    cx.editor.set_status(if cut { "Block cut" } else { "Block copied" });
    cx.editor.mark_dirty();
}

/// Insert a clipboard block at the active cursor position. Used by
/// `block.paste` when the pending-move register has content.
fn paste_clipboard_block(
    editor: &mut Editor,
    window_id: WindowId,
    buffer_id: BufferId,
    block: crate::kedit::ClipboardBlock,
) {
    match block {
        crate::kedit::ClipboardBlock::Char(text) => {
            let cursor = editor
                .windows()
                .get(window_id)
                .map_or(0, |d| d.cursor_byte);
            let len = text.len();
            user_edit(
                editor,
                window_id,
                buffer_id,
                cursor..cursor,
                &text,
                cursor,
                cursor + len,
            );
        }
        crate::kedit::ClipboardBlock::Line(lines) => {
            // Insert the lines below the current line so the user's
            // current position stays put. A trailing newline ensures
            // the paste ends on its own line.
            let Some(buffer) = editor.buffers().get(buffer_id) else {
                return;
            };
            let rope = buffer.rope();
            let cursor = editor
                .windows()
                .get(window_id)
                .map_or(0, |d| d.cursor_byte);
            let line = rope.byte_to_line(cursor);
            let insert_at = if line + 1 >= rope.len_lines() {
                rope.len_bytes()
            } else {
                rope.line_to_byte(line + 1)
            };
            let mut joined = lines.join("\n");
            joined.push('\n');
            // If we're at end of buffer without trailing newline, we
            // need to prepend one so lines land on their own rows.
            let needs_prefix_nl = insert_at == rope.len_bytes()
                && rope.len_bytes() > 0
                && !rope.slice_to_string((rope.len_bytes() - 1)..rope.len_bytes()).ends_with('\n');
            let text = if needs_prefix_nl {
                format!("\n{joined}")
            } else {
                joined
            };
            let text_len = text.len();
            user_edit(
                editor,
                window_id,
                buffer_id,
                insert_at..insert_at,
                &text,
                cursor,
                insert_at + text_len,
            );
        }
        crate::kedit::ClipboardBlock::Box(lines) => {
            crate::column::yank_rectangle(editor, window_id, buffer_id, &lines);
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

    // ---- KEDIT profile + command line ----

    #[tokio::test]
    async fn kedit_profile_enables_cmdline_on_new_editor() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_scratch();
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        let (enabled, focused) = bus
            .invoke(|editor| (editor.kedit().is_enabled(), editor.kedit().is_focused()))
            .await
            .unwrap();
        assert!(enabled, "kedit cmd line should be enabled by default");
        assert!(!focused, "cmd line should not have focus until requested");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn kedit_focus_cmdline_routes_printable_chars_to_query() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_from_text("hello", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_FOCUS_CMDLINE).await;
        bus.invoke(|editor| {
            for ch in "QUIT".chars() {
                editor.handle_printable_fallback(ch);
            }
        })
        .await
        .unwrap();
        let (query, buffer_text) = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                (
                    editor.kedit().query().to_owned(),
                    editor.buffers().get(data.buffer_id).unwrap().text(),
                )
            })
            .await
            .unwrap();
        // Buffer must be untouched.
        assert_eq!(buffer_text, "hello");
        assert_eq!(query, "QUIT");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn kedit_cmdline_quit_verb_requests_shutdown() {
        // The event loop shuts down as soon as `request_quit` flips the
        // flag, so we observe it from inside the same `invoke` that
        // runs the execute command.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_scratch();
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_FOCUS_CMDLINE).await;
        bus.invoke(|editor| {
            for ch in "QUIT".chars() {
                editor.handle_printable_fallback(ch);
            }
        })
        .await
        .unwrap();
        let bus_clone = bus.clone();
        let quit = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get(names::KEDIT_CMDLINE_EXECUTE).unwrap();
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                cx.editor.quit_requested()
            })
            .await
            .unwrap();
        assert!(quit, "QUIT should have requested shutdown");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn kedit_cmdline_goto_line_jumps_cursor() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_from_text("a\nb\nc\nd", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_FOCUS_CMDLINE).await;
        bus.invoke(|editor| {
            for ch in ":3".chars() {
                editor.handle_printable_fallback(ch);
            }
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_CMDLINE_EXECUTE).await;
        let cursor = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                editor.windows().get(win).unwrap().cursor_byte
            })
            .await
            .unwrap();
        // Line 3 (1-based) starts at byte 4 in "a\nb\nc\nd".
        assert_eq!(cursor, 4);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn kedit_cmdline_locate_moves_cursor_to_match() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("alpha\nbeta\ngamma", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_FOCUS_CMDLINE).await;
        bus.invoke(|editor| {
            for ch in "LOCATE gamma".chars() {
                editor.handle_printable_fallback(ch);
            }
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_CMDLINE_EXECUTE).await;
        let cursor = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                editor.windows().get(win).unwrap().cursor_byte
            })
            .await
            .unwrap();
        // "gamma" lives at byte 11 in "alpha\nbeta\ngamma".
        assert_eq!(cursor, 11);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn kedit_cmdline_change_replaces_first_match() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_from_text("foo bar foo", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_FOCUS_CMDLINE).await;
        bus.invoke(|editor| {
            for ch in "CHANGE /foo/baz/".chars() {
                editor.handle_printable_fallback(ch);
            }
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_CMDLINE_EXECUTE).await;
        let text = active_text_and_cursor(&bus).await.0;
        assert_eq!(text, "baz bar foo");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn kedit_focus_buffer_pops_cmdline_layer() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_scratch();
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_named(&bus, names::KEDIT_FOCUS_CMDLINE).await;
        let pushed = bus
            .invoke(|editor| editor.keymap().has_layer("kedit.cmdline"))
            .await
            .unwrap();
        assert!(pushed, "cmd line layer should be pushed after focus");
        run_named(&bus, names::KEDIT_FOCUS_BUFFER).await;
        let popped = bus
            .invoke(|editor| editor.keymap().has_layer("kedit.cmdline"))
            .await
            .unwrap();
        assert!(!popped, "cmd line layer should be popped after blur");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    // ---- Block editing ----

    #[tokio::test]
    async fn block_mark_line_then_copy_pushes_to_kill_ring() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor
                .buffers_mut()
                .create_from_text("line one\nline two\nline three", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        // Mark line block at cursor 0.
        run_named(&bus, names::BLOCK_MARK_LINE).await;
        // Move cursor down one line.
        run_named(&bus, names::CURSOR_DOWN).await;
        // Copy the block.
        run_named(&bus, names::BLOCK_COPY).await;
        let has_top = bus
            .invoke(|editor| editor.kill_ring_top().is_some())
            .await
            .unwrap();
        assert!(has_top, "kill ring should have an entry after block.copy");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn block_delete_char_block_removes_range_and_keeps_kill_ring() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello world", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 0;
        })
        .await
        .unwrap();
        run_named(&bus, names::BLOCK_MARK_CHAR).await;
        // Move cursor 5 bytes right (to after "hello").
        for _ in 0..5 {
            run_named(&bus, names::CURSOR_RIGHT).await;
        }
        run_named(&bus, names::BLOCK_DELETE).await;
        let (text, cursor) = active_text_and_cursor(&bus).await;
        assert_eq!(text, " world");
        assert_eq!(cursor, 0);
        let has_top = bus
            .invoke(|editor| editor.kill_ring_top().is_some())
            .await
            .unwrap();
        assert!(has_top);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn block_move_then_paste_relocates_text() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("abcdef", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 0;
        })
        .await
        .unwrap();
        run_named(&bus, names::BLOCK_MARK_CHAR).await;
        // Select "abc".
        for _ in 0..3 {
            run_named(&bus, names::CURSOR_RIGHT).await;
        }
        // Move → text cut + stashed in pending-move register.
        run_named(&bus, names::BLOCK_MOVE).await;
        let (after_move, _) = active_text_and_cursor(&bus).await;
        assert_eq!(after_move, "def");
        // Move cursor to the end and paste.
        run_named(&bus, names::CURSOR_BUFFER_END).await;
        run_named(&bus, names::BLOCK_PASTE).await;
        let (after_paste, _) = active_text_and_cursor(&bus).await;
        assert_eq!(after_paste, "defabc");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn block_unmark_clears_mark_and_kind() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("hello", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_named(&bus, names::BLOCK_MARK_BOX).await;
        let had_mark = bus
            .invoke(|editor| {
                editor.windows().active().is_some_and(|id| {
                    editor.mark(id).is_some() && editor.block_kind(id) == BlockKind::Box
                })
            })
            .await
            .unwrap();
        assert!(had_mark);
        run_named(&bus, names::BLOCK_UNMARK).await;
        let cleared = bus
            .invoke(|editor| {
                editor
                    .windows()
                    .active()
                    .is_some_and(|id| editor.mark(id).is_none())
            })
            .await
            .unwrap();
        assert!(cleared);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    // ---- KEDIT ALL command ----

    /// Helper: open a kedit-profile editor over `text` and run the
    /// given cmd-line input through `kedit.cmdline-execute`. Leaves
    /// the editor in buffer-focus mode (execute blurs the cmd line).
    async fn run_kedit_cmdline(
        bus: &crate::CommandBus,
        input: &'static str,
    ) {
        bus.invoke(move |editor| {
            editor.kedit_mut().enable();
            editor.kedit_mut().focus();
            editor.kedit_mut().set_query(input);
        })
        .await
        .unwrap();
        run_named(bus, names::KEDIT_CMDLINE_EXECUTE).await;
    }

    #[tokio::test]
    async fn all_filter_excludes_non_matching_lines() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_from_text(
                "foo line\nbar line\nanother foo\nbaz\nfoo again",
                None,
            );
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        let (has_filter, excluded) = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let f = editor.filter(data.buffer_id);
                let count = f.map_or(0, crate::filter::FilterState::excluded_count);
                (f.is_some(), count)
            })
            .await
            .unwrap();
        assert!(has_filter, "ALL should install a filter");
        // Lines 1 ("bar line") and 3 ("baz") don't match /foo/.
        assert_eq!(excluded, 2);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn all_with_no_args_clears_filter() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nfoo\n", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_kedit_cmdline(&bus, "ALL").await;
        let has_filter = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor.filter(data.buffer_id).is_some()
            })
            .await
            .unwrap();
        assert!(!has_filter, "bare ALL should clear the filter");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn all_replacing_filter_re_evaluates_against_full_buffer() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nbaz\nbar", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_kedit_cmdline(&bus, "ALL /bar/").await;
        let excluded = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor
                    .filter(data.buffer_id)
                    .map_or(0, crate::filter::FilterState::excluded_count)
            })
            .await
            .unwrap();
        // Lines 0 ("foo") and 2 ("baz") don't match /bar/ → 2 excluded.
        assert_eq!(excluded, 2);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn all_invalid_regex_reports_error_and_leaves_filter_off() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_from_text("line\n", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /(/").await;
        let has_filter = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor.filter(data.buffer_id).is_some()
            })
            .await
            .unwrap();
        assert!(!has_filter, "invalid regex should not install a filter");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn cursor_down_skips_excluded_lines() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            // Lines 0, 2, 4 match; 1, 3 don't.
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nfoo\nbar\nfoo", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 0;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_named(&bus, names::CURSOR_DOWN).await;
        let cursor_line = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let buffer = editor.buffers().get(data.buffer_id).unwrap();
                buffer.rope().byte_to_line(data.cursor_byte)
            })
            .await
            .unwrap();
        // One cursor.down should land on line 2 (skipping excluded 1).
        assert_eq!(cursor_line, 2);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn edit_on_visible_line_succeeds_under_filter() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nfoo", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 3;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        // Cursor was at byte 3 (end of "foo") — line 0, which is
        // visible. Self-insert is allowed.
        bus.invoke(|editor| editor.handle_printable_fallback('X'))
            .await
            .unwrap();
        let text = active_text_and_cursor(&bus).await.0;
        assert_eq!(text, "fooX\nbar\nfoo");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn edit_on_excluded_line_is_blocked() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nfoo", None);
            let win = editor.windows_mut().open(buf);
            // Park the cursor explicitly on line 1 (the "bar" line)
            // which will be excluded by ALL /foo/. This bypasses the
            // snap-to-visible logic so we can test the edit guard in
            // isolation.
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 4;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        // Manually plant the cursor back on the excluded line (the
        // ALL command snaps it off); then attempt an edit.
        bus.invoke(|editor| {
            let win = editor.windows().active().unwrap();
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 5;
        })
        .await
        .unwrap();
        bus.invoke(|editor| editor.handle_printable_fallback('Z'))
            .await
            .unwrap();
        let text = active_text_and_cursor(&bus).await.0;
        assert_eq!(text, "foo\nbar\nfoo", "edit on excluded line must be rejected");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn all_snaps_cursor_off_excluded_line_into_next_visible() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("bar\nfoo\nbar\nfoo", None);
            let win = editor.windows_mut().open(buf);
            // Cursor on line 0 ("bar"), which will be excluded.
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 0;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        let cursor_line = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let buffer = editor.buffers().get(data.buffer_id).unwrap();
                buffer.rope().byte_to_line(data.cursor_byte)
            })
            .await
            .unwrap();
        // Snapped down to line 1 ("foo").
        assert_eq!(cursor_line, 1);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn more_narrows_existing_filter() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            // 0: foo alpha   1: foo beta   2: bar alpha   3: foo alpha
            let buf = editor
                .buffers_mut()
                .create_from_text("foo alpha\nfoo beta\nbar alpha\nfoo alpha", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_kedit_cmdline(&bus, "MORE /alpha/").await;
        let excluded = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor
                    .filter(data.buffer_id)
                    .map_or(0, crate::filter::FilterState::excluded_count)
            })
            .await
            .unwrap();
        // Final visible: 0 and 3 only (both "foo alpha"). 1 and 2 excluded.
        assert_eq!(excluded, 2);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn less_reincludes_lines_matching_pattern() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nbaz\nbar", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        // ALL /foo/ → excluded = {1, 2, 3}
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        // LESS /bar/ → re-include 1 and 3; leaves {2}.
        run_kedit_cmdline(&bus, "LESS /bar/").await;
        let excluded = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor
                    .filter(data.buffer_id)
                    .map_or(0, crate::filter::FilterState::excluded_count)
            })
            .await
            .unwrap();
        assert_eq!(excluded, 1);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn less_without_active_filter_is_a_noop() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor.buffers_mut().create_from_text("foo\nbar", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "LESS /foo/").await;
        let has_filter = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor.filter(data.buffer_id).is_some()
            })
            .await
            .unwrap();
        assert!(!has_filter, "LESS without ALL should not install a filter");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn more_without_active_filter_bootstraps_as_all() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nfoo", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "MORE /foo/").await;
        let excluded = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor
                    .filter(data.buffer_id)
                    .map_or(0, crate::filter::FilterState::excluded_count)
            })
            .await
            .unwrap();
        // Same effect as ALL /foo/: one line ("bar") excluded.
        assert_eq!(excluded, 1);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn goto_line_snaps_to_visible_under_filter() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nbaz\nfoo", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        // `:2` targets line 2 ("bar") which is excluded; expect
        // snap-down to line 3 ("foo", the next visible).
        run_kedit_cmdline(&bus, ":2").await;
        let cursor_line = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let buffer = editor.buffers().get(data.buffer_id).unwrap();
                buffer.rope().byte_to_line(data.cursor_byte)
            })
            .await
            .unwrap();
        // Cursor on line 3 (0-based) — the next visible "foo" line.
        assert_eq!(cursor_line, 3);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn locate_skips_matches_on_excluded_lines() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            // Three lines all contain "xy"; ALL /foo/ hides the first
            // two. LOCATE xy should find the match on the visible
            // third line (line 2) rather than on the hidden line 0.
            let buf = editor
                .buffers_mut()
                .create_from_text("xy\nxy\nfoo xy", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 0;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_kedit_cmdline(&bus, "LOCATE xy").await;
        let cursor_line = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let buffer = editor.buffers().get(data.buffer_id).unwrap();
                buffer.rope().byte_to_line(data.cursor_byte)
            })
            .await
            .unwrap();
        assert_eq!(cursor_line, 2, "LOCATE should skip excluded-line matches");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn paragraph_forward_skips_excluded_lines() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            // Buffer: foo, blank, foo, blank, foo.
            // Plant a filter that excludes line 1 only (the first
            // blank). Paragraph-forward from line 0 must skip that
            // hidden blank and stop at the visible blank on line 3.
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\n\nfoo\n\nfoo", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 0;
        })
        .await
        .unwrap();
        bus.invoke(|editor| {
            let win = editor.windows().active().unwrap();
            let data = editor.windows().get(win).unwrap();
            // Build an empty-excluded filter, then manually exclude
            // line 1. `.*` matches every line (including empty ones
            // in the regex crate's default semantics).
            let mut filter = crate::filter::FilterState::build(".*", "foo\n\nfoo\n\nfoo").unwrap();
            filter.excluded.clear();
            filter.excluded.insert(1);
            editor.set_filter(data.buffer_id, filter);
        })
        .await
        .unwrap();
        run_named(&bus, names::CURSOR_PARAGRAPH_FORWARD).await;
        let cursor_line = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let buffer = editor.buffers().get(data.buffer_id).unwrap();
                buffer.rope().byte_to_line(data.cursor_byte)
            })
            .await
            .unwrap();
        // Next visible blank is line 3.
        assert_eq!(cursor_line, 3);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn newline_insert_shifts_excluded_indices_down() {
        // Buffer: 0 "foo", 1 "bar", 2 "baz". ALL /foo/ excludes {1, 2}.
        // Press Enter on line 0 (cursor at byte 3, end of "foo") →
        // new line 1 "" is visible, old line 1 "bar" becomes line 2,
        // old line 2 "baz" becomes line 3. Excluded set must update
        // to {2, 3}, so the new empty line 1 stays visible.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nbaz", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 3;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_named(&bus, names::BUFFER_NEWLINE).await;
        let (excluded, total_lines) = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let buffer = editor.buffers().get(data.buffer_id).unwrap();
                let filter = editor.filter(data.buffer_id).unwrap();
                (
                    filter.excluded.iter().copied().collect::<Vec<usize>>(),
                    buffer.rope().len_lines(),
                )
            })
            .await
            .unwrap();
        assert_eq!(total_lines, 4);
        assert_eq!(excluded, vec![2, 3]);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn newline_delete_shifts_excluded_indices_up() {
        // Buffer: 0 "foo", 1 "foo", 2 "bar", 3 "baz".
        // ALL /foo/ excludes {2, 3}. On line 0 (cursor at byte 3,
        // end of first "foo"), delete-forward consumes the newline,
        // joining lines 0 and 1 into one "foofoo". Excluded set
        // must become {1, 2}.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nfoo\nbar\nbaz", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 3;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_named(&bus, names::BUFFER_DELETE_FORWARD).await;
        let (excluded, text) = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                let buffer = editor.buffers().get(data.buffer_id).unwrap();
                let filter = editor.filter(data.buffer_id).unwrap();
                (
                    filter.excluded.iter().copied().collect::<Vec<usize>>(),
                    buffer.text(),
                )
            })
            .await
            .unwrap();
        assert_eq!(text, "foofoo\nbar\nbaz");
        assert_eq!(excluded, vec![1, 2]);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn newly_inserted_line_below_visible_stays_visible() {
        // Same setup as the shift test: pressing Enter on a visible
        // line must leave the freshly-created line unexcluded. The
        // end-to-end check: self-insert text on the new line should
        // NOT be blocked by the edit guard.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo\nbar\nbaz", None);
            let win = editor.windows_mut().open(buf);
            editor.windows_mut().get_mut(win).unwrap().cursor_byte = 3;
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_named(&bus, names::BUFFER_NEWLINE).await;
        // Cursor should now sit on the new empty line 1. Self-insert
        // to confirm the edit guard lets it through.
        bus.invoke(|editor| editor.handle_printable_fallback('X'))
            .await
            .unwrap();
        let text = active_text_and_cursor(&bus).await.0;
        assert_eq!(text, "foo\nX\nbar\nbaz");
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn filter_chain_is_reflected_in_describe() {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            *editor = Editor::with_profile(arx_keymap::profiles::kedit());
            let buf = editor
                .buffers_mut()
                .create_from_text("foo alpha\nfoo beta\nbar alpha", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        run_kedit_cmdline(&bus, "ALL /foo/").await;
        run_kedit_cmdline(&bus, "MORE /alpha/").await;
        let desc = bus
            .invoke(|editor| {
                let win = editor.windows().active().unwrap();
                let data = editor.windows().get(win).unwrap();
                editor
                    .filter(data.buffer_id)
                    .map(crate::filter::FilterState::describe)
                    .unwrap_or_default()
            })
            .await
            .unwrap();
        assert_eq!(desc, "ALL /foo/ MORE /alpha/");
        drop(bus);
        let _ = handle.await.unwrap();
    }
}
