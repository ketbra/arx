//! End-to-end tests for the event loop + command bus + buffer manager
//! pipeline. These mirror the spec §2.1 / §3.4 concurrency model: a single
//! writer task drives the editor while many reader tasks observe buffer
//! snapshots without locks.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use arx_buffer::{AdjustmentPolicy, EditOrigin, Interval, PropertyValue, StickyBehavior};
use arx_core::EventLoop;

#[tokio::test]
async fn writer_and_readers_observe_consistent_state() {
    let (event_loop, bus) = EventLoop::new();
    let driver = tokio::spawn(event_loop.run());

    // Create a buffer and capture both an `O(1)` snapshot now and a watch
    // receiver that will be updated as edits land.
    let id = bus
        .invoke(|editor| editor.buffers_mut().create_from_text("hello", None))
        .await
        .unwrap();
    let initial_snapshot = bus
        .invoke(move |editor| editor.buffers().snapshot(id).unwrap())
        .await
        .unwrap();
    let mut rx = bus
        .invoke(move |editor| editor.buffers().subscribe(id).unwrap())
        .await
        .unwrap();

    // Apply a sequence of edits via the writer task.
    bus.invoke(move |editor| {
        editor.buffers_mut().edit(id, 5..5, " world", EditOrigin::User);
    })
    .await
    .unwrap();
    bus.invoke(move |editor| {
        editor.buffers_mut().edit(id, 0..0, "say: ", EditOrigin::User);
    })
    .await
    .unwrap();

    // Drain the watch channel: the latest value reflects every edit.
    rx.changed().await.unwrap();
    let latest = rx.borrow_and_update().clone();
    assert_eq!(latest.text(), "say: hello world");
    assert_eq!(latest.version(), 2);

    // The original snapshot is untouched.
    assert_eq!(initial_snapshot.text(), "hello");
    assert_eq!(initial_snapshot.version(), 0);

    drop(bus);
    let _ = driver.await.unwrap();
}

#[tokio::test]
async fn many_concurrent_readers_see_latest_snapshot() {
    let (event_loop, bus) = EventLoop::new();
    let driver = tokio::spawn(event_loop.run());

    let id = bus
        .invoke(|editor| editor.buffers_mut().create_from_text("0", None))
        .await
        .unwrap();

    // Spawn 16 reader tasks that each subscribe and wait for a final marker.
    let observed = Arc::new(AtomicUsize::new(0));
    let mut readers = Vec::new();
    for _ in 0..16 {
        let bus = bus.clone();
        let observed = observed.clone();
        readers.push(tokio::spawn(async move {
            let mut rx = bus
                .invoke(move |editor| editor.buffers().subscribe(id).unwrap())
                .await
                .unwrap();
            loop {
                if rx.borrow_and_update().text().contains("END") {
                    observed.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                rx.changed().await.unwrap();
            }
        }));
    }

    // Apply 100 small edits, then a final marker.
    for i in 1..=100 {
        bus.invoke(move |editor| {
            let len = editor.buffers().get(id).unwrap().len_bytes();
            editor
                .buffers_mut()
                .edit(id, len..len, &format!(",{i}"), EditOrigin::User);
        })
        .await
        .unwrap();
    }
    bus.invoke(move |editor| {
        let len = editor.buffers().get(id).unwrap().len_bytes();
        editor
            .buffers_mut()
            .edit(id, len..len, " END", EditOrigin::User);
    })
    .await
    .unwrap();

    for r in readers {
        r.await.unwrap();
    }
    assert_eq!(observed.load(Ordering::SeqCst), 16);

    drop(bus);
    let _ = driver.await.unwrap();
}

#[tokio::test]
async fn property_layer_survives_dispatches() {
    let (event_loop, bus) = EventLoop::new();
    let driver = tokio::spawn(event_loop.run());

    // Create a buffer with a tracked-edit property layer holding an
    // anchor over the first six bytes.
    let id = bus
        .invoke(|editor| {
            let id = editor
                .buffers_mut()
                .create_from_text("ANCHOR rest", None);
            editor
                .buffers_mut()
                .get_mut(id)
                .unwrap()
                .properties_mut()
                .ensure_layer("marks", AdjustmentPolicy::TrackEdits)
                .insert(Interval::new(
                    0..6,
                    PropertyValue::Flag,
                    StickyBehavior::RearSticky,
                ));
            id
        })
        .await
        .unwrap();

    // Insert a bunch of bytes after the anchor through normal command dispatch.
    for _ in 0..50 {
        bus.invoke(move |editor| {
            editor
                .buffers_mut()
                .edit(id, 11..11, "X", EditOrigin::User);
        })
        .await
        .unwrap();
    }

    // The anchor is still attached to "ANCHOR".
    let anchor_text = bus
        .invoke(move |editor| {
            let buf = editor.buffers().get(id).unwrap();
            let iv = buf
                .properties()
                .layer("marks")
                .unwrap()
                .tree()
                .iter()
                .next()
                .unwrap()
                .clone();
            buf.rope().slice_to_string(iv.range.clone())
        })
        .await
        .unwrap();
    assert_eq!(anchor_text, "ANCHOR");

    drop(bus);
    let _ = driver.await.unwrap();
}
