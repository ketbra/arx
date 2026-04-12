//! Canonical names for the stock command catalogue.
//!
//! These are the names profiles bind against and that the
//! [`arx_core::CommandRegistry`](https://docs.rs/arx-core) registers. A
//! single module so the "known command set" is discoverable from both
//! sides without hardcoded string literals scattered through the tree.
//!
//! Follow-up milestones will add: `cursor.word-*`, `buffer.undo`,
//! `buffer.redo`, `buffer.indent`, `window.*`, `search.*`,
//! `command-palette.*`, Vim-flavoured `vim.*`, KEDIT-flavoured
//! `kedit.*`, etc.

/// Move the primary cursor left by one grapheme.
pub const CURSOR_LEFT: &str = "cursor.left";
/// Move the primary cursor right by one grapheme.
pub const CURSOR_RIGHT: &str = "cursor.right";
/// Move the primary cursor up one line, preserving column.
pub const CURSOR_UP: &str = "cursor.up";
/// Move the primary cursor down one line, preserving column.
pub const CURSOR_DOWN: &str = "cursor.down";
/// Move the primary cursor to the start of its current line.
pub const CURSOR_LINE_START: &str = "cursor.line-start";
/// Move the primary cursor to the end of its current line.
pub const CURSOR_LINE_END: &str = "cursor.line-end";
/// Move the primary cursor forward one word.
pub const CURSOR_WORD_FORWARD: &str = "cursor.word-forward";
/// Move the primary cursor backward one word.
pub const CURSOR_WORD_BACKWARD: &str = "cursor.word-backward";
/// Move the primary cursor to the start of the buffer.
pub const CURSOR_BUFFER_START: &str = "cursor.buffer-start";
/// Move the primary cursor to the end of the buffer.
pub const CURSOR_BUFFER_END: &str = "cursor.buffer-end";

/// Insert a newline at the cursor.
pub const BUFFER_NEWLINE: &str = "buffer.newline";
/// Delete the grapheme before the cursor.
pub const BUFFER_DELETE_BACKWARD: &str = "buffer.delete-backward";
/// Delete the grapheme at the cursor.
pub const BUFFER_DELETE_FORWARD: &str = "buffer.delete-forward";
/// Kill (cut) from the cursor to the end of the line.
pub const BUFFER_KILL_LINE: &str = "buffer.kill-line";
/// Kill (cut) the word after the cursor.
pub const BUFFER_KILL_WORD: &str = "buffer.kill-word";
/// Kill (cut) the word before the cursor.
pub const BUFFER_KILL_WORD_BACKWARD: &str = "buffer.kill-word-backward";
/// Kill (cut) the region between the mark and cursor.
pub const BUFFER_KILL_REGION: &str = "buffer.kill-region";
/// Copy the region between the mark and cursor (without deleting).
pub const BUFFER_COPY_REGION: &str = "buffer.copy-region";
/// Yank (paste) the most recently killed text.
pub const BUFFER_YANK: &str = "buffer.yank";
/// Set the mark at the cursor position (start a selection).
pub const BUFFER_SET_MARK: &str = "buffer.set-mark";
/// Close the current buffer.
pub const BUFFER_CLOSE: &str = "buffer.close";
/// Switch to a different open buffer (via palette).
pub const BUFFER_SWITCH: &str = "buffer.switch";
/// Persist the active buffer to its associated path.
pub const BUFFER_SAVE: &str = "buffer.save";
/// Undo the last user edit on the active buffer, walking the buffer's
/// undo tree up toward the root.
pub const BUFFER_UNDO: &str = "buffer.undo";
/// Re-apply the last undone edit, walking the buffer's undo tree back
/// down toward the most recent leaf.
pub const BUFFER_REDO: &str = "buffer.redo";
/// Switch the undo tree's redo branch to the next sibling.
pub const BUFFER_UNDO_BRANCH_NEXT: &str = "buffer.undo-branch-next";
/// Switch the undo tree's redo branch to the previous sibling.
pub const BUFFER_UNDO_BRANCH_PREV: &str = "buffer.undo-branch-prev";

