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
        let r = await task::wait(platform::sign_in());
        let kind = r["kind"];
        log::info("platform-await {store} {kind}");
        let e = await task::wait(platform::unlock("first_blood"));
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
        let signed: bool = platform::signed_in();
        let store: str = platform::backend();
        let player: platform::Player? = platform::player();
        log::info("platform-empty {signed} {store} {player == nil}");
    }
}
"#,
        &["platform-empty false none true"],
    );
}

#[test]
fn every_call_answers_an_id_to_await_and_a_row_reads_as_its_record() {
    if !e2e_enabled() {
        return;
    }
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    async fn init() {
        let options = platform::ScoresOptions { count: 5, start: 1, scope: "friends", period: "week" };
        let presence = await task::wait(platform::set_presence("in the caves"));
        let progress = await task::wait(platform::progress("first_blood", 50.0));
        let score = await task::wait(platform::submit_score("arena", 12));
        let some = await task::wait(platform::scores("arena", options));
        let read = await task::wait(platform::cloud_read("slot"));
        let write = await task::wait(platform::cloud_write("slot", "saved"));
        let said: [any] = [presence, progress, score, some, read, write];
        for answer in said {
            log::info("platform-call {answer["call"].text()} {answer["kind"].text()}");
        }
        let rows: any = [{ "player": "p", "alias": "ada", "rank": 1, "score": 9 }];
        let scores: [platform::Score] = rows;
        log::info("platform-row {scores[0].alias} {scores[0].rank}");
    }
}
"#,
        &[
            "platform-call set_presence unsupported",
            "platform-call progress unsupported",
            "platform-call submit_score unsupported",
            "platform-call scores unsupported",
            "platform-call cloud_read unsupported",
            "platform-call cloud_write unsupported",
            "platform-row ada 1",
        ],
    );
}
