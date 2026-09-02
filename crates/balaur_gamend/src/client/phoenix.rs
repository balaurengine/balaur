//! A Phoenix Channels client, protocol V2, over a blocking websocket.
//!
//! The caller owns the pacing: [`Socket::poll`] reads whatever frames have
//! arrived (bounded by a short read timeout), keeps the heartbeat alive, and
//! returns the batch. Client→server frames are always JSON text — Gamend
//! sends binary only when a connection asks for protobuf, which this client
//! does not.
//!
//! Wire shape, both directions:
//! `[join_ref, ref, topic, event, payload]` as a JSON array; `join_ref` and
//! `ref` are stringified counters or null.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// How long the server may stay silent before a heartbeat goes unanswered
/// and the connection is declared dead. Phoenix's own default rhythm.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(30);

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

pub struct Socket {
    connection: WebSocket<MaybeTlsStream<TcpStream>>,
    next_ref: u64,
    /// Topic → the join_ref it was joined under; Phoenix routes channel
    /// traffic by it.
    joins: Vec<(String, String)>,
    last_heartbeat: Instant,
    /// The in-flight heartbeat's ref; a second beat coming due while one is
    /// unanswered means the server is gone.
    pending_heartbeat: Option<String>,
}

impl Socket {
    /// Connect and complete the websocket handshake. `url` must be the full
    /// endpoint with query parameters, e.g.
    /// `ws://localhost:4000/socket/websocket?token=...&vsn=2.0.0`.
    #[allow(clippy::disallowed_methods, reason = "connection keep-alive, not simulation")]
    pub fn connect(url: &str) -> Result<Self> {
        let (connection, _) = tungstenite::connect(url).context("websocket handshake")?;
        let mut socket = Self {
            connection,
            next_ref: 1,
            joins: Vec::new(),
            last_heartbeat: Instant::now(),
            pending_heartbeat: None,
        };
        socket.set_read_timeout(Duration::from_millis(25))?;
        Ok(socket)
    }

    /// The read timeout is what turns the blocking read into polling.
    fn set_read_timeout(&mut self, timeout: Duration) -> Result<()> {
        match self.connection.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout))?,
            MaybeTlsStream::Rustls(stream) => stream.get_ref().set_read_timeout(Some(timeout))?,
            _ => {}
        }
        Ok(())
    }

    fn fresh_ref(&mut self) -> String {
        let reference = self.next_ref.to_string();
        self.next_ref += 1;
        reference
    }

    fn send_frame(
        &mut self,
        join_ref: Option<&str>,
        reference: &str,
        topic: &str,
        event: &str,
        payload: &Value,
    ) -> Result<()> {
        let frame = json!([join_ref, reference, topic, event, payload]);
        self.connection
            .send(Message::Text(frame.to_string().into()))
            .context("websocket send")?;
        Ok(())
    }

    fn join_ref_of(&self, topic: &str) -> Option<&str> {
        self.joins
            .iter()
            .find(|(t, _)| t == topic)
            .map(|(_, j)| j.as_str())
    }

    /// Join a topic; the server's verdict arrives as a [`SocketEvent::Reply`]
    /// carrying the returned ref.
    pub fn join(&mut self, topic: &str, payload: &Value) -> Result<String> {
        let reference = self.fresh_ref();
        self.joins.retain(|(t, _)| t != topic);
        self.joins.push((topic.to_string(), reference.clone()));
        self.send_frame(
            Some(&reference.clone()),
            &reference,
            topic,
            "phx_join",
            payload,
        )?;
        Ok(reference)
    }

    /// Leave a topic. The reply, like every reply, comes through `poll`.
    pub fn leave(&mut self, topic: &str) -> Result<String> {
        let reference = self.fresh_ref();
        let join_ref = self
            .join_ref_of(topic)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("not joined to {topic}"))?;
        self.send_frame(Some(&join_ref), &reference, topic, "phx_leave", &json!({}))?;
        self.joins.retain(|(t, _)| t != topic);
        Ok(reference)
    }

    /// Push an event to a joined topic, returning the ref its reply will
    /// carry.
    pub fn push(&mut self, topic: &str, event: &str, payload: &Value) -> Result<String> {
        let reference = self.fresh_ref();
        let join_ref = self
            .join_ref_of(topic)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("not joined to {topic}"))?;
        self.send_frame(Some(&join_ref), &reference, topic, event, payload)?;
        Ok(reference)
    }

    /// Drain arrived frames and keep the heartbeat alive. Blocks at most one
    /// read timeout when the socket is quiet. An `Err` means the connection
    /// is unusable; `SocketEvent::Closed` in the batch means it ended.
    pub fn poll(&mut self) -> Result<Vec<SocketEvent>> {
        self.beat()?;
        let mut events = Vec::new();
        loop {
            match self.connection.read() {
                Ok(Message::Text(text)) => {
                    if let Some(event) = self.decode(text.as_str())? {
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
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    events.push(SocketEvent::Closed {
                        reason: String::new(),
                    });
                    return Ok(events);
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// Send a heartbeat when one is due; fail if the last went unanswered.
    #[allow(clippy::disallowed_methods, reason = "connection keep-alive, not simulation")]
    fn beat(&mut self) -> Result<()> {
        if self.last_heartbeat.elapsed() < HEARTBEAT_EVERY {
            return Ok(());
        }
        if self.pending_heartbeat.is_some() {
            bail!("the server missed a heartbeat; the connection is dead");
        }
        let reference = self.fresh_ref();
        self.send_frame(None, &reference, "phoenix", "heartbeat", &json!({}))?;
        self.pending_heartbeat = Some(reference);
        self.last_heartbeat = Instant::now();
        Ok(())
    }

    /// Decode one V2 frame; `None` for internal traffic (heartbeat replies).
    fn decode(&mut self, text: &str) -> Result<Option<SocketEvent>> {
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

/// A timed-out read is the poll breathing, not a failure.
fn idle(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}
