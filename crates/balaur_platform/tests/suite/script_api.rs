//! The platform bindings called the way a game calls them: from a script,
//! through `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! No store is loaded here, which is the case worth proving: a script written
//! against `platform.*` has to keep running on a machine that has none.

use balaur_testkit::{e2e_enabled, run_until};

/// A call awaited for its answer, and the next.
#[test]
fn a_script_awaits_one_call_after_another() {
    if !e2e_enabled() {
        return;
    }
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    async fn init() {
        let store = platform::backend();
        let r = await task::wait(platform::sign_in().as_int());
        let kind = r["kind"];
        log::info("platform-await {store} {kind}");
        let e = await task::wait(platform::unlock("first_blood").as_int());
        let kind = e["kind"];
        let call = e["call"];
        log::info("platform-second {kind} {call}");
    }
}
"#,
        &[
            "platform-await none unsupported",
            "platform-second unsupported unlock",
        ],
    );
}

#[test]
fn a_script_reads_the_player_before_a_sign_in_has_landed() {
    if !e2e_enabled() {
        return;
    }
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        let signed = platform::signed_in();
        let store = platform::backend();
        log::info("platform-empty {signed} {store}");
    }
}
"#,
        &["platform-empty false none"],
    );
}
