//! Stock keymap profiles.
//!
//! A "profile" is a bundle of [`Keymap`]s that matches a familiar editor
//! tradition. Users pick one as the default; future work will let them
//! layer customisations on top via a config file.
//!
//! Phase 1 ships two working profiles:
//!
//! * [`emacs`] — flat, modeless, Emacs-style `C-` and `M-` chords.
//! * [`vim`] — modal, with a normal map and an insert map that gets
//!   pushed by `mode.enter-insert` and popped by `mode.leave-insert`.
//!
//! A KEDIT profile is planned but requires more editor surface (command
//! line + prefix area) than Phase 1 provides; see `docs/spec.md`.

use std::sync::Arc;

use crate::commands::{
    BUFFER_CLOSE, BUFFER_COPY_REGION, BUFFER_DELETE_BACKWARD, BUFFER_DELETE_FORWARD, BUFFER_FIND_FILE,
    BUFFER_KILL_LINE, BUFFER_KILL_REGION, BUFFER_KILL_WORD, BUFFER_KILL_WORD_BACKWARD,
    BUFFER_NEWLINE, BUFFER_OPEN_LINE, BUFFER_REDO, BUFFER_SAVE, BUFFER_SET_MARK, BUFFER_SWITCH,
    BUFFER_TRANSPOSE_CHARS, BUFFER_UNDO, BUFFER_YANK, COMMAND_PALETTE_BACKSPACE,
    COMMAND_PALETTE_CLOSE, COMMAND_PALETTE_EXECUTE, COMMAND_PALETTE_NEXT, COMMAND_PALETTE_OPEN,
    COMMAND_PALETTE_PREV, COMMAND_PALETTE_HISTORY_NEXT, COMMAND_PALETTE_HISTORY_PREV,
    COMPLETION_ACCEPT, COMPLETION_DISMISS, COMPLETION_NEXT,
    SEARCH_BACKSPACE, SEARCH_CLOSE, SEARCH_EXECUTE, SEARCH_HISTORY_NEXT,
    SEARCH_HISTORY_PREV, SEARCH_NEXT, SEARCH_OPEN, SEARCH_PAGE_DOWN, SEARCH_PAGE_UP,
    SEARCH_PREV, SEARCH_TOGGLE_MODE,
    COMPLETION_PAGE_DOWN, COMPLETION_PAGE_UP, COMPLETION_PREV,
    COMPLETION_TRIGGER, CURSOR_BUFFER_END,
    LSP_HOVER, TERMINAL_OPEN,
    CURSOR_BUFFER_START, CURSOR_DOWN, CURSOR_LEFT, CURSOR_LINE_END, CURSOR_LINE_START,
    CURSOR_RIGHT, CURSOR_UP, CURSOR_WORD_BACKWARD, CURSOR_WORD_FORWARD, EDITOR_CANCEL,
    EDITOR_DESCRIBE_KEY, EDITOR_QUIT, LSP_NEXT_DIAGNOSTIC, LSP_PREV_DIAGNOSTIC,
    MODE_ENTER_INSERT, MODE_LEAVE_INSERT,
    SCROLL_PAGE_DOWN, SCROLL_PAGE_UP, SCROLL_RECENTER, WINDOW_CLOSE, WINDOW_DELETE_OTHER,
    WINDOW_FOCUS_NEXT, WINDOW_FOCUS_PREV,
    WINDOW_SPLIT_HORIZONTAL, WINDOW_SPLIT_VERTICAL,
};
use crate::engine::CountMode;
use crate::keymap::Keymap;

/// A complete profile: one global map plus optional mode maps to push.
#[derive(Debug, Clone)]
pub struct Profile {
    /// The "always there" base map. Lives at the bottom of the engine's
    /// stack.
    pub global: Arc<Keymap>,
    /// Profiles that are modal push an initial layer on top of global
    /// at editor startup. `None` for modeless profiles.
    pub startup_layer: Option<(Arc<str>, Arc<Keymap>)>,
    /// Whether digits at the start of a sequence accumulate a count.
    /// Vim yes; Emacs no.
    pub count_mode: CountMode,
}

