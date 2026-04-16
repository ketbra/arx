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
/// Move the primary cursor to the end of the current/next word.
pub const CURSOR_END_OF_WORD: &str = "cursor.end-of-word";
/// Move forward to the next blank-line boundary (paragraph).
pub const CURSOR_PARAGRAPH_FORWARD: &str = "cursor.paragraph-forward";
/// Move backward to the previous blank-line boundary (paragraph).
pub const CURSOR_PARAGRAPH_BACKWARD: &str = "cursor.paragraph-backward";
/// Jump to the matching bracket / paren / brace.
pub const CURSOR_MATCHING_BRACKET: &str = "cursor.matching-bracket";
/// Move cursor to the top of the visible screen.
pub const CURSOR_SCREEN_TOP: &str = "cursor.screen-top";
/// Move cursor to the middle of the visible screen.
pub const CURSOR_SCREEN_MIDDLE: &str = "cursor.screen-middle";
/// Move cursor to the bottom of the visible screen.
pub const CURSOR_SCREEN_BOTTOM: &str = "cursor.screen-bottom";
/// Find char forward on current line (Vim `f`).
pub const CURSOR_FIND_CHAR_FORWARD: &str = "cursor.find-char-forward";
/// Find char backward on current line (Vim `F`).
pub const CURSOR_FIND_CHAR_BACKWARD: &str = "cursor.find-char-backward";
/// Move to just before char forward (Vim `t`).
pub const CURSOR_TILL_CHAR_FORWARD: &str = "cursor.till-char-forward";
/// Move to just after char backward (Vim `T`).
pub const CURSOR_TILL_CHAR_BACKWARD: &str = "cursor.till-char-backward";
/// Repeat the last find-char / till-char motion.
pub const CURSOR_REPEAT_FIND: &str = "cursor.repeat-find";
/// Repeat the last find-char / till-char motion in reverse.
pub const CURSOR_REPEAT_FIND_REVERSE: &str = "cursor.repeat-find-reverse";

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
/// Open a file by path (find-file).
pub const BUFFER_FIND_FILE: &str = "buffer.find-file";
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
/// Transpose (swap) the two words around the cursor.
pub const BUFFER_TRANSPOSE_WORDS: &str = "buffer.transpose-words";
/// Join current line with the next line, collapsing whitespace.
pub const BUFFER_JOIN_LINES: &str = "buffer.join-lines";
/// Duplicate the current line below.
pub const BUFFER_DUPLICATE_LINE: &str = "buffer.duplicate-line";
/// Move the current line up one position.
pub const BUFFER_MOVE_LINE_UP: &str = "buffer.move-line-up";
/// Move the current line down one position.
pub const BUFFER_MOVE_LINE_DOWN: &str = "buffer.move-line-down";
/// Indent the current line by one level.
pub const BUFFER_INDENT_LINE: &str = "buffer.indent-line";
/// Dedent the current line by one level.
pub const BUFFER_DEDENT_LINE: &str = "buffer.dedent-line";
/// Toggle line comment (language-aware).
pub const BUFFER_COMMENT_TOGGLE: &str = "buffer.comment-toggle";
/// Select the entire buffer (set mark at start, cursor at end).
pub const BUFFER_MARK_WHOLE: &str = "buffer.mark-whole";
/// Exchange point (cursor) and mark.
pub const BUFFER_EXCHANGE_POINT_MARK: &str = "buffer.exchange-point-mark";
/// Cycle the kill ring after a yank (yank-pop).
pub const BUFFER_YANK_POP: &str = "buffer.yank-pop";
/// Delete the current line (Vim `dd`).
pub const BUFFER_DELETE_LINE: &str = "buffer.delete-line";
/// Yank (copy) the current line (Vim `yy`).
pub const BUFFER_YANK_LINE: &str = "buffer.yank-line";
/// Change the current line: delete and enter insert mode (Vim `cc`).
pub const BUFFER_CHANGE_LINE: &str = "buffer.change-line";
/// Delete from cursor to end of line (Vim `D`).
pub const BUFFER_DELETE_TO_EOL: &str = "buffer.delete-to-eol";
/// Change from cursor to end of line (Vim `C`).
pub const BUFFER_CHANGE_TO_EOL: &str = "buffer.change-to-eol";
/// Yank from cursor to end of line (Vim `Y`).
pub const BUFFER_YANK_TO_EOL: &str = "buffer.yank-to-eol";

