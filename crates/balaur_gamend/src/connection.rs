//! The plugin's link to its server, for a dock or a debug screen to read: the
//! configured url, who is signed in, each socket and the topics it holds, and
//! the newest replies by request. An observer: nothing here is read by a tick.

use std::collections::{BTreeMap, VecDeque};

use balaur_script::Value;

use crate::GamendEvent;
use crate::vocabulary::keys;
use crate::vocabulary::words;

const OPEN: &str = "open";

/// How many replies `reply` can still answer for.
const REPLIES: usize = 64;

#[derive(Default)]
struct Socket {
    open: bool,
    topics: Vec<String>,
    /// Why it closed, or why it is trying again.
    reason: Option<String>,
    /// It closed for good, and its row stays only to say why.
    ended: bool,
}

/// A join or a leave waiting on its reply, which moves the socket's topics.
struct Moving {
    request: u64,
    socket: u64,
    topic: String,
    join: bool,
}

#[derive(Default)]
pub(crate) struct Connection {
    url: String,
    user: Option<(String, String)>,
    sockets: BTreeMap<u64, Socket>,
    moving: Vec<Moving>,
    /// Each finished call's event, by request, newest last.
    replies: VecDeque<(u64, Value)>,
}

impl Connection {
    pub(crate) fn configured(&mut self, url: &str) {
        url.clone_into(&mut self.url);
        self.user = None;
        self.sockets.clear();
        self.moving.clear();
    }

    /// A socket is being opened; one that closed for good stays to say why
    /// until then.
    pub(crate) fn connecting(&mut self, socket: u64) {
        self.sockets.retain(|_, held| !held.ended);
        self.sockets.insert(socket, Socket::default());
    }

    /// A join or a leave went out on `socket`.
    pub(crate) fn moving(&mut self, request: u64, socket: u64, topic: String, join: bool) {
        self.moving.push(Moving {
            request,
            socket,
            topic,
            join,
        });
    }

    /// Something came back from a worker.
    pub(crate) fn heard(&mut self, event: &GamendEvent) {
        match event {
            GamendEvent::LoggedIn {
                user_id, username, ..
            } => self.user = Some((user_id.clone(), username.clone())),
            GamendEvent::Replied {
                request, status, ..
            } => {
                if let Some(moved) = self.settled(*request)
                    && status == words::OK
                {
                    let topics = &mut self.sockets.entry(moved.socket).or_default().topics;
                    if !moved.join {
                        topics.retain(|t| *t != moved.topic);
                    } else if !topics.contains(&moved.topic) {
                        topics.push(moved.topic);
                    }
                }
            }
            GamendEvent::Failed { request, .. } => {
                self.settled(*request);
            }
            GamendEvent::RestDone { .. } => {}
            GamendEvent::SocketOpen { socket } => {
                self.sockets.entry(*socket).or_default().open = true;
            }
            GamendEvent::SocketMessage {
                socket,
                topic,
                event,
                ..
            } => {
                // The server closed the channel: the socket no longer has it.
                if event == words::PHX_CLOSE {
                    let state = self.sockets.entry(*socket).or_default();
                    state.topics.retain(|joined| joined != topic);
                }
            }
            GamendEvent::SocketReconnecting { socket, reason, .. } => {
                let state = self.sockets.entry(*socket).or_default();
                state.open = false;
                state.reason = Some(reason.clone());
            }
            GamendEvent::SocketReopened { socket, lost } => {
                let state = self.sockets.entry(*socket).or_default();
                state.open = true;
                state.reason = None;
                state.topics.retain(|topic| !lost.contains(topic));
            }
            GamendEvent::SocketClosed { socket, reason }
            | GamendEvent::SocketError { socket, reason } => {
                let state = self.sockets.entry(*socket).or_default();
                state.open = false;
                state.ended = true;
                state.reason = Some(reason.clone());
            }
        }
    }

