//! Arx editor extension SDK (v0.1).
//!
//! Extensions are Rust dylibs loaded by a running arx daemon (or by an
//! embedded driver during development). An extension:
//!
//! 1. Implements the [`Extension`] trait on a `Default`-constructible
//!    type.
//! 2. Uses the [`declare_extension!`] macro to emit the C-ABI entry
//!    points the host looks up.
//! 3. Compiles as a `cdylib`.
//! 4. Drops into the daemon's extensions directory (or is loaded by
//!    hand via `arx ext load <path>`).
//!
//! Spec §5 — Extension SDK. This module is the **v0.1** cut described
//! in the Phase 1 scope: it lets extensions register named commands
//! against the editor's [`CommandRegistry`](arx_core::CommandRegistry)
//! and have them survive hot-reload. Later milestones will add buffer
//! / window / hook / completion / agent APIs as those editor
//! subsystems stabilise.
//!
//! # Minimal example
//!
//! ```ignore
//! use arx_sdk::{declare_extension, ActivationContext, Extension, ExtensionError, ExtensionMeta, ActivationPolicy};
//!
//! #[derive(Default)]
//! pub struct HelloExt;
//!
//! impl Extension for HelloExt {
//!     fn metadata(&self) -> ExtensionMeta {
//!         ExtensionMeta {
//!             name: "hello".into(),
//!             version: "0.1.0".into(),
//!             description: "Greets the user".into(),
//!             sdk_version: arx_sdk::SDK_VERSION,
//!             activation: ActivationPolicy::Startup,
//!         }
//!     }
//!
//!     fn activate(&self, ctx: &mut ActivationContext) -> Result<(), ExtensionError> {
//!         ctx.register_command("hello.greet", "Say hello", |editor| {
//!             tracing::info!("hello from the extension");
//!             editor.mark_dirty();
//!             Ok(())
//!         });
//!         Ok(())
//!     }
//! }
//!
//! declare_extension!(HelloExt);
//! ```
//!
//! # ABI stability caveat
//!
//! v0.1 uses a **same-compiler-version** contract: extensions and the
//! host must be built from the same rustc toolchain. The SDK version
//! number exported by [`declare_extension!`] is a coarse check — it
//! catches SDK upgrades but does not guarantee rustc ABI compatibility.
//! A future milestone will adopt `abi_stable` for cross-toolchain
//! interop per spec §5.13; until then, hot-reload is a developer
//! feature and production distribution should rebuild from source.

use std::sync::Arc;

use thiserror::Error;

use arx_core::{Command, CommandContext, Editor};

/// Re-export of [`arx_core`] for extensions that need to reach
/// deeper into the editor than the SDK surface currently exposes.
/// v0.1's SDK is intentionally minimal; extensions poke directly at
/// `arx_sdk::core::*` items for anything not yet wrapped. A later
/// milestone will remove the re-export as the SDK grows proper
/// abstractions.
pub use arx_core as core;

/// SDK wire version. Bumped on any breaking change to the [`Extension`]
/// trait, the [`ActivationContext`] API, or the declared-entrypoint
/// signatures. The host refuses to load an extension whose exported
/// [`declare_extension!`]-generated `arx_sdk_version()` returns a
/// different value.
pub const SDK_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Extension trait
// ---------------------------------------------------------------------------

/// A loadable editor extension.
///
/// Implementors must be `Default`-constructible (the host creates an
/// instance via `Default::default()` inside the macro-generated entry
/// point) and `Send + Sync` so they can be shared between the event
/// loop task and command handlers.
pub trait Extension: Send + Sync {
    /// Describe this extension. Called once immediately after load so
    /// the host can log what it picked up and verify SDK compatibility.
    fn metadata(&self) -> ExtensionMeta;

    /// Set up commands, hooks, and keymaps against `ctx`. Runs once
    /// per load. Any commands registered here are automatically
    /// cleaned up when the extension is unloaded — the host records
    /// the names and strips them from the registry on `unload`.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<(), ExtensionError>;

