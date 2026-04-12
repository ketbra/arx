//! Protocol message types.
//!
//! These are the only things that cross the Unix socket. Bump
//! [`PROTOCOL_VERSION`] whenever their wire format changes — the
//! daemon refuses `Hello` messages whose version doesn't match.

use arx_keymap::KeyChord;
use arx_render::DiffOp;
use serde::{Deserialize, Serialize};

/// Wire protocol version. Clients and the daemon must agree; bump on
/// any non-additive change to these types.
pub const PROTOCOL_VERSION: u32 = 1;

/// The opening handshake payload. Sent as the body of a
/// [`ClientMessage::Hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloInfo {
    /// Protocol version the client speaks.
    pub protocol_version: u32,
    /// Optional client-chosen identifier (for logging / multi-client
    /// attribution). May be the empty string.
    pub client_id: String,
    /// Initial terminal size in cells.
    pub cols: u16,
    pub rows: u16,
}

/// Messages sent from the client to the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Initial handshake. MUST be the first message on the connection.
    Hello(HelloInfo),
    /// One keystroke.
    Key(KeyChord),
    /// Terminal was resized. Daemon re-renders at the new dimensions.
    Resize { cols: u16, rows: u16 },
    /// Client is disconnecting cleanly. Daemon should flush and close.
    Goodbye,
}

/// Why the daemon is sending a [`DaemonMessage::Shutdown`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShutdownReason {
    /// The active buffer's `editor.quit` command was invoked.
    UserQuit,
    /// The daemon process is being torn down externally.
    DaemonExit,
    /// The protocol version didn't match.
    VersionMismatch { daemon_version: u32 },
    /// Some other reason — free-text for diagnostics.
    Other(String),
}

/// Messages sent from the daemon to the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonMessage {
    /// Handshake reply. Sent in response to [`ClientMessage::Hello`].
    Welcome {
        /// Protocol version the daemon speaks.
        protocol_version: u32,
        /// Session id the client was attached to. For Phase 1 this is
        /// always 1 (the implicit default session).
        session_id: u64,
    },
    /// Apply these diff ops to the terminal.
    ///
    /// Batching means one socket send can carry a whole frame. The
    /// daemon groups ops by frame before flushing so clients see
    /// atomic updates.
    RenderOps(Vec<DiffOp>),
    /// Ring the terminal bell.
    Bell,
    /// The daemon is shutting down this connection. The client should
    /// restore its terminal and exit.
    Shutdown(ShutdownReason),
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_keymap::{Key, KeyModifiers};

    #[test]
    fn client_message_roundtrip_postcard() {
        let msg = ClientMessage::Hello(HelloInfo {
            protocol_version: PROTOCOL_VERSION,
            client_id: "test-client".into(),
            cols: 120,
            rows: 40,
        });
        let bytes = postcard::to_stdvec(&msg).unwrap();
        let decoded: ClientMessage = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn key_message_roundtrip() {
        let msg = ClientMessage::Key(KeyChord {
            key: Key::Char('x'),
            modifiers: KeyModifiers::CTRL,
        });
        let bytes = postcard::to_stdvec(&msg).unwrap();
        let decoded: ClientMessage = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn daemon_message_welcome_roundtrip() {
        let msg = DaemonMessage::Welcome {
            protocol_version: PROTOCOL_VERSION,
            session_id: 1,
        };
        let bytes = postcard::to_stdvec(&msg).unwrap();
        let decoded: DaemonMessage = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn daemon_message_render_ops_roundtrip() {
        use arx_render::{Cell, CellFlags, CursorRender, CursorStyle, ResolvedFace};
        use compact_str::CompactString;

        let ops = vec![
            DiffOp::Resize { width: 80, height: 24 },
            DiffOp::SetCell {
                x: 3,
                y: 1,
                cell: Cell {
                    grapheme: CompactString::new("h"),
                    face: ResolvedFace::DEFAULT,
                    flags: CellFlags::empty(),
                },
            },
            DiffOp::MoveCursor(CursorRender {
                col: 4,
                row: 1,
                style: CursorStyle::Block,
            }),
            DiffOp::HideCursor,
        ];
        let msg = DaemonMessage::RenderOps(ops);
        let bytes = postcard::to_stdvec(&msg).unwrap();
        let decoded: DaemonMessage = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }
}
