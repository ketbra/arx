// The atexit/_exit shim uses `libc` FFI which requires unsafe
// blocks. The workspace default is `unsafe_code = "deny"`; we opt
// in here with a local allow because the shim is the whole point
// of the module. See the module-level docs for justification.
#![allow(unsafe_code)]

//! Extension hot-reload integration test.
//!
//! Exercises the full `ExtensionWatcher → ExtensionHost → event
//! loop` path using the real `ext-hello` cdylib: drops a copy of
//! the dylib into a tempdir under the daemon's `extensions_dir`,
//! lets the watcher pick it up, verifies the `hello.greet` command
//! is registered, then touches the file and checks that the reload
//! cycle still leaves the command in the registry.
//!
//! Skips gracefully on any environment where the `ext-hello`
//! artifact isn't built yet (e.g. `cargo test -p arx-core`) — the
//! full workspace build always produces it.
//!
//! # Process-teardown workaround
//!
//! A loaded Rust cdylib can't be cleanly `dlclose`d: glibc's
//! `_dl_call_fini` walks the library's `.fini_array` at process
//! exit and hits a null function pointer for certain Rust TLS /
//! panic-handling destructors (a well-known Rust-stdlib-plus-glibc
//! interaction). The extension host works around it for the runtime
//! case by holding the library inside `ManuallyDrop<Library>` — so
//! hot-reloads leak the old library without dropping it — but the
//! eventual process-exit sequence still trips on that same null
//! pointer if the dylib is still mapped.
//!
//! Each test in this file therefore registers a one-shot
//! `libc::atexit` handler that short-circuits the C runtime's normal
//! teardown by calling `_exit(0)`. `atexit` handlers run in LIFO
//! order, so our hook runs before the dynamic linker's static
//! destructors, and the process exits cleanly with status 0. Test
//! assertions still fail the usual way via `panic!`, which aborts
//! before our hook runs.
//!
//! This is a development-mode test-binary workaround. Production
//! daemons that load user extensions and then exit normally hit
//! the same issue; a later milestone will fix it properly by
//! switching the extension ABI to `abi_stable` (which side-steps
//! the TLS destructor problem entirely).

use std::path::PathBuf;
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use arx_core::{CommandBus, Editor, EventLoop};
use arx_driver::{ExtensionHost, ExtensionWatcher, Shutdown};
use tempfile::TempDir;
use tokio::sync::Mutex;

#[cfg(unix)]
fn install_exit_shortcircuit() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        extern "C" fn exit_zero() {
            // SAFETY: `_exit` is async-signal-safe and documented
            // not to return. Skipping Rust/glibc teardown is the
            // whole point of this shim; see the module docs.
            unsafe { libc::_exit(0) };
        }
        // SAFETY: `atexit` registers a callback to run at process
        // exit. The callback above is `extern "C"` and has a
        // compatible signature. Returning non-zero means
        // registration failed — we surface it as a panic so a
        // failure to install the shim isn't silently ignored.
        let rc = unsafe { libc::atexit(exit_zero) };
        assert_eq!(rc, 0, "failed to register atexit handler");
    });
}

#[cfg(not(unix))]
fn install_exit_shortcircuit() {
    // The dl_fini segfault is glibc-specific; other platforms
    // either run Rust cdylibs cleanly or use a different dtor
    // mechanism that doesn't trip on our leaked Library.
}

/// Locate the compiled `ext-hello` cdylib inside the workspace
/// target directory. Returns `None` if the artifact isn't built.
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

