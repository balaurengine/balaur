//! What a dock or a game's own debug screen reads: the session, the server
//! target, each reply by its request, and the recent calls. Observers all,
//! except `restore`, which signs in from a kept session.

use anyhow::Result;
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::GamendState;
use crate::client::Session;

pub(crate) fn install(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("activity", &[], "()", "The last calls and socket messages, newest first, as `{ request, kind, what, status, ms, args, reply }`; `ms` is nil while a call waits, and only the newest fifty keep `args` and `reply`. An observer: never simulate from it."),
        ("connection", &[], "()", "The configured `url`, who is signed in (`user_id`, `username`), and each socket as `{ socket, open, topics, reason }`."),
        ("session", &[], "()", "The signed-in session as `{ user_id, username, display_name, access_token, refresh_token, expires_in, expires_at }`, `expires_at` read from the token itself; nil when nobody is signed in."),
        ("restore", &[], "(session: map?)", "Sign in from a session a previous run kept, as `session` answers it, without asking the server; nil signs out here. False when there is no server to sign in to, or the map carries no `access_token`."),
        ("reply", &[], "(request: int)", "What a call answered, as its event map, once it has; nil while it waits. The newest sixty-four are kept, so a drawing loop can issue a call and read it back on a later frame."),
        ("clear_activity", &[], "()", "Forget the calls and messages `activity` holds."),
        ("target", &[], "()", "The server a `configure()` with no URL uses: `{ name, url, production, local, plugin }` from `[gamend]`, `name` being `production` or `local`."),
    ]);
    m.function("activity", |eng: &Engine, (): ()| {
        Ok(eng.resource::<GamendState>().borrow().activity.entries())
    });
    m.function("connection", |eng: &Engine, (): ()| {
        Ok(eng.resource::<GamendState>().borrow().activity.connection())
    });
    m.function("session", |eng: &Engine, (): ()| {
        let state = eng.resource::<GamendState>();
        let session = state
            .borrow()
            .client
            .as_ref()
            .and_then(crate::backend::SharedClient::session);
        Ok(session.as_ref().map_or(Value::Nil, session_value))
    });
    m.function("restore", |eng: &Engine, session: Option<Value>| {
        Ok(Value::Bool(restore(eng, session.as_ref())))
    });
    m.function("reply", |eng: &Engine, request: i64| {
        let reply = u64::try_from(request)
            .ok()
            .and_then(|id| eng.resource::<GamendState>().borrow().activity.reply(id));
        Ok(reply.unwrap_or(Value::Nil))
    });
    m.function("clear_activity", |eng: &Engine, (): ()| {
        eng.resource::<GamendState>().borrow_mut().activity.clear();
        Ok(Value::Nil)
    });
    m.function("target", |eng: &Engine, (): ()| {
        Ok(crate::target::value(eng))
    });
}

fn restore(eng: &Engine, value: Option<&Value>) -> bool {
    let state = eng.resource::<GamendState>();
    let mut state = state.borrow_mut();
    let Some(client) = state.client.clone() else {
        return false;
    };
    let session = match value {
        None | Some(Value::Nil) => None,
        Some(value) => match session_of(value) {
            Ok(session) => Some(session),
            Err(_) => return false,
        },
    };
    state.activity.signed_in(
        session
            .as_ref()
            .map(|s| (s.user_id.clone(), s.username.clone())),
    );
    client.set_session(session);
    true
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    let Value::Map(pairs) = value else {
        return None;
    };
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| match v {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
}

fn session_of(value: &Value) -> Result<Session> {
    let access_token = text_of(value, "access_token")
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow::anyhow!("a session needs an access_token"))?;
    let expires_in = match value {
        Value::Map(pairs) => pairs
            .iter()
            .find(|(k, _)| k == "expires_in")
            .and_then(|(_, v)| match v {
                Value::Int(n) => u64::try_from(*n).ok(),
                _ => None,
            })
            .unwrap_or(0),
        _ => 0,
    };
    Ok(Session {
        access_token,
        refresh_token: text_of(value, "refresh_token").unwrap_or_default(),
        user_id: text_of(value, "user_id").unwrap_or_default(),
        username: text_of(value, "username").unwrap_or_default(),
        display_name: text_of(value, "display_name").unwrap_or_default(),
        expires_in,
    })
}

fn session_value(session: &Session) -> Value {
    let expires_at = expiry(&session.access_token).map_or(Value::Nil, Value::Int);
    Value::Map(vec![
        (String::from("user_id"), Value::Str(session.user_id.clone())),
        (
            String::from("username"),
            Value::Str(session.username.clone()),
        ),
        (
            String::from("display_name"),
            Value::Str(session.display_name.clone()),
        ),
        (
            String::from("access_token"),
            Value::Str(session.access_token.clone()),
        ),
        (
            String::from("refresh_token"),
            Value::Str(session.refresh_token.clone()),
        ),
        (
            String::from("expires_in"),
            Value::Int(i64::try_from(session.expires_in).unwrap_or(i64::MAX)),
        ),
        (String::from("expires_at"), expires_at),
    ])
}

/// When a JWT stops working, in seconds since 1970, read from its own `exp`
/// claim; `None` for a token that is not a JWT.
fn expiry(token: &str) -> Option<i64> {
    use base64::Engine as _;
    let claims = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(claims.trim_end_matches('='))
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json.get("exp")?.as_i64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_survives_a_round_trip_and_its_expiry_comes_from_the_token() {
        use base64::Engine as _;
        let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"u1","exp":1790000000}"#);
        let session = Session {
            access_token: format!("head.{claims}.sig"),
            refresh_token: String::from("r"),
            user_id: String::from("u1"),
            username: String::from("tester"),
            display_name: String::new(),
            expires_in: 900,
        };
        let value = session_value(&session);
        let back = session_of(&value).unwrap();
        assert_eq!(back.access_token, session.access_token);
        assert_eq!(back.username, "tester");
        assert_eq!(back.expires_in, 900);
        assert_eq!(expiry(&session.access_token), Some(1_790_000_000));
    }

    #[test]
    fn a_session_without_a_token_is_refused() {
        let value = Value::Map(vec![(String::from("user_id"), Value::Str("u".into()))]);
        assert!(session_of(&value).is_err());
    }
}
