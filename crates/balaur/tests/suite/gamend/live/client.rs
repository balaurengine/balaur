//! The script client against a live server: the REST API read, and an
//! account signed in, connected, hooked and deleted.

use std::time::Duration;

use crate::gamend::{run_with_addon, tag};

#[test]
fn a_script_reads_the_public_api_of_the_server() {
    let Some(url) = crate::gamend::server() else {
        return;
    };
    // A query, a path parameter and a bare call.
    let source = format!(
        r#"
use addons::gamend::client;
use addons::gamend::leaderboards;
use addons::gamend::lobbies;

pub class Scenario {{ node: Node }}
impl Scenario {{
    pub async fn init() {{
        client::configure("{url}", "");
        let boards = await leaderboards::list_leaderboards({{ "page": 1 }});
        let listed = boards["body"]["data"];
        let records = 200;
        if listed.len() > 0 {{
            let got = await leaderboards::list_leaderboard_records(listed[0]["id"].text());
            records = got["status"].as_int();
        }}
        let stats = await lobbies::lobby_stats();
        log::info("rest {{boards["status"]}} {{records}} {{stats["status"]}} timed {{boards["ms"].as_float() > 0.0}}");
    }}
}}
"#
    );
    run_with_addon(
        &source,
        &["rest 200 200 200 timed true"],
        Duration::from_secs(20),
    );
}

#[test]
fn a_script_signs_in_connects_calls_a_hook_and_deletes_its_account() {
    let Some(url) = crate::gamend::server() else {
        return;
    };
    let device = format!("balaur-script-{}", tag());
    let source = format!(
        r#"
use addons::gamend::client;
use addons::gamend::users;

pub class Scenario {{ node: Node, socket: int }}
impl Scenario {{
    pub async fn init() {{
        client::configure("{url}", "sdk_probe");
        let login = await client::login_device("{device}");
        while login["error"].as_str().contains("(429)") {{
            let until = engine::unix_time() + 15.0;
            while engine::unix_time() < until {{
                await task::frames(1);
            }}
            login = await client::login_device("{device}");
        }}
        if login["kind"].as_str() != "login" {{
            log::info("gamend-live login failed: {{login}}");
            return;
        }}
        self.socket = client::connect(self.node);
    }}

    pub async fn on_gamend_event(e: any) {{
        if e["kind"].as_str() != "open" {{
            return;
        }}
        let me = await users::get_current_user();
        let hook = await client::call(self.socket, "echo", ["hi"]);
        client::close(self.socket);
        let gone = await users::delete_current_user();
        log::info("gamend-live {{me["status"]}} {{hook.ok}} {{gone["status"]}}");
    }}
}}
"#
    );
    // No plugin answers `sdk_probe` on a stock server, so the hook's reply is
    // an error, which still proves the whole path.
    run_with_addon(
        &source,
        &["gamend-live 200 false 200"],
        Duration::from_secs(300),
    );
}
