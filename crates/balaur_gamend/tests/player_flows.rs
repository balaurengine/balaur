//! What a player does on a live Gamend server (`GAMEND_URL`, or gamend.org):
//! each flow in `player_flows.rn` calls the SDK addon the way a game does, and
//! a second player signed in here answers over plain HTTP.
//!
//! Part of the e2e suite. Every account a flow makes is deleted before it
//! ends, and the flow proves it; a flow that fails part-way is cleaned up
//! here.

use std::time::Duration;

use balaur_gamend::client::{Client, Credentials, Session, auth};
use balaur_testkit::{e2e_enabled, gamend_url, run_until_within};
use serde_json::json;

const FLOWS: &str = include_str!("player_flows.rn");

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

#[allow(
    clippy::disallowed_methods,
    reason = "names throwaway test accounts, not simulation"
)]
fn tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("t{}{}", std::process::id() % 1000, nanos % 1_000_000_000)
}

/// Sign in by device, waiting out the server's limit on sign-ins: gamend.org
/// takes ten a minute from one address.
fn sign_in(client: &mut Client, device: &str) -> anyhow::Result<Session> {
    let credentials = Credentials::Device {
        device_id: device.to_string(),
    };
    for _ in 0..6 {
        match auth::login(client, &credentials) {
            Err(e) if e.to_string().contains("(429)") => {
                std::thread::sleep(Duration::from_secs(15));
            }
            other => return other,
        }
    }
    auth::login(client, &credentials)
}

/// Deletes whatever the flow's devices still sign in to, unless the flow
/// finished and deleted them itself.
struct Leftovers {
    server: String,
    devices: Vec<String>,
    password: String,
    finished: bool,
}

impl Drop for Leftovers {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        for device in &self.devices {
            let mut client = Client::new(&self.server);
            if sign_in(&mut client, device).is_ok() {
                let body = json!({ "current_password": self.password });
                let _ = client.call("DELETE", "/api/v1/me", Some(&body));
            }
        }
    }
}

/// Run one flow and hold its report to `steps`, each `name=status` or a
/// check's `name=yes`.
fn run_flow(flow: &str, with_b: bool, steps: &[&str]) {
    let server = gamend_url();
    let tag = tag();
    let device = format!("balaur-flow-{tag}");
    let mut leftovers = Leftovers {
        server: server.clone(),
        devices: vec![device.clone(), format!("{device}-2"), format!("{device}-b")],
        password: format!("pw-{tag}-long"),
        finished: false,
    };
    let mut b = Client::new(&server);
    let (b_id, b_token) = if with_b {
        let session = sign_in(&mut b, &format!("{device}-b"))
            .unwrap_or_else(|e| panic!("player B could not sign in to {server}: {e}"));
        (session.user_id, session.access_token)
    } else {
        (String::new(), String::new())
    };
    let source = FLOWS
        .replace("@URL@", &server)
        .replace("@FLOW@", flow)
        .replace("@DEVICE@", &device)
        .replace("@TAG@", &tag)
        .replace("@B_ID@", &b_id)
        .replace("@B_TOKEN@", &b_token);
    let files = gamend_addon();
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let prefix = format!("flow {flow}:");
    let lines = run_until_within(&borrowed, &source, &[&prefix], Duration::from_secs(600));
    let line = &lines[0];
    let reported: Vec<&str> = line[line.find(&prefix).unwrap() + prefix.len()..]
        .split_whitespace()
        .collect();
    let mut expected = steps.to_vec();
    expected.push("end");
    assert_eq!(reported, expected, "{flow} reported something else");
    if with_b {
        let gone = b.call("DELETE", "/api/v1/me", None).unwrap();
        assert_eq!(gone.status, 200, "{}", gone.body);
    }
    leftovers.finished = true;
}

#[test]
fn a_player_renames_links_a_device_sets_a_password_and_deletes_the_account() {
    if !e2e_enabled() {
        return;
    }
    run_flow(
        "profile",
        false,
        &[
            "me=200",
            "display_name=200",
            "username=200",
            "me_again=200",
            "renamed=yes",
            "get_user=200",
            "public_name=yes",
            "search=200",
            "found=yes",
            "refresh=200",
            "refreshed=yes",
            "healed=200",
            "renewed=yes",
            "socket_healed=yes",
            "link_device=200",
            "linked=yes",
            "unlink_last=400",
            "set_password=200",
            "signed_out=401",
            "delete_refused=401",
            "delete=200",
            "lookup_deleted=404",
        ],
    );
}

