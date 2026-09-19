//! The gamend bindings called the way a game calls them: from a script,
//! through `balaur::standard_app`, against a real Gamend server
//! (`GAMEND_URL`, or gamend.org).
//!
//! All of it runs with the e2e suite. A test that signs in registers its own
//! account by device and deletes it through the SDK before it ends.

use balaur_testkit::{e2e_enabled, gamend_url, run_until_with};

/// The SDK addon `editor/library/addons/gamend` holds, as a game requires it.
fn gamend_addon() -> Vec<(String, String)> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/library/addons/gamend");
    std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("no SDK addon at {}: {e}", root.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?.to_string();
            if path.extension().is_none_or(|e| e != "rn") {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            Some((format!("addons/gamend/{name}"), text))
        })
        .collect()
}

#[test]
fn the_sdk_addon_reads_the_public_api_of_the_server() {
    if !e2e_enabled() {
        return;
    }
    let url = gamend_url();
    let files = gamend_addon();
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    // A query, a path parameter and a bare call, through the addon's own
    // functions. One line at the end: the harness reads a fifty-entry ring.
    let source = format!(
        r#"
pub async fn init(this) {{
    let api = script::require("addons/gamend/api.rn");
    let events = script::require("addons/gamend/events.rn");
    gamend::configure("{url}");
    let boards = task::wait((api.leaderboards_list_leaderboards)((), #{{ "page": 1 }})).await;
    let listed = boards["body"]["data"];
    let records = if listed.len() > 0 {{
        task::wait((api.leaderboards_list_leaderboard_records)((), listed[0]["id"], #{{}})).await["status"]
    }} else {{
        200
    }};
    let stats = task::wait((api.lobbies_lobby_stats)(())).await;
    let named = (events.decode)("lobby:7", "user_joined", #{{ "user_id": 3 }});
    log::info(`sdk ${{boards["status"]}} ${{records}} ${{stats["status"]}} | ${{named["kind"]}}`);
}}
"#
    );
    run_until_with(
        &borrowed,
        &source,
        &["sdk 200 200 200 | lobby_member_joined"],
    );
}

#[test]
fn a_script_signs_in_connects_calls_a_hook_and_deletes_its_account() {
    if !e2e_enabled() {
        return;
    }
    let url = gamend_url();
    let device = device_id();
    let files = gamend_addon();
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let source = format!(
        r#"
pub async fn init(this) {{
    gamend::configure("{url}");
    let login = task::wait(gamend::login((), #{{ device_id: "{device}" }})).await;
    if login.contains_key("error") {{
        log::info(format!("gamend-live login failed: {{}}", login["error"]));
        return;
    }}
    this.socket = gamend::connect(this.node);
}}

pub async fn on_gamend_event(this, e) {{
    if e["kind"] == "open" {{
        let api = script::require("addons/gamend/api.rn");
        let me = task::wait((api.users_get_current_user)(())).await;
        let hook = task::wait(gamend::call_hook(this.socket, "sdk_probe", "echo", ["hi"])).await;
        gamend::close(this.socket);
        let gone = task::wait((api.user_delete_current_user)((), ())).await;
        log::info(format!("gamend-live {{}} {{}} {{}}", me["status"], hook["status"], gone["status"]));
    }}
}}
"#
    );
    // No plugin answers `sdk_probe` on a stock server, so the hook's reply is
    // an error, which still proves the whole path.
    run_until_with(&borrowed, &source, &["gamend-live 200 error 200"]);
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