/// The profile shipped by default.
pub fn default() -> Profile {
    emacs()
}

// ---------------------------------------------------------------------------
// Emacs
// ---------------------------------------------------------------------------

/// Full Emacs profile covering every stock command.
pub fn emacs() -> Profile {
    let mut m = Keymap::named("emacs");

    // Cursor movement (bare + C-*).
    m.bind_str("<Left>", CURSOR_LEFT).unwrap();
    m.bind_str("<Right>", CURSOR_RIGHT).unwrap();
    m.bind_str("<Up>", CURSOR_UP).unwrap();
    m.bind_str("<Down>", CURSOR_DOWN).unwrap();
    m.bind_str("C-b", CURSOR_LEFT).unwrap();
    m.bind_str("C-f", CURSOR_RIGHT).unwrap();
    m.bind_str("C-p", CURSOR_UP).unwrap();
    m.bind_str("C-n", CURSOR_DOWN).unwrap();
    m.bind_str("C-a", CURSOR_LINE_START).unwrap();
    m.bind_str("C-e", CURSOR_LINE_END).unwrap();
    m.bind_str("<Home>", CURSOR_LINE_START).unwrap();
    m.bind_str("<End>", CURSOR_LINE_END).unwrap();
    // Word- and buffer-level motions.
    m.bind_str("M-f", CURSOR_WORD_FORWARD).unwrap();
    m.bind_str("M-b", CURSOR_WORD_BACKWARD).unwrap();
    m.bind_str("M-<", CURSOR_BUFFER_START).unwrap();
    m.bind_str("M->", CURSOR_BUFFER_END).unwrap();

    // Basic editing.
    m.bind_str("<Enter>", BUFFER_NEWLINE).unwrap();
    m.bind_str("<Backspace>", BUFFER_DELETE_BACKWARD).unwrap();
    m.bind_str("<Delete>", BUFFER_DELETE_FORWARD).unwrap();
    m.bind_str("C-d", BUFFER_DELETE_FORWARD).unwrap();
    m.bind_str("C-t", BUFFER_TRANSPOSE_CHARS).unwrap();
    m.bind_str("C-o", BUFFER_OPEN_LINE).unwrap();

    // Kill / yank / mark.
    m.bind_str("C-k", BUFFER_KILL_LINE).unwrap();
    m.bind_str("M-d", BUFFER_KILL_WORD).unwrap();
    m.bind_str("M-<Backspace>", BUFFER_KILL_WORD_BACKWARD).unwrap();
    m.bind_str("C-w", BUFFER_KILL_REGION).unwrap();
    m.bind_str("M-w", BUFFER_COPY_REGION).unwrap();
    m.bind_str("C-y", BUFFER_YANK).unwrap();
    m.bind_str("C-<Space>", BUFFER_SET_MARK).unwrap();

    // Scrolling.
    m.bind_str("<PageUp>", SCROLL_PAGE_UP).unwrap();
    m.bind_str("<PageDown>", SCROLL_PAGE_DOWN).unwrap();
    m.bind_str("M-v", SCROLL_PAGE_UP).unwrap();
    m.bind_str("C-v", SCROLL_PAGE_DOWN).unwrap();
    m.bind_str("C-l", SCROLL_RECENTER).unwrap();

    // Cancel + help.
    m.bind_str("C-g", EDITOR_CANCEL).unwrap();
    m.bind_str("C-h k", EDITOR_DESCRIBE_KEY).unwrap();

    // File / editor / buffer management.
    m.bind_str("C-x C-f", BUFFER_FIND_FILE).unwrap();
    m.bind_str("C-x C-s", BUFFER_SAVE).unwrap();
    m.bind_str("C-x C-c", EDITOR_QUIT).unwrap();
    m.bind_str("C-x C-q", EDITOR_QUIT).unwrap();
    m.bind_str("C-x k", BUFFER_CLOSE).unwrap();
    m.bind_str("C-x b", BUFFER_SWITCH).unwrap();

    // Undo / redo. `C-/` and `C-_` are the classic Emacs undo keys
    // (most terminals conflate them); `C-x u` is the long form. `M-_`
    // matches the `undo-tree.el` convention for redo.
    m.bind_str("C-/", BUFFER_UNDO).unwrap();
    m.bind_str("C-_", BUFFER_UNDO).unwrap();
    m.bind_str("C-x u", BUFFER_UNDO).unwrap();
    m.bind_str("M-_", BUFFER_REDO).unwrap();

    // Completion.
    m.bind_str("M-/", COMPLETION_TRIGGER).unwrap();

    // Interactive buffer search (swiper-style).
    m.bind_str("C-s", SEARCH_OPEN).unwrap();

    // LSP / diagnostic.
    m.bind_str("C-c l h", LSP_HOVER).unwrap();
    m.bind_str("M-n", LSP_NEXT_DIAGNOSTIC).unwrap();
    m.bind_str("M-p", LSP_PREV_DIAGNOSTIC).unwrap();

    // Terminal.
    m.bind_str("C-x t", TERMINAL_OPEN).unwrap();

    // Window splits (Emacs conventions).
    m.bind_str("C-x 1", WINDOW_DELETE_OTHER).unwrap();
    m.bind_str("C-x 2", WINDOW_SPLIT_HORIZONTAL).unwrap();
    m.bind_str("C-x 3", WINDOW_SPLIT_VERTICAL).unwrap();
    m.bind_str("C-x 0", WINDOW_CLOSE).unwrap();
    m.bind_str("C-x o", WINDOW_FOCUS_NEXT).unwrap();

    // Command palette.
    m.bind_str("M-x", COMMAND_PALETTE_OPEN).unwrap();

    Profile {
        global: Arc::new(m),
        startup_layer: None,
        count_mode: CountMode::Reject,
    }
}

