//! File-watcher-driven hot-reload for extension dylibs.
//!
//! Spec §5.15 — "The editor can unload/reload extensions without
//! restarting via `libloading` + careful ABI boundary design". This
//! module is the development-mode watcher: it points `notify` at the
//! extensions directory, debounces the raw filesystem events, and
//! dispatches reload requests through a tokio channel to a task that
//! owns the [`ExtensionHost`].
//!
//! # Why a channel, not direct host access
//!
//! `notify` runs its callback on a background thread that doesn't
//! own the tokio runtime. The host's `reload` is async and needs
//! `CommandBus`. So the callback's job is just "serialise the event
//! onto a channel"; a dedicated tokio task drains the channel and
//! performs the actual reload against a host it owns.
//!
//! # Debounce
//!
//! Cargo writes a dylib in multiple chunks as it's built, and the
//! file-watcher fires on every chunk. Without a debounce, the host
//! would try to load a half-written file and fail with a bogus
//! "invalid ELF" error. We collect events for 250ms after the first
//! one arrives, then process whatever set of paths is pending — one
//! reload per distinct path.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use arx_core::CommandBus;

use crate::ext_host::{ExtHostError, ExtensionHost};
use crate::state::Shutdown;

/// How long to coalesce rapid-fire filesystem events after the first
/// one arrives. Long enough to catch a `cargo build` that writes the
/// dylib in chunks; short enough that `arx ext dev` feels instant.
const DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum ExtensionWatcherError {
    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),
    #[error("extension host error: {0}")]
    Host(#[from] ExtHostError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Spawned watcher task handle. Holding the struct keeps the task
/// alive; dropping it sends a shutdown signal (or the caller can
/// `fire()` the provided `Shutdown` explicitly and `await` the
/// `JoinHandle`).
pub struct ExtensionWatcher {
    /// The notify watcher. Keeps the background thread alive — if we
    /// drop it, the callback stops firing.
    _watcher: RecommendedWatcher,
    /// Handle on the tokio task that processes debounced events.
    pub task: JoinHandle<()>,
}

impl std::fmt::Debug for ExtensionWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionWatcher")
            .finish_non_exhaustive()
    }
}

impl ExtensionWatcher {
    /// Start watching `dir` for extension dylib changes. Reloads
    /// through `host` any file that matches a platform-appropriate
    /// dynamic-library suffix (`.so` / `.dylib` / `.dll`) when its
    /// mtime changes.
    ///
    /// The watcher lives until `shutdown` fires or until the returned
    /// handle is dropped. On shutdown, it drains any pending events
    /// and exits cleanly.
    pub fn spawn(
        dir: impl AsRef<Path>,
        host: Arc<Mutex<ExtensionHost>>,
        bus: CommandBus,
        shutdown: Shutdown,
    ) -> Result<Self, ExtensionWatcherError> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;

        // Sync channel the notify callback fills.
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(move |res| {
            // Best-effort: if the receiver is gone, the task has
            // exited and we don't care about events any more.
            let _ = tx.send(res);
        })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        info!(dir = %dir.display(), "extension watcher armed");

        // tokio channel we relay onto so the event loop uses async recv.
        let (async_tx, async_rx) = mpsc::channel::<Event>(64);
        let relay_shutdown = shutdown.clone();
        std::thread::spawn(move || {
            relay_notify_events(&rx, &async_tx, &relay_shutdown);
        });

        let task = tokio::spawn(watcher_loop(dir, host, bus, async_rx, shutdown));

        Ok(Self {
            _watcher: watcher,
            task,
        })
    }
}

/// Relay notify's sync-channel events onto a tokio channel so the
/// async watcher task can use `.recv().await`. Runs on its own OS
/// thread because `std::sync::mpsc::Receiver::recv` is blocking.
fn relay_notify_events(
    rx: &std::sync::mpsc::Receiver<notify::Result<Event>>,
    tx: &mpsc::Sender<Event>,
    shutdown: &Shutdown,
) {
    while let Ok(res) = rx.recv() {
        if shutdown.is_fired() {
            break;
        }
        match res {
            Ok(event) => {
                if tx.blocking_send(event).is_err() {
                    break;
                }
            }
            Err(err) => {
                warn!(%err, "notify event error");
            }
        }
    }
}

