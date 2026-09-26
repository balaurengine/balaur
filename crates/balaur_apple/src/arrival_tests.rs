//! What an arrival looks like to a script: its kind and what it carries.

use crate::{AppleEvent, event_value};
use balaur_script::Value;

fn field(value: &Value, key: &str) -> Value {
    let Value::Map(pairs) = value else {
        panic!("not a map: {value:?}");
    };
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map_or(Value::Nil, |(_, v)| v.clone())
}

#[test]
fn an_arrival_reaches_a_script_with_its_kind_and_what_it_carries() {
    let shown = event_value(AppleEvent::NotificationReceived {
        id: "daily".into(),
        title: "Chest".into(),
        body: "Ready".into(),
        data: serde_json::json!({ "chest": 3 }),
    });
    assert_eq!(
        field(&shown, "kind"),
        Value::Str("notification_received".into())
    );
    assert_eq!(field(&field(&shown, "data"), "chest"), Value::Int(3));
    let asked = event_value(AppleEvent::MatchRequested {
        players: vec!["G:1".into(), "G:2".into()],
    });
    assert_eq!(
        field(&asked, "players"),
        Value::List(vec![Value::Str("G:1".into()), Value::Str("G:2".into())])
    );
    let changed = event_value(AppleEvent::CloudChanged {
        reason: "server".into(),
        keys: vec!["save".into()],
    });
    assert_eq!(field(&changed, "reason"), Value::Str("server".into()));
    assert_eq!(
        field(&changed, "request"),
        Value::Int(0),
        "nobody asked for it"
    );
}
