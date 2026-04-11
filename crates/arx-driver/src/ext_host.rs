// This module calls into `libloading`, which is inherently unsafe —
// every symbol lookup is a transmute and every dylib load runs
// arbitrary initializer code. The workspace default is
// `unsafe_code = "deny"`; we opt in here with a local allow. Every
// `unsafe` block below carries a SAFETY comment explaining why it's
// sound (modulo the same-compiler-version contract documented in
// `arx-sdk`).
#![allow(unsafe_code)]

//! Extension host — loads, tracks, and unloads arx-sdk dylibs.
//!
//! Spec §5. Phase 1 "SDK v0.1 with hot-reload". The host owns one
//! [`libloading::Library`] per loaded extension plus the
//! `Box<dyn Extension>` created from it, and records the commands
//! each extension registered so they can be stripped cleanly on
//! unload.
//!
//! # Lifecycle
//!
//! ```text
//!     load(path, bus)
//!         │
//!         │ 1. libloading::Library::new(path)
//!         │ 2. verify exported `arx_sdk_version()` matches SDK_VERSION
//!         │ 3. call `arx_extension_create()` → Box<dyn Extension>
//!         │ 4. call `ext.metadata()` + `ext.activate(&mut ctx)`
//!         │ 5. bus.invoke → push every pending command into the real
//!         │    CommandRegistry, record names in LoadedExtension
//!         ▼
//!     loaded
//!         │
//!         │ unload(name, bus) / reload(path, bus)
//!         │ 1. bus.invoke → unregister every recorded command
//!         │ 2. ext.deactivate()
//!         │ 3. drop Box<dyn Extension>
//!         │ 4. drop Library (only now — the vtable pointers live in
//!         │    its code pages and can't outlive it)
//! ```
//!
//! The drop ordering in `LoadedExtension` is load-bearing. See the
//! field comments on [`LoadedExtension`].
//!
//! # ABI caveats
//!
//! v0.1 relies on a same-compiler-version contract: the host trusts
//! that any dylib whose `arx_sdk_version()` matches its own was built
//! with the same rustc toolchain. If that's false, we're in undefined
//! behaviour territory — `Box<dyn Extension>` isn't a stable-ABI
//! type. A future milestone will swap in `abi_stable` to make this
//! safe across toolchains.

use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;
use thiserror::Error;
use tracing::{debug, info, warn};

use arx_core::{CommandBus, DispatchError, Editor};
use arx_sdk::{
    ActivationContext, Extension, ExtensionCommand, ExtensionMeta, PendingCommand, SDK_VERSION,
};

// ---------------------------------------------------------------------------
// Types exposed by the dylib
// ---------------------------------------------------------------------------

/// Signature of the `arx_sdk_version()` symbol every extension must
/// export (via [`arx_sdk::declare_extension!`]).
type SdkVersionFn = fn() -> u32;

/// Signature of the `arx_extension_create()` symbol every extension
/// must export.
type CreateFn = fn() -> Box<dyn Extension>;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ExtHostError {
    #[error("libloading error for {path}: {source}")]
    Library {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },
    #[error("extension {path} missing required symbol {symbol:?}: {source}")]
    MissingSymbol {
        path: PathBuf,
        symbol: &'static str,
        #[source]
        source: libloading::Error,
    },
    #[error(
        "extension {path} reports SDK version {found} but host expects {expected}"
    )]
    SdkVersionMismatch {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    #[error("extension {name:?} failed to activate: {source}")]
    Activate {
        name: String,
        #[source]
        source: arx_sdk::ExtensionError,
    },
    #[error("extension {name:?} is not loaded")]
    NotLoaded { name: String },
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("event loop bus closed")]
    BusClosed,
}

impl From<DispatchError> for ExtHostError {
    fn from(_: DispatchError) -> Self {
        Self::BusClosed
    }
}

// ---------------------------------------------------------------------------
// LoadedExtension
// ---------------------------------------------------------------------------