/// Scroll the active window up by one page.
pub const SCROLL_PAGE_UP: &str = "scroll.page-up";
/// Scroll the active window down by one page.
pub const SCROLL_PAGE_DOWN: &str = "scroll.page-down";

/// Insert a newline without moving the cursor (open-line).
pub const BUFFER_OPEN_LINE: &str = "buffer.open-line";
/// Transpose (swap) the two characters around the cursor.
pub const BUFFER_TRANSPOSE_CHARS: &str = "buffer.transpose-chars";

/// Request editor shutdown.
pub const EDITOR_QUIT: &str = "editor.quit";
/// Cancel the current operation (keyboard-quit).
pub const EDITOR_CANCEL: &str = "editor.cancel";
/// Recenter the view so the cursor is in the middle of the window.
pub const SCROLL_RECENTER: &str = "scroll.recenter";

/// Enter Vim-style insert mode (push an `insert` layer over `vim.normal`).
pub const MODE_ENTER_INSERT: &str = "mode.enter-insert";
/// Leave insert mode (pop the top layer back to the enclosing mode).
pub const MODE_LEAVE_INSERT: &str = "mode.leave-insert";

/// Open the command palette (M-x-style fuzzy command search).
pub const COMMAND_PALETTE_OPEN: &str = "command-palette.open";
/// Close the command palette without executing a command.
pub const COMMAND_PALETTE_CLOSE: &str = "command-palette.close";
/// Execute the currently-highlighted command in the palette.
pub const COMMAND_PALETTE_EXECUTE: &str = "command-palette.execute";
/// Move the palette selection down one row.
pub const COMMAND_PALETTE_NEXT: &str = "command-palette.next";
/// Move the palette selection up one row.
pub const COMMAND_PALETTE_PREV: &str = "command-palette.prev";
/// Remove the last character from the palette query.
pub const COMMAND_PALETTE_BACKSPACE: &str = "command-palette.backspace";

/// Split the active window horizontally — stacking a new pane beneath
/// it. Vim `:split` / Emacs `C-x 2`.
pub const WINDOW_SPLIT_HORIZONTAL: &str = "window.split-horizontal";
/// Split the active window vertically — placing a new pane beside it.
/// Vim `:vsplit` / Emacs `C-x 3`.
pub const WINDOW_SPLIT_VERTICAL: &str = "window.split-vertical";
/// Close the active window, collapsing its enclosing split into the
/// surviving sibling. No-op when only one window is left.
pub const WINDOW_CLOSE: &str = "window.close";
/// Cycle focus to the next window in depth-first layout order.
pub const WINDOW_FOCUS_NEXT: &str = "window.focus-next";
/// Cycle focus to the previous window in depth-first layout order.
pub const WINDOW_FOCUS_PREV: &str = "window.focus-prev";

/// Show hover information (type, docs) at the cursor position.
pub const LSP_HOVER: &str = "lsp.hover";
/// Jump to the next diagnostic in the current buffer.
pub const LSP_NEXT_DIAGNOSTIC: &str = "lsp.next-diagnostic";
/// Jump to the previous diagnostic in the current buffer.
pub const LSP_PREV_DIAGNOSTIC: &str = "lsp.prev-diagnostic";

/// Trigger code completion at the cursor position.
pub const COMPLETION_TRIGGER: &str = "completion.trigger";
/// Accept the currently-selected completion item.
pub const COMPLETION_ACCEPT: &str = "completion.accept";
/// Dismiss the completion popup.
pub const COMPLETION_DISMISS: &str = "completion.dismiss";
/// Move the completion selection down one row.
pub const COMPLETION_NEXT: &str = "completion.next";
/// Move the completion selection up one row.
pub const COMPLETION_PREV: &str = "completion.prev";

/// Open an embedded terminal in a split pane.
pub const TERMINAL_OPEN: &str = "terminal.open";
