//! The store's purchases and a Java shim's answers, through the seam every
//! store sits behind: a canned store answering as StoreKit's and Play's shims
//! do, a test's stand-ins, and a replay of what they answered.

use std::sync::mpsc::Sender;

use balaur_core::replay::{self, ReplayFeed, ReplayMode};
use balaur_core::rollback::Clock;
use balaur_core::{App, AppConfig};
use balaur_platform::{
    Call, PlatformBackend, PlatformEvent, PlatformPlugin, PlatformSnapshot, PlatformState, StandIns,
};
use balaur_script::Value;
use serde_json::json;

/// A store answering as the shims do: the map its waiter receives.
struct Shimmed;

impl PlatformBackend for Shimmed {
    fn name(&self) -> &'static str {
        "shimmed"
    }

    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        let answer = match call {
            Call::SignIn => {
                json!({ "kind": "signed_in", "player": { "id": "g1", "alias": "Ana" }, "id": "g1", "alias": "Ana" })
            }
            Call::Products { ids } => {
                json!({ "kind": "products", "products": ids.iter().map(|id| json!({
                "id": id, "title": "Gold", "description": "", "price": 0.99,
                "display_price": "€0.99", "currency": "EUR", "subscription": false,
            })).collect::<Vec<_>>() })
            }
            Call::Purchase { product } => json!({
                "kind": "purchased", "id": "tok-1", "product": product, "purchased_at": 1.0,
                "quantity": 1, "revoked": false, "token": "tok-1", "verified": false,
            }),
            Call::FinishPurchase {
                transaction,
                consume,
            } => {
                json!({ "kind": "finished", "id": transaction, "consumed": consume })
            }
            Call::RestorePurchases => json!({ "kind": "restored" }),
            Call::Entitlements => json!({ "kind": "entitlements", "items": [] }),
            Call::Identity => json!({ "kind": "identity", "code": "4/one-time" }),
            other => json!({ "kind": "done", "call": other.name() }),
        };
        let _ = report.send(PlatformEvent::Answer { request, answer });
        if matches!(call, Call::Purchase { .. }) {
            // A pending payment that cleared, which nobody asked for.
            let _ = report.send(PlatformEvent::Answer {
                request: 0,
                answer: json!({ "kind": "transaction", "id": "tok-0", "product": "gems", "token": "tok-0" }),
            });
        }
    }
}

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    app
}

fn start(app: &App, id: u64, call: Call) {
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start(&app.engine, id, call);
}

fn field(map: &Value, key: &str) -> Option<Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

fn text(value: &str) -> Value {
    Value::Str(value.into())
}

fn events(app: &App) -> Vec<Value> {
    app.engine
        .resource::<PlatformSnapshot>()
        .borrow()
        .events
        .clone()
}

fn every_purchase_call() -> Vec<Call> {
    vec![
        Call::Products {
            ids: vec!["gold".into()],
        },
        Call::Purchase {
            product: "gold".into(),
        },
        Call::FinishPurchase {
            transaction: "tok-1".into(),
            consume: true,
        },
        Call::RestorePurchases,
        Call::Entitlements,
    ]
}

#[test]
fn every_purchase_call_answers_unsupported_with_no_store() {
    let mut app = app();
    let calls = every_purchase_call();
    let names: Vec<&str> = calls.iter().map(Call::name).collect();
    assert_eq!(
        names,
        [
            "products",
            "purchase",
            "finish_purchase",
            "restore_purchases",
            "entitlements"
        ]
    );
    for (n, call) in calls.into_iter().enumerate() {
        start(&app, n as u64 + 1, call);
    }
    app.tick(1.0 / 60.0);
    let answered = events(&app);
    assert_eq!(answered.len(), 5);
    for (event, name) in answered.iter().zip(names) {
        assert_eq!(field(event, "kind"), Some(text("unsupported")));
        assert_eq!(field(event, "call"), Some(text(name)));
    }
}

#[test]
fn a_purchase_answers_its_transaction_and_an_unasked_one_is_the_platform_event() {
    let mut app = app();
    balaur_platform::set_backend(&app.engine, Box::new(Shimmed)).unwrap();
    start(
        &app,
        4,
        Call::Purchase {
            product: "gold".into(),
        },
    );
    app.tick(1.0 / 60.0);
    let answered = events(&app);
    assert_eq!(field(&answered[0], "kind"), Some(text("purchased")));
    assert_eq!(field(&answered[0], "request"), Some(Value::Int(4)));
    assert_eq!(field(&answered[0], "token"), Some(text("tok-1")));
    assert_eq!(field(&answered[1], "kind"), Some(text("transaction")));
    assert_eq!(field(&answered[1], "request"), Some(Value::Int(0)));
}