/// Scroll down by half a page (Vim `C-d`).
pub const SCROLL_HALF_PAGE_DOWN: &str = "scroll.half-page-down";
/// Scroll up by half a page (Vim `C-u`).
pub const SCROLL_HALF_PAGE_UP: &str = "scroll.half-page-up";
/// Scroll so the cursor line is at the top of the window.
pub const SCROLL_CURSOR_TOP: &str = "scroll.cursor-top";
/// Scroll so the cursor line is at the bottom of the window.
pub const SCROLL_CURSOR_BOTTOM: &str = "scroll.cursor-bottom";

/// Go to a specific line number (opens prompt).
pub const GOTO_LINE: &str = "goto.line";

/// Request editor shutdown.
pub const EDITOR_QUIT: &str = "editor.quit";
/// Suspend the editor (SIGTSTP). Returns to the shell so the user
/// can use `fg` to bring the editor back. No-op on Windows.
pub const EDITOR_SUSPEND: &str = "editor.suspend";
/// Cancel the current operation (keyboard-quit).
pub const EDITOR_CANCEL: &str = "editor.cancel";
/// Recenter the view so the cursor is in the middle of the window.
pub const SCROLL_RECENTER: &str = "scroll.recenter";

/// Describe what a key sequence is bound to. Prompts for a key,
/// then shows the bound command in the status bar.
pub const EDITOR_DESCRIBE_KEY: &str = "editor.describe-key";

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
/// Navigate to the previous (older) palette history entry.
pub const COMMAND_PALETTE_HISTORY_PREV: &str = "command-palette.history-prev";
/// Navigate to the next (newer) palette history entry.
pub const COMMAND_PALETTE_HISTORY_NEXT: &str = "command-palette.history-next";

/// Split the active window horizontally — stacking a new pane beneath
/// it. Vim `:split` / Emacs `C-x 2`.
pub const WINDOW_SPLIT_HORIZONTAL: &str = "window.split-horizontal";
/// Split the active window vertically — placing a new pane beside it.
/// Vim `:vsplit` / Emacs `C-x 3`.
pub const WINDOW_SPLIT_VERTICAL: &str = "window.split-vertical";
/// Close the active window, collapsing its enclosing split into the
/// surviving sibling. No-op when only one window is left.
pub const WINDOW_CLOSE: &str = "window.close";
/// Close all windows except the active one, collapsing the layout to
/// a single leaf. Emacs `C-x 1` / Vim `C-w o`.
pub const WINDOW_DELETE_OTHER: &str = "window.delete-other";
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
/// Jump to the definition of the symbol under the cursor.
pub const LSP_GOTO_DEFINITION: &str = "lsp.goto-definition";
/// Return to the previous location after a goto-definition jump.
pub const LSP_POP_BACK: &str = "lsp.pop-back";

/// Jump to the next function/method definition (tree-sitter).
pub const TREESITTER_NEXT_FUNCTION: &str = "treesitter.next-function";
/// Jump to the previous function/method definition (tree-sitter).
pub const TREESITTER_PREV_FUNCTION: &str = "treesitter.prev-function";
/// Jump to the enclosing parent syntax node (tree-sitter).
pub const TREESITTER_PARENT_NODE: &str = "treesitter.parent-node";

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
/// Move the completion selection down one page.
pub const COMPLETION_PAGE_DOWN: &str = "completion.page-down";
/// Move the completion selection up one page.
pub const COMPLETION_PAGE_UP: &str = "completion.page-up";

/// Open interactive buffer search (swiper / telescope style).
pub const SEARCH_OPEN: &str = "search.open";
/// Close the search overlay and restore cursor to original position.
pub const SEARCH_CLOSE: &str = "search.close";
/// Accept the selected search match and jump to it.
pub const SEARCH_EXECUTE: &str = "search.execute";
/// Move the search selection down one row.
pub const SEARCH_NEXT: &str = "search.next";
/// Move the search selection up one row.
pub const SEARCH_PREV: &str = "search.prev";
/// Move the search selection down one page.
pub const SEARCH_PAGE_DOWN: &str = "search.page-down";
/// Move the search selection up one page.
pub const SEARCH_PAGE_UP: &str = "search.page-up";
/// Cycle search mode: fuzzy → literal → regex → fuzzy.
pub const SEARCH_TOGGLE_MODE: &str = "search.toggle-mode";
/// Remove the last character from the search query.
pub const SEARCH_BACKSPACE: &str = "search.backspace";
/// Navigate to the previous (older) search history entry.
pub const SEARCH_HISTORY_PREV: &str = "search.history-prev";
/// Navigate to the next (newer) search history entry.
pub const SEARCH_HISTORY_NEXT: &str = "search.history-next";

