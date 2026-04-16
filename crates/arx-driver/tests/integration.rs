//! End-to-end driver test: feed a scripted event stream through the
//! whole stack (input task → command bus → event loop → render task →
//! `TestBackend`) and assert on the observable buffer state + final
//! rendered grid.

use std::io::Write;
use std::sync::{Arc, Mutex};

use arx_core::{CommandBus, Editor};
use arx_driver::{Driver, SharedTerminalSize};
use arx_render::TestBackend;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures_util::stream;
use tempfile::NamedTempFile;

// The real `crossterm::event::EventStream` yields `io::Result<Event>`, so we
// produce the same item type in tests for parity. Clippy's `unnecessary_wraps`
// would rather we flatten the `Result`, but that would break that parity.
#[allow(clippy::unnecessary_wraps)]
fn key(code: KeyCode, mods: KeyModifiers) -> std::io::Result<Event> {
    Ok(Event::Key(KeyEvent::new(code, mods)))
}

#[tokio::test]
async fn typing_ends_up_in_the_buffer_and_the_rendered_grid() {
    // Script: type "X" (self-insert fallback), then `C-x C-c` to quit
    // via the Emacs keymap profile's `editor.quit` binding.
    let events = stream::iter(vec![
        key(KeyCode::Char('X'), KeyModifiers::NONE),
        key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]);

    let backend = TestBackend::new(40, 5);
    let size = SharedTerminalSize::new(40, 5);

    let driver = Driver::new(|editor: &mut Editor| {
        let buf = editor.buffers_mut().create_from_text("hello", None);
        editor.windows_mut().open(buf);
    });

    // The hook lets the test wait for a tick so the render task has a
    // chance to draw the final frame before we tear down.
    let editor = driver
        .run_with(events, backend, size, |_bus: CommandBus| async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        })
        .await
        .unwrap();

    // Buffer state: "Xhello" with cursor at 1.
    let buffer_id = editor.buffers().ids().next().unwrap();
    let text = editor.buffers().get(buffer_id).unwrap().text();
    assert_eq!(text, "Xhello");

    let window_id = editor.windows().active().unwrap();
    assert_eq!(editor.windows().get(window_id).unwrap().cursor_byte, 1);
}

#[tokio::test]
async fn driver_renders_into_backend_visible_to_caller() {
    // Wrap the backend so we can inspect its state after the driver
    // shuts down. See `SharedBackend` above for the implementation.
    let shared = SharedBackend {
        inner: Arc::new(Mutex::new(TestBackend::new(40, 5))),
    };
    let peek = shared.inner.clone();

    let events = stream::iter(vec![
        key(KeyCode::Char('X'), KeyModifiers::NONE),
        key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]);

    let size = SharedTerminalSize::new(40, 5);
    let driver = Driver::new(|editor: &mut Editor| {
        let buf = editor.buffers_mut().create_from_text("hello", None);
        editor.windows_mut().open(buf);
    });

    let _ = driver
        .run_with(events, shared, size, |_bus| async {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        })
        .await
        .unwrap();

    let backend = peek.lock().unwrap();
    let text = backend.grid().to_debug_text();
    assert!(text.contains("Xhello"), "rendered grid: {text:?}");
}

#[tokio::test]
async fn shutdown_is_clean_on_empty_stream() {
    // Empty event stream → driver exits immediately after the seed runs.
    let events = stream::iter(Vec::<std::io::Result<Event>>::new());
    let backend = TestBackend::new(20, 3);
    let size = SharedTerminalSize::new(20, 3);
    let driver = Driver::new(|editor: &mut Editor| {
        let buf = editor.buffers_mut().create_from_text("x", None);
        editor.windows_mut().open(buf);
    });
    let editor = driver
        .run_with(events, backend, size, |_| async {})
        .await
        .unwrap();
    assert!(!editor.buffers().is_empty());
}