    fn settled(&mut self, request: u64) -> Option<Moving> {
        let at = self.moving.iter().position(|m| m.request == request)?;
        Some(self.moving.remove(at))
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

    /// Who a restored session says is signed in, or nobody.
    pub(crate) fn signed_in(&mut self, user: Option<(String, String)>) {
        self.user = user;
    }

    /// The server, who is signed in, and each socket with its topics.
    pub(crate) fn value(&self) -> Value {
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
                    (String::from(keys::SOCKET), crate::int(*id)),
                    (String::from(OPEN), Value::Bool(socket.open)),
                    (
                        String::from("topics"),
                        Value::List(socket.topics.iter().cloned().map(Value::text).collect()),
                    ),
                    (
                        String::from(keys::REASON),
                        socket.reason.clone().map_or(Value::Nil, Value::text),
                    ),
                ])
            })
            .collect();
        Value::Map(vec![
            (
                String::from(words::target::URL),
                Value::Str(self.url.clone()),
            ),
            (String::from(words::inspect::USER_ID), user_id),
            (String::from(words::auth::USERNAME), username),
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

    fn first_socket(link: &Connection) -> Value {
        let Value::List(sockets) = field(&link.value(), "sockets").clone() else {
            panic!("sockets are a list")
        };
        sockets[0].clone()
    }

    fn joined(link: &mut Connection, request: u64, topic: &str) {
        link.moving(request, 1, String::from(topic), true);
        link.heard(&GamendEvent::Replied {
            request,
            status: String::from("ok"),
            response: serde_json::Value::Null,
            ms: 3.0,
        });
    }

    #[test]
    fn a_join_answered_ok_adds_its_topic_and_a_leave_takes_it_away() {
        let mut link = Connection::default();
        link.connecting(1);
        link.heard(&GamendEvent::SocketOpen { socket: 1 });
        joined(&mut link, 2, "lobby:7");
        assert_eq!(field(&first_socket(&link), "open"), &Value::Bool(true));
        assert_eq!(
            field(&first_socket(&link), "topics"),
            &Value::List(vec![Value::Str("lobby:7".into())])
        );
        link.moving(3, 1, String::from("lobby:7"), false);
        link.heard(&GamendEvent::Replied {
            request: 3,
            status: String::from("ok"),
            response: serde_json::Value::Null,
            ms: 3.0,
        });
        assert_eq!(
            field(&first_socket(&link), "topics"),
            &Value::List(Vec::new())
        );
    }

    #[test]
    fn a_refused_join_adds_nothing() {
        let mut link = Connection::default();
        link.connecting(1);
        link.moving(2, 1, String::from("lobby:7"), true);
        link.heard(&GamendEvent::Replied {
            request: 2,
            status: String::from("error"),
            response: serde_json::Value::Null,
            ms: 3.0,
        });
        assert_eq!(
            field(&first_socket(&link), "topics"),
            &Value::List(Vec::new())
        );
    }

    #[test]
    fn a_new_connect_clears_a_closed_socket_and_keeps_one_coming_back() {
        let mut link = Connection::default();
        link.connecting(1);
        link.heard(&GamendEvent::SocketReconnecting {
            socket: 1,
            attempt: 1,
            reason: String::from("reset by peer"),
            wait: 1.0,
        });
        link.heard(&GamendEvent::SocketClosed {
            socket: 2,
            reason: String::from("closed by the game"),
        });
        link.connecting(3);
        assert_eq!(link.sockets.keys().copied().collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn a_drop_shows_while_it_reconnects_and_a_topic_refused_again_leaves_the_socket() {
        let mut link = Connection::default();
        link.connecting(1);
        link.heard(&GamendEvent::SocketOpen { socket: 1 });
        joined(&mut link, 2, "lobby:7");
        link.heard(&GamendEvent::SocketReconnecting {
            socket: 1,
            attempt: 1,
            reason: String::from("reset by peer"),
            wait: 1.0,
        });
        assert_eq!(field(&first_socket(&link), "open"), &Value::Bool(false));
        link.heard(&GamendEvent::SocketReopened {
            socket: 1,
            lost: vec![String::from("lobby:7")],
        });
        assert_eq!(field(&first_socket(&link), "open"), &Value::Bool(true));
        assert_eq!(
            field(&first_socket(&link), "topics"),
            &Value::List(Vec::new())
        );
    }

    #[test]
    fn a_login_names_the_user() {
        let mut link = Connection::default();
        link.heard(&GamendEvent::LoggedIn {
            request: 3,
            user_id: String::from("u1"),
            username: String::from("tester"),
            display_name: String::new(),
            ms: 12.0,
        });
        assert_eq!(
            field(&link.value(), "username"),
            &Value::Str("tester".into())
        );
    }

    #[test]
    fn only_the_newest_replies_are_kept() {
        let mut link = Connection::default();
        for request in 0..100 {
            link.keep_reply(request, Value::Int(i64::try_from(request).unwrap()));
        }
        assert_eq!(link.reply(99), Some(Value::Int(99)));
        assert_eq!(link.reply(100 - REPLIES as u64), Some(Value::Int(36)));
        assert_eq!(link.reply(0), None);
    }
}
