//! End-to-end tests exercising the rope + buffer + property machinery
//! together. These tests guard the invariants that multiple consumers of the
//! Arx buffer will rely on: O(1) snapshots, persistent history, property
//! tracking across many edits, and graceful behaviour on large inputs.

use std::sync::Arc;

use arx_buffer::{
    AdjustmentPolicy, Buffer, BufferId, Diagnostic, EditOrigin, Face, Interval, PropertyFlags,
    PropertyMap, PropertyValue, Rope, Severity, StickyBehavior,
};

#[test]
fn snapshots_form_persistent_history() {
    let mut buf = Buffer::from_str(BufferId(42), "Arx");

    let s0 = buf.snapshot();
    buf.edit(3..3, " editor", EditOrigin::User);
    let s1 = buf.snapshot();
    buf.edit(0..0, "the ", EditOrigin::User);
    let s2 = buf.snapshot();
    buf.edit(4..7, "new", EditOrigin::Extension("demo".into()));
    let s3 = buf.snapshot();

    assert_eq!(s0.text(), "Arx");
    assert_eq!(s1.text(), "Arx editor");
    assert_eq!(s2.text(), "the Arx editor");
    assert_eq!(s3.text(), "the new editor");

    assert_eq!(s0.version(), 0);
    assert_eq!(s1.version(), 1);
    assert_eq!(s2.version(), 2);
    assert_eq!(s3.version(), 3);

    // The current buffer matches the latest snapshot.
    assert_eq!(buf.text(), s3.text());
    assert_eq!(buf.version(), s3.version());
}

#[test]
fn track_edits_layer_survives_many_edits() {
    let starting_text: String = (0..200).map(|i| format!("row {i:03}\n")).collect();
    let mut buf = Buffer::from_str(BufferId(7), &starting_text);

    // Attach a diagnostic at a specific row.
    let row = 50;
    let start = buf.rope().line_to_byte(row);
    let end = buf.rope().line_to_byte(row + 1).saturating_sub(1);
    buf.properties_mut()
        .ensure_layer("diagnostics", AdjustmentPolicy::TrackEdits)
        .insert(Interval::new(
            start..end,
            PropertyValue::Diagnostic(Arc::new(Diagnostic {
                severity: Severity::Warning,
                message: Arc::from("unused variable"),
                code: Some(Arc::from("W123")),
                source: Some(Arc::from("rustc")),
            })),
            StickyBehavior::RearSticky,
        ));

    // Insert a fresh row at the very top 50 times.
    for i in 0..50 {
        let insertion = format!("new {i:03}\n");
        buf.edit(0..0, &insertion, EditOrigin::User);
    }

    // The diagnostic interval should have been shifted forward by 50 rows
    // worth of bytes, still aligned with the original row contents.
    let iv = buf
        .properties()
        .layer("diagnostics")
        .unwrap()
        .tree()
        .iter()
        .next()
        .expect("diagnostic survives");
    let text_in_range = buf.rope().slice_to_string(iv.range.clone());
    assert_eq!(text_in_range, "row 050");
}

#[test]
fn styled_runs_cover_entire_query() {
    let mut map = PropertyMap::new();
    let layer = map.ensure_layer("syntax", AdjustmentPolicy::TrackEdits);
    layer.insert(Interval::new(
        0..3,
        PropertyValue::Decoration(Face {
            bold: Some(true),
            fg: Some(0xff_00_00),
            priority: 5,
            ..Default::default()
        }),
        StickyBehavior::RearSticky,
    ));
    layer.insert(Interval::new(
        5..10,
        PropertyValue::Decoration(Face {
            italic: Some(true),
            priority: 5,
            ..Default::default()
        }),
        StickyBehavior::RearSticky,
    ));

    let runs = map.styled_runs(0..10);
    // Runs must tile [0..10) with no gaps.
    let mut cursor = 0;
    for run in &runs {
        assert_eq!(run.range.start, cursor);
        cursor = run.range.end;
    }
    assert_eq!(cursor, 10);
    assert!(runs.iter().any(|r| r.face.bold == Some(true)));
    assert!(runs.iter().any(|r| r.face.italic == Some(true)));
}

