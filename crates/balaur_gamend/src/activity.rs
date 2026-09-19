//! What the plugin has done lately, for a dock to show: each call with its
//! reply and round trip, each socket and the topics it joined, and who is
//! signed in. Wall time, so an observer: nothing here is recorded or read by
//! a tick.

use std::collections::{BTreeMap, VecDeque};

use balaur_core::engine_api::from_json;
use balaur_core::time::Instant;
use balaur_script::Value;
use serde_json::Value as Json;

use crate::GamendEvent;

/// How many entries are kept, oldest dropped first.
const KEPT: usize = 200;

/// How many of the newest entries keep their arguments and reply.
const DETAILED: usize = 50;

/// A payload past this many bytes of JSON is kept as its size alone.
const LARGEST: usize = 16 * 1024;

/// How many replies `reply` can still answer for.
const REPLIES: usize = 64;

/// The status of a call no reply has reached yet.
const WAITING: &str = "waiting";

struct Entry {
    /// Its place among every entry ever kept, from 1: a reader that saw one
    /// knows anything above it is new.
    seq: u64,
    request: u64,
    /// The socket a join, push, leave or hook went out on.
    socket: Option<u64>,
    kind: &'static str,
    what: String,
    status: String,
    started: Option<Instant>,
    ms: Option<f32>,
    args: Option<Value>,
    reply: Option<Value>,
}

#[derive(Default)]
struct Socket {
    open: bool,
    topics: Vec<String>,
    /// Why it closed, once it has.
    reason: Option<String>,
}

#[derive(Default)]
pub(crate) struct Activity {
    entries: VecDeque<Entry>,
    url: String,
    user: Option<(String, String)>,
    sockets: BTreeMap<u64, Socket>,
    /// Each finished call's event, by request, newest last.
    replies: VecDeque<(u64, Value)>,
    /// How many entries were ever kept.
    pushed: u64,
}

/// A payload as the dock shows it, or its size when it is too big to keep.
fn kept(json: &Json) -> Value {
    let size = serde_json::to_string(json).map_or(0, |text| text.len());
    if size > LARGEST {
        return Value::Str(format!("{size} bytes, not kept"));
    }
    from_json(json).unwrap_or(Value::Nil)
}

impl Activity {
    pub(crate) fn configured(&mut self, url: &str) {
        url.clone_into(&mut self.url);
        self.user = None;
    }

