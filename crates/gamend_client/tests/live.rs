//! The whole client against a real Gamend server.
//!
//! Ignored by default: run a server with `mix dev.start` in the gamend
//! checkout, then `cargo test -p gamend_client -- --ignored`. Device login
//! creates its own throwaway account, so a fresh dev database is enough.

use gamend_client::{auth, Client, Credentials, Socket, SocketEvent};
use serde_json::json;

const SERVER: &str = "http://localhost:4000";

fn device_id() -> String {
    // Unique enough per run; the account is disposable.
    format!(
        "balaur-sdk-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

/// Poll until `pick` matches. Unmatched events stay in `backlog`, because a
/// reply and a follow-up push often arrive in one batch.
fn wait_for<T>(
    socket: &mut Socket,
    backlog: &mut Vec<SocketEvent>,
    mut pick: impl FnMut(&SocketEvent) -> Option<T>,
) -> T {
    for _ in 0..200 {
        let mut events = std::mem::take(backlog).into_iter();
        while let Some(event) = events.next() {
            if let Some(found) = pick(&event) {
                backlog.extend(events);
                return found;
            }
        }
        backlog.extend(socket.poll().unwrap());
    }
    panic!("timed out waiting for a socket event");
}

#[test]
#[ignore = "needs a running gamend server on localhost:4000"]
fn login_me_refresh_and_realtime_against_a_live_server() {
    let mut client = Client::new(SERVER);

    // Unauthenticated health check proves the server is there.
    let health = client.call("GET", "/api/v1/health", None).unwrap();
    assert_eq!(health.status, 200, "is `mix dev.start` running?");

    // Device login creates an anonymous account on first use.
    let session = auth::login(
        &mut client,
        &Credentials::Device {
            device_id: device_id(),
        },
    )
    .unwrap();
    assert!(!session.access_token.is_empty());
    assert!(!session.user_id.is_empty());
    assert_eq!(session.expires_in, 900);

    // The token authenticates REST calls; /me is the flat, unenveloped one.
    let me = client.call("GET", "/api/v1/me", None).unwrap();
    assert_eq!(me.status, 200);
    assert_eq!(me.body["id"], json!(session.user_id));

    // The refresh token trades for a working access token.
    let refreshed = auth::refresh(&client, &session.refresh_token).unwrap();
    assert!(!refreshed.access_token.is_empty());
    assert_eq!(refreshed.user_id, session.user_id);

    // Realtime: connect with the raw access token, join the own-user topic.
    let ws = format!(
        "{}/socket/websocket?token={}&vsn=2.0.0",
        SERVER.replace("http", "ws"),
        session.access_token
    );
    let mut socket = Socket::connect(&ws).unwrap();
    let mut backlog = Vec::new();
    let topic = format!("user:{}", session.user_id);
    let join_ref = socket.join(&topic, &json!({})).unwrap();
    let status = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference, status, ..
        } if *reference == join_ref => Some(status.clone()),
        _ => None,
    });
    assert_eq!(status, "ok");

    // Joining pushes the own profile; its id is ours.
    let updated = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Message { event, payload, .. } if event == "updated" => Some(payload.clone()),
        _ => None,
    });
    assert_eq!(updated["id"], json!(session.user_id));

    // call_hook round-trips even with no plugin installed: the error reply
    // proves the request/reply path.
    let hook_ref = socket
        .push(
            &topic,
            "call_hook",
            &json!({"plugin": "sdk_probe", "fn": "echo", "args": ["hi"]}),
        )
        .unwrap();
    let (status, response) = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference,
            status,
            response,
            ..
        } if *reference == hook_ref => Some((status.clone(), response.clone())),
        _ => None,
    });
    assert!(
        status == "ok" || response.get("error").is_some(),
        "call_hook should reply one way or the other: {status} {response}"
    );

    // A joined-but-wrong user topic is refused, not ignored.
    let bad_ref = socket
        .join("user:00000000-0000-7000-8000-000000000000", &json!({}))
        .unwrap();
    let status = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference, status, ..
        } if *reference == bad_ref => Some(status.clone()),
        _ => None,
    });
    assert_eq!(status, "error");
}
