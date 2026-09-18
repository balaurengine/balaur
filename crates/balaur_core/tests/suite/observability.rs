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

#[test]
fn a_repeated_report_is_first_only_once_per_site_and_key() {
    assert!(logbuf::first_time("observability test", "a"));
    assert!(!logbuf::first_time("observability test", "a"));
    assert!(logbuf::first_time("observability test", "b"));
    assert!(logbuf::first_time("another site", "a"));
}

#[test]
fn the_cursor_reads_each_line_once_and_counts_what_the_ring_dropped() {
    let _guard = CAPTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    logbuf::capture_for_test();
    let start = logbuf::total();
    tracing::info!("cursor one");
    tracing::info!("cursor two");

    let (entries, cursor, missed) = logbuf::since(start);
    let mine: Vec<&str> = entries
        .iter()
        .filter(|e| e.message.starts_with("cursor "))
        .map(|e| e.message.as_str())
        .collect();
    assert_eq!(mine, ["cursor one", "cursor two"]);
    assert_eq!(missed, 0);
    assert!(
        logbuf::since(cursor)
            .0
            .iter()
            .all(|e| !e.message.starts_with("cursor "))
    );

    for i in 0..700 {
        tracing::info!(seq = i, "flood");
    }
    let (entries, _, missed) = logbuf::since(cursor);
    assert!(entries.len() <= 500, "the ring held more than its capacity");
    assert!(
        missed >= 200,
        "the lines the ring dropped went uncounted: {missed}"
    );
}

#[test]
fn the_log_file_starts_with_what_came_before_it_and_keeps_the_last_run() {
    let _guard = CAPTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    logbuf::capture_for_test();
    let dir = tempfile::tempdir().expect("a temporary directory");
    tracing::info!("logged before the file opened");
    let path = logbuf::open_file(dir.path(), "run", 2).expect("the file opens");
    tracing::warn!(code = 7, "logged after it opened");

    let text = std::fs::read_to_string(&path).expect("the file reads");
    assert!(text.contains("logged before the file opened"), "{text}");
    assert!(
        text.contains("warn  observability: logged after it opened code=7"),
        "{text}"
    );

    logbuf::open_file(dir.path(), "run", 2).expect("the file opens again");
    let kept = std::fs::read_to_string(dir.path().join("run.1.log")).expect("the last run is kept");
    assert!(kept.contains("logged after it opened"));
}