    /// Optional cleanup hook called before the extension is unloaded.
    /// Default is a no-op.
    fn deactivate(&self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

/// Static metadata describing an extension.
///
/// v0.1 keeps this small. Spec §5.1 lists more fields (`dependencies`,
/// `commands: &[CommandMeta]`, etc.) which land with later SDK
/// milestones — the host ignores unknown metadata so an older host can
/// still load a newer extension provided [`SDK_VERSION`] matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionMeta {
    /// Machine-readable name, e.g. `"hello"`, `"git-gutter"`. Must be
    /// unique across loaded extensions; collisions cause the newer
    /// load to replace the older one.
    pub name: String,
    /// Semver-ish version string. Informational only for v0.1.
    pub version: String,
    /// Human description shown by `arx ext list` / the command palette.
    pub description: String,
    /// SDK version this extension was compiled against. Must equal
    /// [`SDK_VERSION`] at load time.
    pub sdk_version: u32,
    /// When to activate.
    pub activation: ActivationPolicy,
}

/// When the host should call [`Extension::activate`]. v0.1 supports
/// only the two simplest cases — lazy language/project activation
/// lands with the LSP milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationPolicy {
    /// Activate at editor startup. The current default.
    Startup,
    /// Activate on demand only (user runs `extension.activate <name>`).
    Manual,
}

/// Errors an extension can return from [`Extension::activate`] or
/// [`Extension::deactivate`]. Deliberately simple — the host logs the
/// message and continues.
#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("{0}")]
    Msg(String),
}

impl ExtensionError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }
}

// ---------------------------------------------------------------------------
// ActivationContext
// ---------------------------------------------------------------------------

/// Shared handler-object type. Boxed (`Arc`) so multiple registry
/// consumers can share the closure and so the dylib's TLS /
/// static-destructor handling matches the host's expectations when
/// the closure is dropped.
pub type CommandHandler =
    Arc<dyn Fn(&mut Editor) -> Result<(), ExtensionError> + Send + Sync + 'static>;

/// The sandbox an extension sees during [`Extension::activate`].
///
/// v0.1 is a thin wrapper around a pending-command buffer: the
/// extension calls [`register_command`](Self::register_command) zero
/// or more times, then the host drains the pending list onto the
/// editor's real [`arx_core::CommandRegistry`] via the event loop.
/// The indirection lets us activate extensions without holding a
/// `&mut Editor` open for the entire `activate()` call — the extension
/// doesn't need to be written with care about `CommandBus` plumbing.
///
/// Later milestones will expose more editor subsystems here (keymaps,
/// buffers, windows, hooks, completion, agents, ...).
pub struct ActivationContext {
    pending: Vec<PendingCommand>,
}

impl Default for ActivationContext {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ActivationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivationContext")
            .field("pending", &self.pending.len())
            .finish()
    }
}

impl ActivationContext {
    /// Create an empty context. Callers typically don't do this
    /// directly — the host builds and drains one per load.
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// Register a named command that runs `handler` with a mutable
    /// reference to the editor when dispatched by the keymap or the
    /// command palette.
    ///
    /// `name` should match the convention `domain.action` (e.g.
    /// `"hello.greet"`, `"git.status"`) so profiles can bind it by
    /// name without collision.
    pub fn register_command<F>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) where
        F: Fn(&mut Editor) -> Result<(), ExtensionError> + Send + Sync + 'static,
    {
        self.pending.push(PendingCommand {
            name: name.into(),
            description: description.into(),
            handler: Arc::new(handler),
        });
    }

    /// Drain the pending commands. Called by the host after `activate`
    /// returns — the resulting vec is fed into the real
    /// [`arx_core::CommandRegistry`] on the event loop task.
    pub fn into_pending(self) -> Vec<PendingCommand> {
        self.pending
    }
}

