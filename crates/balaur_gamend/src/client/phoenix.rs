//! A Phoenix Channels client, protocol V2.
//!
//! [`Protocol`] is the wire logic with no socket in it: refs, joins, the
//! heartbeat and frame decoding, fed text and a clock by whoever owns the
//! connection. [`Socket`] is that protocol over a blocking tungstenite
//! websocket, for the native worker thread; the browser backend drives the
//! same [`Protocol`] from `WebSocket` callbacks.
//!
//! Client→server frames are always JSON text — Gamend sends binary only when
//! a connection asks for protobuf, which this client does not.
//!
//! Wire shape, both directions:
//! `[join_ref, ref, topic, event, payload]` as a JSON array; `join_ref` and
//! `ref` are stringified counters or null.

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Value};

/// How long the server may stay silent before a heartbeat goes unanswered
/// and the connection is declared dead. Phoenix's own default rhythm.
const HEARTBEAT_EVERY_SECONDS: f64 = 30.0;

/// One frame from the server, decoded.
#[derive(Debug)]
pub enum SocketEvent {
    /// The reply to a `join`, `push`, or `leave`, correlated by `reference`.
    Reply {
        topic: String,
        reference: String,
        /// `"ok"` or `"error"`.
        status: String,
        response: Value,
    },
    /// A server-pushed event on a joined topic.
    Message {
        topic: String,
        event: String,
        payload: Value,
    },
    /// The connection ended; no further events will come.
    Closed { reason: String },
}

/// The channel protocol without a transport: what to send, and what a
/// received frame means. Time is handed in as seconds, so a browser can pass
/// `Date.now()` and a worker thread its own clock.
pub struct Protocol {
    next_ref: u64,
    /// Topic → the join_ref it was joined under; Phoenix routes channel
    /// traffic by it.
    joins: Vec<(String, String)>,
    last_heartbeat: f64,
    /// The in-flight heartbeat's ref; a second beat coming due while one is
    /// unanswered means the server is gone.
    pending_heartbeat: Option<String>,
}

impl Protocol {
    #[must_use]
    pub fn new(now: f64) -> Self {
        Self {
            next_ref: 1,
            joins: Vec::new(),
            last_heartbeat: now,
            pending_heartbeat: None,
        }
    }

    fn fresh_ref(&mut self) -> String {
        let reference = self.next_ref.to_string();
        self.next_ref += 1;
        reference
    }

    fn frame(
        join_ref: Option<&str>,
        reference: &str,
        topic: &str,
        event: &str,
        payload: &Value,
    ) -> String {
        json!([join_ref, reference, topic, event, payload]).to_string()
    }

    fn join_ref_of(&self, topic: &str) -> Option<&str> {
        self.joins
            .iter()
            .find(|(t, _)| t == topic)
            .map(|(_, j)| j.as_str())
    }

    /// The frame that joins a topic, and the ref its reply will carry.
    pub fn join(&mut self, topic: &str, payload: &Value) -> (String, String) {
        let reference = self.fresh_ref();
        self.joins.retain(|(t, _)| t != topic);
        self.joins.push((topic.to_string(), reference.clone()));
        let frame = Self::frame(Some(&reference), &reference, topic, "phx_join", payload);
        (reference, frame)
    }