// A purchase opens the store's sheet: a tick a late input could still send
// back must not open it twice.
#[test]
fn a_purchase_from_a_tick_that_can_still_roll_back_waits_for_it_to_settle() {
    let mut app = app();
    balaur_platform::set_backend(&app.engine, Box::new(Shimmed)).unwrap();
    set_clock(&app, 5, 3);
    start(
        &app,
        1,
        Call::Purchase {
            product: "gold".into(),
        },
    );
    set_clock(&app, 6, 3);
    app.tick(1.0 / 60.0);
    assert!(kinds(&app).is_empty(), "tick 5 can still be re-simulated");
    set_clock(&app, 9, 8);
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["purchased", "transaction"]);
}

fn set_clock(app: &App, tick: u64, settled: u64) {
    *app.engine.resource::<Clock>().borrow_mut() = Clock { tick, settled };
}

#[test]
fn a_shims_sign_in_lands_the_player_the_module_reads_back() {
    let mut app = app();
    balaur_platform::set_backend(&app.engine, Box::new(Shimmed)).unwrap();
    start(&app, 2, Call::SignIn);
    app.tick(1.0 / 60.0);
    let state = app.engine.resource::<PlatformState>();
    let player = state.borrow().player().cloned().expect("a player");
    assert_eq!((player.id.as_str(), player.alias.as_str()), ("g1", "Ana"));
}

#[test]
fn a_stand_in_answers_a_purchase_for_a_test() {
    let mut app = app();
    let stand_ins = StandIns::parse(
        r#"{ "platform.purchase": { "kind": "purchased", "id": "t", "product": "gold", "token": "test-token" },
             "platform.finish_purchase": { "kind": "finished", "id": "t" } }"#,
    )
    .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    start(
        &app,
        3,
        Call::Purchase {
            product: "gold".into(),
        },
    );
    start(
        &app,
        4,
        Call::FinishPurchase {
            transaction: "t".into(),
            consume: false,
        },
    );
    app.tick(1.0 / 60.0);
    let answered = events(&app);
    assert_eq!(field(&answered[0], "token"), Some(text("test-token")));
    assert_eq!(field(&answered[1], "kind"), Some(text("finished")));
    assert_eq!(field(&answered[1], "request"), Some(Value::Int(4)));
}

/// The kinds a tick delivered, in order.
fn kinds(app: &App) -> Vec<String> {
    events(app)
        .iter()
        .filter_map(|event| match field(event, "kind") {
            Some(Value::Str(kind)) => Some(kind),
            _ => None,
        })
        .collect()
}

// What Play's shims answered, played back from the file with no store
// loaded: a replay that reached Billing again would buy twice.
#[test]
fn recorded_shim_answers_replay_with_no_store_behind_them() {
    let calls = || {
        let mut calls = every_purchase_call();
        calls.push(Call::SignIn);
        calls.push(Call::Identity);
        calls
    };
    let mut recorded = app();
    balaur_platform::set_backend(&recorded.engine, Box::new(Shimmed)).unwrap();
    for (n, call) in calls().into_iter().enumerate() {
        start(&recorded, n as u64 + 1, call);
    }
    let mut frames = Vec::new();
    let mut heard = Vec::new();
    for _ in 0..4 {
        recorded.tick(1.0 / 60.0);
        frames.push(replay::capture(&recorded.engine));
        heard.extend(kinds(&recorded));
    }
    assert_eq!(
        heard,
        [
            "products",
            "purchased",
            "transaction",
            "finished",
            "restored",
            "entitlements",
            "signed_in",
            "identity"
        ]
    );

    let mut played = app();
    *played.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    for (n, call) in calls().into_iter().enumerate() {
        start(&played, n as u64 + 1, call);
    }
    let feed = played.engine.resource::<ReplayFeed>();
    let mut replayed = Vec::new();
    for frame in &frames {
        feed.borrow_mut().0 = Some(frame.clone());
        played.tick(1.0 / 60.0);
        replayed.extend(kinds(&played));
    }
    assert_eq!(replayed, heard);
    let state = played.engine.resource::<PlatformState>();
    assert_eq!(
        state.borrow().player().map(|p| p.id.clone()).as_deref(),
        Some("g1"),
        "the replayed sign-in lands the player too"
    );
}
