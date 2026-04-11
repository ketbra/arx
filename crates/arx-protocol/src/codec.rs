//! Length-prefixed framing over any [`AsyncRead`]/[`AsyncWrite`].
//!
//! Frame layout:
//!
//! ```text
//! ┌──────────────┬────────────────────────┐
//! │  u32 BE len  │  `postcard`-encoded msg │
//! └──────────────┴────────────────────────┘
//! ```
//!
//! [`read_frame`] reads exactly one frame, deserialises the body, and
//! returns a typed message. [`write_frame`] encodes, writes the length
//! prefix, writes the body, and flushes. Both are cancel-safe at
//! frame boundaries — a cancelled read either produces a full message
//! or leaves the stream unchanged (as long as the caller discards the
//! half-read stream, which they will, because IPC errors kill the
//! connection).

use std::io;

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Hard upper bound on a single frame's body size. Prevents a malicious
/// or confused peer from asking us to allocate 4 GB. 16 MiB is enough
/// for a truly gigantic render-ops batch on a 500×200 terminal.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Errors from framing / deserialisation.
#[derive(Debug, Error)]
pub enum FrameError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("postcard (de)serialisation error: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("peer sent a frame of {len} bytes, larger than the {max}-byte limit")]
    FrameTooLarge { len: usize, max: usize },
    #[error("stream closed before the frame header was fully read")]
    UnexpectedEof,
}

/// Read and decode one frame.
pub async fn read_frame<R, M>(reader: &mut R) -> Result<M, FrameError>
where
    R: AsyncRead + Unpin,
    M: DeserializeOwned,
{
    // Read the u32 big-endian length prefix.
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FrameError::UnexpectedEof);
        }
        Err(e) => return Err(FrameError::Io(e)),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::FrameTooLarge {
            len,
            max: MAX_FRAME_BYTES,
        });
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    let msg: M = postcard::from_bytes(&body)?;
    Ok(msg)
}

/// Encode and write one frame, then flush.
pub async fn write_frame<W, M>(writer: &mut W, msg: &M) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    M: Serialize,
{
    let body = postcard::to_stdvec(msg)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(FrameError::FrameTooLarge {
            len: body.len(),
            max: MAX_FRAME_BYTES,
        });
    }
    let len_buf = (body.len() as u32).to_be_bytes();
    writer.write_all(&len_buf).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ClientMessage, HelloInfo, PROTOCOL_VERSION};
    use tokio::io::duplex;

    #[tokio::test]
    async fn round_trip_single_frame() {
        let (mut a, mut b) = duplex(1024);
        let msg = ClientMessage::Hello(HelloInfo {
            protocol_version: PROTOCOL_VERSION,
            client_id: "tests".into(),
            cols: 80,
            rows: 24,
        });
        write_frame(&mut a, &msg).await.unwrap();
        let decoded: ClientMessage = read_frame(&mut b).await.unwrap();
        assert_eq!(decoded, msg);
    }

    #[tokio::test]
    async fn round_trip_multiple_frames() {
        let (mut a, mut b) = duplex(4096);
        let msgs = vec![
            ClientMessage::Goodbye,
            ClientMessage::Resize {
                cols: 120,
                rows: 40,
            },
            ClientMessage::Hello(HelloInfo {
                protocol_version: PROTOCOL_VERSION,
                client_id: "multi".into(),
                cols: 100,
                rows: 30,
            }),
        ];
        for m in &msgs {
            write_frame(&mut a, m).await.unwrap();
        }
        for m in &msgs {
            let decoded: ClientMessage = read_frame(&mut b).await.unwrap();
            assert_eq!(&decoded, m);
        }
    }

    #[tokio::test]
    async fn eof_before_header_errors() {
        let (a, mut b) = duplex(1024);
        drop(a); // close the write side
        let res: Result<ClientMessage, _> = read_frame(&mut b).await;
        assert!(matches!(res, Err(FrameError::UnexpectedEof)));
    }

    #[tokio::test]
    async fn oversize_frame_rejected_by_reader() {
        use tokio::io::AsyncWriteExt;
        let (mut a, mut b) = duplex(1024);
        // Synthesise a bogus 2 GiB header.
        let mut header = Vec::new();
        header.extend_from_slice(&(u32::MAX).to_be_bytes());
        a.write_all(&header).await.unwrap();
        let res: Result<ClientMessage, _> = read_frame(&mut b).await;
        assert!(matches!(
            res,
            Err(FrameError::FrameTooLarge { max: MAX_FRAME_BYTES, .. })
        ));
    }
}