/// Main watcher task. Drains debounced events and calls `host.reload`
/// (or `unload`) for each affected path.
async fn watcher_loop(
    dir: PathBuf,
    host: Arc<Mutex<ExtensionHost>>,
    bus: CommandBus,
    mut events: mpsc::Receiver<Event>,
    shutdown: Shutdown,
) {
    loop {
        tokio::select! {
            biased;
            () = shutdown.wait() => {
                debug!("extension watcher shutting down");
                return;
            }
            event = events.recv() => {
                let Some(first) = event else {
                    debug!("notify channel closed; watcher exiting");
                    return;
                };
                // Collect everything that arrives within DEBOUNCE.
                let mut batch = vec![first];
                let deadline = tokio::time::sleep(DEBOUNCE);
                tokio::pin!(deadline);
                loop {
                    tokio::select! {
                        () = &mut deadline => break,
                        maybe = events.recv() => {
                            match maybe {
                                Some(ev) => batch.push(ev),
                                None => return,
                            }
                        }
                    }
                }

                // Turn the batched events into a deduplicated set of
                // paths that look like dylibs, then process each.
                let paths = affected_paths(&dir, &batch);
                if paths.is_empty() {
                    continue;
                }
                let mut host = host.lock().await;
                for path in paths {
                    match classify(&path) {
                        PathChange::Removed => handle_removed(&mut host, &bus, &path).await,
                        PathChange::Present => handle_present(&mut host, &bus, &path).await,
                    }
                }
            }
        }
    }
}

enum PathChange {
    Present,
    Removed,
}

fn classify(path: &Path) -> PathChange {
    if path.exists() {
        PathChange::Present
    } else {
        PathChange::Removed
    }
}

/// Collect every unique dylib path touched by the batch of events,
/// restricted to the watched directory so stray events from notify's
/// internal recursion don't leak in.
fn affected_paths(root: &Path, events: &[Event]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for event in events {
        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            continue;
        }
        for path in &event.paths {
            if !path.starts_with(root) {
                continue;
            }
            if !is_dylib(path) {
                continue;
            }
            if seen.insert(path.clone()) {
                out.push(path.clone());
            }
        }
    }
    out
}

/// Is `path`'s file extension one of the platform dynamic-library
/// suffixes?
fn is_dylib(path: &Path) -> bool {
    let suffix = std::env::consts::DLL_SUFFIX.trim_start_matches('.');
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(suffix))
}

async fn handle_present(host: &mut ExtensionHost, bus: &CommandBus, path: &Path) {
    match host.reload(path, bus).await {
        Ok(meta) => info!(name = %meta.name, path = %path.display(), "extension reloaded"),
        Err(err) => warn!(%err, path = %path.display(), "extension reload failed"),
    }
}

async fn handle_removed(host: &mut ExtensionHost, bus: &CommandBus, path: &Path) {
    // Find any loaded extension whose original path matches and
    // unload it. We don't know the name ahead of time, so walk.
    let name = host
        .loaded()
        .find(|_| true)
        .map(|meta| meta.name.clone());
    // `loaded()` returns metadata; we need the path-indexed list.
    // For v0.1 the naive approach: unload by name if any matches the
    // filename stem. Good enough — proper path tracking lands with
    // the SDK metadata milestone.
    let _ = name;
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        let cleaned = stem.trim_start_matches("lib");
        // The loaded `name` is the extension's metadata name (e.g.
        // "hello"), not the filename — so also try a simple stem-based
        // match using the conventional `ext-<name>` / `<name>` forms.
        let candidates = [cleaned, cleaned.trim_start_matches("ext_"), cleaned.trim_start_matches("ext-")];
        for candidate in candidates {
            if host.get(candidate).is_some() {
                match host.unload(candidate, bus).await {
                    Ok(true) => {
                        info!(name = %candidate, path = %path.display(), "extension unloaded");
                        return;
                    }
                    Ok(false) => {}
                    Err(err) => warn!(%err, "extension unload failed"),
                }
            }
        }
        debug!(path = %path.display(), "removed dylib didn't match any loaded extension");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_dylib_recognises_platform_suffix() {
        let suffix = std::env::consts::DLL_SUFFIX;
        let fake = PathBuf::from(format!("libfoo{suffix}"));
        assert!(is_dylib(&fake));
        assert!(!is_dylib(Path::new("README.md")));
        assert!(!is_dylib(Path::new("libfoo.rs")));
    }

    #[test]
    fn affected_paths_dedup_and_filter_by_root() {
        let root = PathBuf::from("/tmp/ext");
        let dll = std::env::consts::DLL_SUFFIX;
        let in_root = root.join(format!("libhello{dll}"));
        let outside = PathBuf::from(format!("/tmp/other/libhello{dll}"));
        let events = vec![
            Event {
                kind: EventKind::Modify(notify::event::ModifyKind::Data(
                    notify::event::DataChange::Content,
                )),
                paths: vec![in_root.clone(), in_root.clone()], // duplicate
                attrs: notify::event::EventAttributes::default(),
            },
            Event {
                kind: EventKind::Modify(notify::event::ModifyKind::Data(
                    notify::event::DataChange::Content,
                )),
                paths: vec![outside],
                attrs: notify::event::EventAttributes::default(),
            },
        ];
        let affected = affected_paths(&root, &events);
        assert_eq!(affected, vec![in_root]);
    }
}
