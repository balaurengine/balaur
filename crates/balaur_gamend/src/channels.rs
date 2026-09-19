//! Channels the server ended while the socket stayed up, shared by both
//! backends.
//!
//! `phx_error` means the channel's process crashed: the topic is joined again
//! after a wait, as Phoenix's own client does. `phx_close` means the server
//! closed it on purpose, a kicked player's lobby for one: the topic is
//! forgotten, so a reconnect does not ask for it again either.

use serde_json::Value as Json;

/// One topic waiting to be joined again, `at` in the backend's seconds.
struct Due {
    topic: String,
    payload: Json,
    at: f64,
    attempt: u32,
}

/// A join a rejoin sent, until its reply.
pub(crate) struct Rejoin {
    pub(crate) topic: String,
    pub(crate) payload: Json,
    pub(crate) attempt: u32,
}

#[derive(Default)]
pub(crate) struct Rejoins {
    due: Vec<Due>,
}

impl Rejoins {
    /// A channel event from the server. The own-user topic crashes like any
    /// other and is joined again, though the game never joined it itself.
    pub(crate) fn heard(
        &mut self,
        event: &str,
        topic: &str,
        user_topic: &str,
        topics: &mut Vec<(String, Json)>,
        now: f64,
    ) {
        match event {
            "phx_error" => {
                let payload = if topic == user_topic {
                    Json::Object(serde_json::Map::new())
                } else {
                    match topics.iter().find(|(joined, _)| joined == topic) {
                        Some((_, payload)) => payload.clone(),
                        None => return,
                    }
                };
                self.due.retain(|due| due.topic != topic);
                self.due.push(Due {
                    topic: topic.to_string(),
                    payload,
                    at: now + crate::backoff(1),
                    attempt: 1,
                });
            }
            "phx_close" => {
                topics.retain(|(joined, _)| joined != topic);
                self.due.retain(|due| due.topic != topic);
            }
            _ => {}
        }
    }

    /// The joins due by `now`, off the wait list.
    pub(crate) fn take_due(&mut self, now: f64) -> Vec<Rejoin> {
        let (ready, waiting): (Vec<Due>, Vec<Due>) = std::mem::take(&mut self.due)
            .into_iter()
            .partition(|due| due.at <= now);
        self.due = waiting;
        ready
            .into_iter()
            .map(|due| Rejoin {
                topic: due.topic,
                payload: due.payload,
                attempt: due.attempt,
            })
            .collect()
    }

    /// A rejoin's reply. Refused, it waits and tries again, until it runs
    /// out of tries and the topic is forgotten.
    pub(crate) fn answered(
        &mut self,
        rejoin: Rejoin,
        ok: bool,
        topics: &mut Vec<(String, Json)>,
        now: f64,
    ) {
        if ok {
            return;
        }
        if rejoin.attempt >= crate::RECONNECT_TRIES {
            topics.retain(|(joined, _)| *joined != rejoin.topic);
            return;
        }
        let attempt = rejoin.attempt + 1;
        self.due.push(Due {
            topic: rejoin.topic,
            payload: rejoin.payload,
            at: now + crate::backoff(attempt),
            attempt,
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn lobby() -> Vec<(String, Json)> {
        vec![(String::from("lobby:7"), json!({ "password": "x" }))]
    }

    #[test]
    fn a_crashed_channel_is_joined_again_with_its_payload_after_a_second() {
        let mut rejoins = Rejoins::default();
        let mut topics = lobby();
        rejoins.heard("phx_error", "lobby:7", "user:1", &mut topics, 10.0);
        assert!(rejoins.take_due(10.5).is_empty());
        let due = rejoins.take_due(11.0);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].topic, "lobby:7");
        assert_eq!(due[0].payload, json!({ "password": "x" }));
        assert!(rejoins.take_due(100.0).is_empty(), "taken once");
    }

    #[test]
    fn a_closed_channel_is_forgotten_and_never_joined_again() {
        let mut rejoins = Rejoins::default();
        let mut topics = lobby();
        rejoins.heard("phx_error", "lobby:7", "user:1", &mut topics, 0.0);
        rejoins.heard("phx_close", "lobby:7", "user:1", &mut topics, 0.5);
        assert!(topics.is_empty(), "a reconnect must not ask for it");
        assert!(rejoins.take_due(100.0).is_empty());
    }

    #[test]
    fn the_own_user_topic_is_joined_again_though_the_game_never_joined_it() {
        let mut rejoins = Rejoins::default();
        let mut topics = Vec::new();
        rejoins.heard("phx_error", "user:1", "user:1", &mut topics, 0.0);
        assert_eq!(rejoins.take_due(1.0)[0].topic, "user:1");
    }

    #[test]
    fn a_topic_the_game_never_joined_is_left_alone() {
        let mut rejoins = Rejoins::default();
        let mut topics = lobby();
        rejoins.heard("phx_error", "party:3", "user:1", &mut topics, 0.0);
        assert!(rejoins.take_due(100.0).is_empty());
    }

    #[test]
    fn a_refused_rejoin_backs_off_and_the_last_refusal_forgets_the_topic() {
        let mut rejoins = Rejoins::default();
        let mut topics = lobby();
        rejoins.heard("phx_error", "lobby:7", "user:1", &mut topics, 0.0);
        let mut now = 1.0;
        for attempt in 1..=crate::RECONNECT_TRIES {
            let mut due = rejoins.take_due(now);
            assert_eq!(due.len(), 1, "try {attempt} is due at {now}");
            rejoins.answered(due.remove(0), false, &mut topics, now);
            now += crate::backoff(attempt + 1);
        }
        assert!(topics.is_empty());
        assert!(rejoins.take_due(f64::MAX).is_empty());
    }
}
