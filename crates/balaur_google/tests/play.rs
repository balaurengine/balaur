//! What Play's backend sends each shim, checked off a phone: the class a
//! call reaches and the JSON the shim reads (`play/<capability>/*.java`).
//! The shims themselves need a device with Play services and a Play
//! Console app.

use balaur_google::play::{BILLING, GAMES, left_out, route};
use balaur_platform::{Call, Period, Scope};
use serde_json::json;

#[test]
fn the_player_achievements_and_leaderboards_reach_play_games() {
    let cases = [
        (Call::SignIn, json!({})),
        (
            Call::Unlock {
                achievement: "CgkI".into(),
            },
            json!({ "achievement": "CgkI" }),
        ),
        (
            Call::Progress {
                achievement: "CgkI".into(),
                percent: 50.0,
            },
            json!({ "achievement": "CgkI", "percent": 50.0 }),
        ),
        (
            Call::SubmitScore {
                board: "arena".into(),
                score: 12,
            },
            json!({ "board": "arena", "score": 12 }),
        ),
        (
            Call::Scores {
                board: "arena".into(),
                count: 5,
                scope: Scope::Friends,
                period: Period::Week,
                start: 3,
            },
            json!({ "board": "arena", "count": 5, "start": 3, "scope": "friends", "period": "week" }),
        ),
    ];
    for (call, args) in cases {
        assert_eq!(route(&call, ""), Some((GAMES, args)), "{}", call.name());
    }
}

#[test]
fn a_saved_game_and_the_server_code_reach_play_games() {
    assert_eq!(
        route(
            &Call::CloudWrite {
                key: "slot".into(),
                value: "{}".into()
            },
            ""
        ),
        Some((GAMES, json!({ "key": "slot", "value": "{}" })))
    );
    assert_eq!(
        route(&Call::CloudRead { key: "slot".into() }, ""),
        Some((GAMES, json!({ "key": "slot" })))
    );
    // The code is issued for the server's web client id.
    assert_eq!(
        route(&Call::Identity, "web.apps.googleusercontent.com"),
        Some((
            GAMES,
            json!({ "server_client_id": "web.apps.googleusercontent.com" })
        ))
    );
}

#[test]
fn purchases_reach_billing() {
    assert_eq!(
        route(
            &Call::Products {
                ids: vec!["gold".into()]
            },
            ""
        ),
        Some((BILLING, json!({ "ids": ["gold"] })))
    );
    assert_eq!(
        route(
            &Call::Purchase {
                product: "gold".into()
            },
            ""
        ),
        Some((BILLING, json!({ "product": "gold" })))
    );
    assert_eq!(
        route(
            &Call::FinishPurchase {
                transaction: "tok".into(),
                consume: true
            },
            ""
        ),
        Some((BILLING, json!({ "transaction": "tok", "consume": true })))
    );
    assert_eq!(
        route(&Call::RestorePurchases, ""),
        Some((BILLING, json!({})))
    );
    assert_eq!(route(&Call::Entitlements, ""), Some((BILLING, json!({}))));
}

// Play has no presence, and a sign-in page is the Custom Tab's.
#[test]
fn presence_and_a_sign_in_page_are_not_plays() {
    assert_eq!(route(&Call::SetPresence { text: "x".into() }, ""), None);
    let page = Call::WebSignIn {
        url: "https://provider.example".into(),
        scheme: None,
    };
    assert_eq!(route(&page, ""), None);
}

#[test]
fn a_capability_the_export_left_out_names_the_key_to_add() {
    assert!(left_out(GAMES).contains("\"play-games\""));
    assert!(left_out(BILLING).contains("\"in-app-purchase\""));
}
