//! The library's Gamend addon as a game calls it: a script client over the
//! engine's `http` and `websocket`. The live tests talk to a Gamend on port
//! 4000, or `GAMEND_URL`, on an e2e run, and each account they make is
//! deleted before they end.

mod client;
mod flows;
mod replay;

use std::time::Duration;

/// The addon `editor/library/addons/gamend` holds, as a game carries it:
/// every `.bs` module, `events/` among them, under `addons/gamend/`, with
/// the save addon it keeps its session in.
pub(crate) fn addon_files() -> Vec<(String, String)> {
    let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/library");
    let mut files = Vec::new();
    for addon in ["addons/gamend", "addons/save"] {
        let root = library.join(addon);
        let before = files.len();
        modules_under(&root, addon, &mut files);
        assert!(files.len() > before, "no addon at {}", root.display());
    }
    files
}

fn modules_under(dir: &std::path::Path, published: &str, files: &mut Vec<(String, String)>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let at = format!("{published}/{name}");
        if path.is_dir() {
            modules_under(&path, &at, files);
        } else if path.extension().is_some_and(|e| e == "bs") {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            files.push((at, text));
        }
    }
}

/// `source` with the addon beside it, run until it has said `markers`.
pub(crate) fn run_with_addon(source: &str, markers: &[&str], deadline: Duration) -> Vec<String> {
    let files = addon_files();
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    balaur_testkit::run_until_within(&borrowed, source, markers, deadline)
}

/// The live server, held by this test alone: a local Gamend on SQLite runs
/// out of connections when the suite's processes all reach it at once.
/// `None` skips the test, as `balaur_testkit::gamend_url` does.
pub(crate) fn server() -> Option<(String, std::fs::File)> {
    let url = balaur_testkit::gamend_url()?;
    let path = std::env::temp_dir().join("balaur-gamend-suite.lock");
    let turn = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));
    turn.lock()
        .unwrap_or_else(|e| panic!("cannot lock {}: {e}", path.display()));
    Some((url, turn))
}

/// A name no other run uses, for the accounts a test makes.
#[allow(
    clippy::disallowed_methods,
    reason = "names throwaway test accounts, not simulation"
)]
pub(crate) fn tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("t{}{}", std::process::id() % 1000, nanos % 1_000_000_000)
}

/// A device account on `server`, signed in over plain HTTP, waiting out the
/// server's limit on sign-ins: its id and access token.
pub(crate) fn sign_in(server: &str, device: &str) -> Option<(String, String)> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    for _ in 0..6 {
        let mut reply = agent
            .post(format!("{server}/api/v1/login/device"))
            .header("content-type", "application/json")
            .send(serde_json::json!({ "device_id": device }).to_string())
            .ok()?;
        if reply.status().as_u16() == 429 {
            std::thread::sleep(Duration::from_secs(15));
            continue;
        }
        let body: serde_json::Value =
            serde_json::from_str(&reply.body_mut().read_to_string().ok()?).ok()?;
        let data = &body["data"];
        return Some((
            data["user_id"].as_str()?.to_string(),
            data["access_token"].as_str()?.to_string(),
        ));
    }
    None
}

/// Delete the account a token signs in, with `current_password` when it has
/// one. Answers the status.
pub(crate) fn delete_account(server: &str, token: &str, password: &str) -> u16 {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    agent
        .delete(format!("{server}/api/v1/me"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .force_send_body()
        .send(serde_json::json!({ "current_password": password }).to_string())
        .map_or(0, |reply| reply.status().as_u16())
}
