//! KEDIT-style command line and block-editing state.
//!
//! [KEDIT][kedit] (the Mansfield Software editor from the XEDIT /
//! THE family) has two load-bearing UX quirks that this module models:
//!
//! 1. A **persistent command line** at the bottom of the screen. The
//!    user edits the buffer in one place and types ex-style commands
//!    in the other. Focus toggles between the two with `Home` / `F11`
//!    / `F12` (kedit historically used the Home key to "park" the
//!    cursor on the cmd line).
//!
//! 2. A **typed block model**: a selection is either a *line* block
//!    (whole lines), a *box* block (a rectangle), or a *character*
//!    block (contiguous bytes). Block operations (`Alt-K` copy,
//!    `Alt-M` move, `Alt-D` delete, `Alt-P` paste, ...) then apply the
//!    operation with the right semantics for the block's kind.
//!
//! This module holds the state for (1); block kind is stored on the
//! existing [`crate::MarkState`] via its [`crate::SelectionMode`] tag
//! plus a new line-mode wrapper in [`BlockKind`].
//!
//! The command line itself is a tiny ring-buffered single-line editor:
//! a `String` + insert cursor + history. It's intentionally modelled
//! separately from `CommandPalette` because the kedit cmd line is
//! *always visible when the profile is active* rather than being an
//! overlay that pops open on demand.
//!
//! [kedit]: https://en.wikipedia.org/wiki/KEDIT

/// Which kind of block selection is pending. Matches kedit's three
/// block flavours. Stored alongside the mark so block operations know
/// how to interpret the region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockKind {
    /// Whole-line block (kedit `Alt-L`). Covers the full text of every
    /// line between mark and cursor, including newlines.
    Line,
    /// Rectangular / box block (kedit `Alt-B`). Uses the display-column
    /// rectangle between mark and cursor.
    Box,
    /// Contiguous-character block (kedit `Alt-A`). Byte range from
    /// mark to cursor.
    #[default]
    Char,
}

impl BlockKind {
    /// Human-readable label for status messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Box => "box",
            Self::Char => "char",
        }
    }
}

/// Editor-side state for the KEDIT command line.
///
/// Lives on [`crate::Editor`]. Normally disabled (zero-cost); enabled
/// by profiles that want the kedit-style bottom input field visible
/// at all times. When `enabled` is true the render layer paints a
/// `====>` prompt row above the modeline; when `focused` is true as
/// well, keystrokes route to the cmd line rather than the buffer.
#[derive(Debug, Default, Clone)]
pub struct KeditState {
    /// Whether the cmd line is visible at all.
    enabled: bool,
    /// Whether keystrokes go to the cmd line (true) or the buffer
    /// (false). Only meaningful when `enabled`.
    focused: bool,
    /// Current cmd line text.
    query: String,
    /// Byte cursor position inside `query` (insertion point).
    cursor: usize,
    /// Command history. Most-recent is last.
    history: Vec<String>,
    /// Position within `history` while browsing with Up/Down. `None`
    /// means a fresh query (not browsing). `Some(i)` is a reverse
    /// index: 0 = most recent.
    history_index: Option<usize>,
    /// Query saved before entering history browsing, so moving past
    /// the newest entry restores what the user was typing.
    saved_query: String,
    /// A transient message shown on the cmd line when it's not
    /// focused (e.g. "No match found"). Cleared on the next focus
    /// switch or successful command.
    message: Option<String>,
    /// A pending block waiting to be pasted by a later `block.paste`.
    /// kedit's `Alt-M` (move) captures the block here, removes the
    /// original, and the next `Alt-P` drops it in. Stays `None` when
    /// the kedit profile is inactive or no move is pending.
    pending_move: Option<ClipboardBlock>,
}

/// The content captured by a `block.move` pending paste. The block's
/// kind is preserved so the paste side can reproduce the right
/// geometry (line block → line insert, box block → rect insert, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardBlock {
    /// Whole-line block: a vector of line texts (no trailing `\n`s).
    Line(Vec<String>),
    /// Box block: a vector of line slices already trimmed to the
    /// block's column range.
    Box(Vec<String>),
    /// Contiguous text block.
    Char(String),
}

/// Cap the number of history entries so the state stays bounded.
const MAX_HISTORY: usize = 64;