/// A command an extension registered during activation but that the
/// host hasn't yet pushed onto the real [`arx_core::CommandRegistry`].
///
/// Public because the host in `arx-driver` is in another crate —
/// treat this type as `pub(crate)` in spirit.
#[doc(hidden)]
pub struct PendingCommand {
    pub name: String,
    pub description: String,
    pub handler: CommandHandler,
}

impl std::fmt::Debug for PendingCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Bridge: pending → arx_core::Command
// ---------------------------------------------------------------------------

/// Adapts a [`PendingCommand`] into an `arx_core::Command` the host
/// can drop into the registry. Reaches into `cx.editor` and runs the
/// extension's closure; logs any error the closure returns.
pub struct ExtensionCommand {
    name: String,
    description: String,
    handler: CommandHandler,
}

impl ExtensionCommand {
    pub fn from_pending(pending: PendingCommand) -> Self {
        Self {
            name: pending.name,
            description: pending.description,
            handler: pending.handler,
        }
    }
}

impl std::fmt::Debug for ExtensionCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

impl Command for ExtensionCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn run(&self, cx: &mut CommandContext<'_>) {
        if let Err(e) = (self.handler)(cx.editor) {
            tracing::warn!(name = %self.name, error = %e, "extension command failed");
        }
    }
}

// ---------------------------------------------------------------------------
// Macro
// ---------------------------------------------------------------------------

/// Emit the C-ABI entry points the extension host looks up.
///
/// Use it at the *top level* of your `cdylib` crate, passing the
/// concrete `Extension`-implementing type (must be
/// `Default`-constructible):
///
/// ```ignore
/// declare_extension!(MyExtension);
/// ```
///
/// This expands to two `#[no_mangle] pub fn` items:
///
/// * `arx_sdk_version() -> u32` — returns [`SDK_VERSION`]; the host
///   refuses to load any dylib whose exported version doesn't match
///   its own compiled-in constant.
/// * `arx_extension_create() -> Box<dyn Extension>` — factory the
///   host calls exactly once to obtain the extension instance.
///
/// Both functions are intentionally *not* `extern "C"`: the
/// same-compiler-version contract in v0.1 means Rust ABI is fine, and
/// `Box<dyn Extension>` is already a Rust type.
#[macro_export]
macro_rules! declare_extension {
    ($ty:ty) => {
        #[unsafe(no_mangle)]
        pub fn arx_sdk_version() -> u32 {
            $crate::SDK_VERSION
        }

        #[unsafe(no_mangle)]
        pub fn arx_extension_create() -> ::std::boxed::Box<dyn $crate::Extension> {
            ::std::boxed::Box::new(<$ty as ::core::default::Default>::default())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestExt;

    impl Extension for TestExt {
        fn metadata(&self) -> ExtensionMeta {
            ExtensionMeta {
                name: "test".into(),
                version: "0.1.0".into(),
                description: "A test extension".into(),
                sdk_version: SDK_VERSION,
                activation: ActivationPolicy::Startup,
            }
        }

        fn activate(&self, ctx: &mut ActivationContext) -> Result<(), ExtensionError> {
            ctx.register_command("test.hello", "Test command", |editor| {
                editor.mark_dirty();
                Ok(())
            });
            Ok(())
        }
    }

    #[test]
    fn activation_context_collects_pending_commands() {
        let ext = TestExt;
        let mut ctx = ActivationContext::new();
        ext.activate(&mut ctx).unwrap();
        let pending = ctx.into_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, "test.hello");
        assert_eq!(pending[0].description, "Test command");
    }

    #[test]
    fn extension_command_description_is_runtime_string() {
        let pending = PendingCommand {
            name: "x.y".into(),
            description: "runtime desc".into(),
            handler: Arc::new(|_editor| Ok(())),
        };
        let cmd = ExtensionCommand::from_pending(pending);
        assert_eq!(cmd.description(), "runtime desc");
    }
}
