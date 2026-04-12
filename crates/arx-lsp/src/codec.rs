//! JSON-RPC 2.0 framing over the LSP base protocol.
//!
//! The LSP wire format is dead simple: each message is a UTF-8 JSON
//! body preceded by HTTP-style headers. In practice the only header
//! that matters is `Content-Length: N\r\n\r\n`, where N is the byte
//! length of the JSON body that follows.
//!
//! This module provides:
//!
//! * [`encode`] — serialize a JSON value into a length-prefixed byte
//!   vector ready for writing to the server's stdin.
//! * [`FrameReader`] — an async reader that pulls complete JSON bodies
//!   out of a byte stream (the server's stdout), handling partial
//!   reads and multi-message buffering.

use std::io;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

/// Encode a JSON-RPC message into the LSP base protocol wire format.
pub fn encode(body: &serde_json::Value) -> Vec<u8> {
    let json = serde_json::to_string(body).expect("Value is always serializable");
    let header = format!("Content-Length: {}\r\n\r\n", json.len());
    let mut out = Vec::with_capacity(header.len() + json.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(json.as_bytes());
    out
}

/// Async reader that decodes length-prefixed JSON-RPC messages from a
/// byte stream. Wraps a `BufReader<R>` and yields one
/// `serde_json::Value` per call to [`FrameReader::read_message`].
#[derive(Debug)]
pub struct FrameReader<R> {
    reader: BufReader<R>,
}

impl<R: tokio::io::AsyncRead + Unpin> FrameReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
        }
    }

    /// Read the next complete JSON-RPC message. Returns `Ok(None)` on
    /// clean EOF (the server closed stdout). Returns `Err` on I/O
    /// errors or malformed framing.
    pub async fn read_message(&mut self) -> io::Result<Option<serde_json::Value>> {
        // Parse headers until we see the blank line.
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).await?;
            if n == 0 {
                // EOF — server closed stdout.
                return Ok(None);
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                // End of headers.
                break;
            }
            if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                content_length = value.trim().parse::<usize>().ok();
            }
            // Ignore unknown headers (e.g. Content-Type).
        }
        let length = content_length.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length header")
        })?;
        let mut body = vec![0u8; length];
        self.reader.read_exact(&mut body).await?;
        let value: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("invalid JSON: {e}"))
        })?;
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_produces_valid_frame() {
        let body = serde_json::json!({"jsonrpc": "2.0", "method": "test"});
        let frame = encode(&body);
        let frame_str = std::str::from_utf8(&frame).unwrap();
        assert!(frame_str.starts_with("Content-Length: "));
        let parts: Vec<&str> = frame_str.splitn(2, "\r\n\r\n").collect();
        assert_eq!(parts.len(), 2);
        let declared_len: usize = parts[0]
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(declared_len, parts[1].len());
        let parsed: serde_json::Value = serde_json::from_str(parts[1]).unwrap();
        assert_eq!(parsed["method"], "test");
    }

    #[tokio::test]
    async fn frame_reader_decodes_single_message() {
        let body = serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": null});
        let frame = encode(&body);
        let cursor = std::io::Cursor::new(frame);
        let mut reader = FrameReader::new(cursor);
        let msg = reader.read_message().await.unwrap().unwrap();
        assert_eq!(msg["id"], 1);
    }

    #[tokio::test]
    async fn frame_reader_decodes_two_concatenated_messages() {
        let a = serde_json::json!({"id": 1});
        let b = serde_json::json!({"id": 2});
        let mut data = encode(&a);
        data.extend_from_slice(&encode(&b));
        let cursor = std::io::Cursor::new(data);
        let mut reader = FrameReader::new(cursor);
        assert_eq!(reader.read_message().await.unwrap().unwrap()["id"], 1);
        assert_eq!(reader.read_message().await.unwrap().unwrap()["id"], 2);
    }

    #[tokio::test]
    async fn frame_reader_returns_none_on_eof() {
        let cursor = std::io::Cursor::new(Vec::<u8>::new());
        let mut reader = FrameReader::new(cursor);
        assert!(reader.read_message().await.unwrap().is_none());
    }
}
