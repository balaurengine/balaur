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
pub class Scenario { node: Node, request: int }
impl Scenario {
    async fn init() {
        let store = platform::backend();
        let r = await task::wait(platform::sign_in().as_int());
        let kind = r.get("kind");
        log::info("platform-await {store} {kind}");
        self.request = platform::unlock(self.node, "first_blood", map().with("on_event", "on_store")).as_int();
    }

    pub fn on_store(e: any) {
        if e.get("request").as_int() == self.request {
            let kind = e.get("kind");
            let call = e.get("call");
            log::info("platform-handler {kind} {call}");
        }
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