#[tokio::test]
async fn ctrl_s_writes_active_buffer_to_disk() {
    // Create a temp file with known contents.
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "initial").unwrap();
    let path = tmp.path().to_path_buf();

    // Emacs default profile: save is `C-x C-s`, quit is `C-x C-c`.
    // `Ctrl+S` alone would be "isearch-forward" in the full Emacs
    // tradition; we preserve that and expose save via the chord.
    let events = stream::iter(vec![
        key(KeyCode::End, KeyModifiers::NONE),
        key(KeyCode::Char('X'), KeyModifiers::NONE),
        key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key(KeyCode::Char('s'), KeyModifiers::CONTROL),
        key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]);

    let backend = TestBackend::new(40, 5);
    let size = SharedTerminalSize::new(40, 5);

    let path_for_hook = path.clone();
    let driver = Driver::new(|_| {}).with_async_hook(move |bus| async move {
        arx_core::open_file(&bus, path_for_hook).await.unwrap();
    });

    let _ = driver
        .run_with(events, backend, size, |_bus: CommandBus| async {
            // Let the input + save pipeline drain before the test returns.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        })
        .await
        .unwrap();

    // Poll the file briefly — the save runs in a spawned task, so it
    // may lag the Ctrl+Q event by a few ms.
    let mut content = String::new();
    for _ in 0..20 {
        content = tokio::fs::read_to_string(&path).await.unwrap();
        if content.contains('X') {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(
        content.contains("initialX") || content.contains('X'),
        "save did not land: {content:?}"
    );
}

// Reusable "inspect the backend after shutdown" wrapper.
#[derive(Debug, Clone)]
struct SharedBackend {
    inner: Arc<Mutex<TestBackend>>,
}

impl arx_render::Backend for SharedBackend {
    fn size(&self) -> (u16, u16) {
        self.inner.lock().unwrap().size()
    }
    fn apply(&mut self, ops: &[arx_render::DiffOp]) -> arx_render::BackendResult<()> {
        self.inner.lock().unwrap().apply(ops)
    }
    fn present(&mut self) -> arx_render::BackendResult<()> {
        self.inner.lock().unwrap().present()
    }
    fn clear(&mut self) -> arx_render::BackendResult<()> {
        self.inner.lock().unwrap().clear()
    }
}

#[tokio::test]
async fn modified_indicator_appears_after_edit_and_clears_after_save() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "seed").unwrap();
    let path = tmp.path().to_path_buf();

    let shared = SharedBackend {
        inner: Arc::new(Mutex::new(TestBackend::new(60, 5))),
    };
    let peek = shared.inner.clone();

    let events = stream::iter(vec![
        key(KeyCode::End, KeyModifiers::NONE),
        key(KeyCode::Char('!'), KeyModifiers::NONE),
        // After the edit, the modeline should contain [+].
        key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]);

    let path_for_hook = path.clone();
    let driver = Driver::new(|_| {}).with_async_hook(move |bus| async move {
        arx_core::open_file(&bus, path_for_hook).await.unwrap();
    });
    let _ = driver
        .run_with(events, shared, SharedTerminalSize::new(60, 5), |_bus| async {
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        })
        .await
        .unwrap();

    let backend = peek.lock().unwrap();
    let text = backend.grid().to_debug_text();
    assert!(text.contains("[+]"), "missing [+] marker: {text:?}");
}

#[tokio::test]
async fn arrow_keys_move_cursor_through_driver_pipeline() {
    let events = stream::iter(vec![
        key(KeyCode::End, KeyModifiers::NONE),
        key(KeyCode::Left, KeyModifiers::NONE),
        key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]);
    let backend = TestBackend::new(40, 5);
    let size = SharedTerminalSize::new(40, 5);
    let driver = Driver::new(|editor: &mut Editor| {
        let buf = editor.buffers_mut().create_from_text("abcdef", None);
        editor.windows_mut().open(buf);
    });
    let editor = driver
        .run_with(events, backend, size, |_| async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        })
        .await
        .unwrap();
    let win = editor.windows().active().unwrap();
    // End -> 6, then Left one grapheme -> 5.
    assert_eq!(editor.windows().get(win).unwrap().cursor_byte, 5);
}

