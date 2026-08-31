//! The Phoenix V2 protocol against a miniature in-process server, so the
//! framing, refs and reply routing are proven without an Elixir install.

use std::net::TcpListener;

use balaur_gamend::client::{Socket, SocketEvent};
use serde_json::{json, Value};

/// A one-connection Phoenix-ish server: acks joins, echoes `echo` pushes
/// into their replies, then pushes one server event on the joined topic.
fn serve_phoenix() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection = tungstenite::accept(stream).unwrap();
        loop {
            let message = match connection.read() {
                Ok(m) if m.is_text() => m,
                Ok(tungstenite::Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            };
            let frame: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            let (join_ref, reference, topic, event, payload) = (
                frame[0].clone(),
                frame[1].clone(),
                frame[2].as_str().unwrap().to_string(),
                frame[3].as_str().unwrap().to_string(),
                frame[4].clone(),
            );
            let reply = |status: &str, response: Value| {
                json!([join_ref, reference, topic, "phx_reply",
                       {"status": status, "response": response}])
                .to_string()
            };
            match event.as_str() {
                "phx_join" => {
                    connection
                        .send(tungstenite::Message::Text(reply("ok", json!({})).into()))
                        .unwrap();
                    // What a joined channel does: push something unprompted.
                    let push = json!([frame[0], Value::Null, topic, "hello",
                                      {"greeting": "hi"}]);
                    connection
                        .send(tungstenite::Message::Text(push.to_string().into()))
                        .unwrap();
                }
                "echo" => {
                    connection
                        .send(tungstenite::Message::Text(
                            reply("ok", json!({"data": payload})).into(),
                        ))
                        .unwrap();
                }
                "heartbeat" | "phx_leave" => {
                    connection
                        .send(tungstenite::Message::Text(reply("ok", json!({})).into()))
                        .unwrap();
                }
                _ => {
                    connection
                        .send(tungstenite::Message::Text(
                            reply("error", json!({"error": "unknown_event"})).into(),
                        ))
                        .unwrap();
                }
            }
        }
    });
    format!("ws://{addr}/socket/websocket?vsn=2.0.0")
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
fn join_reply_push_and_leave_round_trip() {
    let url = serve_phoenix();
    let mut socket = Socket::connect(&url).unwrap();
    let mut backlog = Vec::new();

    let join_ref = socket.join("room:1", &json!({})).unwrap();
    let status = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference, status, ..
        } if *reference == join_ref => Some(status.clone()),
        _ => None,
    });
    assert_eq!(status, "ok");

    // The unprompted server push arrives as a message on the topic.
    let greeting = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Message { event, payload, .. } if event == "hello" => {
            Some(payload["greeting"].as_str().unwrap_or_default().to_string())
        }
        _ => None,
    });
    assert_eq!(greeting, "hi");

    let echo_ref = socket.push("room:1", "echo", &json!({"n": 7})).unwrap();
    let response = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference,
            response,
            ..
        } if *reference == echo_ref => Some(response.clone()),
        _ => None,
    });
    assert_eq!(response["data"]["n"], 7);

    let leave_ref = socket.leave("room:1").unwrap();
    let status = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference, status, ..
        } if *reference == leave_ref => Some(status.clone()),
        _ => None,
    });
    assert_eq!(status, "ok");

    // After leaving, pushing to the topic is a local error, not a frame.
    assert!(socket.push("room:1", "echo", &json!({})).is_err());
}

#[test]
fn an_unknown_event_reply_carries_the_error_status() {
    let url = serve_phoenix();
    let mut socket = Socket::connect(&url).unwrap();
    let mut backlog = Vec::new();
    socket.join("room:1", &json!({})).unwrap();
    let bad_ref = socket.push("room:1", "no_such_event", &json!({})).unwrap();
    let (status, response) = wait_for(&mut socket, &mut backlog, |event| match event {
        SocketEvent::Reply {
            reference,
            status,
            response,
            ..
        } if *reference == bad_ref => Some((status.clone(), response.clone())),
        _ => None,
    });
    assert_eq!(status, "error");
    assert_eq!(response["error"], "unknown_event");
}