/// One loaded extension.
///
/// # Why `ManuallyDrop<Library>`
///
/// The Library is deliberately **never dropped**. Rust cdylibs have
/// a long-standing interaction with glibc's `dlclose` where the
/// dynamic linker's static-destructor pass (`_dl_call_fini`) can
/// call into TLS destructors or other trampolines that `dlclose`
/// has already cleared, producing a null-function-pointer segfault.
/// The issue is well-documented in the Rust ecosystem: loader
/// authors either use `dlmopen` with namespace isolation (Linux-
/// specific) or just leak the library at drop. Phase 1 takes the
/// simpler option.
///
/// The practical effect: each hot-reload *adds* a loaded library
/// without releasing the previous one. The previous library's
/// commands are unregistered from the registry so they're
/// unreachable, but the code pages stay mapped. For a development
/// feature (`arx ext dev` watching a single dylib as the author
/// rebuilds), this is a tiny amount of memory — a few tens of MB
/// after dozens of reloads in a session. Production users who don't
/// hot-reload see zero cost.
///
/// A later milestone can swap in `abi_stable` + `dlmopen` to make
/// this proper.
struct LoadedExtension {
    name: String,
    path: PathBuf,
    meta: ExtensionMeta,
    registered_commands: Vec<String>,
    // Intentionally dropped before the library (declaration order).
    // The Extension's Drop impl might run `.text` code from the
    // dylib, so the library must still be mapped at this point.
    extension: Box<dyn Extension>,
    // Never dropped; see the struct-level doc comment.
    #[allow(dead_code)]
    library: ManuallyDrop<Library>,
}

impl std::fmt::Debug for LoadedExtension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedExtension")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("meta", &self.meta)
            .field("registered_commands", &self.registered_commands)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// ExtensionHost
// ---------------------------------------------------------------------------

/// Owns every currently-loaded extension plus the bookkeeping needed
/// to hot-reload or drop any of them individually.
///
/// Constructed once by the driver at daemon startup. Not `Send` —
/// must stay on the task that owns it, typically the driver's main
/// task (not the event loop), because loading/unloading walks its
/// internal vec mutably.
#[derive(Default)]
pub struct ExtensionHost {
    loaded: Vec<LoadedExtension>,
}

impl std::fmt::Debug for ExtensionHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionHost")
            .field("loaded", &self.loaded.len())
            .finish_non_exhaustive()
    }
}