/// Kill (delete) the rectangular region between mark and cursor.
pub const RECT_KILL: &str = "rect.kill";
/// Yank (paste) the most recent rectangular kill at the cursor.
pub const RECT_YANK: &str = "rect.yank";
/// Copy the rectangular region between mark and cursor (without deleting).
pub const RECT_COPY: &str = "rect.copy";
/// Open (insert blank space into) the rectangular region.
pub const RECT_OPEN: &str = "rect.open";

/// Enter Vim visual-block selection mode.
pub const MODE_ENTER_VISUAL_BLOCK: &str = "mode.enter-visual-block";
/// Leave Vim visual-block selection mode.
pub const MODE_LEAVE_VISUAL_BLOCK: &str = "mode.leave-visual-block";

// --- Vim operator-pending ---

/// Start a delete operator (Vim `d`): waits for motion/text-object.
pub const OPERATOR_DELETE: &str = "operator.delete";
/// Start a change operator (Vim `c`): waits for motion/text-object.
pub const OPERATOR_CHANGE: &str = "operator.change";
/// Start a yank operator (Vim `y`): waits for motion/text-object.
pub const OPERATOR_YANK: &str = "operator.yank";
/// Start an indent operator (Vim `>`): waits for motion.
pub const OPERATOR_INDENT: &str = "operator.indent";
/// Start a dedent operator (Vim `<`): waits for motion.
pub const OPERATOR_DEDENT: &str = "operator.dedent";
/// Cancel the pending operator.
pub const OPERATOR_CANCEL: &str = "operator.cancel";
/// Apply pending operator to the current line (dd/cc/yy/>>/<< shortcut).
pub const OPERATOR_LINE: &str = "operator.line";

// --- Vim text objects ---

/// Inner word text object (Vim `iw`).
pub const TEXT_OBJECT_INNER_WORD: &str = "text-object.inner-word";
/// A word text object including surrounding whitespace (Vim `aw`).
pub const TEXT_OBJECT_A_WORD: &str = "text-object.a-word";
/// Inner paragraph text object (Vim `ip`).
pub const TEXT_OBJECT_INNER_PARAGRAPH: &str = "text-object.inner-paragraph";
/// A paragraph including surrounding blank lines (Vim `ap`).
pub const TEXT_OBJECT_A_PARAGRAPH: &str = "text-object.a-paragraph";
/// Inner double-quoted string (Vim `i"`).
pub const TEXT_OBJECT_INNER_DOUBLE_QUOTE: &str = "text-object.inner-double-quote";
/// A double-quoted string including quotes (Vim `a"`).
pub const TEXT_OBJECT_A_DOUBLE_QUOTE: &str = "text-object.a-double-quote";
/// Inner single-quoted string (Vim `i'`).
pub const TEXT_OBJECT_INNER_SINGLE_QUOTE: &str = "text-object.inner-single-quote";
/// A single-quoted string including quotes (Vim `a'`).
pub const TEXT_OBJECT_A_SINGLE_QUOTE: &str = "text-object.a-single-quote";
/// Inner parentheses (Vim `i(` / `ib`).
pub const TEXT_OBJECT_INNER_PAREN: &str = "text-object.inner-paren";
/// Including parentheses (Vim `a(` / `ab`).
pub const TEXT_OBJECT_A_PAREN: &str = "text-object.a-paren";
/// Inner curly braces (Vim `i{` / `iB`).
pub const TEXT_OBJECT_INNER_BRACE: &str = "text-object.inner-brace";
/// Including curly braces (Vim `a{` / `aB`).
pub const TEXT_OBJECT_A_BRACE: &str = "text-object.a-brace";
/// Inner square brackets (Vim `i[`).
pub const TEXT_OBJECT_INNER_BRACKET: &str = "text-object.inner-bracket";
/// Including square brackets (Vim `a[`).
pub const TEXT_OBJECT_A_BRACKET: &str = "text-object.a-bracket";
/// Inner angle brackets (Vim `i<`).
pub const TEXT_OBJECT_INNER_ANGLE: &str = "text-object.inner-angle";
/// Including angle brackets (Vim `a<`).
pub const TEXT_OBJECT_A_ANGLE: &str = "text-object.a-angle";
/// Inner backtick-quoted string (Vim `` i` ``).
pub const TEXT_OBJECT_INNER_BACKTICK: &str = "text-object.inner-backtick";
/// Including backtick quotes (Vim `` a` ``).
pub const TEXT_OBJECT_A_BACKTICK: &str = "text-object.a-backtick";

