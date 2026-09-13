//! The harness the socket-facing e2e suites share: the gate that keeps them
//! off a plain `cargo test`, the lock around the global log, and one booted
//! app ticked until its script has said what it was meant to say.
//!
//! Every suite that boots `balaur::standard_app` over a real socket wants the
//! same three things, and had its own copy of them. A scenario's own server —
//! the canned HTTP response, the echo socket — stays in its crate, because
//! that is the part each suite is actually testing.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use balaur::{AppConfig, standard_app};

/// These tests boot full apps and speak real sockets: CI's job. A plain local
/// `cargo test` skips them so iteration stays fast; `BALAUR_E2E=1` (what
/// `scripts/e2e_tests.sh` and CI set) runs them.
#[must_use]
pub fn e2e_enabled() -> bool {
    if std::env::var_os("BALAUR_E2E").is_some() {
        return true;
    }
    eprintln!("skipped: e2e suite; run scripts/e2e_tests.sh or set BALAUR_E2E=1");
    false
}

/// The log buffer is global and tests run in parallel, so one test's lines
/// would surface in another's assertions.
static LOG: Mutex<()> = Mutex::new(());

/// Boot a one-node project whose script is `source`, then tick until every
/// marker shows up in the log. No sleeps: ticking full-tilt costs little and
/// the sockets answer in milliseconds. Panics on any logged error or on the
/// deadline.
///
/// # Panics
///
/// If the script logs an error, or if the deadline passes before every marker
/// has been seen.
#[allow(
    clippy::disallowed_methods,
    reason = "a test's timeout, not simulation"
)]
pub fn run_until(source: &str, markers: &[&str]) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().expect("could not make a temp project directory");
    std::fs::create_dir_all(dir.path().join("scripts"))
        .expect("could not make the project's scripts directory");
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"n\"\nmain_scene = \"main.toml\"\n",
    )
    .expect("could not write the project manifest");
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Node\"\nscript = { source = \"scripts/s.rn\" }\n",
    )
    .expect("could not write the project's main scene");
    std::fs::write(dir.path().join("scripts/s.rn"), source)
        .expect("could not write the script under test");

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref()))
        .expect("the standard app would not boot");
    app.load_project().expect("the temp project would not load");
    // Markers accumulate across ticks: dependency debug logging (ureq's pool
    // chatter) floods the bounded buffer, so all of them are never in one
    // window together.
    let mut seen: Vec<bool> = markers.iter().map(|_| false).collect();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        app.tick(1.0 / 60.0);
        let recent = balaur_core::logbuf::recent(50);
        let errors: Vec<_> = recent
            .iter()
            .filter(|e| e.level.eq_ignore_ascii_case("error"))
            .collect();
        assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
        for entry in &recent {
            for (at, marker) in markers.iter().enumerate() {
                if entry.message.contains(marker) {
                    seen[at] = true;
                }
            }
        }
        if seen.iter().all(|s| *s) {
            return;
        }
    }
    panic!(
        "the script never logged all of {markers:?}; log: {:#?}",
        balaur_core::logbuf::recent(50)
    );
}