impl ExtensionHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many extensions are currently loaded.
    pub fn len(&self) -> usize {
        self.loaded.len()
    }

    /// Whether no extensions are loaded.
    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }

    /// Iterate the metadata of every loaded extension.
    pub fn loaded(&self) -> impl Iterator<Item = &ExtensionMeta> + '_ {
        self.loaded.iter().map(|l| &l.meta)
    }

    /// Look up an extension's metadata by name.
    pub fn get(&self, name: &str) -> Option<&ExtensionMeta> {
        self.loaded.iter().find(|l| l.name == name).map(|l| &l.meta)
    }

    /// Load an extension dylib and register its commands.
    ///
    /// Steps in order:
    /// 1. `libloading::Library::new(path)` — `unsafe` because loading
    ///    a dylib executes its static initializers.
    /// 2. Resolve and call `arx_sdk_version()`; reject if it doesn't
    ///    match the host's compiled-in [`SDK_VERSION`].
    /// 3. Resolve and call `arx_extension_create()`.
    /// 4. Call `ext.activate(&mut ctx)` synchronously, collecting
    ///    pending commands.
    /// 5. Flush pending commands onto the real `CommandRegistry` via
    ///    `bus.invoke`, and record their names for later cleanup.
    pub async fn load(
        &mut self,
        path: &Path,
        bus: &CommandBus,
    ) -> Result<ExtensionMeta, ExtHostError> {
        // If the path is already loaded (same canonicalised path),
        // treat as a reload.
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(existing) =
            self.loaded.iter().find(|l| l.path == canonical).map(|l| l.name.clone())
        {
            debug!(%existing, "extension already loaded; unloading first");
            self.unload(&existing, bus).await?;
        }

        // SAFETY: Loading arbitrary dylibs is inherently unsafe — the
        // file can run arbitrary code in its init routines, export
        // symbols with wrong signatures, or violate ABI assumptions.
        // We mitigate via (1) path control (the caller is the daemon
        // or a test), (2) SDK version check, and (3) a big fat
        // caveat in arx-sdk docs.
        let library = unsafe {
            Library::new(&canonical).map_err(|e| ExtHostError::Library {
                path: canonical.clone(),
                source: e,
            })?
        };

        // SAFETY: The same-compiler-version contract; see crate-level
        // docs. Symbol lookups can return symbols with arbitrary
        // types — we cast to the signatures the SDK guarantees via
        // `declare_extension!`.
        let sdk_version: libloading::Symbol<SdkVersionFn> = unsafe {
            library
                .get(b"arx_sdk_version")
                .map_err(|e| ExtHostError::MissingSymbol {
                    path: canonical.clone(),
                    symbol: "arx_sdk_version",
                    source: e,
                })?
        };
        let version = sdk_version();
        if version != SDK_VERSION {
            return Err(ExtHostError::SdkVersionMismatch {
                path: canonical,
                found: version,
                expected: SDK_VERSION,
            });
        }
        // Symbol carries a lifetime bound to the Library; we're
        // done with it now and only want to hold the Library going
        // forward. A plain shadow drop suffices — Symbol has no
        // Drop impl so `drop()` would be a no-op flagged by
        // clippy. Scope-end drop does the same thing.

        let create: libloading::Symbol<CreateFn> = unsafe {
            library
                .get(b"arx_extension_create")
                .map_err(|e| ExtHostError::MissingSymbol {
                    path: canonical.clone(),
                    symbol: "arx_extension_create",
                    source: e,
                })?
        };
        let extension: Box<dyn Extension> = create();

        let meta = extension.metadata();
        info!(
            name = %meta.name,
            version = %meta.version,
            sdk = meta.sdk_version,
            "extension loaded",
        );

        // Run the extension's activate() hook synchronously to
        // collect pending commands. No awaits here — `activate`
        // itself is sync in v0.1.
        let mut ctx = ActivationContext::new();
        extension.activate(&mut ctx).map_err(|e| ExtHostError::Activate {
            name: meta.name.clone(),
            source: e,
        })?;
        let pending = ctx.into_pending();

        // Push the pending commands onto the real registry via the
        // bus. Returns the list of names on success so we can record
        // them for cleanup.
        let names = register_pending(bus, pending).await?;

        self.loaded.push(LoadedExtension {
            name: meta.name.clone(),
            path: canonical,
            meta: meta.clone(),
            registered_commands: names,
            extension,
            library: ManuallyDrop::new(library),
        });
        Ok(meta)
    }

    /// Unload an extension by name. No-op returning `Ok(false)` if
    /// nothing was loaded under that name. Strips the extension's
    /// commands from the registry first, then drops the boxed
    /// extension, then drops the library handle (in that order, for
    /// the reason spelled out on [`LoadedExtension`]).
    pub async fn unload(
        &mut self,
        name: &str,
        bus: &CommandBus,
    ) -> Result<bool, ExtHostError> {
        let Some(idx) = self.loaded.iter().position(|l| l.name == name) else {
            return Ok(false);
        };
        let loaded = self.loaded.remove(idx);
        // Give the extension a chance to clean up first.
        if let Err(e) = loaded.extension.deactivate() {
            warn!(%name, error = %e, "extension deactivate() errored; continuing unload");
        }
        // Strip commands from the registry before the library
        // unloads, otherwise any cloned Arc<dyn Command> inside the
        // registry would hold dangling fn pointers.
        unregister_commands(bus, loaded.registered_commands.clone()).await?;
        debug!(%name, "extension unloaded");
        drop(loaded);
        Ok(true)
    }

    /// Reload the extension at `path`: unload if it's already loaded
    /// under any name, then load fresh. Equivalent to a manual
    /// unload-then-load but uses a single canonicalised path lookup.
    pub async fn reload(
        &mut self,
        path: &Path,
        bus: &CommandBus,
    ) -> Result<ExtensionMeta, ExtHostError> {
        // Find by path (canonicalised) because the user might have
        // changed the extension's metadata-name between builds.
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(name) = self
            .loaded
            .iter()
            .find(|l| l.path == canonical)
            .map(|l| l.name.clone())
        {
            self.unload(&name, bus).await?;
        }
        self.load(path, bus).await
    }

    /// Unload every extension. Called from the daemon shutdown path.
    pub async fn unload_all(&mut self, bus: &CommandBus) -> Result<(), ExtHostError> {
        while let Some(loaded) = self.loaded.pop() {
            if let Err(e) = loaded.extension.deactivate() {
                warn!(
                    name = %loaded.name,
                    error = %e,
                    "extension deactivate() errored during shutdown; continuing",
                );
            }
            unregister_commands(bus, loaded.registered_commands.clone()).await?;
            drop(loaded);
        }
        Ok(())
    }

    // --- Synchronous variants -------------------------------------
    //
    // The daemon loads extensions at startup *before* any event loop
    // has been created, so the async bus-going variants above aren't
    // usable yet. These sync variants operate directly on `&mut
    // Editor` and are only called from places that already hold
    // exclusive editor access.

    /// Synchronous [`Self::load`]: loads a dylib, activates it, and
    /// registers its commands directly on `editor.commands_mut()`.
    /// Equivalent to `load` but skips the bus round-trip.
    pub fn load_sync(
        &mut self,
        path: &Path,
        editor: &mut Editor,
    ) -> Result<ExtensionMeta, ExtHostError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(existing) =
            self.loaded.iter().find(|l| l.path == canonical).map(|l| l.name.clone())
        {
            debug!(%existing, "extension already loaded; unloading first");
            self.unload_sync(&existing, editor);
        }

        // SAFETY: see `load` above — the caller controls the path
        // and the SDK version check is the contractual trust anchor.
        let library = unsafe {
            Library::new(&canonical).map_err(|e| ExtHostError::Library {
                path: canonical.clone(),
                source: e,
            })?
        };
        // SAFETY: same as above — symbol types match the contract
        // emitted by `arx_sdk::declare_extension!`.
        let sdk_version: libloading::Symbol<SdkVersionFn> = unsafe {
            library
                .get(b"arx_sdk_version")
                .map_err(|e| ExtHostError::MissingSymbol {
                    path: canonical.clone(),
                    symbol: "arx_sdk_version",
                    source: e,
                })?
        };
        let version = sdk_version();
        if version != SDK_VERSION {
            return Err(ExtHostError::SdkVersionMismatch {
                path: canonical,
                found: version,
                expected: SDK_VERSION,
            });
        }
        // Symbol carries a lifetime bound to the Library; we're
        // done with it now and only want to hold the Library going
        // forward. A plain shadow drop suffices — Symbol has no
        // Drop impl so `drop()` would be a no-op flagged by
        // clippy. Scope-end drop does the same thing.
        // SAFETY: same as above.
        let create: libloading::Symbol<CreateFn> = unsafe {
            library
                .get(b"arx_extension_create")
                .map_err(|e| ExtHostError::MissingSymbol {
                    path: canonical.clone(),
                    symbol: "arx_extension_create",
                    source: e,
                })?
        };
        let extension: Box<dyn Extension> = create();

        let meta = extension.metadata();
        info!(
            name = %meta.name,
            version = %meta.version,
            sdk = meta.sdk_version,
            "extension loaded (sync)",
        );

        let mut ctx = ActivationContext::new();
        extension.activate(&mut ctx).map_err(|e| ExtHostError::Activate {
            name: meta.name.clone(),
            source: e,
        })?;
        let pending = ctx.into_pending();

        let names = register_pending_sync(editor, pending);

        self.loaded.push(LoadedExtension {
            name: meta.name.clone(),
            path: canonical,
            meta: meta.clone(),
            registered_commands: names,
            extension,
            library: ManuallyDrop::new(library),
        });
        Ok(meta)
    }

    /// Synchronous [`Self::unload`]. Returns whether an extension was
    /// actually unloaded.
    pub fn unload_sync(&mut self, name: &str, editor: &mut Editor) -> bool {
        let Some(idx) = self.loaded.iter().position(|l| l.name == name) else {
            return false;
        };
        let loaded = self.loaded.remove(idx);
        if let Err(e) = loaded.extension.deactivate() {
            warn!(%name, error = %e, "extension deactivate() errored; continuing unload");
        }
        for cmd_name in &loaded.registered_commands {
            editor.commands_mut().unregister(cmd_name);
        }
        drop(loaded);
        true
    }

    /// Synchronous [`Self::unload_all`].
    pub fn unload_all_sync(&mut self, editor: &mut Editor) {
        while let Some(loaded) = self.loaded.pop() {
            if let Err(e) = loaded.extension.deactivate() {
                warn!(
                    name = %loaded.name,
                    error = %e,
                    "extension deactivate() errored during shutdown; continuing",
                );
            }
            for cmd_name in &loaded.registered_commands {
                editor.commands_mut().unregister(cmd_name);
            }
            drop(loaded);
        }
    }

    /// Walk a directory and synchronously load every dylib in it.
    /// Ignores files that don't look like dylibs, and logs but
    /// doesn't fail on individual load errors — one broken extension
    /// shouldn't refuse the whole daemon.
    pub fn load_dir_sync(
        &mut self,
        dir: &Path,
        editor: &mut Editor,
    ) -> Result<usize, ExtHostError> {
        std::fs::create_dir_all(dir).map_err(|source| ExtHostError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let mut count = 0;
        for entry in std::fs::read_dir(dir).map_err(|source| ExtHostError::Io {
            path: dir.to_path_buf(),
            source,
        })? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !is_dylib_suffix(&path) {
                continue;
            }
            match self.load_sync(&path, editor) {
                Ok(meta) => {
                    debug!(name = %meta.name, path = %path.display(), "auto-loaded");
                    count += 1;
                }
                Err(err) => {
                    warn!(%err, path = %path.display(), "failed to load extension");
                }
            }
        }
        Ok(count)
    }
}

