//! Observability: assert that the engine reports what it claims to report.
//!
//! These assert on structured fields, not on wording, so rephrasing a message
//! does not break them.

use balaur_core::logbuf;

/// One global buffer, and `capture_for_test` replaces it: two of these running
/// at once clear each other's entries.
static CAPTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn events_carry_their_structured_fields() {
    let _guard = CAPTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    logbuf::capture_for_test();
    logbuf::clear();

    tracing::info!(script = "pig.rn", nodes = 24, "reloaded");

    let entry = logbuf::recent(10)
        .into_iter()
        .find(|e| e.message == "reloaded")
        .expect("the reload event should have been captured");

    assert_eq!(entry.level, "info");
    assert_eq!(entry.field("script"), Some("pig.rn"));
    assert_eq!(entry.field("nodes"), Some("24"));
}

#[test]
fn the_buffer_is_bounded_and_keeps_the_newest() {
    let _guard = CAPTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    logbuf::capture_for_test();
    logbuf::clear();

    for i in 0..600 {
        tracing::info!(seq = i, "tick");
    }

    let recent = logbuf::recent(1000);
    assert!(recent.len() <= 500, "buffer grew past its capacity");
    assert_eq!(
        recent.last().and_then(|e| e.field("seq")),
        Some("599"),
        "the newest event should survive"
    );
}