// ---------------------------------------------------------------------------
// Command palette
// ---------------------------------------------------------------------------

/// Keymap pushed on top of the current profile when the command
/// palette opens. Every printable key falls through this layer
/// unbound so the driver's `Editor::handle_printable_fallback` can
/// append it to the query; the few bindings here handle navigation,
/// editing the query, executing, and dismissing.
///
/// Shared between every profile — a Vim user and an Emacs user both
/// invoke `M-x` (or their profile's equivalent) and get the same
/// palette UX. Keeping it in `arx-keymap` means profiles in other
/// crates can reference it without reaching into `arx-core::stock`.
pub fn palette_layer() -> Keymap {
    let mut m = Keymap::named("palette");
    m.bind_str("<Enter>", COMMAND_PALETTE_EXECUTE).unwrap();
    m.bind_str("<Esc>", COMMAND_PALETTE_CLOSE).unwrap();
    m.bind_str("C-g", COMMAND_PALETTE_CLOSE).unwrap();
    m.bind_str("<Up>", COMMAND_PALETTE_PREV).unwrap();
    m.bind_str("<Down>", COMMAND_PALETTE_NEXT).unwrap();
    m.bind_str("C-p", COMMAND_PALETTE_PREV).unwrap();
    m.bind_str("C-n", COMMAND_PALETTE_NEXT).unwrap();
    m.bind_str("M-p", COMMAND_PALETTE_HISTORY_PREV).unwrap();
    m.bind_str("M-n", COMMAND_PALETTE_HISTORY_NEXT).unwrap();
    m.bind_str("<Backspace>", COMMAND_PALETTE_BACKSPACE).unwrap();
    m
}