/// Open an embedded terminal in a split pane.
pub const TERMINAL_OPEN: &str = "terminal.open";

// --- KEDIT command line + block editing ---

/// Move keyboard focus to the KEDIT command line (F11 / Home).
pub const KEDIT_FOCUS_CMDLINE: &str = "kedit.focus-cmdline";
/// Move keyboard focus back from the command line to the buffer (F12 / Esc).
pub const KEDIT_FOCUS_BUFFER: &str = "kedit.focus-buffer";
/// Toggle command-line focus (Esc when no operation is pending).
pub const KEDIT_TOGGLE_FOCUS: &str = "kedit.toggle-focus";
/// Execute the text on the KEDIT command line as a command.
pub const KEDIT_CMDLINE_EXECUTE: &str = "kedit.cmdline-execute";
/// Remove the character before the cmd-line cursor.
pub const KEDIT_CMDLINE_BACKSPACE: &str = "kedit.cmdline-backspace";
/// Remove the character at the cmd-line cursor.
pub const KEDIT_CMDLINE_DELETE_FORWARD: &str = "kedit.cmdline-delete-forward";
/// Clear the cmd-line query entirely.
pub const KEDIT_CMDLINE_CLEAR: &str = "kedit.cmdline-clear";
/// Move the cmd-line cursor one character left.
pub const KEDIT_CMDLINE_CURSOR_LEFT: &str = "kedit.cmdline-cursor-left";
/// Move the cmd-line cursor one character right.
pub const KEDIT_CMDLINE_CURSOR_RIGHT: &str = "kedit.cmdline-cursor-right";
/// Move the cmd-line cursor to the start.
pub const KEDIT_CMDLINE_CURSOR_HOME: &str = "kedit.cmdline-cursor-home";
/// Move the cmd-line cursor to the end.
pub const KEDIT_CMDLINE_CURSOR_END: &str = "kedit.cmdline-cursor-end";
/// Walk to the previous (older) cmd-line history entry.
pub const KEDIT_CMDLINE_HISTORY_PREV: &str = "kedit.cmdline-history-prev";
/// Walk to the next (newer) cmd-line history entry.
pub const KEDIT_CMDLINE_HISTORY_NEXT: &str = "kedit.cmdline-history-next";

/// Mark a whole-line block (kedit `Alt-L`).
pub const BLOCK_MARK_LINE: &str = "block.mark-line";
/// Mark a rectangular / box block (kedit `Alt-B`).
pub const BLOCK_MARK_BOX: &str = "block.mark-box";
/// Mark a contiguous-character block (kedit `Alt-A`).
pub const BLOCK_MARK_CHAR: &str = "block.mark-char";
/// Copy the marked block to the block clipboard (kedit `Alt-K`).
pub const BLOCK_COPY: &str = "block.copy";
/// Cut the marked block for a subsequent paste (kedit `Alt-M`).
pub const BLOCK_MOVE: &str = "block.move";
/// Delete the marked block (kedit `Alt-D`).
pub const BLOCK_DELETE: &str = "block.delete";
/// Paste the most recent block clipboard at the cursor (kedit `Alt-P`).
pub const BLOCK_PASTE: &str = "block.paste";
/// Unmark / clear the current block (kedit `Alt-U`).
pub const BLOCK_UNMARK: &str = "block.unmark";
/// Overlay the marked rectangle at the cursor, replacing underlying
/// text instead of pushing it right (kedit `Alt-O`).
pub const BLOCK_OVERLAY: &str = "block.overlay";
/// Fill the marked rectangle with a single character (kedit `Alt-Z`).
pub const BLOCK_FILL: &str = "block.fill";