impl KeditState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Is the kedit cmd line visible?
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable the cmd line. Safe to call more than once.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the cmd line entirely. Also blurs it so the buffer
    /// gets keyboard focus back on the next keystroke.
    pub fn disable(&mut self) {
        self.enabled = false;
        self.focused = false;
        self.message = None;
    }

    /// Are keystrokes currently routed to the cmd line?
    pub fn is_focused(&self) -> bool {
        self.enabled && self.focused
    }

    /// Move keyboard focus to the cmd line. No-op when disabled.
    pub fn focus(&mut self) {
        if self.enabled {
            self.focused = true;
        }
    }

    /// Move keyboard focus back to the buffer.
    pub fn blur(&mut self) {
        self.focused = false;
    }

    /// Current cmd-line query text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Cursor position within the query (byte offset).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Overwrite the cmd-line query and reset the cursor to the end.
    pub fn set_query(&mut self, text: impl Into<String>) {
        self.query = text.into();
        self.cursor = self.query.len();
        self.history_index = None;
    }

    /// Append a character at the cursor.
    pub fn append_char(&mut self, ch: char) {
        self.history_index = None;
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        self.query.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the character to the left of the cursor.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.history_index = None;
        let prev = prev_char_boundary(&self.query, self.cursor);
        self.query.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// Delete the character at the cursor.
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.query.len() {
            return;
        }
        self.history_index = None;
        let next = next_char_boundary(&self.query, self.cursor);
        self.query.drain(self.cursor..next);
    }

    /// Clear the entire query.
    pub fn clear_query(&mut self) {
        self.query.clear();
        self.cursor = 0;
        self.history_index = None;
    }

    /// Move the cursor one character left.
    pub fn cursor_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = prev_char_boundary(&self.query, self.cursor);
    }

    /// Move the cursor one character right.
    pub fn cursor_right(&mut self) {
        if self.cursor >= self.query.len() {
            return;
        }
        self.cursor = next_char_boundary(&self.query, self.cursor);
    }

    /// Move to the start of the query.
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move to the end of the query.
    pub fn cursor_end(&mut self) {
        self.cursor = self.query.len();
    }

    /// Commit the current query to history (deduplicating against the
    /// most recent entry) and return the text. Clears the query for
    /// the next command.
    pub fn commit(&mut self) -> String {
        let committed = std::mem::take(&mut self.query);
        self.cursor = 0;
        self.history_index = None;
        self.saved_query.clear();
        if !committed.is_empty()
            && self.history.last().map(String::as_str) != Some(committed.as_str())
        {
            self.history.push(committed.clone());
            while self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
        }
        committed
    }

    /// Walk to the previous (older) history entry. First press saves
    /// the in-progress query so Down can restore it.
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next_idx = match self.history_index {
            None => {
                self.saved_query = self.query.clone();
                0
            }
            Some(i) if i + 1 < self.history.len() => i + 1,
            Some(_) => return,
        };
        self.history_index = Some(next_idx);
        let entry = &self.history[self.history.len() - 1 - next_idx];
        self.query = entry.clone();
        self.cursor = self.query.len();
    }

    /// Walk to the next (newer) history entry. Past the newest,
    /// restore the saved query.
    pub fn history_next(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx == 0 {
            self.history_index = None;
            self.query = std::mem::take(&mut self.saved_query);
            self.cursor = self.query.len();
        } else {
            self.history_index = Some(idx - 1);
            let entry = &self.history[self.history.len() - idx];
            self.query = entry.clone();
            self.cursor = self.query.len();
        }
    }

    /// Read-only view of the history list, most-recent last.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Set a transient message shown on the prompt row (e.g. "File
    /// saved", "Not found"). Cleared on the next user edit.
    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
    }

    /// The transient message, if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Clear any transient message.
    pub fn clear_message(&mut self) {
        self.message = None;
    }

    /// The pending move-block clipboard (if a `block.move` is waiting
    /// for its paste).
    pub fn pending_move(&self) -> Option<&ClipboardBlock> {
        self.pending_move.as_ref()
    }

    /// Stash a pending-move block.
    pub fn set_pending_move(&mut self, block: ClipboardBlock) {
        self.pending_move = Some(block);
    }

    /// Consume the pending-move block.
    pub fn take_pending_move(&mut self) -> Option<ClipboardBlock> {
        self.pending_move.take()
    }
}

/// Byte offset of the grapheme boundary that starts just before `pos`.
/// Assumes `pos` is already on a valid UTF-8 boundary in `s`; if it
/// isn't (defensive programming only — callers should never pass an
/// invalid offset), walks the `char_indices` iterator to find the
/// closest one at or before `pos`.
fn prev_char_boundary(s: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    s[..pos]
        .char_indices()
        .last()
        .map_or(0, |(idx, _)| idx)
}

