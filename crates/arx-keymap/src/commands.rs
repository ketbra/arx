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
/// Persist the active buffer to its associated path.
pub const BUFFER_SAVE: &str = "buffer.save";

/// Scroll the active window up by one page.
pub const SCROLL_PAGE_UP: &str = "scroll.page-up";
/// Scroll the active window down by one page.
pub const SCROLL_PAGE_DOWN: &str = "scroll.page-down";

/// Request editor shutdown.
pub const EDITOR_QUIT: &str = "editor.quit";

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
