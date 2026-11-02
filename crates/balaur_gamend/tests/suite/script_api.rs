//! The gamend bindings called the way a game calls them: from a script,
//! through `balaur::standard_app`, against a real Gamend server: the local one
//! on port 4000, or `GAMEND_URL`.
//!
//! All of it runs with the e2e suite. A test that signs in registers its own
//! account by device and deletes it before it ends.

use balaur_testkit::{gamend_url, run_until_within};

#[test]
fn a_script_reads_the_public_api_of_the_server() {
    let Some(url) = gamend_url() else {
        return;
    };
    // A query, a path parameter and a bare call. One line at the end: the
    // harness reads a fifty-entry ring.
    let source = format!(
        r#"
pub class Scenario {{ node: Node }}
impl Scenario {{
    async fn init() {{
        gamend::configure("{url}");
        let boards = await task::wait(gamend::rest("GET", "/api/v1/leaderboards?page=1").as_int());
        let listed = boards["body"]["data"];
        let records = 200;
        if listed.len() > 0 {{
            let id = listed[0]["id"];
            let got = await task::wait(gamend::rest("GET", "/api/v1/leaderboards/{{id}}/records").as_int());
            records = got["status"].as_int();
        }}
        let stats = await task::wait(gamend::rest("GET", "/api/v1/lobbies/stats").as_int());
        let a = boards["status"];
        let c = stats["status"];
        log::info("rest {{a}} {{records}} {{c}}");
    }}
}}
"#
    );
    run_until_within(
        &[],
        &source,
        &["rest 200 200 200"],
        std::time::Duration::from_secs(20),
    );
}

#[test]
fn a_script_signs_in_connects_calls_a_hook_and_deletes_its_account() {
    let Some(url) = gamend_url() else {
        return;
    };
    let device = device_id();
    let source = format!(
        r#"
pub class Scenario {{ node: Node, socket: int }}
impl Scenario {{
    async fn init() {{
        gamend::configure("{url}");
        // A stock Gamend takes ten sign-ins a minute from one address.
        let login = await task::wait(gamend::login(gamend::Credentials {{ device_id: "{device}" }}));
        while login.has("error") && login["error"].as_str().contains("(429)") {{
            let until = engine::unix_time() + 15.0;
            while engine::unix_time() < until {{
                await task::frames(1);
            }}
            let again = await task::wait(gamend::login(gamend::Credentials {{ device_id: "{device}" }}));
            login = again;
        }}
        if login.has("error") {{
            let error = login["error"];
            log::info("gamend-live login failed: {{error}}");
            return;
        }}
        self.socket = gamend::connect(self.node);
    }}

    async fn on_gamend_event(e: any) {{
        if e["kind"].as_str() != "open" {{
            return;
        }}
        let me = await task::wait(gamend::rest("GET", "/api/v1/me").as_int());
        let hook = await task::wait(gamend::call_hook(self.socket, "sdk_probe", "echo", ["hi"]));
        gamend::close(self.socket);
        let gone = await task::wait(gamend::rest("DELETE", "/api/v1/me").as_int());
        let a = me["status"];
        let b = hook["status"];
        let c = gone["status"];
        log::info("gamend-live {{a}} {{b}} {{c}}");
    }}
}}
"#
    );
    // No plugin answers `sdk_probe` on a stock server, so the hook's reply is
    // an error, which still proves the whole path.
    run_until_within(
        &[],
        &source,
        &["gamend-live 200 error 200"],
        std::time::Duration::from_secs(300),
    );
}

#[allow(
    clippy::disallowed_methods,
    reason = "names a throwaway test account, not simulation"
)]
fn device_id() -> String {
    format!(
        "balaur-script-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}
