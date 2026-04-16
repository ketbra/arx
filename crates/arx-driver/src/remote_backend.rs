//! [`RemoteBackend`]: a [`Backend`] that ships [`DiffOp`] batches to a
//! remote client over a channel.
//!
//! The render task runs *synchronously* (`Backend::apply` isn't async),
//! but shipping bytes to a socket is async. The two sides are stitched
//! together by a `tokio::sync::mpsc::UnboundedSender<Vec<DiffOp>>`: the
//! render task sends its batches in; a separate "writer task" drains
//! the channel and frames them onto the wire via
//! [`arx_protocol::write_frame`].
//!
//! This keeps the render task itself fully synchronous and free of any
//! IPC concerns — the exact same task drives the local
//! `CrosstermBackend` and the remote socket backend.
//!
//! The backend keeps track of the remote cursor state locally so that
//! [`Backend::size`] and `clear` behave sensibly, and so that a client
//! that reconnects can be re-bootstrapped with a full repaint later.

use std::io;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use arx_render::{Backend, BackendResult, DiffOp};

/// A [`Backend`] that forwards every `apply` batch into an
/// [`UnboundedSender`] for a writer task to serialise.
#[derive(Debug)]
pub struct RemoteBackend {
    width: u16,
    height: u16,
    tx: UnboundedSender<Vec<DiffOp>>,
}

impl RemoteBackend {
    /// Create a remote backend + its paired receiver.
    ///
    /// Spawn a task that consumes from the receiver and ships each
    /// `Vec<DiffOp>` as a framed [`DaemonMessage::RenderOps`].
    pub fn new(width: u16, height: u16) -> (Self, UnboundedReceiver<Vec<DiffOp>>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                width,
                height,
                tx,
            },
            rx,
        )
    }

    pub fn set_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }
}

impl Backend for RemoteBackend {
    fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn apply(&mut self, ops: &[DiffOp]) -> BackendResult<()> {
        if ops.is_empty() {
            return Ok(());
        }
        // A resize op in the middle of a batch updates our cached
        // terminal size too, so later non-resize ops in the batch are
        // bounded correctly for accessors.
        for op in ops {
            if let DiffOp::Resize { width, height } = op {
                self.width = *width;
                self.height = *height;
            }
        }
        self.tx.send(ops.to_vec()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "remote backend receiver closed",
            )
        })?;
        Ok(())
    }

    fn present(&mut self) -> BackendResult<()> {
        // Nothing buffered at this layer — the writer task flushes the
        // underlying socket on every frame.
        Ok(())
    }

    fn clear(&mut self) -> BackendResult<()> {
        // A "clear" is a resize to the same dimensions: issue a Resize
        // op so the client repaints from a blank grid.
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

    fn plain(c: &str) -> Cell {
        Cell {
            grapheme: CompactString::new(c),
            face: ResolvedFace::DEFAULT,
            flags: CellFlags::empty(),
        }
    }

    #[tokio::test]
    async fn apply_forwards_ops_through_channel() {
        let (mut backend, mut rx) = RemoteBackend::new(40, 10);
        backend
            .apply(&[DiffOp::SetCell {
                x: 0,
                y: 0,
                cell: plain("H"),
            }])
            .unwrap();
        backend
            .apply(&[DiffOp::SetCell {
                x: 1,
                y: 0,
                cell: plain("i"),
            }])
            .unwrap();

        let first = rx.recv().await.unwrap();
        assert_eq!(first.len(), 1);
        let second = rx.recv().await.unwrap();
        assert_eq!(second.len(), 1);
    }

    #[tokio::test]
    async fn apply_tracks_resize_ops() {
        let (mut backend, _rx) = RemoteBackend::new(40, 10);
        backend
            .apply(&[DiffOp::Resize {
                width: 120,
                height: 32,
            }])
            .unwrap();
        assert_eq!(backend.size(), (120, 32));
    }

    #[tokio::test]
    async fn apply_errors_when_receiver_dropped() {
        let (mut backend, rx) = RemoteBackend::new(40, 10);
        drop(rx);
        let res = backend.apply(&[DiffOp::SetCell {
            x: 0,
            y: 0,
            cell: plain("X"),
        }]);
        assert!(res.is_err());
    }
}