    /// The frame that leaves a topic, and the ref its reply will carry.
    ///
    /// # Errors
    /// If the topic was never joined.
    pub fn leave(&mut self, topic: &str) -> Result<(String, String)> {
        let reference = self.fresh_ref();
        let join_ref = self
            .join_ref_of(topic)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("not joined to {topic}"))?;
        let frame = Self::frame(Some(&join_ref), &reference, topic, "phx_leave", &json!({}));
        self.joins.retain(|(t, _)| t != topic);
        Ok((reference, frame))
    }

    /// The frame that pushes an event to a joined topic, and its ref.
    ///
    /// # Errors
    /// If the topic was never joined.
    pub fn push(&mut self, topic: &str, event: &str, payload: &Value) -> Result<(String, String)> {
        let reference = self.fresh_ref();
        let join_ref = self
            .join_ref_of(topic)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("not joined to {topic}"))?;
        let frame = Self::frame(Some(&join_ref), &reference, topic, event, payload);
        Ok((reference, frame))
    }

    /// A heartbeat frame when one is due, or nothing.
    ///
    /// # Errors
    /// If the last heartbeat went unanswered: the connection is dead.
    pub fn heartbeat(&mut self, now: f64) -> Result<Option<String>> {
        if now - self.last_heartbeat < HEARTBEAT_EVERY_SECONDS {
            return Ok(None);
        }
        if self.pending_heartbeat.is_some() {
            bail!("the server missed a heartbeat; the connection is dead");
        }
        let reference = self.fresh_ref();
        let frame = Self::frame(None, &reference, "phoenix", "heartbeat", &json!({}));
        self.pending_heartbeat = Some(reference);
        self.last_heartbeat = now;
        Ok(Some(frame))
    }

    /// Decode one V2 frame; `None` for internal traffic (heartbeat replies).
    ///
    /// # Errors
    /// If the text is not a five-element JSON array.
    pub fn decode(&mut self, text: &str) -> Result<Option<SocketEvent>> {
        let frame: Value = serde_json::from_str(text).context("malformed frame")?;
        let parts = frame
            .as_array()
            .filter(|a| a.len() == 5)
            .ok_or_else(|| anyhow!("a frame is a 5-element array, got: {text}"))?;
        let reference = parts[1].as_str().unwrap_or_default().to_string();
        let topic = parts[2].as_str().unwrap_or_default().to_string();
        let event = parts[3].as_str().unwrap_or_default().to_string();
        let payload = parts[4].clone();

        if topic == "phoenix" || Some(reference.as_str()) == self.pending_heartbeat.as_deref() {
            self.pending_heartbeat = None;
            return Ok(None);
        }
        if event == "phx_reply" {
            return Ok(Some(SocketEvent::Reply {
                topic,
                reference,
                status: payload
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                response: payload.get("response").cloned().unwrap_or(Value::Null),
            }));
        }
        if event == "phx_error" || event == "phx_close" {
            self.joins.retain(|(t, _)| t != &topic);
        }
        Ok(Some(SocketEvent::Message {
            topic,
            event,
            payload,
        }))
    }
}

#[cfg(not(target_family = "wasm"))]
pub use blocking::Socket;

/// The protocol over a blocking websocket, polled from a worker thread.
#[cfg(not(target_family = "wasm"))]
mod blocking {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    use anyhow::{Context as _, Result};
    use serde_json::Value;
    use tungstenite::stream::MaybeTlsStream;
    use tungstenite::{Message, WebSocket};

    use super::{Protocol, SocketEvent};

    pub struct Socket {
        connection: WebSocket<MaybeTlsStream<TcpStream>>,
        protocol: Protocol,
        started: Instant,
    }