    /// A call went out; `socket` is the one it went on, where it has one.
    #[allow(
        clippy::disallowed_methods,
        reason = "a round trip for a dock to show, never a simulation input"
    )]
    pub(crate) fn started(
        &mut self,
        request: u64,
        socket: Option<u64>,
        kind: &'static str,
        what: String,
        args: Option<&Json>,
    ) {
        self.push(Entry {
            seq: 0,
            request,
            socket,
            kind,
            what,
            status: String::from(WAITING),
            started: Some(Instant::now()),
            ms: None,
            args: args.map(kept),
            reply: None,
        });
        if kind == "connect" {
            self.sockets.insert(request, Socket::default());
        }
    }

    /// Something came back from a worker.
    pub(crate) fn heard(&mut self, event: &GamendEvent) {
        match event {
            GamendEvent::LoggedIn {
                request,
                user_id,
                username,
                ..
            } => {
                self.user = Some((user_id.clone(), username.clone()));
                self.finish(*request, String::from("ok"), None);
            }
            GamendEvent::RestDone {
                request,
                status,
                body,
            } => self.finish(*request, status.to_string(), Some(kept(body))),
            GamendEvent::Failed { request, message } => {
                self.finish(*request, message.clone(), None);
            }
            GamendEvent::Replied {
                request,
                status,
                response,
            } => self.replied(*request, status, kept(response)),
            GamendEvent::SocketOpen { socket } => {
                self.sockets.entry(*socket).or_default().open = true;
                self.finish(*socket, String::from("open"), None);
            }
            GamendEvent::SocketMessage {
                socket,
                topic,
                event,
                payload,
            } => self.push(Entry {
                seq: 0,
                request: 0,
                socket: Some(*socket),
                kind: "message",
                what: format!("{topic} {event}"),
                status: String::new(),
                started: None,
                ms: None,
                args: None,
                reply: Some(kept(payload)),
            }),
            GamendEvent::SocketClosed { socket, reason }
            | GamendEvent::SocketError { socket, reason } => {
                let state = self.sockets.entry(*socket).or_default();
                state.open = false;
                state.reason = Some(reason.clone());
                self.finish(*socket, reason.clone(), None);
            }
        }
    }

    /// A reply to a join or a leave also moves the socket's topics.
    fn replied(&mut self, request: u64, status: &str, reply: Value) {
        let call = self
            .entries
            .iter()
            .rev()
            .find(|entry| entry.request == request)
            .map(|entry| (entry.kind, entry.socket, entry.what.clone()));
        if let Some((kind, Some(socket), topic)) = call
            && status == "ok"
        {
            let topics = &mut self.sockets.entry(socket).or_default().topics;
            match kind {
                "join" if !topics.contains(&topic) => topics.push(topic),
                "leave" => topics.retain(|t| *t != topic),
                _ => {}
            }
        }
        self.finish(request, status.to_string(), Some(reply));
    }

    fn finish(&mut self, request: u64, status: String, reply: Option<Value>) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| entry.request == request && entry.status == WAITING)
        {
            entry.ms = entry.started.map(|at| at.elapsed().as_secs_f32() * 1000.0);
            entry.status = status;
            entry.reply = reply;
        }
    }

    fn push(&mut self, mut entry: Entry) {
        if self.entries.len() == KEPT {
            self.entries.pop_front();
        }
        self.pushed += 1;
        entry.seq = self.pushed;
        self.entries.push_back(entry);
        if let Some(older) = self
            .entries
            .len()
            .checked_sub(DETAILED + 1)
            .and_then(|at| self.entries.get_mut(at))
        {
            older.args = None;
            older.reply = None;
        }
    }

    /// Keep a finished call's event, for `reply` to answer by request.
    pub(crate) fn keep_reply(&mut self, request: u64, event: Value) {
        if self.replies.len() == REPLIES {
            self.replies.pop_front();
        }
        self.replies.push_back((request, event));
    }

    pub(crate) fn reply(&self, request: u64) -> Option<Value> {
        self.replies
            .iter()
            .rev()
            .find(|(id, _)| *id == request)
            .map(|(_, event)| event.clone())
    }

    /// Forget every call and message; sockets and the user stay.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Who a restored session says is signed in, or nobody.
    pub(crate) fn signed_in(&mut self, user: Option<(String, String)>) {
        self.user = user;
    }

    /// Every entry, newest first.
    pub(crate) fn entries(&self) -> Value {
        let rows = self
            .entries
            .iter()
            .rev()
            .map(|entry| {
                Value::Map(vec![
                    (String::from("seq"), crate::int(entry.seq)),
                    (String::from("request"), crate::int(entry.request)),
                    (String::from("kind"), Value::Str(entry.kind.into())),
                    (String::from("what"), Value::Str(entry.what.clone())),
                    (String::from("status"), Value::Str(entry.status.clone())),
                    (
                        String::from("ms"),
                        entry.ms.map_or(Value::Nil, |ms| Value::Num(f64::from(ms))),
                    ),
                    (
                        String::from("args"),
                        entry.args.clone().unwrap_or(Value::Nil),
                    ),
                    (
                        String::from("reply"),
                        entry.reply.clone().unwrap_or(Value::Nil),
                    ),
                ])
            })
            .collect();
        Value::List(rows)
    }

    /// The server, who is signed in, and each socket with its topics.
    pub(crate) fn connection(&self) -> Value {
        let (user_id, username) = self
            .user
            .clone()
            .map_or((Value::Nil, Value::Nil), |(id, name)| {
                (Value::Str(id), Value::Str(name))
            });
        let sockets = self
            .sockets
            .iter()
            .map(|(id, socket)| {
                Value::Map(vec![
                    (String::from("socket"), crate::int(*id)),
                    (String::from("open"), Value::Bool(socket.open)),
                    (
                        String::from("topics"),
                        Value::List(socket.topics.iter().cloned().map(Value::Str).collect()),
                    ),
                    (
                        String::from("reason"),
                        socket.reason.clone().map_or(Value::Nil, Value::Str),
                    ),
                ])
            })
            .collect();
        Value::Map(vec![
            (String::from("url"), Value::Str(self.url.clone())),
            (String::from("user_id"), user_id),
            (String::from("username"), username),
            (String::from("sockets"), Value::List(sockets)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
        let Value::Map(pairs) = row else {
            panic!("not a map: {row:?}")
        };
        &pairs.iter().find(|(k, _)| k == key).unwrap().1
    }

    fn rows(activity: &Activity) -> Vec<Value> {
        let Value::List(rows) = activity.entries() else {
            panic!("entries are a list")
        };
        rows
    }

    #[test]
    fn a_reply_finishes_its_call_and_a_join_adds_its_topic() {
        let mut activity = Activity::default();
        activity.started(1, None, "connect", String::from("realtime"), None);
        activity.heard(&GamendEvent::SocketOpen { socket: 1 });
        activity.started(2, Some(1), "join", String::from("lobby:7"), None);
        activity.heard(&GamendEvent::Replied {
            request: 2,
            status: String::from("ok"),
            response: serde_json::Value::Null,
        });
        let rows = rows(&activity);
        assert_eq!(field(&rows[0], "kind"), &Value::Str("join".into()));
        assert_eq!(field(&rows[0], "status"), &Value::Str("ok".into()));
        assert!(matches!(field(&rows[0], "ms"), Value::Num(_)));
        let Value::List(sockets) = field(&activity.connection(), "sockets").clone() else {
            panic!("sockets are a list")
        };
        assert_eq!(field(&sockets[0], "open"), &Value::Bool(true));
        assert_eq!(
            field(&sockets[0], "topics"),
            &Value::List(vec![Value::Str("lobby:7".into())])
        );
    }

    #[test]
    fn a_call_waits_until_its_reply_and_the_login_names_the_user() {
        let mut activity = Activity::default();
        activity.started(3, None, "login", String::from("device"), None);
        assert_eq!(field(&rows(&activity)[0], "ms"), &Value::Nil);
        activity.heard(&GamendEvent::LoggedIn {
            request: 3,
            user_id: String::from("u1"),
            username: String::from("tester"),
            display_name: String::new(),
        });
        assert_eq!(
            field(&rows(&activity)[0], "status"),
            &Value::Str("ok".into())
        );
        assert_eq!(
            field(&activity.connection(), "username"),
            &Value::Str("tester".into())
        );
    }

    #[test]
    fn only_the_newest_entries_are_kept() {
        let mut activity = Activity::default();
        for request in 0..300 {
            activity.started(request, None, "rest", String::from("GET /"), None);
        }
        let rows = rows(&activity);
        assert_eq!(rows.len(), KEPT);
        assert_eq!(field(&rows[0], "request"), &Value::Int(299));
        // The count keeps going past what is kept, so a reader sees new rows.
        assert_eq!(field(&rows[0], "seq"), &Value::Int(300));
        assert_eq!(
            field(&rows[KEPT - 1], "seq"),
            &crate::int(300 - KEPT as u64 + 1)
        );
    }
}
