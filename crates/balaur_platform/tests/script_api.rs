//! The platform bindings called the way a game calls them: from a script,
//! through `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! No store is loaded here, which is the case worth proving: a script written
//! against `platform.*` has to keep running on a machine that has none.

use std::time::{Duration, Instant};

use balaur::{standard_app, AppConfig};

/// These boot full apps: CI's job. A plain local `cargo test` skips them so
/// iteration stays fast; `BALAUR_E2E=1` (what `scripts/e2e_tests.sh` and CI
/// set) runs them.
fn e2e_enabled() -> bool {
    if std::env::var_os("BALAUR_E2E").is_some() {
        return true;
    }
    eprintln!("skipped: e2e suite; run scripts/e2e_tests.sh or set BALAUR_E2E=1");
    false
}

/// The log buffer is global and tests run in parallel, so one test's lines
/// would surface in another's assertions.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Boot a one-node project whose script is `source`, then tick until every
/// marker shows up in the log.
#[allow(
    clippy::disallowed_methods,
    reason = "a test's timeout, not simulation"
)]
fn run_until(source: &str, markers: &[&str]) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"n\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Node\"\nscript = \"scripts/s.rn\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), source).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
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

/// Both shapes in one boot: a call awaited for its answer, and a call whose
/// answer arrives at a named handler method.
#[test]
fn a_script_awaits_a_call_and_takes_another_through_a_handler() {
    if !e2e_enabled() {
        return;
    }
    run_until(
        r#"
pub async fn init(this) {
    let store = platform::backend();
    let r = task::wait(platform::sign_in()).await;
    log::info(`platform-await ${store} ${r["kind"]}`);
    this.request = platform::unlock(this.node, "first_blood", #{ on_platform: "on_store" });
}

pub fn on_store(this, e) {
    if e["request"] == this.request {
        log::info(`platform-handler ${e["kind"]} ${e["call"]}`);
    }
}
"#,
        &[
            "platform-await none unsupported",
            "platform-handler unsupported unlock",
        ],
    );
}

#[test]
fn a_script_reads_the_player_before_a_sign_in_has_landed() {
    if !e2e_enabled() {
        return;
    }
    run_until(
        r"
pub fn init(this) {
    log::info(`platform-empty ${platform::signed_in()} ${platform::backend()}`);
}
",
        &["platform-empty false none"],
    );
}
