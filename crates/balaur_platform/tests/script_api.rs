//! The platform bindings called the way a game calls them: from a script,
//! through `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! No store is loaded here, which is the case worth proving: a script written
//! against `platform.*` has to keep running on a machine that has none.

use balaur_testkit::{e2e_enabled, run_until};

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