#[test]
fn large_buffer_is_tree_shaped_not_linear() {
    // ~1 MB payload: should produce many leaf chunks (Ropey's internal
    // B-tree handles its own balancing — we just verify the rope doesn't
    // collapse into one giant leaf).
    let payload = "the quick brown fox jumps over the lazy dog\n".repeat(24_000);
    let rope = Rope::from_str(&payload);
    assert_eq!(rope.len_bytes(), payload.len());
    assert_eq!(rope.len_lines(), payload.matches('\n').count() + 1);
    let chunk_count = rope.chunks().count();
    assert!(
        chunk_count > 16,
        "expected the rope to be split into many chunks, got {chunk_count}"
    );

    // Full round-trip.
    assert_eq!(rope.text(), payload);
}

#[test]
fn stress_many_edits_with_tracked_properties() {
    // A pseudo-random walk of insertions and deletions with a RearSticky
    // property attached at a point we track through the edit history. The
    // property's range should always land on the same chunk of text we
    // originally tagged.
    let mut buf = Buffer::from_str(BufferId(1), "ANCHOR middle text");
    buf.properties_mut()
        .ensure_layer("marks", AdjustmentPolicy::TrackEdits)
        .insert(Interval::new(
            0..6,
            PropertyValue::Flag,
            StickyBehavior::RearSticky,
        ));

    // 200 small edits that avoid the anchor region.
    let mut cursor = buf.len_bytes();
    for i in 0..200u32 {
        // Alternate appending and mid-body insertions, all past the anchor.
        let edit_pos = if i % 3 == 0 {
            cursor
        } else {
            // Insert just after the anchor.
            6 + (i as usize % 3)
        };
        let snippet = match i % 4 {
            0 => "x",
            1 => "yy",
            2 => "zzz",
            _ => " ",
        };
        let pos = edit_pos.min(buf.len_bytes());
        buf.edit(pos..pos, snippet, EditOrigin::System);
        cursor = buf.len_bytes();
    }

    // The interval should still cover "ANCHOR" (6 bytes of anchor text).
    let iv = buf
        .properties()
        .layer("marks")
        .unwrap()
        .tree()
        .iter()
        .next()
        .expect("anchor interval still present");
    assert_eq!(iv.range.end - iv.range.start, 6);
    assert_eq!(buf.rope().slice_to_string(iv.range.clone()), "ANCHOR");

    // And the buffer version reflects every edit.
    assert_eq!(buf.version(), 200);
}

#[test]
fn property_flags_propagate_for_read_only_and_agent_edits() {
    let mut map = PropertyMap::new();
    let layer = map.ensure_layer("edits", AdjustmentPolicy::TrackEdits);
    layer.insert(Interval::new(
        0..8,
        PropertyValue::ReadOnly,
        StickyBehavior::RearSticky,
    ));
    layer.insert(Interval::new(
        4..12,
        PropertyValue::AgentAttribution {
            agent: arx_buffer::AgentId(9),
            edit_id: 1,
        },
        StickyBehavior::RearSticky,
    ));

    let runs = map.styled_runs(0..12);
    let ro_only = runs.iter().find(|r| r.range == (0..4)).unwrap();
    let both = runs.iter().find(|r| r.range == (4..8)).unwrap();
    let agent_only = runs.iter().find(|r| r.range == (8..12)).unwrap();

    assert!(ro_only.flags.contains(PropertyFlags::READONLY));
    assert!(!ro_only.flags.contains(PropertyFlags::AGENT_EDIT));
    assert!(both.flags.contains(PropertyFlags::READONLY));
    assert!(both.flags.contains(PropertyFlags::AGENT_EDIT));
    assert!(!agent_only.flags.contains(PropertyFlags::READONLY));
    assert!(agent_only.flags.contains(PropertyFlags::AGENT_EDIT));
}
