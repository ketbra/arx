//! Small bits of state shared between driver tasks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

/// Shared cell of the current terminal size.
///
/// The input task and the render task both need to see the current
/// terminal size — the former gets it from `crossterm::event::Event::Resize`,
/// the latter needs it to build a [`arx_render::ViewState`]. Rather than
/// going through the event loop for something so small, we keep it in a
/// cheap `Arc<Mutex>`. The value updates ~never (just on resize) so
/// contention is a non-issue.
#[derive(Debug, Clone)]
pub struct SharedTerminalSize {
    inner: Arc<Mutex<(u16, u16)>>,
}

impl SharedTerminalSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            inner: Arc::new(Mutex::new((cols, rows))),
        }
    }

    pub fn set(&self, cols: u16, rows: u16) {
        *self.inner.lock().unwrap() = (cols, rows);
    }

    pub fn get(&self) -> (u16, u16) {
        *self.inner.lock().unwrap()
    }
}

/// A latched shutdown signal.
///
/// Unlike a bare [`tokio::sync::Notify`], `Shutdown` stores a sticky
/// "fired" bit so that waiters which arrive **after** the signal is
/// fired still observe it. This is the race the raw `Notify` pattern has:
/// `notify_waiters` wakes only waiters that are already registered, and
/// doesn't store a permit for future waiters.
///
/// Usage:
/// ```ignore
/// let s = Shutdown::new();
/// let s2 = s.clone();
/// tokio::spawn(async move { s2.wait().await; /* ...clean up... */ });
/// s.fire();
/// ```
#[derive(Debug, Clone)]
pub struct Shutdown {
    inner: Arc<ShutdownInner>,
}

#[derive(Debug)]
struct ShutdownInner {
    fired: AtomicBool,
    notify: Notify,
}

impl Shutdown {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ShutdownInner {
                fired: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    /// Fire the shutdown signal. Idempotent.
    pub fn fire(&self) {
        self.inner.fired.store(true, Ordering::Release);
        // Wake anyone currently awaiting; the atomic flag above catches
        // callers who arrive after this point.
        self.inner.notify.notify_waiters();
    }

    /// Check whether the signal has already fired (non-blocking).
    pub fn is_fired(&self) -> bool {
        self.inner.fired.load(Ordering::Acquire)
    }

    /// Wait until the signal fires. Returns immediately if already fired.
    ///
    /// Race-free because we register as a `Notify` waiter (`notified()`)
    /// **before** checking the atomic: if `fire()` happens between the
    /// two steps, our registered waiter is woken on the next poll; if it
    /// happened before, the atomic already reports true.
    pub async fn wait(&self) {
        loop {
            let waiter = self.inner.notify.notified();
            if self.is_fired() {
                return;
            }
            waiter.await;
            if self.is_fired() {
                return;
            }
        }
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn already_fired_returns_immediately() {
        let s = Shutdown::new();
        s.fire();
        tokio::time::timeout(std::time::Duration::from_millis(10), s.wait())
            .await
            .expect("should return immediately");
    }

    #[tokio::test]
    async fn waiter_registered_first_sees_fire() {
        let s = Shutdown::new();
        let s2 = s.clone();
        let wait = tokio::spawn(async move { s2.wait().await });
        tokio::task::yield_now().await;
        s.fire();
        tokio::time::timeout(std::time::Duration::from_millis(100), wait)
            .await
            .expect("wait should resolve")
            .unwrap();
    }

    #[tokio::test]
    async fn waiter_registered_after_fire_still_sees_it() {
        let s = Shutdown::new();
        s.fire();
        let s2 = s.clone();
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            tokio::spawn(async move { s2.wait().await }),
        )
        .await
        .expect("late waiter should return immediately")
        .unwrap();
    }
}
