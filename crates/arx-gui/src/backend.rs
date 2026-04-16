//! [`GpuBackend`]: a channel-based [`Backend`] that ships render ops
//! from the editor's tokio thread to the main thread's winit loop.
//!
//! This is structurally identical to
//! [`arx_driver::RemoteBackend`] — neither actually draws anything. It
//! just forwards [`DiffOp`] batches over a channel. The winit side
//! maintains its own [`arx_render::TestBackend`] mirror, applies the
//! same ops to it, and then rasterises with wgpu.
//!
//! ### Wake-up mechanism
//!
//! Sending a frame over the channel doesn't, by itself, wake the winit
//! event loop. After every successful `apply` we poke the event loop
//! via a user-supplied `wake` callback — in practice,
//! [`winit::event_loop::EventLoopProxy::send_event`]. That way the main
//! thread goes to sleep on winit's native wait and doesn't need to
//! poll the channel.

use std::sync::{Arc, mpsc};

use arx_render::{Backend, BackendError, BackendResult, DiffOp};

/// A single batch of [`DiffOp`]s produced by one render-task tick.
#[derive(Debug, Clone)]
pub struct BackendFrame {
    pub ops: Vec<DiffOp>,
}

/// Type-erased "poke the event loop" callback.
///
/// In production this is `move |_| proxy.send_event(UserEvent::Wake)`.
/// Tests pass a no-op or a flag-setter.
pub type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// Channel-backed [`Backend`] that sits on the editor's render task.
///
/// The paired receiver lives on the main (winit) thread.
pub struct GpuBackend {
    width: u16,
    height: u16,
    tx: mpsc::Sender<BackendFrame>,
    wake: WakeFn,
}

impl std::fmt::Debug for GpuBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuBackend")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl GpuBackend {
    /// Build a [`GpuBackend`] paired with a receiver.
    ///
    /// The `wake` callback is invoked after every non-empty batch is
    /// pushed onto the channel.
    pub fn new(
        width: u16,
        height: u16,
        wake: WakeFn,
    ) -> (Self, mpsc::Receiver<BackendFrame>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                width,
                height,
                tx,
                wake,
            },
            rx,
        )
    }

    /// Update the cached cell size (called from the driver when the
    /// winit window resizes).
    pub fn set_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }
}

impl Backend for GpuBackend {
    fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn apply(&mut self, ops: &[DiffOp]) -> BackendResult<()> {
        if ops.is_empty() {
            return Ok(());
        }
        for op in ops {
            if let DiffOp::Resize { width, height } = op {
                self.width = *width;
                self.height = *height;
            }
        }
        self.tx
            .send(BackendFrame { ops: ops.to_vec() })
            .map_err(|_| BackendError::other("GUI frame receiver dropped"))?;
        (self.wake)();
        Ok(())
    }

    fn present(&mut self) -> BackendResult<()> {
        // The winit side is free to coalesce multiple batches into one
        // rasterised frame, so we don't force a present here.
        Ok(())
    }

    fn clear(&mut self) -> BackendResult<()> {
        // Request a full repaint on the winit side by sending a
        // `Resize` to the same dimensions, mirroring
        // `RemoteBackend::clear`.
        self.apply(&[DiffOp::Resize {
            width: self.width,
            height: self.height,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_render::{Cell, CellFlags, ResolvedFace};
    use compact_str::CompactString;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn cell(ch: &str) -> Cell {
        Cell {
            grapheme: CompactString::new(ch),
            face: ResolvedFace::DEFAULT,
            flags: CellFlags::empty(),
        }
    }

    #[test]
    fn apply_forwards_ops_and_wakes() {
        let pokes = Arc::new(AtomicUsize::new(0));
        let pokes_clone = pokes.clone();
        let wake: WakeFn = Arc::new(move || {
            pokes_clone.fetch_add(1, Ordering::SeqCst);
        });

        let (mut backend, rx) = GpuBackend::new(20, 5, wake);
        backend
            .apply(&[DiffOp::SetCell {
                x: 0,
                y: 0,
                cell: cell("H"),
            }])
            .unwrap();
        backend
            .apply(&[DiffOp::SetCell {
                x: 1,
                y: 0,
                cell: cell("i"),
            }])
            .unwrap();

        assert_eq!(pokes.load(Ordering::SeqCst), 2);
        let frame1 = rx.try_recv().unwrap();
        assert_eq!(frame1.ops.len(), 1);
        let frame2 = rx.try_recv().unwrap();
        assert_eq!(frame2.ops.len(), 1);
    }

    #[test]
    fn empty_apply_does_not_wake() {
        let pokes = Arc::new(AtomicUsize::new(0));
        let pokes_clone = pokes.clone();
        let wake: WakeFn = Arc::new(move || {
            pokes_clone.fetch_add(1, Ordering::SeqCst);
        });

        let (mut backend, _rx) = GpuBackend::new(20, 5, wake);
        backend.apply(&[]).unwrap();
        assert_eq!(pokes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn resize_updates_cached_size() {
        let wake: WakeFn = Arc::new(|| {});
        let (mut backend, _rx) = GpuBackend::new(20, 5, wake);
        backend
            .apply(&[DiffOp::Resize {
                width: 40,
                height: 10,
            }])
            .unwrap();
        assert_eq!(backend.size(), (40, 10));
    }

    #[test]
    fn apply_errors_when_receiver_dropped() {
        let wake: WakeFn = Arc::new(|| {});
        let (mut backend, rx) = GpuBackend::new(20, 5, wake);
        drop(rx);
        let result = backend.apply(&[DiffOp::SetCell {
            x: 0,
            y: 0,
            cell: cell("X"),
        }]);
        assert!(result.is_err());
    }
}
