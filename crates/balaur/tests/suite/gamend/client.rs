//! The addon's client by itself: the calls a script makes before it talks
//! to a server, and the shortest round trips with one.

use std::time::Duration;

use super::run_with_addon;

/// Whether a call of the addon answers, said as `sent <kind>`. A call that
/// sends nothing answers at once.
fn answer_of(call: &str) -> Vec<String> {
    let source = format!(
        r#"
use addons::gamend::admin_analytics;
use addons::gamend::users;

pub class Scenario {{ node: Node }}
impl Scenario {{
    pub async fn init() {{
        let answer = await {call};
        log::info("sent {{answer["kind"]}}");
    }}
}}
"#
    );
    run_with_addon(&source, &["sent "], Duration::from_secs(20))
}

#[test]
#[should_panic(expected = "gamend::users::get_user needs `id`")]
fn a_call_with_an_empty_path_id_is_refused() {
    answer_of("users::get_user(\"\")");
}

#[test]
#[should_panic(expected = "`users::get_user` takes 1 argument(s), 0 given")]
fn a_call_with_its_path_id_left_out_does_not_compile() {
    answer_of("users::get_user()");
}

#[test]
fn a_call_with_no_server_to_reach_answers_an_error() {
    let source = r#"
use addons::gamend::client;
use addons::gamend::users;

pub class Scenario { node: Node }
impl Scenario {
    pub async fn init() {
        client::configure("http://127.0.0.1:9", "");
        let answer = await users::get_user("7");
        log::info("sent {answer["kind"]}");
    }
}
"#;
    run_with_addon(source, &["sent error"], Duration::from_secs(20));
}

#[test]
fn a_socket_call_with_no_socket_wakes_what_awaits_it_with_the_reason() {
    let source = r#"
use addons::gamend::client;

pub class Scenario { node: Node }
impl Scenario {
    pub async fn init() {
        let reply = await task::wait(client::join(4242, "lobby:1"));
        log::info("refused {reply["kind"]} {reply["error"]}");
    }
}
"#;
    run_with_addon(
        source,
        &["refused error no such gamend socket"],
        Duration::from_secs(20),
    );
}

#[test]
fn a_socket_with_no_session_answers_its_connect_and_tells_its_node_once() {
    let source = r#"
use addons::gamend::client;

pub class Scenario { node: Node, heard: int }
impl Scenario {
    pub async fn init() {
        let first = await task::wait(client::connect(self.node));
        let second = await task::wait(client::connect(self.node));
        await task::frames(2);
        log::info("awaited {first["kind"]} {second["kind"]} {self.heard} {first["reason"]}");
    }

    pub fn on_gamend_event(e: any) {
        self.heard += 1;
    }
}
"#;
    run_with_addon(
        source,
        &["awaited error error 2 connect needs a logged-in session"],
        Duration::from_secs(20),
    );
}

#[test]
fn a_reconnect_waits_twice_as_long_each_try_up_to_thirty_seconds() {
    let source = r#"
use addons::gamend::socket;

pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let waits = "";
        for attempt in 1..socket::RECONNECT_TRIES + 1 {
            waits = waits + " {socket::backoff(attempt)}";
        }
        log::info("waits{waits}");
    }
}
"#;
    run_with_addon(
        source,
        &["waits 1.0 2.0 4.0 8.0 16.0 30.0 30.0 30.0"],
        Duration::from_secs(20),
    );
}

#[test]
fn a_kept_session_comes_back_and_one_without_a_token_is_refused() {
    let source = r#"
use addons::gamend::auth;

pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let kept = auth::restore({ "user_id": "u1", "username": "tester", "access_token": "a", "refresh_token": "r", "expires_in": 900, "expires_at": 1790000000 });
        let s = auth::session().unwrap();
        let refused = auth::restore({ "user_id": "u2" });
        let still = auth::session().unwrap().user_id;
        auth::restore(nil);
        log::info("restored {kept} {s.username} {s.refresh_token} {s.expires_at.unwrap()} {refused} {still} {auth::signed_in()}");
    }
}
"#;
    run_with_addon(
        source,
        &["restored true tester r 1790000000 false u1 false"],
        Duration::from_secs(20),
    );
}

#[test]
fn a_run_from_the_sources_talks_to_the_local_server_until_someone_picks() {
    if std::env::var_os("GAMEND_URL").is_some() {
        return;
    }
    let source = r#"
use addons::gamend::client;

pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        settings::set("gamend/url", "https://play.example");
        settings::set("gamend/local_url", "http://localhost:4000");
        let near = client::server();
        settings::set("editor/gamend/server", "production");
        let far = client::server();
        log::info("target {near.name} {near.url} {far.name} {far.url}");
    }
}
"#;
    run_with_addon(
        source,
        &["target local http://localhost:4000 production https://play.example"],
        Duration::from_secs(20),
    );
}