#[test]
fn a_host_runs_a_lobby_with_chat_a_ready_check_and_moderation() {
    if !e2e_enabled() {
        return;
    }
    run_flow(
        "lobby",
        true,
        &[
            "create=201",
            "socket_open=yes",
            "channel_joined=yes",
            "get=200",
            "hosted=yes",
            "list=200",
            "listed=yes",
            "update=200",
            "updated=yes",
            "state=200",
            "playing=yes",
            "b_join=200",
            "get_full=200",
            "two=yes",
            "chat_send=201",
            "b_unread=200",
            "b_unread_one=yes",
            "chat_list=200",
            "chat_listed=yes",
            "chat_edit=200",
            "chat_get=200",
            "chat_edited=yes",
            "chat_read=200",
            "chat_delete=200",
            "ready_open=201",
            "b_ready=200",
            "ready_passed=yes",
            "mute=200",
            "mutes=200",
            "b_muted=yes",
            "unmute=200",
            "kick=200",
            "get_kicked=200",
            "one=yes",
            "heard_lobby_updated=yes",
            "heard_lobby_state_changed=yes",
            "heard_lobby_member_joined=yes",
            "heard_lobby_chat_message=yes",
            "heard_ready_check_passed=yes",
            "heard_lobby_member_kicked=yes",
            "leave=200",
            "gone=404",
            "quick_join=200",
            "disband=200",
            "quick_gone=404",
            "stats=200",
            "delete=200",
            "lookup_deleted=404",
        ],
    );
}

#[test]
fn two_players_befriend_party_up_and_share_groups() {
    if !e2e_enabled() {
        return;
    }
    run_flow(
        "social",
        true,
        &[
            // friends and notifications
            "b_befriend=201",
            "requests=200",
            "incoming=yes",
            "accept=200",
            "friends=200",
            "befriended=yes",
            "notify=201",
            "b_inbox=200",
            "delivered=yes",
            "b_dismiss=200",
            "own_inbox=200",
            "unfriend=200",
            "block=200",
            "blacklist=200",
            "b_blocked=yes",
            "unblock=200",
            "b_befriend_again=201",
            "accept_again=200",
            // party
            "party=201",
            "invite=200",
            "sent=200",
            "invite_sent=yes",
            "b_join_party=200",
            "show=200",
            "party_of_two=yes",
            "party_update=200",
            "ready_open=201",
            "b_ready=200",
            "ready_passed=yes",
            "party_lobby=201",
            "b_me=200",
            "b_followed=yes",
            "b_leave_lobby=200",
            "leave_lobby=200",
            "party_kick=200",
            "disband=200",
            // groups
            "group=201",
            "group_invite=200",
            "b_invites=200",
            "b_join_group=200",
            "members=200",
            "group_of_two=yes",
            "my_groups=200",
            "mine=yes",
            "group_update=200",
            "promote=200",
            "demote=200",
            "group_mute=200",
            "group_mutes=200",
            "b_muted=yes",
            "group_unmute=200",
            "group_chat=201",
            "b_unread=200",
            "b_unread_one=yes",
            "group_kick=200",
            "group_leave=200",
            "group_gone=200",
            "no_group=yes",
            "asked_group=201",
            "b_ask=201",
            "join_requests=200",
            "pending=yes",
            "approve=200",
            "b_leave_asked=200",
            "leave_asked=200",
            "asked_gone=200",
            "no_asked_group=yes",
            "delete=200",
            "lookup_deleted=404",
        ],
    );
}

#[test]
fn a_player_reads_the_economy_quests_boards_and_enters_a_tournament() {
    if !e2e_enabled() {
        return;
    }
    run_flow(
        "catalog",
        false,
        &[
            "wallet=200",
            "inventory=200",
            "ledger=200",
            "quests=200",
            "my_quests=200",
            "user_quests=200",
            "quest_stats=200",
            "store=200",
            "entitlements=200",
            "kv_missing=404",
            "log_policy=200",
            "hooks=200",
            "time=200",
            "clock=yes",
            "stats=200",
            "user_stats=200",
            "signaling=200",
            "providers=200",
            "health=200",
            "party_invites=200",
            "group_invites=200",
            "party_stats=200",
            "boards=200",
            "board=200",
            "records=200",
            "my_record=404",
            "around_me=200",
            "resolve=200",
            "tournaments=200",
            "cup=200",
            "enter=200",
            "entries=200",
            "entered=yes",
            "my_match=404",
            "standings=200",
            "withdraw=200",
            "push_register=201",
            "push_list=200",
            "push_listed=yes",
            "push_delete=200",
            "no_ticket=yes",
            "cancel=200",
            "queues=200",
            "delete=200",
            "lookup_deleted=404",
        ],
    );
}
