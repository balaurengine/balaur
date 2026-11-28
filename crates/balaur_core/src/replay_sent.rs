//! What a run sent out of the process, as its recording's events say, held
//! against what its replay would have sent (`balaur replay --verify`).

use super::{Event, EventLog, ReplayPlayer};
use crate::engine::Engine;

/// The kinds of event that say what a run sent out of the process: a replay
/// sends nothing, and says it would have sent the same (`--verify`).
pub const SENT: &[&str] = &[HTTP_REQUEST, WEBSOCKET_CONNECT, WEBSOCKET_SEND];
pub const HTTP_REQUEST: &str = "http.request";
pub const WEBSOCKET_CONNECT: &str = "websocket.connect";
pub const WEBSOCKET_SEND: &str = "websocket.send";

/// What a body sent carries, as its length and an FNV-1a hash of its bytes:
/// the same on every platform, and small whatever was sent.
#[must_use]
pub fn body_print(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{}:{hash:016x}", bytes.len())
}

/// The first tick a replay sent other than the recording did, with what each
/// sent then, a line each.
#[derive(Clone, Debug)]
pub struct SentDivergence {
    pub tick: u64,
    pub recorded: Vec<String>,
    pub replayed: Vec<String>,
}

/// What this tick sent held against what the recording's tick sent, the
/// first difference kept.
pub(super) fn compare_sent(eng: &Engine) {
    let sent = |events: &[Event]| -> Vec<String> {
        events
            .iter()
            .filter(|event| SENT.contains(&event.kind.as_str()))
            .map(|event| {
                let data = event
                    .data
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                format!("{} {data}", event.kind)
            })
            .collect()
    };
    let replayed = sent(&eng.resource::<EventLog>().borrow().0);
    let player = eng.resource::<ReplayPlayer>();
    let mut player = player.borrow_mut();
    if player.sent_diverged.is_some() {
        return;
    }
    let Some(session) = player
        .session
        .as_ref()
        .filter(|session| session.header.sent)
    else {
        return;
    };
    let Some(frame) = player
        .cursor
        .checked_sub(1)
        .and_then(|at| session.frames.get(at))
    else {
        return;
    };
    let recorded = sent(&frame.events);
    if recorded != replayed {
        player.sent_diverged = Some(SentDivergence {
            tick: frame.tick,
            recorded,
            replayed,
        });
    }
}