    impl Socket {
        /// Connect and complete the websocket handshake. `url` must be the
        /// full endpoint with query parameters, e.g.
        /// `ws://localhost:4000/socket/websocket?token=...&vsn=2.0.0`.
        #[allow(
            clippy::disallowed_methods,
            reason = "connection keep-alive, not simulation"
        )]
        pub fn connect(url: &str) -> Result<Self> {
            let (connection, _) = tungstenite::connect(url).context("websocket handshake")?;
            let mut socket = Self {
                connection,
                protocol: Protocol::new(0.0),
                started: Instant::now(),
            };
            socket.set_read_timeout(Duration::from_millis(25))?;
            Ok(socket)
        }

        /// The read timeout is what turns the blocking read into polling.
        fn set_read_timeout(&mut self, timeout: Duration) -> Result<()> {
            match self.connection.get_ref() {
                MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout))?,
                MaybeTlsStream::Rustls(stream) => {
                    stream.get_ref().set_read_timeout(Some(timeout))?;
                }
                _ => {}
            }
            Ok(())
        }

        #[allow(
            clippy::disallowed_methods,
            reason = "connection keep-alive, not simulation"
        )]
        fn now(&self) -> f64 {
            self.started.elapsed().as_secs_f64()
        }

        fn send_text(&mut self, frame: String) -> Result<()> {
            self.connection
                .send(Message::Text(frame.into()))
                .context("websocket send")
        }

        /// Join a topic; the server's verdict arrives as a
        /// [`SocketEvent::Reply`] carrying the returned ref.
        pub fn join(&mut self, topic: &str, payload: &Value) -> Result<String> {
            let (reference, frame) = self.protocol.join(topic, payload);
            self.send_text(frame)?;
            Ok(reference)
        }

        /// Leave a topic. The reply, like every reply, comes through `poll`.
        pub fn leave(&mut self, topic: &str) -> Result<String> {
            let (reference, frame) = self.protocol.leave(topic)?;
            self.send_text(frame)?;
            Ok(reference)
        }

        /// Push an event to a joined topic, returning the ref its reply will
        /// carry.
        pub fn push(&mut self, topic: &str, event: &str, payload: &Value) -> Result<String> {
            let (reference, frame) = self.protocol.push(topic, event, payload)?;
            self.send_text(frame)?;
            Ok(reference)
        }

        /// Drain arrived frames and keep the heartbeat alive. Blocks at most
        /// one read timeout when the socket is quiet. An `Err` means the
        /// connection is unusable; `SocketEvent::Closed` in the batch means
        /// it ended.
        pub fn poll(&mut self) -> Result<Vec<SocketEvent>> {
            if let Some(frame) = self.protocol.heartbeat(self.now())? {
                self.send_text(frame)?;
            }
            let mut events = Vec::new();
            loop {
                match self.connection.read() {
                    Ok(Message::Text(text)) => {
                        if let Some(event) = self.protocol.decode(text.as_str())? {
                            events.push(event);
                        }
                    }
                    Ok(Message::Binary(_)) => {
                        tracing::warn!("binary frame on a JSON connection; dropped");
                    }
                    Ok(Message::Close(frame)) => {
                        events.push(SocketEvent::Closed {
                            reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
                        });
                        return Ok(events);
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(err)) if idle(&err) => return Ok(events),
                    Err(
                        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed,
                    ) => {
                        events.push(SocketEvent::Closed {
                            reason: String::new(),
                        });
                        return Ok(events);
                    }
                    Err(err) => return Err(err.into()),
                }
            }
        }
    }

    /// A timed-out read is the poll breathing, not a failure.
    fn idle(err: &std::io::Error) -> bool {
        matches!(
            err.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_join_frame_carries_its_own_ref_as_join_ref() {
        let mut protocol = Protocol::new(0.0);
        let (reference, frame) = protocol.join("room:1", &json!({ "a": 1 }));
        assert_eq!(reference, "1");
        assert_eq!(frame, r#"["1","1","room:1","phx_join",{"a":1}]"#);
    }

    #[test]
    fn a_push_reuses_the_topics_join_ref_and_takes_a_fresh_ref() {
        let mut protocol = Protocol::new(0.0);
        protocol.join("room:1", &json!({}));
        let (reference, frame) = protocol.push("room:1", "shout", &json!({})).unwrap();
        assert_eq!(reference, "2");
        assert_eq!(frame, r#"["1","2","room:1","shout",{}]"#);
    }

    #[test]
    fn pushing_to_an_unjoined_topic_is_refused() {
        let mut protocol = Protocol::new(0.0);
        assert!(protocol.push("room:1", "shout", &json!({})).is_err());
    }

    #[test]
    fn a_heartbeat_is_due_after_thirty_seconds_and_an_unanswered_one_kills_the_link() {
        let mut protocol = Protocol::new(0.0);
        assert!(protocol.heartbeat(10.0).unwrap().is_none());
        let beat = protocol.heartbeat(31.0).unwrap().unwrap();
        assert!(beat.contains(r#""phoenix","heartbeat""#));
        assert!(protocol.heartbeat(62.0).is_err());
    }

    #[test]
    fn a_heartbeat_reply_is_swallowed_and_clears_the_pending_beat() {
        let mut protocol = Protocol::new(0.0);
        let beat = protocol.heartbeat(31.0).unwrap().unwrap();
        let reference: Value = serde_json::from_str(&beat).unwrap();
        let reference = reference[1].as_str().unwrap().to_string();
        let reply = format!(
            r#"[null,"{reference}","phoenix","phx_reply",{{"status":"ok","response":{{}}}}]"#
        );
        assert!(protocol.decode(&reply).unwrap().is_none());
        assert!(protocol.heartbeat(62.0).unwrap().is_some());
    }

    #[test]
    fn a_reply_decodes_with_its_status_and_response() {
        let mut protocol = Protocol::new(0.0);
        let (reference, _) = protocol.join("room:1", &json!({}));
        let text = format!(
            r#"["1","{reference}","room:1","phx_reply",{{"status":"ok","response":{{"n":3}}}}]"#
        );
        match protocol.decode(&text).unwrap() {
            Some(SocketEvent::Reply {
                reference: got,
                status,
                response,
                ..
            }) => {
                assert_eq!(got, reference);
                assert_eq!(status, "ok");
                assert_eq!(response, json!({ "n": 3 }));
            }
            other => panic!("expected a reply, got {other:?}"),
        }
    }

    #[test]
    fn a_server_close_forgets_the_topics_join() {
        let mut protocol = Protocol::new(0.0);
        protocol.join("room:1", &json!({}));
        let text = r#"["1","2","room:1","phx_close",{}]"#;
        assert!(matches!(
            protocol.decode(text).unwrap(),
            Some(SocketEvent::Message { .. })
        ));
        assert!(protocol.push("room:1", "shout", &json!({})).is_err());
    }
}
