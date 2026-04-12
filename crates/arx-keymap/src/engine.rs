//! [`KeymapEngine`]: the runtime keymap state machine.
//!
//! Holds a stack of [`Keymap`]s (one per active mode), the pending
//! key-sequence buffer, and an optional numeric count prefix. Feed it one
//! [`KeyChord`] at a time; it returns a [`FeedOutcome`] telling the
//! caller what to do.
//!
//! Lookup walks the mode stack from top to bottom, so e.g. Vim normal
//! mode can shadow global bindings, and Emacs minor-mode maps can
//! intercept before the major-mode map.

use std::sync::Arc;

use crate::key::{Key, KeyChord};
use crate::keymap::{CommandRef, Keymap, Lookup};

/// Identifier for a keymap layer in the engine's mode stack. Arbitrary
/// string — commonly `"global"`, `"emacs"`, `"vim.normal"`, `"insert"`.
pub type LayerId = Arc<str>;

/// One layer in the mode stack.
#[derive(Debug, Clone)]
pub struct Layer {
    pub id: LayerId,
    pub map: Arc<Keymap>,
}

impl Layer {
    pub fn new(id: impl Into<LayerId>, map: Arc<Keymap>) -> Self {
        Self {
            id: id.into(),
            map,
        }
    }
}

/// Outcome of feeding a single key to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedOutcome {
    /// A command has been resolved and the caller should invoke it. The
    /// `count` is the accumulated numeric prefix (Vim `3j` → count 3,
    /// Emacs `C-u 5 C-n` → count 5) if one was being built; otherwise 1.
    Execute {
        command: CommandRef,
        count: u32,
    },
    /// The sequence is a live prefix — wait for more keys.
    Pending,
    /// No binding matched. If `printable_fallback` is `Some`, the caller
    /// should treat that chord as a self-insert character; otherwise
    /// the key is silently dropped.
    Unbound {
        printable_fallback: Option<char>,
    },
}

/// The runtime keymap state machine.
#[derive(Debug, Clone)]
pub struct KeymapEngine {
    /// Mode stack. The last element is the active (top) layer. Lookup
    /// walks top-to-bottom, so later entries shadow earlier ones.
    stack: Vec<Layer>,
    /// Keys accumulated so far in the current sequence.
    pending: Vec<KeyChord>,
    /// Numeric count being accumulated (Vim `3`, `22`, …). `None` means
    /// "no count started yet".
    count: Option<u32>,
    /// Whether the active mode accepts numeric count prefixes (Vim
    /// normal mode yes; Emacs insert no — counts would eat typed digits).
    count_mode: CountMode,
}

/// How the engine interprets leading digit keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CountMode {
    /// Digits at the start of a sequence accumulate a numeric prefix
    /// (Vim normal mode).
    Accept,
    /// Digits are ordinary keys (Emacs, Vim insert, every printable-
    /// first editing mode).
    #[default]
    Reject,
}

impl KeymapEngine {
    /// Create an engine with a single global layer.
    pub fn new(global: Arc<Keymap>) -> Self {
        Self {
            stack: vec![Layer::new("global", global)],
            pending: Vec::new(),
            count: None,
            count_mode: CountMode::Reject,
        }
    }

    /// Replace the global (bottom-of-stack) layer. Clears pending state.
    pub fn set_global(&mut self, map: Arc<Keymap>) {
        if self.stack.is_empty() {
            self.stack.push(Layer::new("global", map));
        } else {
            self.stack[0] = Layer::new("global", map);
        }
        self.reset();
    }

    /// Push a mode layer on top of the stack. Subsequent lookups try
    /// this map first before falling through to lower layers.
    pub fn push_layer(&mut self, layer: Layer) {
        self.stack.push(layer);
        self.reset();
    }

    /// Pop the top layer (but never pop below the global layer). Returns
    /// the popped layer's id, or `None` if only the global layer exists.
    pub fn pop_layer(&mut self) -> Option<LayerId> {
        if self.stack.len() <= 1 {
            return None;
        }
        let popped = self.stack.pop()?;
        self.reset();
        Some(popped.id)
    }

    /// Is a layer with `id` currently on the stack?
    pub fn has_layer(&self, id: &str) -> bool {
        self.stack.iter().any(|l| &*l.id == id)
    }

    /// Top layer id.
    pub fn top_layer(&self) -> &str {
        self.stack.last().map_or("global", |l| &l.id)
    }

    /// The key sequence currently accumulated (for a "waiting for…"
    /// modeline indicator).
    pub fn pending_sequence(&self) -> &[KeyChord] {
        &self.pending
    }

    /// Current count prefix, if any.
    pub fn count(&self) -> Option<u32> {
        self.count
    }

