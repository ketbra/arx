//! Named command registry.
//!
//! The [`KeymapEngine`](arx_keymap::KeymapEngine) resolves key sequences
//! to command *names*. This module maps those names back to
//! [`Arc<dyn Command>`] implementations that actually mutate the editor.
//!
//! Separating "key → name" from "name → behaviour" lets us:
//!
//! * Share commands across profiles (Emacs, Vim, KEDIT all bind the same
//!   underlying `cursor.right`).
//! * Surface a command palette later: enumerate the registry, fuzzy
//!   match against the user's query, invoke the selected entry.
//! * Let extensions register their own commands without touching the
//!   keymap layer.
//!
//! Every [`Command`] runs inside a [`CommandContext`] that exposes
//! `&mut Editor`, a clone of the [`CommandBus`], and the accumulated
//! count prefix from the engine. Synchronous commands touch `editor`
//! directly; async commands (file save, etc.) spawn a task on the bus.

use std::collections::HashMap;
use std::sync::Arc;

use crate::command::CommandBus;
use crate::editor::Editor;

/// Context handed to every command invocation.
pub struct CommandContext<'a> {
    /// Exclusive access to editor state for the duration of the call.
    pub editor: &'a mut Editor,
    /// Clone of the command bus. Use this to spawn async follow-ups
    /// (e.g. `tokio::spawn(async move { save_file(&bus, ...).await })`).
    pub bus: CommandBus,
    /// Numeric count prefix from the keymap engine (`3dw` in Vim → 3,
    /// Emacs `C-u 5 C-n` → 5). Defaults to 1 for commands that don't
    /// care.
    pub count: u32,
}

impl std::fmt::Debug for CommandContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandContext")
            .field("count", &self.count)
            .field("bus", &self.bus)
            .finish_non_exhaustive()
    }
}

impl CommandContext<'_> {
    /// Run the count-aware body `n` times. Most "move by one" commands
    /// use this so `3j` → "move down three lines" transparently.
    pub fn repeat<F: FnMut(&mut Editor)>(&mut self, mut body: F) {
        let n = self.count.max(1);
        for _ in 0..n {
            body(self.editor);
        }
    }
}

/// A named, dispatchable command.
///
/// Implementors are `Send + Sync` so the registry can clone `Arc<dyn
/// Command>` out to callers and share them across invocations. Commands
/// themselves typically carry no state.
pub trait Command: Send + Sync {
    /// Short, stable machine name (`"cursor.right"`, `"buffer.save"`).
    fn name(&self) -> &str;

    /// Human-readable description for the command palette and `C-h k`
    /// lookup screens.
    fn description(&self) -> &'static str {
        ""
    }

    /// Run the command.
    fn run(&self, cx: &mut CommandContext<'_>);
}

/// A registry of named commands.
#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<Arc<str>, Arc<dyn Command>>,
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("len", &self.commands.len())
            .finish()
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command. Panics if `name` already has a binding —
    /// duplicate names are programmer errors, not user configuration.
    pub fn register<C: Command + 'static>(&mut self, command: C) {
        let name: Arc<str> = Arc::from(command.name());
        assert!(
            !self.commands.contains_key(&name),
            "command {name:?} is already registered"
        );
        self.commands.insert(name, Arc::new(command));
    }

    /// Register a freshly-allocated command as an `Arc<dyn Command>`.
    pub fn register_arc(&mut self, command: Arc<dyn Command>) {
        let name: Arc<str> = Arc::from(command.name());
        assert!(
            !self.commands.contains_key(&name),
            "command {name:?} is already registered"
        );
        self.commands.insert(name, command);
    }

    /// Look up a command by name. Returns an `Arc` clone so the caller
    /// can drop any borrow into the registry before invoking.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Command>> {
        self.commands.get(name).cloned()
    }

    /// Iterate every registered `(name, description)` pair. Useful for
    /// the eventual command palette.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.commands
            .iter()
            .map(|(name, cmd)| (name.as_ref(), cmd.description()))
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventLoop;

    struct Dummy(&'static str);
    impl Command for Dummy {
        fn name(&self) -> &str {
            self.0
        }
        fn run(&self, _cx: &mut CommandContext<'_>) {}
    }

    #[test]
    fn register_and_get_by_name() {
        let mut reg = CommandRegistry::new();
        reg.register(Dummy("test.x"));
        assert_eq!(reg.len(), 1);
        assert!(reg.get("test.x").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    #[should_panic(expected = "already registered")]
    fn duplicate_registration_panics() {
        let mut reg = CommandRegistry::new();
        reg.register(Dummy("test.x"));
        reg.register(Dummy("test.x"));
    }

    #[tokio::test]
    async fn context_repeat_honours_count() {
        // Sanity-check that CommandContext::repeat works with a count.
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());

        let bus_clone = bus.clone();
        let counter = bus
            .invoke(move |editor| {
                let mut n = 0;
                let mut cx = CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 4,
                };
                cx.repeat(|_| n += 1);
                n
            })
            .await
            .unwrap();
        assert_eq!(counter, 4);
        drop(bus);
        let _ = handle.await.unwrap();
    }
}