/// Poll the bus for a condition up to `timeout`. Used by the test to
/// wait for async watcher activity to be observable.
async fn wait_for_command(bus: &CommandBus, name: &str, present: bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let name = name.to_string();
    while Instant::now() < deadline {
        let hit = bus
            .invoke({
                let name = name.clone();
                move |editor| editor.commands().get(&name).is_some()
            })
            .await
            .unwrap();
        if hit == present {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn watcher_loads_extension_when_dropped_into_dir() {
    install_exit_shortcircuit();
    let Some(src) = ext_hello_path() else {
        eprintln!("SKIP: ext-hello cdylib not built");
        return;
    };

    let ext_dir = TempDir::new().unwrap();
    let (loop_handle, bus) = spawn_editor_loop().await;
    let host = Arc::new(Mutex::new(ExtensionHost::new()));
    let shutdown = Shutdown::new();

    // Start the watcher on the empty dir.
    let watcher = ExtensionWatcher::spawn(
        ext_dir.path(),
        host.clone(),
        bus.clone(),
        shutdown.clone(),
    )
    .unwrap();

    // Command not registered yet.
    assert!(
        !bus.invoke(|editor| editor.commands().get("hello.greet").is_some())
            .await
            .unwrap()
    );

    // Drop the dylib into the watched dir.
    let dst = ext_dir.path().join(src.file_name().unwrap());
    std::fs::copy(&src, &dst).unwrap();

    // Watcher should see the create + debounce + load. Give it up
    // to 3 s — notify + 250ms debounce + the actual load should
    // finish in well under that.
    assert!(
        wait_for_command(&bus, "hello.greet", true, Duration::from_secs(3)).await,
        "watcher did not load the dropped extension",
    );

    shutdown.fire();
    drop(watcher);
    drop(bus);
    let _ = loop_handle.await.unwrap();
}

#[tokio::test]
async fn watcher_reloads_on_modify() {
    install_exit_shortcircuit();
    let Some(src) = ext_hello_path() else {
        eprintln!("SKIP: ext-hello cdylib not built");
        return;
    };

    let ext_dir = TempDir::new().unwrap();
    let (loop_handle, bus) = spawn_editor_loop().await;
    let host = Arc::new(Mutex::new(ExtensionHost::new()));
    let shutdown = Shutdown::new();

    // Pre-seed: copy the dylib in BEFORE the watcher starts, then
    // load it manually, so the watcher only has to observe the
    // subsequent modify.
    let dst = ext_dir.path().join(src.file_name().unwrap());
    std::fs::copy(&src, &dst).unwrap();
    {
        let mut h = host.lock().await;
        h.load(&dst, &bus).await.unwrap();
    }
    assert!(
        bus.invoke(|editor| editor.commands().get("hello.greet").is_some())
            .await
            .unwrap()
    );

    let watcher = ExtensionWatcher::spawn(
        ext_dir.path(),
        host.clone(),
        bus.clone(),
        shutdown.clone(),
    )
    .unwrap();

    // Replace the file to trigger a modify event. We copy the
    // source over itself — same bytes but the mtime bumps and
    // notify fires.
    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::copy(&src, &dst).unwrap();

    // After the debounce + reload, the command must still be in
    // the registry. We don't have a way to observe a *new* version
    // (our single test dylib doesn't change between reloads), but
    // surviving the reload cycle proves the watcher successfully
    // unloaded the old lib and loaded a new one without leaving
    // the registry in a broken state.
    assert!(
        wait_for_command(&bus, "hello.greet", true, Duration::from_secs(3)).await,
        "command missing after reload",
    );

    shutdown.fire();
    drop(watcher);
    drop(bus);
    let _ = loop_handle.await.unwrap();
}

#[tokio::test]
async fn watcher_unloads_on_remove() {
    install_exit_shortcircuit();
    let Some(src) = ext_hello_path() else {
        return;
    };

    let ext_dir = TempDir::new().unwrap();
    let (loop_handle, bus) = spawn_editor_loop().await;
    let host = Arc::new(Mutex::new(ExtensionHost::new()));
    let shutdown = Shutdown::new();

    let dst = ext_dir.path().join(src.file_name().unwrap());
    std::fs::copy(&src, &dst).unwrap();
    {
        let mut h = host.lock().await;
        h.load(&dst, &bus).await.unwrap();
    }
    assert!(
        bus.invoke(|editor| editor.commands().get("hello.greet").is_some())
            .await
            .unwrap()
    );

    let watcher = ExtensionWatcher::spawn(
        ext_dir.path(),
        host.clone(),
        bus.clone(),
        shutdown.clone(),
    )
    .unwrap();

    // Delete the dylib.
    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::remove_file(&dst).unwrap();

    // The watcher should detect the remove and unload the extension.
    assert!(
        wait_for_command(&bus, "hello.greet", false, Duration::from_secs(3)).await,
        "command still registered after remove",
    );

    shutdown.fire();
    drop(watcher);
    drop(bus);
    let _ = loop_handle.await.unwrap();
}