    /// Configure how leading digits are interpreted. Call with
    /// [`CountMode::Accept`] when entering a mode that counts (Vim
    /// normal) and [`CountMode::Reject`] when leaving it.
    pub fn set_count_mode(&mut self, mode: CountMode) {
        self.count_mode = mode;
    }

    /// Reset pending sequence + count. The stack is untouched.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.count = None;
    }

    /// Feed one key to the engine and observe the result.
    pub fn feed(&mut self, chord: KeyChord) -> FeedOutcome {
        // Numeric count prefix (Vim-style). Only active when we're
        // between sequences (pending is empty) and the mode allows it.
        if self.count_mode == CountMode::Accept
            && self.pending.is_empty()
            && chord.modifiers.is_empty()
        {
            if let Key::Char(c @ '0'..='9') = &chord.key {
                let digit = u32::from(*c as u8 - b'0');
                // A leading '0' when no count has been accumulated is
                // typically "move to start of line" in Vim. Don't eat it.
                if !(digit == 0 && self.count.is_none()) {
                    let current = self.count.unwrap_or(0);
                    self.count = Some(current.saturating_mul(10).saturating_add(digit));
                    return FeedOutcome::Pending;
                }
            }
        }

        self.pending.push(chord);
        self.resolve()
    }

    fn resolve(&mut self) -> FeedOutcome {
        // Walk the stack top to bottom until we either find a Command or
        // determine there's no match anywhere.
        let mut any_pending = false;
        // Collect into a local to release the borrow before mutating self.
        let mut found: Option<CommandRef> = None;
        for layer in self.stack.iter().rev() {
            match layer.map.lookup(&self.pending) {
                Lookup::Command(cmd) => {
                    found = Some(cmd.clone());
                    break;
                }
                Lookup::Pending => {
                    any_pending = true;
                }
                Lookup::Unbound => {
                    // An explicit unbind in an upper layer blocks
                    // fallthrough — act as if definitively unbound.
                    return self.finish_unbound();
                }
                Lookup::NoMatch => {}
            }
        }

        if let Some(cmd) = found {
            let count = self.count.unwrap_or(1);
            self.reset();
            return FeedOutcome::Execute { command: cmd, count };
        }
        if any_pending {
            return FeedOutcome::Pending;
        }
        self.finish_unbound()
    }

    fn finish_unbound(&mut self) -> FeedOutcome {
        // Capture the last chord for printable-fallback before resetting.
        let fallback = self
            .pending
            .last()
            .filter(|_| self.pending.len() == 1) // only single-key sequences can self-insert
            .and_then(KeyChord::as_printable_char);
        self.reset();
        FeedOutcome::Unbound {
            printable_fallback: fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_sequence;

    fn seq(s: &str) -> Vec<KeyChord> {
        parse_sequence(s).unwrap()
    }

    fn first(chord: &str) -> KeyChord {
        seq(chord).into_iter().next().unwrap()
    }

    fn emacs_map() -> Arc<Keymap> {
        let mut m = Keymap::named("test");
        m.bind(&seq("C-f"), "cursor.right");
        m.bind(&seq("C-b"), "cursor.left");
        m.bind(&seq("C-x C-s"), "buffer.save");
        m.bind(&seq("C-x C-c"), "editor.quit");
        Arc::new(m)
    }

    #[test]
    fn resolves_single_chord_immediately() {
        let mut engine = KeymapEngine::new(emacs_map());
        let out = engine.feed(first("C-f"));
        assert!(matches!(out, FeedOutcome::Execute { count: 1, .. }));
        if let FeedOutcome::Execute { command, .. } = out {
            assert_eq!(&*command.name, "cursor.right");
        }
    }

    #[test]
    fn resolves_two_chord_prefix() {
        let mut engine = KeymapEngine::new(emacs_map());
        assert_eq!(engine.feed(first("C-x")), FeedOutcome::Pending);
        let out = engine.feed(first("C-s"));
        if let FeedOutcome::Execute { command, .. } = out {
            assert_eq!(&*command.name, "buffer.save");
        } else {
            panic!("{out:?}");
        }
    }

    #[test]
    fn printable_unbound_key_returns_fallback() {
        let mut engine = KeymapEngine::new(emacs_map());
        let out = engine.feed(first("a"));
        assert_eq!(
            out,
            FeedOutcome::Unbound {
                printable_fallback: Some('a')
            }
        );
    }

    #[test]
    fn non_printable_unbound_is_dropped() {
        let mut engine = KeymapEngine::new(emacs_map());
        let out = engine.feed(first("<F5>"));
        assert_eq!(
            out,
            FeedOutcome::Unbound {
                printable_fallback: None
            }
        );
    }

    #[test]
    fn resetting_after_unbound_first_chord_clears_pending() {
        let mut engine = KeymapEngine::new(emacs_map());
        engine.feed(first("a"));
        // Next key should start a fresh sequence.
        assert!(engine.pending_sequence().is_empty());
    }

    #[test]
    fn mode_stack_shadows_global() {
        let mut global = Keymap::named("global");
        global.bind(&seq("a"), "global.a");
        let mut overlay = Keymap::named("overlay");
        overlay.bind(&seq("a"), "overlay.a");

        let mut engine = KeymapEngine::new(Arc::new(global));
        engine.push_layer(Layer::new("overlay", Arc::new(overlay)));

        let out = engine.feed(first("a"));
        if let FeedOutcome::Execute { command, .. } = out {
            assert_eq!(&*command.name, "overlay.a");
        } else {
            panic!("{out:?}");
        }
    }

    #[test]
    fn mode_stack_falls_through_when_upper_has_no_binding() {
        let mut global = Keymap::named("global");
        global.bind(&seq("a"), "global.a");
        let overlay = Keymap::named("overlay"); // empty
        let mut engine = KeymapEngine::new(Arc::new(global));
        engine.push_layer(Layer::new("overlay", Arc::new(overlay)));

        let out = engine.feed(first("a"));
        if let FeedOutcome::Execute { command, .. } = out {
            assert_eq!(&*command.name, "global.a");
        } else {
            panic!("{out:?}");
        }
    }

    #[test]
    fn explicit_unbind_blocks_fallthrough() {
        let mut global = Keymap::named("global");
        global.bind(&seq("a"), "global.a");
        let mut overlay = Keymap::named("overlay");
        overlay.unbind(&seq("a"));
        let mut engine = KeymapEngine::new(Arc::new(global));
        engine.push_layer(Layer::new("overlay", Arc::new(overlay)));

        let out = engine.feed(first("a"));
        assert!(matches!(out, FeedOutcome::Unbound { .. }));
    }

    #[test]
    fn pop_layer_respects_global_floor() {
        let engine_map = emacs_map();
        let mut engine = KeymapEngine::new(engine_map);
        assert_eq!(engine.pop_layer(), None);
        engine.push_layer(Layer::new("insert", Arc::new(Keymap::new())));
        assert_eq!(engine.pop_layer().as_deref(), Some("insert"));
        assert_eq!(engine.pop_layer(), None);
    }

    #[test]
    fn count_prefix_accumulates_when_enabled() {
        let mut m = Keymap::named("vim-ish");
        m.bind(&seq("j"), "cursor.down");
        let mut engine = KeymapEngine::new(Arc::new(m));
        engine.set_count_mode(CountMode::Accept);

        assert_eq!(engine.feed(first("3")), FeedOutcome::Pending);
        let out = engine.feed(first("j"));
        if let FeedOutcome::Execute { command, count } = out {
            assert_eq!(&*command.name, "cursor.down");
            assert_eq!(count, 3);
        } else {
            panic!("{out:?}");
        }
    }

    #[test]
    fn count_prefix_multi_digit() {
        let mut m = Keymap::named("vim-ish");
        m.bind(&seq("j"), "cursor.down");
        let mut engine = KeymapEngine::new(Arc::new(m));
        engine.set_count_mode(CountMode::Accept);
        engine.feed(first("1"));
        engine.feed(first("2"));
        let out = engine.feed(first("j"));
        if let FeedOutcome::Execute { count, .. } = out {
            assert_eq!(count, 12);
        } else {
            panic!("{out:?}");
        }
    }

    #[test]
    fn leading_zero_is_still_a_bindable_key_when_count_accepted() {
        // In Vim, `0` is "move to start of line" — it should reach the
        // keymap, not get eaten by the count accumulator.
        let mut m = Keymap::named("vim-ish");
        m.bind(&seq("0"), "line.start");
        let mut engine = KeymapEngine::new(Arc::new(m));
        engine.set_count_mode(CountMode::Accept);
        let out = engine.feed(first("0"));
        if let FeedOutcome::Execute { command, .. } = out {
            assert_eq!(&*command.name, "line.start");
        } else {
            panic!("{out:?}");
        }
    }

    #[test]
    fn count_mode_reject_treats_digits_as_keys() {
        let mut m = Keymap::named("emacs-ish");
        m.bind(&seq("3"), "digit-three");
        let mut engine = KeymapEngine::new(Arc::new(m));
        // Default count_mode is Reject.
        let out = engine.feed(first("3"));
        if let FeedOutcome::Execute { command, .. } = out {
            assert_eq!(&*command.name, "digit-three");
        } else {
            panic!("{out:?}");
        }
    }
}