/// Filename-extension sniff — matches the platform dynamic-library
/// suffix. Duplicated from `ext_watcher::is_dylib` to avoid a
/// cross-module dep from the host to the watcher.
fn is_dylib_suffix(path: &Path) -> bool {
    let suffix = std::env::consts::DLL_SUFFIX.trim_start_matches('.');
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(suffix))
}

/// Synchronous counterpart of `register_pending`. Used by
/// `load_sync` when we already hold `&mut Editor`.
fn register_pending_sync(editor: &mut Editor, pending: Vec<PendingCommand>) -> Vec<String> {
    let mut names = Vec::with_capacity(pending.len());
    for cmd in pending {
        let name = cmd.name.clone();
        if editor.commands().get(&name).is_some() {
            warn!(%name, "command name already registered; extension version will win");
            editor.commands_mut().unregister(&name);
        }
        let arc: Arc<dyn arx_core::Command> = Arc::new(ExtensionCommand::from_pending(cmd));
        editor.commands_mut().register_arc(arc);
        names.push(name);
    }
    names
}

/// Push every pending command from `pending` onto the editor's real
/// `CommandRegistry`. Runs on the event loop task via `bus.invoke`.
/// Returns the list of registered names so the host can record them
/// for cleanup on unload.
async fn register_pending(
    bus: &CommandBus,
    pending: Vec<PendingCommand>,
) -> Result<Vec<String>, ExtHostError> {
    let names: Vec<String> = pending.iter().map(|p| p.name.clone()).collect();
    bus.invoke(move |editor: &mut Editor| {
        for cmd in pending {
            let name = cmd.name.clone();
            // Skip silently if the name is already taken — not our
            // problem, the second loader wins on the next reload.
            // A warn-and-skip keeps the host alive when a user drops
            // two copies of the same extension into the dir.
            if editor.commands().get(&name).is_some() {
                warn!(%name, "command name already registered; extension version will win");
                editor.commands_mut().unregister(&name);
            }
            let arc: Arc<dyn arx_core::Command> =
                Arc::new(ExtensionCommand::from_pending(cmd));
            editor.commands_mut().register_arc(arc);
        }
    })
    .await?;
    Ok(names)
}

