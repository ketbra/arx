//! End-to-end daemon + IPC test.
//!
//! Spawns a real [`DaemonServer`] bound to a per-test IPC endpoint
//! (Unix domain socket on Unix, named pipe on Windows), connects a
//! synthetic client that speaks the wire protocol directly (not
//! [`arx_driver::DaemonClient`] — we don't want to grab the real
//! terminal in a test), sends a handshake + a couple of key events,
//! and asserts:
//!
//! * the daemon replies with a `Welcome`;
//! * the daemon ships at least one `RenderOps` frame reflecting a
//!   typed character;
//! * the daemon tears the session down cleanly when the client sends
//!   `Goodbye` — but keeps the Editor state (next connection sees it).

use std::path::Path;
use std::time::Duration;

use arx_core::Editor;
use arx_driver::DaemonServer;
use arx_keymap::{Key, KeyChord, KeyModifiers};
use arx_protocol::{
    ClientMessage, DaemonMessage, HelloInfo, IpcAddress, IpcStream, PROTOCOL_VERSION, read_frame,
    write_frame,
};
use arx_render::DiffOp;
use tempfile::TempDir;

fn seeded_editor() -> Editor {
    let mut editor = Editor::new();
    let buf = editor.buffers_mut().create_from_text("hi", None);
    editor.windows_mut().open(buf);
    editor
}

/// Pick an IPC address unique to this test / pid so parallel tests
/// can't collide on the same pipe name.
#[cfg(unix)]
fn test_address(dir: &Path, tag: &str) -> IpcAddress {
    IpcAddress::Path(dir.join(format!("arx-test-{tag}.sock")))
}

#[cfg(windows)]
fn test_address(_dir: &Path, tag: &str) -> IpcAddress {
    let pid = std::process::id();
    IpcAddress::Pipe(format!(r"\\.\pipe\arx-test-{tag}-{pid}"))
}

async fn connect(address: &IpcAddress) -> IpcStream {
    for _ in 0..20 {
        if let Ok(s) = IpcStream::connect(address).await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon endpoint never appeared at {address}");
}

#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn handshake_and_render_round_trip() {
    let dir = TempDir::new().unwrap();
    let address = test_address(dir.path(), "roundtrip");

    // Spawn the daemon.
    let server = DaemonServer::bind(address.clone(), seeded_editor()).unwrap();
    let daemon_handle = tokio::spawn(async move {
        // run() loops forever; we abort it when the test ends.
        let _ = server.run().await;
    });

    // Connect a test client.
    let stream = connect(&address).await;
    let (mut reader, mut writer) = stream.into_split();

    // Handshake: Hello → Welcome.
    write_frame(
        &mut writer,
        &ClientMessage::Hello(HelloInfo {
            protocol_version: PROTOCOL_VERSION,
            client_id: "test".into(),
            cols: 40,
            rows: 5,
        }),
    )
    .await
    .unwrap();
    let welcome: DaemonMessage = read_frame(&mut reader).await.unwrap();
    assert!(matches!(
        welcome,
        DaemonMessage::Welcome {
            protocol_version: PROTOCOL_VERSION,
            ..
        }
    ));

    // The daemon should ship an initial RenderOps frame (from the
    // render task's first draw, triggered by the Editor::mark_dirty
    // inside `handle_client`). Give the render task a moment to catch
    // up and collect the first ops batch.
    let first_ops = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            let msg: DaemonMessage = read_frame(&mut reader).await.unwrap();
            if let DaemonMessage::RenderOps(ops) = msg {
                return ops;
            }
        }
    })
    .await
    .expect("initial render frame");
    assert!(
        !first_ops.is_empty(),
        "daemon shipped an empty initial frame"
    );

    // Send a self-inserting 'X'. The Emacs profile leaves 'X' unbound
    // so it falls through to the self-insert fallback.
    write_frame(
        &mut writer,
        &ClientMessage::Key(KeyChord {
            key: Key::Char('X'),
            modifiers: KeyModifiers::NONE,
        }),
    )
    .await
    .unwrap();

    // Wait for at least one RenderOps frame containing an 'X' cell.
    let found_x = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let msg: DaemonMessage = read_frame(&mut reader).await.unwrap();
            let DaemonMessage::RenderOps(ops) = msg else {
                continue;
            };
            for op in ops {
                if let DiffOp::SetCell { cell, .. } = op {
                    if cell.grapheme.as_str() == "X" {
                        return true;
                    }
                }
            }
        }
    })
    .await
    .expect("X should appear in render ops");
    assert!(found_x);

    // Clean client shutdown.
    write_frame(&mut writer, &ClientMessage::Goodbye).await.unwrap();
    drop(writer);
    drop(reader);

    // The daemon keeps running, ready for another connection.
    // Reconnect and verify state persisted (buffer still contains "Xhi").
    let stream2 = connect(&address).await;
    let (mut reader2, mut writer2) = stream2.into_split();
    write_frame(
        &mut writer2,
        &ClientMessage::Hello(HelloInfo {
            protocol_version: PROTOCOL_VERSION,
            client_id: "test-2".into(),
            cols: 40,
            rows: 5,
        }),
    )
    .await
    .unwrap();
    // Read the Welcome, then watch the initial render for "Xhi".
    let _welcome: DaemonMessage = read_frame(&mut reader2).await.unwrap();
    let saw_state = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let msg: DaemonMessage = read_frame(&mut reader2).await.unwrap();
            if let DaemonMessage::RenderOps(ops) = msg {
                let mut row0 = String::new();
                for op in ops {
                    if let DiffOp::SetCell { y, cell, .. } = op {
                        if y == 0 {
                            row0.push_str(cell.grapheme.as_str());
                        }
                    }
                }
                if row0.contains("Xhi") {
                    return true;
                }
            }
        }
    })
    .await
    .expect("state should persist across reconnects");
    assert!(saw_state);

    write_frame(&mut writer2, &ClientMessage::Goodbye)
        .await
        .unwrap();
    drop(writer2);
    drop(reader2);

    daemon_handle.abort();
    let _ = daemon_handle.await;
}

#[tokio::test]
async fn version_mismatch_is_rejected_gracefully() {
    let dir = TempDir::new().unwrap();
    let address = test_address(dir.path(), "version");

    let server = DaemonServer::bind(address.clone(), Editor::new()).unwrap();
    let daemon_handle = tokio::spawn(async move {
        let _ = server.run().await;
    });

    let stream = connect(&address).await;
    let (mut reader, mut writer) = stream.into_split();
    write_frame(
        &mut writer,
        &ClientMessage::Hello(HelloInfo {
            // Intentionally wrong version:
            protocol_version: PROTOCOL_VERSION + 99,
            client_id: "bad".into(),
            cols: 20,
            rows: 5,
        }),
    )
    .await
    .unwrap();

    let reply: DaemonMessage = read_frame(&mut reader).await.unwrap();
    assert!(matches!(
        reply,
        DaemonMessage::Shutdown(arx_protocol::ShutdownReason::VersionMismatch { .. })
    ));

    drop(writer);
    drop(reader);
    daemon_handle.abort();
    let _ = daemon_handle.await;
}