/// Keymap pushed when the completion popup opens. `<Tab>` / `<Enter>`
/// accept, `<Esc>` dismisses, `<Up>` / `<Down>` (or `C-p` / `C-n`)
/// navigate. Unbound printable keys fall through to self-insert,
/// which is what the user expects — typing more characters with the
/// popup open should narrow the filter (though actual filtering is a
/// follow-up; for now the popup just stays open until the user
/// accepts or dismisses).
pub fn completion_layer() -> Keymap {
    let mut m = Keymap::named("completion");
    m.bind_str("<Tab>", COMPLETION_ACCEPT).unwrap();
    m.bind_str("<Enter>", COMPLETION_ACCEPT).unwrap();
    m.bind_str("<Esc>", COMPLETION_DISMISS).unwrap();
    m.bind_str("C-g", COMPLETION_DISMISS).unwrap();
    m.bind_str("<Up>", COMPLETION_PREV).unwrap();
    m.bind_str("<Down>", COMPLETION_NEXT).unwrap();
    m.bind_str("C-p", COMPLETION_PREV).unwrap();
    m.bind_str("C-n", COMPLETION_NEXT).unwrap();
    m.bind_str("C-v", COMPLETION_PAGE_DOWN).unwrap();
    m.bind_str("M-v", COMPLETION_PAGE_UP).unwrap();
    m.bind_str("<PageDown>", COMPLETION_PAGE_DOWN).unwrap();
    m.bind_str("<PageUp>", COMPLETION_PAGE_UP).unwrap();
    m
}

// ---------------------------------------------------------------------------
// Interactive buffer search
// ---------------------------------------------------------------------------

/// Keymap pushed when the interactive search overlay opens. Similar to
/// the palette layer: `<Enter>` accepts, `<Esc>` / `C-g` cancels,
/// arrows and `C-n` / `C-p` navigate, printable keys fall through to
/// the query via `handle_printable_fallback`.
pub fn search_layer() -> Keymap {
    let mut m = Keymap::named("search");
    m.bind_str("<Enter>", SEARCH_EXECUTE).unwrap();
    m.bind_str("<Esc>", SEARCH_CLOSE).unwrap();
    m.bind_str("C-g", SEARCH_CLOSE).unwrap();
    m.bind_str("<Up>", SEARCH_PREV).unwrap();
    m.bind_str("<Down>", SEARCH_NEXT).unwrap();
    m.bind_str("C-p", SEARCH_PREV).unwrap();
    m.bind_str("C-n", SEARCH_NEXT).unwrap();
    m.bind_str("C-v", SEARCH_PAGE_DOWN).unwrap();
    m.bind_str("M-v", SEARCH_PAGE_UP).unwrap();
    m.bind_str("<PageDown>", SEARCH_PAGE_DOWN).unwrap();
    m.bind_str("<PageUp>", SEARCH_PAGE_UP).unwrap();
    m.bind_str("M-s", SEARCH_TOGGLE_MODE).unwrap();
    m.bind_str("<Backspace>", SEARCH_BACKSPACE).unwrap();
    m.bind_str("M-p", SEARCH_HISTORY_PREV).unwrap();
    m.bind_str("M-n", SEARCH_HISTORY_NEXT).unwrap();
    m
}

// ---------------------------------------------------------------------------
// Vim
// ---------------------------------------------------------------------------

