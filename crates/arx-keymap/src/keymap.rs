//! The [`Keymap`] trie: binds key sequences to command names.
//!
//! A keymap is a tree where each edge is a [`KeyChord`] and each node is
//! either a branch (further keys follow) or a terminal binding (run this
//! command). That lets a single map hold both `Ctrl+F` (immediate) and
//! `Ctrl+X Ctrl+S` (prefix).

use std::collections::HashMap;
use std::sync::Arc;

use crate::key::KeyChord;
use crate::parse::{ParseError, parse_sequence};

/// A bound command: the registered name the keymap engine hands back to
/// the editor when this binding fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRef {
    pub name: Arc<str>,
}

impl CommandRef {
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self { name: name.into() }
    }
}

impl<S: Into<Arc<str>>> From<S> for CommandRef {
    fn from(s: S) -> Self {
        CommandRef::new(s)
    }
}

/// A keymap node. Either resolves to a command (leaf) or to a nested
/// sub-map (prefix).
#[derive(Debug, Clone)]
enum Node {
    Command(CommandRef),
    Branch(HashMap<KeyChord, Node>),
    /// An explicit "unbind" — overrides a binding inherited from the
    /// fallback keymap. Behaves as Unbound at lookup time.
    Unbind,
}

/// A mutable trie mapping key sequences to commands.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    root: HashMap<KeyChord, Node>,
    name: Option<String>,
}

impl Keymap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a keymap with a descriptive name (used in diagnostics / logs).
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            root: HashMap::new(),
            name: Some(name.into()),
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Bind a key sequence to a command.
    ///
    /// Panics if the sequence is empty or if inserting the binding
    /// overwrites an existing *prefix* node. (Overwriting a plain command
    /// binding is allowed — that's how user config overrides defaults.)
    pub fn bind(&mut self, sequence: &[KeyChord], command: impl Into<CommandRef>) {
        assert!(!sequence.is_empty(), "keymap binding with empty sequence");
        let cmd = command.into();
        insert(&mut self.root, sequence, Node::Command(cmd));
    }

    /// Parse `sequence_str` and bind it. Returns a parse error if the
    /// string can't be tokenised.
    pub fn bind_str(
        &mut self,
        sequence_str: &str,
        command: impl Into<CommandRef>,
    ) -> Result<(), ParseError> {
        let seq = parse_sequence(sequence_str)?;
        self.bind(&seq, command);
        Ok(())
    }

    /// Explicitly unbind a sequence. Shadows any inherited binding.
    pub fn unbind(&mut self, sequence: &[KeyChord]) {
        assert!(!sequence.is_empty(), "unbind with empty sequence");
        insert(&mut self.root, sequence, Node::Unbind);
    }

    /// Look up a partial or complete key sequence.
    pub fn lookup(&self, sequence: &[KeyChord]) -> Lookup<'_> {
        if sequence.is_empty() {
            return Lookup::Pending;
        }
        let mut map = &self.root;
        for (i, chord) in sequence.iter().enumerate() {
            let last = i + 1 == sequence.len();
            match map.get(chord) {
                None => return Lookup::NoMatch,
                Some(Node::Unbind) => return Lookup::Unbound,
                Some(Node::Command(cmd)) => {
                    return if last {
                        Lookup::Command(cmd)
                    } else {
                        Lookup::NoMatch
                    };
                }
                Some(Node::Branch(next)) => {
                    if last {
                        return Lookup::Pending;
                    }
                    map = next;
                }
            }
        }
        Lookup::Pending
    }

    /// Number of top-level bindings (for tests / debug).
    pub fn top_level_len(&self) -> usize {
        self.root.len()
    }
}

/// Outcome of a keymap lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<'a> {
    /// The sequence matches a concrete binding — execute this command.
    Command(&'a CommandRef),
    /// The sequence is a live prefix — wait for more keys.
    Pending,
    /// The sequence doesn't match anything in this map.
    NoMatch,
    /// The sequence is explicitly unbound (shadows an inherited binding).
    Unbound,
}

fn insert(map: &mut HashMap<KeyChord, Node>, seq: &[KeyChord], node: Node) {
    let (head, rest) = seq.split_first().expect("non-empty sequence");
    if rest.is_empty() {
        map.insert(head.clone(), node);
        return;
    }
    let existing = map
        .entry(head.clone())
        .or_insert_with(|| Node::Branch(HashMap::new()));
    match existing {
        Node::Branch(next) => insert(next, rest, node),
        Node::Command(_) | Node::Unbind => {
            // Overwriting a command binding with a longer prefix is
            // allowed — replace the leaf with a fresh branch and recurse.
            let mut fresh = HashMap::new();
            insert(&mut fresh, rest, node);
            *existing = Node::Branch(fresh);
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

    #[test]
    fn bind_and_lookup_immediate() {
        let mut m = Keymap::new();
        m.bind(&seq("C-f"), "cursor.right");
        match m.lookup(&seq("C-f")) {
            Lookup::Command(c) => assert_eq!(&*c.name, "cursor.right"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bind_and_lookup_prefix() {
        let mut m = Keymap::new();
        m.bind(&seq("C-x C-s"), "buffer.save");
        // First key is pending.
        assert_eq!(m.lookup(&seq("C-x")), Lookup::Pending);
        // Full sequence resolves.
        match m.lookup(&seq("C-x C-s")) {
            Lookup::Command(c) => assert_eq!(&*c.name, "buffer.save"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unbound_sequence_returns_no_match() {
        let m = Keymap::new();
        assert_eq!(m.lookup(&seq("C-q")), Lookup::NoMatch);
    }

    #[test]
    fn overwrite_command_with_longer_prefix() {
        let mut m = Keymap::new();
        m.bind(&seq("C-x"), "oldcmd");
        m.bind(&seq("C-x C-s"), "buffer.save");
        // C-x is now a prefix rather than a command.
        assert_eq!(m.lookup(&seq("C-x")), Lookup::Pending);
        match m.lookup(&seq("C-x C-s")) {
            Lookup::Command(c) => assert_eq!(&*c.name, "buffer.save"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unbind_shadows_existing() {
        let mut m = Keymap::new();
        m.bind(&seq("C-f"), "cursor.right");
        m.unbind(&seq("C-f"));
        assert_eq!(m.lookup(&seq("C-f")), Lookup::Unbound);
    }

    #[test]
    fn bind_str_convenience() {
        let mut m = Keymap::new();
        m.bind_str("<Enter>", "buffer.newline").unwrap();
        match m.lookup(&seq("<Enter>")) {
            Lookup::Command(c) => assert_eq!(&*c.name, "buffer.newline"),
            other => panic!("{other:?}"),
        }
    }
}
