//! The whole client against a real Gamend server: `GAMEND_URL`, or gamend.org.
//!
//! Part of the e2e suite. Each test registers its own account by device and
//! deletes it before it ends. A local `mix dev.start` is
//! `GAMEND_URL=http://localhost:4000`.

use balaur_gamend::client::{Client, Credentials, Session, Socket, SocketEvent, auth};
use balaur_testkit::{e2e_enabled, gamend_url};
use serde_json::json;

#[allow(
    clippy::disallowed_methods,
    reason = "names a throwaway test account, not simulation"
)]
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

/// Retry while the server answers 429: gamend.org takes ten sign-ins a
/// minute from one address.
fn patiently<T>(mut attempt: impl FnMut() -> anyhow::Result<T>) -> T {
    for _ in 0..6 {
        match attempt() {
            Err(e) if e.to_string().contains("(429)") => {
                std::thread::sleep(std::time::Duration::from_secs(15));
            }
            other => return other.unwrap(),
        }
    }
    attempt().unwrap()
}

/// Sign in by device on a client of its own.
fn sign_in(server: &str, device: &str) -> (Client, Session) {
    let mut client = Client::new(server);
    let credentials = Credentials::Device {
        device_id: device.to_string(),
    };
    let session = patiently(|| auth::login(&mut client, &credentials));
    (client, session)
}

#[test]
fn login_me_refresh_and_realtime_against_a_live_server() {
    if !e2e_enabled() {
        return;
    }
    let server = gamend_url();
    let mut client = Client::new(&server);

    let health = client.call("GET", "/api/v1/health", None).unwrap();
    assert_eq!(health.status, 200, "is {server} up?");

    // Device login creates an anonymous account on first use.
    let credentials = Credentials::Device {
        device_id: device_id(),
    };
    let session = patiently(|| auth::login(&mut client, &credentials));
    assert!(!session.access_token.is_empty());
    assert!(!session.user_id.is_empty());
    assert_eq!(session.expires_in, 900);

    // The token authenticates REST calls. /me comes under `data` from servers
    // with the API envelope and flat from older ones.
    let me = client.call("GET", "/api/v1/me", None).unwrap();
    assert_eq!(me.status, 200);
    let profile = me.body.get("data").unwrap_or(&me.body);
    assert_eq!(profile["id"], json!(session.user_id));

    let refreshed = patiently(|| auth::refresh(&client, &session.refresh_token));
    assert!(!refreshed.access_token.is_empty());
    assert_eq!(refreshed.user_id, session.user_id);

    let ws = format!(
        "{}/socket/websocket?token={}&vsn=2.0.0",
        server.replacen("http", "ws", 1),
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

    // Leaving the own topic is acknowledged like a join.
    let leave_ref = socket.leave(&topic).unwrap();
    let status = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference, status, ..
        } if *reference == leave_ref => Some(status.clone()),
        _ => None,
    });
    assert_eq!(status, "ok");

    let deleted = client.call("DELETE", "/api/v1/me", None).unwrap();
    assert_eq!(deleted.status, 200, "{}", deleted.body);
}

#[test]
fn a_device_registers_signs_in_again_and_deletes_its_account() {
    if !e2e_enabled() {
        return;
    }
    let server = gamend_url();
    let device = device_id();

    // A device the server has not seen registers a new account; the same
    // device again is the same account.
    let (mut client, first) = sign_in(&server, &device);
    let (_, second) = sign_in(&server, &device);
    assert_eq!(second.user_id, first.user_id);

    // With a password, deleting is something to prove.
    let password = format!("balaur-test-{}", device_id());
    let set = client
        .call(
            "PATCH",
            "/api/v1/me/password",
            Some(&json!({ "password": password })),
        )
        .unwrap();
    assert_eq!(set.status, 200, "{}", set.body);
    // A new password signs every session out; the refused refresh leaves the
    // caller its 401.
    let signed_out = client.call("GET", "/api/v1/me", None).unwrap();
    assert_eq!(signed_out.status, 401, "{}", signed_out.body);
    let (mut client, _) = sign_in(&server, &device);
    let refused = client.call("DELETE", "/api/v1/me", None).unwrap();
    assert_eq!(refused.status, 401, "{}", refused.body);
    let deleted = client
        .call(
            "DELETE",
            "/api/v1/me",
            Some(&json!({ "current_password": password })),
        )
        .unwrap();
    assert_eq!(deleted.status, 200, "{}", deleted.body);

    // Gone: the same device now registers a new account, deleted in turn.
    let (mut client, third) = sign_in(&server, &device);
    assert_ne!(third.user_id, first.user_id);
    let cleaned = client.call("DELETE", "/api/v1/me", None).unwrap();
    assert_eq!(cleaned.status, 200, "{}", cleaned.body);
}