/// Byte offset of the next grapheme boundary after `pos`.
fn next_char_boundary(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    s[pos..]
        .char_indices()
        .nth(1)
        .map_or(s.len(), |(idx, _)| pos + idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_and_focus_gates_are_independent() {
        let mut k = KeditState::new();
        // Focus request on a disabled cmd line is a no-op.
        k.focus();
        assert!(!k.is_focused());
        k.enable();
        assert!(k.is_enabled());
        assert!(!k.is_focused());
        k.focus();
        assert!(k.is_focused());
        k.blur();
        assert!(!k.is_focused());
    }

    #[test]
    fn append_and_backspace_edit_query() {
        let mut k = KeditState::new();
        k.enable();
        k.append_char('Q');
        k.append_char('U');
        k.append_char('I');
        k.append_char('T');
        assert_eq!(k.query(), "QUIT");
        assert_eq!(k.cursor(), 4);
        k.backspace();
        assert_eq!(k.query(), "QUI");
        assert_eq!(k.cursor(), 3);
    }

    #[test]
    fn cursor_left_right_moves_within_query() {
        let mut k = KeditState::new();
        for ch in "abc".chars() {
            k.append_char(ch);
        }
        assert_eq!(k.cursor(), 3);
        k.cursor_left();
        k.cursor_left();
        assert_eq!(k.cursor(), 1);
        k.append_char('X');
        assert_eq!(k.query(), "aXbc");
        assert_eq!(k.cursor(), 2);
    }

    #[test]
    fn delete_forward_removes_at_cursor() {
        let mut k = KeditState::new();
        for ch in "abc".chars() {
            k.append_char(ch);
        }
        k.cursor_home();
        k.delete_forward();
        assert_eq!(k.query(), "bc");
    }

    #[test]
    fn commit_returns_text_and_pushes_history() {
        let mut k = KeditState::new();
        k.set_query("QUIT");
        let committed = k.commit();
        assert_eq!(committed, "QUIT");
        assert_eq!(k.query(), "");
        assert_eq!(k.history().last().map(String::as_str), Some("QUIT"));
    }

    #[test]
    fn commit_dedupes_against_most_recent() {
        let mut k = KeditState::new();
        k.set_query("QUIT");
        k.commit();
        k.set_query("QUIT");
        k.commit();
        assert_eq!(k.history().len(), 1);
    }

    #[test]
    fn history_prev_next_walks_list() {
        let mut k = KeditState::new();
        for cmd in ["ONE", "TWO", "THREE"] {
            k.set_query(cmd);
            k.commit();
        }
        k.history_prev();
        assert_eq!(k.query(), "THREE");
        k.history_prev();
        assert_eq!(k.query(), "TWO");
        k.history_prev();
        assert_eq!(k.query(), "ONE");
        // Further prev saturates (stays on oldest).
        k.history_prev();
        assert_eq!(k.query(), "ONE");
        k.history_next();
        assert_eq!(k.query(), "TWO");
        k.history_next();
        assert_eq!(k.query(), "THREE");
        // Past newest → saved query (empty here).
        k.history_next();
        assert_eq!(k.query(), "");
    }

    #[test]
    fn history_saves_in_progress_query() {
        let mut k = KeditState::new();
        k.set_query("OLD");
        k.commit();
        // Start typing a fresh query, then browse history.
        k.set_query("PARTIAL");
        k.history_prev();
        assert_eq!(k.query(), "OLD");
        k.history_next();
        assert_eq!(k.query(), "PARTIAL");
    }

    #[test]
    fn message_round_trips() {
        let mut k = KeditState::new();
        assert_eq!(k.message(), None);
        k.set_message("saved");
        assert_eq!(k.message(), Some("saved"));
        k.clear_message();
        assert_eq!(k.message(), None);
    }

    #[test]
    fn pending_move_stash_and_take() {
        let mut k = KeditState::new();
        k.set_pending_move(ClipboardBlock::Char("foo".into()));
        assert!(k.pending_move().is_some());
        let taken = k.take_pending_move().unwrap();
        assert_eq!(taken, ClipboardBlock::Char("foo".into()));
        assert!(k.pending_move().is_none());
    }

    #[test]
    fn disable_blurs_focus() {
        let mut k = KeditState::new();
        k.enable();
        k.focus();
        assert!(k.is_focused());
        k.disable();
        assert!(!k.is_focused());
        assert!(!k.is_enabled());
    }
}