/// Minimal Vim profile. Proves the engine's mode-stack machinery works.
///
/// * Global layer: always-active bindings (arrow keys, function keys,
///   Ctrl+S to save — the "rescue bindings" most Vim configs also
///   provide so Ctrl+S works in every mode).
/// * Startup layer: `vim.normal` (modal motions, `i`/`a`/`o` to enter
///   insert, `x` to delete). Count prefixes are enabled here.
///
/// To enter insert mode, run [`MODE_ENTER_INSERT`]; to leave, run
/// [`MODE_LEAVE_INSERT`]. Insert mode itself is an *unbound* layer —
/// pushing an empty keymap on top means every printable key falls
/// through to the global layer (which has nothing for them either),
/// which in turn means the input task self-inserts them. Non-printable
/// keys still come from the global layer (arrow keys, Esc to leave,
/// Backspace, etc.).
pub fn vim() -> Profile {
    // Global layer: bindings that should work in every mode.
    let mut global = Keymap::named("vim.global");
    global.bind_str("<Left>", CURSOR_LEFT).unwrap();
    global.bind_str("<Right>", CURSOR_RIGHT).unwrap();
    global.bind_str("<Up>", CURSOR_UP).unwrap();
    global.bind_str("<Down>", CURSOR_DOWN).unwrap();
    global.bind_str("<Home>", CURSOR_LINE_START).unwrap();
    global.bind_str("<End>", CURSOR_LINE_END).unwrap();
    global.bind_str("<PageUp>", SCROLL_PAGE_UP).unwrap();
    global.bind_str("<PageDown>", SCROLL_PAGE_DOWN).unwrap();
    global.bind_str("<Backspace>", BUFFER_DELETE_BACKWARD).unwrap();
    global.bind_str("<Delete>", BUFFER_DELETE_FORWARD).unwrap();
    global.bind_str("<Enter>", BUFFER_NEWLINE).unwrap();
    // Esc leaves insert mode. The engine only pops if insert is on the
    // stack; otherwise it's a no-op command that the registry maps to
    // "nothing to do".
    global.bind_str("<Esc>", MODE_LEAVE_INSERT).unwrap();
    // Rescue save/quit bindings that work in every mode.
    global.bind_str("C-s", BUFFER_SAVE).unwrap();
    global.bind_str("C-q", EDITOR_QUIT).unwrap();
    // Window splits (Vim conventions — prefix is `C-w`).
    global.bind_str("C-w s", WINDOW_SPLIT_HORIZONTAL).unwrap();
    global.bind_str("C-w v", WINDOW_SPLIT_VERTICAL).unwrap();
    global.bind_str("C-w c", WINDOW_CLOSE).unwrap();
    global.bind_str("C-w q", WINDOW_CLOSE).unwrap();
    global.bind_str("C-w o", WINDOW_DELETE_OTHER).unwrap();
    global.bind_str("C-w w", WINDOW_FOCUS_NEXT).unwrap();
    global.bind_str("C-w W", WINDOW_FOCUS_PREV).unwrap();
    // Terminal.
    global.bind_str("C-w t", TERMINAL_OPEN).unwrap();
    // Completion (works in insert mode via the global layer).
    global.bind_str("C-x C-o", COMPLETION_TRIGGER).unwrap();
    // Command palette. `:` is Vim's usual command-line trigger; in
    // Phase 1 we point it at the generic palette since we don't have
    // a distinct ex-command-line yet.
    global.bind_str("M-x", COMMAND_PALETTE_OPEN).unwrap();

    // Normal layer: push on top of global at startup.
    let mut normal = Keymap::named("vim.normal");
    normal.bind_str("h", CURSOR_LEFT).unwrap();
    normal.bind_str("l", CURSOR_RIGHT).unwrap();
    normal.bind_str("k", CURSOR_UP).unwrap();
    normal.bind_str("j", CURSOR_DOWN).unwrap();
    normal.bind_str("0", CURSOR_LINE_START).unwrap();
    normal.bind_str("$", CURSOR_LINE_END).unwrap();
    // Word- and buffer-level motions. `gg` is a two-chord sequence
    // matching Vim's go-to-top; `G` jumps to end-of-buffer. A count
    // prefix on `G` (`42G`) is the canonical "go to line 42" idiom
    // but needs modal input we don't have yet, so 1c's `G` is
    // unconditional end-of-buffer.
    normal.bind_str("w", CURSOR_WORD_FORWARD).unwrap();
    normal.bind_str("b", CURSOR_WORD_BACKWARD).unwrap();
    normal.bind_str("g g", CURSOR_BUFFER_START).unwrap();
    normal.bind_str("G", CURSOR_BUFFER_END).unwrap();
    // `:` opens the command palette as a stand-in for the ex-command
    // line until Phase 2 wires up a real ex-prompt.
    normal.bind_str(":", COMMAND_PALETTE_OPEN).unwrap();
    normal.bind_str("i", MODE_ENTER_INSERT).unwrap();
    normal.bind_str("a", MODE_ENTER_INSERT).unwrap(); // simplified: no trailing cursor move yet
    normal.bind_str("o", MODE_ENTER_INSERT).unwrap(); // simplified: no newline-below yet
    normal.bind_str("x", BUFFER_DELETE_FORWARD).unwrap();
    // Interactive buffer search (Vim `/` in normal mode).
    normal.bind_str("/", SEARCH_OPEN).unwrap();
    // Undo / redo: Vim's canonical `u` in normal mode, `C-r` for redo.
    normal.bind_str("u", BUFFER_UNDO).unwrap();
    normal.bind_str("C-r", BUFFER_REDO).unwrap();
    // LSP / diagnostic navigation.
    normal.bind_str("K", LSP_HOVER).unwrap();
    normal.bind_str("] d", LSP_NEXT_DIAGNOSTIC).unwrap();
    normal.bind_str("[ d", LSP_PREV_DIAGNOSTIC).unwrap();
    // Shift-Z Shift-Z → save and quit. Minimalist vim exit.
    // Ex-command line (`:w`, `:q`) is a follow-up milestone.

    Profile {
        global: Arc::new(global),
        startup_layer: Some(("vim.normal".into(), Arc::new(normal))),
        count_mode: CountMode::Accept,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FeedOutcome, KeymapEngine, Layer};
    use crate::parse::parse_sequence;

    #[test]
    fn emacs_profile_resolves_standard_bindings() {
        let p = emacs();
        let mut engine = KeymapEngine::new(p.global);
        let seq = parse_sequence("C-x C-s").unwrap();
        let mut last = FeedOutcome::Pending;
        for chord in seq {
            last = engine.feed(chord);
        }
        if let FeedOutcome::Execute { command, .. } = last {
            assert_eq!(&*command.name, BUFFER_SAVE);
        } else {
            panic!("{last:?}");
        }
    }

    #[test]
    fn vim_profile_modal_switch() {
        let p = vim();
        let mut engine = KeymapEngine::new(p.global);
        if let Some((id, map)) = p.startup_layer {
            engine.push_layer(Layer::new(id, map));
        }
        engine.set_count_mode(p.count_mode);
        assert_eq!(engine.top_layer(), "vim.normal");

        // 'h' should hit normal mode's cursor.left binding.
        let chord = parse_sequence("h").unwrap().remove(0);
        if let FeedOutcome::Execute { command, .. } = engine.feed(chord) {
            assert_eq!(&*command.name, CURSOR_LEFT);
        } else {
            panic!("h didn't resolve");
        }

        // 'i' returns the mode-enter-insert command.
        let chord = parse_sequence("i").unwrap().remove(0);
        if let FeedOutcome::Execute { command, .. } = engine.feed(chord) {
            assert_eq!(&*command.name, MODE_ENTER_INSERT);
        } else {
            panic!("i didn't resolve");
        }
    }

    #[test]
    fn vim_count_prefix_works() {
        let p = vim();
        let mut engine = KeymapEngine::new(p.global);
        engine.push_layer(Layer::new(
            p.startup_layer.clone().unwrap().0,
            p.startup_layer.unwrap().1,
        ));
        engine.set_count_mode(p.count_mode);
        let seq = parse_sequence("5 j").unwrap();
        let mut last = FeedOutcome::Pending;
        for chord in seq {
            last = engine.feed(chord);
        }
        if let FeedOutcome::Execute { command, count } = last {
            assert_eq!(&*command.name, CURSOR_DOWN);
            assert_eq!(count, 5);
        } else {
            panic!("{last:?}");
        }
    }
}