/// Unregister a batch of commands from the editor's registry. Runs
/// on the event loop task via `bus.invoke`. Missing commands are
/// silently ignored — they might have been unregistered some other
/// way before we got here.
async fn unregister_commands(bus: &CommandBus, names: Vec<String>) -> Result<(), ExtHostError> {
    bus.invoke(move |editor: &mut Editor| {
        for name in names {
            editor.commands_mut().unregister(&name);
        }
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_core::EventLoop;

    /// Find the compiled `ext-hello` cdylib by walking up from
    /// `CARGO_MANIFEST_DIR` to locate `target/<profile>/`. Tries
    /// the debug and release profiles in that order so the test
    /// works regardless of how cargo was invoked.
    ///
    /// Returns `None` if the artifact isn't built yet (`cargo test
    /// --workspace` should always build it, but a test running
    /// against a subset package might not see it).
    fn ext_hello_path() -> Option<PathBuf> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // crates/arx-driver -> workspace root
        let workspace = manifest.parent()?.parent()?;
        let dll_name = format!(
            "{}ext_hello{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX,
        );
        for profile in ["debug", "release"] {
            let candidate = workspace.join("target").join(profile).join(&dll_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    async fn spawn_editor_loop() -> (tokio::task::JoinHandle<Editor>, CommandBus) {
        let (event_loop, bus) = EventLoop::new();
        let handle = tokio::spawn(event_loop.run());
        bus.invoke(|editor| {
            let buf = editor.buffers_mut().create_from_text("", None);
            editor.windows_mut().open(buf);
        })
        .await
        .unwrap();
        (handle, bus)
    }

    #[tokio::test]
    async fn load_registers_extension_command() {
        let Some(path) = ext_hello_path() else {
            eprintln!("SKIP: ext-hello cdylib not built; run `cargo test --workspace`");
            return;
        };
        let (handle, bus) = spawn_editor_loop().await;
        let mut host = ExtensionHost::new();
        let meta = host.load(&path, &bus).await.unwrap();
        assert_eq!(meta.name, "hello");
        assert_eq!(meta.sdk_version, SDK_VERSION);

        // The registered command must be callable through the bus.
        let bus_clone = bus.clone();
        let text_after = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get("hello.greet").unwrap();
                let mut cx = arx_core::CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                let id = cx.editor.windows().active().unwrap();
                let buf = cx.editor.windows().get(id).unwrap().buffer_id;
                cx.editor.buffers().get(buf).unwrap().text()
            })
            .await
            .unwrap();
        assert_eq!(text_after, "Hello from ext-hello!");

        host.unload_all(&bus).await.unwrap();
        drop(host);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn unload_strips_extension_command() {
        let Some(path) = ext_hello_path() else {
            return;
        };
        let (handle, bus) = spawn_editor_loop().await;
        let mut host = ExtensionHost::new();
        host.load(&path, &bus).await.unwrap();

        // Before unload: command exists.
        let exists_before = bus
            .invoke(|editor| editor.commands().get("hello.greet").is_some())
            .await
            .unwrap();
        assert!(exists_before);

        host.unload("hello", &bus).await.unwrap();

        // After unload: command is gone from the registry.
        let exists_after = bus
            .invoke(|editor| editor.commands().get("hello.greet").is_some())
            .await
            .unwrap();
        assert!(!exists_after);
        assert!(host.is_empty());

        drop(host);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn reload_same_dylib_swaps_cleanly() {
        // Load → reload → load-count is still 1, command still works.
        let Some(path) = ext_hello_path() else {
            return;
        };
        let (handle, bus) = spawn_editor_loop().await;
        let mut host = ExtensionHost::new();
        host.load(&path, &bus).await.unwrap();
        assert_eq!(host.len(), 1);

        // Reload the same path — the host should unload the old
        // instance (freeing its Library) and load a fresh one.
        host.reload(&path, &bus).await.unwrap();
        assert_eq!(host.len(), 1);

        // Command still works after the reload.
        let bus_clone = bus.clone();
        let text = bus
            .invoke(move |editor| {
                let cmd = editor.commands().get("hello.greet").unwrap();
                let mut cx = arx_core::CommandContext {
                    editor,
                    bus: bus_clone,
                    count: 1,
                };
                cmd.run(&mut cx);
                let id = cx.editor.windows().active().unwrap();
                let buf = cx.editor.windows().get(id).unwrap().buffer_id;
                cx.editor.buffers().get(buf).unwrap().text()
            })
            .await
            .unwrap();
        assert_eq!(text, "Hello from ext-hello!");

        host.unload_all(&bus).await.unwrap();
        drop(host);
        drop(bus);
        let _ = handle.await.unwrap();
    }

    #[tokio::test]
    async fn missing_symbol_reports_error() {
        // Loading a dylib that exists but doesn't look like an arx
        // extension (the host's own .so) surfaces as
        // MissingSymbol, not a panic.
        let Some(path) = ext_hello_path() else {
            return;
        };
        let bogus = path.with_file_name("definitely-not-an-arx-extension.so");
        let (handle, bus) = spawn_editor_loop().await;
        let mut host = ExtensionHost::new();
        let err = host.load(&bogus, &bus).await.unwrap_err();
        assert!(matches!(err, ExtHostError::Library { .. }));
        drop(host);
        drop(bus);
        let _ = handle.await.unwrap();
    }
}
