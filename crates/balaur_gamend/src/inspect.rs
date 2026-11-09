//! What a dock or a game's own debug screen reads: the session, the server
//! target, each reply by its request, and the recent calls. Observers all,
//! except `restore`, which signs in from a kept session.

use anyhow::Result;
use balaur_core::Engine;
use balaur_script::{Module, Value};

use crate::GamendState;
use crate::client::Session;
use crate::vocabulary::words;

const ACTIVITY: &str = "activity";
const CLEAR_ACTIVITY: &str = "clear_activity";
const CONNECTION: &str = "connection";
const RESTORE: &str = "restore";
const RUN_ID: &str = "run_id";
const SERVER: &str = "server";
const SESSION: &str = "session";

pub(crate) fn install(m: &mut Module<Engine>) {
    m.records(&[
        (
            "Entry",
            "{ seq: int, request: int, kind: string, what: string, status: string, ms: float?, args: any, reply: any }",
            "One call or socket message the plugin kept.",
        ),
        (
            "Socket",
            "{ socket: int, open: bool, topics: [string], reason: string? }",
            "A realtime socket, whether it is open and the topics it joined.",
        ),
        (
            "Connection",
            "{ url: string, user_id: string?, username: string?, sockets: [Socket] }",
            "The configured server, who is signed in and each socket.",
        ),
        (
            "Session",
            "{ user_id: string, username: string, display_name: string, access_token: string, refresh_token: string, expires_in: int, expires_at: int? }",
            "A signed-in session and when its token expires.",
        ),
        (
            "Server",
            "{ name: string, url: string, production: string, local: string, plugin: string }",
            "The Gamend server a run talks to and the two the project names.",
        ),
    ]);
    m.describe(&[
        (ACTIVITY, &[], "() -> [Entry]", "The last calls and socket messages, newest first, as `{ seq, request, kind, what, status, ms, args, reply }`, `seq` counting every row ever kept; `ms` is nil while a call waits, and only the newest fifty keep `args` and `reply`. An observer: never simulate from it."),
        (CONNECTION, &[], "() -> Connection", "The configured `url`, who is signed in (`user_id`, `username`), and each socket as `{ socket, open, topics, reason }`."),
        (SESSION, &[], "() -> Session?", "The signed-in session as `{ user_id, username, display_name, access_token, refresh_token, expires_in, expires_at }`, `expires_at` read from the token itself; nil when nobody is signed in."),
        (RESTORE, &[], "(session: Session?) -> bool", "Sign in from a session a previous run kept, as `session` answers it, without asking the server; nil signs out here. False when there is no server to sign in to, or the map carries no `access_token`."),
        (words::functions::REPLY, &[], "(request: int) -> any", "What a call answered, as its event map, once it has; nil while it waits. The newest sixty-four are kept, so a drawing loop can issue a call and read it back on a later frame."),
        (CLEAR_ACTIVITY, &[], "()", "Forget the calls and messages `activity` holds."),
        (SERVER, &[], "() -> Server", "The server this run talks to until a script names another: `{ name, url, production, local, plugin }` from `[gamend]`, `name` being `production` or `local`."),
        (RUN_ID, &[], "() -> string", "This run's id: every call sends it as `x-gamend-session` and the socket as `client_session`, so the server files a run's lines together. The log shipper names its run with it."),
    ]);
    m.function(ACTIVITY, |eng: &Engine, (): ()| {
        Ok(eng.resource::<GamendState>().borrow().activity.entries())
    });
    m.function(CONNECTION, |eng: &Engine, (): ()| {
        Ok(eng.resource::<GamendState>().borrow().activity.connection())
    });
    m.function(SESSION, |eng: &Engine, (): ()| {
        let state = eng.resource::<GamendState>();
        let session = state
            .borrow()
            .client
            .as_ref()
            .and_then(crate::backend::SharedClient::session);
        Ok(session.as_ref().map_or(Value::Nil, session_value))
    });
    m.function(RESTORE, |eng: &Engine, session: Option<Value>| {
        Ok(Value::Bool(restore(eng, session.as_ref())))
    });
    m.function(words::functions::REPLY, |eng: &Engine, request: i64| {
        let reply = u64::try_from(request)
            .ok()
            .and_then(|id| eng.resource::<GamendState>().borrow().activity.reply(id));
        Ok(reply.unwrap_or(Value::Nil))
    });
    m.function(CLEAR_ACTIVITY, |eng: &Engine, (): ()| {
        eng.resource::<GamendState>().borrow_mut().activity.clear();
        Ok(Value::Nil)
    });
    m.function(SERVER, |eng: &Engine, (): ()| Ok(crate::target::value(eng)));
    m.function(RUN_ID, |_: &Engine, (): ()| {
        Ok(Value::Str(crate::client::run_id().to_string()))
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
    let access_token = text_of(value, words::inspect::ACCESS_TOKEN)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow::anyhow!("a session needs an access_token"))?;
    let expires_in = match value {
        Value::Map(pairs) => pairs
            .iter()
            .find(|(k, _)| k == words::EXPIRES_IN)
            .and_then(|(_, v)| match v {
                Value::Int(n) => u64::try_from(*n).ok(),
                _ => None,
            })
            .unwrap_or(0),
        _ => 0,
    };
    Ok(Session {
        access_token,
        refresh_token: text_of(value, words::inspect::REFRESH_TOKEN).unwrap_or_default(),
        user_id: text_of(value, words::inspect::USER_ID).unwrap_or_default(),
        username: text_of(value, words::auth::USERNAME).unwrap_or_default(),
        display_name: text_of(value, words::inspect::DISPLAY_NAME).unwrap_or_default(),
        expires_in,
    })
}

fn session_value(session: &Session) -> Value {
    let expires_at = session.expires_at().map_or(Value::Nil, Value::Int);
    Value::Map(vec![
        (
            String::from(words::inspect::USER_ID),
            Value::Str(session.user_id.clone()),
        ),
        (
            String::from(words::auth::USERNAME),
            Value::Str(session.username.clone()),
        ),
        (
            String::from(words::inspect::DISPLAY_NAME),
            Value::Str(session.display_name.clone()),
        ),
        (
            String::from(words::inspect::ACCESS_TOKEN),
            Value::Str(session.access_token.clone()),
        ),
        (
            String::from(words::inspect::REFRESH_TOKEN),
            Value::Str(session.refresh_token.clone()),
        ),
        (
            String::from(words::EXPIRES_IN),
            Value::Int(i64::try_from(session.expires_in).unwrap_or(i64::MAX)),
        ),
        (String::from("expires_at"), expires_at),
    ])
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
        assert_eq!(session.expires_at(), Some(1_790_000_000));
    }

    #[test]
    fn a_session_without_a_token_is_refused() {
        let value = Value::Map(vec![(String::from("user_id"), Value::Str("u".into()))]);
        assert!(session_of(&value).is_err());
    }
}
