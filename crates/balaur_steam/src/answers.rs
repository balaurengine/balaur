//! How a script's call reaches Steam and stays reproducible. A call that
//! answers at once is asked once, and a recording keeps each tick's answers
//! in order, so a replay answers from them without Steam. A call that answers
//! later (a `SteamAPICall_t`) returns a token whose answer crosses
//! [`ExternalIo`], as `google.sign_in`'s does.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use balaur_core::Engine;
use balaur_core::handler::id_value;
use balaur_core::replay::{self, ExternalIo};
use balaur_script::Value;

use crate::client::{Arrival, Inbox, Pending, Steam, UNSUPPORTED, failure};
use crate::sdk::api::Api;
use crate::sdk::types::SteamAPICall_t;

/// The replay source and setup name of the answers kept.
pub(crate) const ANSWERS: &str = "steam_answers";

/// The client, if one started, and everything its calls leave in flight.
#[derive(Default)]
pub struct SteamState {
    pub(crate) steam: Option<Rc<Steam>>,
    pub(crate) io: ExternalIo<Arrival>,
    pub(crate) pending: Pending,
    /// What the backend behind `platform.*` waits on.
    pub(crate) inbox: Rc<RefCell<Inbox>>,
    answers: Answers,
}

impl SteamState {
    /// Whether Valve's library loaded and Steam started for this game.
    pub fn is_loaded(&self) -> bool {
        self.steam.is_some()
    }

    /// The started client, for a game's own Rust: every flat function is
    /// `steam.api()`.
    pub fn steam(&self) -> Option<&Steam> {
        self.steam.as_deref()
    }
}

/// This tick's call-result answers and callbacks, as their waiters and
/// listeners received them.
#[derive(Default)]
pub struct SteamSnapshot {
    pub answers: Vec<Value>,
    pub events: Vec<Value>,
}

/// One tick's answers: what this run asked, or what its recording holds.
#[derive(Default)]
struct Answers {
    tick: u64,
    kept: Vec<serde_json::Value>,
    given: VecDeque<serde_json::Value>,
}

/// Answer a call at once: nil without Steam, the recorded answer in a replay.
pub(crate) fn answer(eng: &Engine, ask: impl FnOnce(&Api) -> Option<Value>) -> Value {
    let state = eng.resource::<SteamState>();
    let mut state = state.borrow_mut();
    if replay::is_playing(eng) {
        let given = state.answers.given.pop_front().unwrap_or_default();
        return balaur_core::engine_api::from_json(&given).unwrap_or(Value::Nil);
    }
    let Some(steam) = state.steam.clone() else {
        return Value::Nil;
    };
    let value = ask(&steam.api).unwrap_or(Value::Nil);
    let answers = &mut state.answers;
    if answers.tick != eng.tick() {
        answers.tick = eng.tick();
        answers.kept.clear();
    }
    answers
        .kept
        .push(balaur_core::engine_api::to_json(&value).unwrap_or_default());
    // `init` runs in the frame a recording opens in, which is not recorded.
    replay::amend_setup(eng, ANSWERS, serde_json::Value::Array(answers.kept.clone()));
    value
}

/// Start a call Steam answers later, and return the token its answer wakes:
/// `unsupported` without Steam, `error` when Steam refuses it.
pub(crate) fn call_result(
    eng: &Engine,
    expected: i32,
    start: impl FnOnce(&Api) -> Option<SteamAPICall_t>,
) -> Value {
    let request = eng.next_token();
    let state = eng.resource::<SteamState>();
    let mut state = state.borrow_mut();
    let SteamState {
        steam, io, pending, ..
    } = &mut *state;
    io.start(eng, |report| {
        let answer = match steam.as_deref() {
            None => failure(UNSUPPORTED, "Steam is not running this game"),
            Some(steam) => match start(&steam.api) {
                None => failure(UNSUPPORTED, "this steam_api lacks the function"),
                Some(0) => failure(balaur_core::handler::ERROR, "Steam refused the call"),
                Some(handle) => {
                    pending.insert(handle, (request, expected));
                    return;
                }
            },
        };
        let _ = report.send(Arrival::Answer { request, answer });
    });
    id_value(request)
}

pub(crate) fn capture(eng: &Engine) -> serde_json::Value {
    let state = eng.resource::<SteamState>();
    let state = state.borrow();
    if state.answers.tick != eng.tick() || state.answers.kept.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::Value::Array(state.answers.kept.clone())
}

pub(crate) fn restore(eng: &Engine, value: &serde_json::Value) {
    let state = eng.resource::<SteamState>();
    let mut state = state.borrow_mut();
    state.answers.given = value.as_array().cloned().unwrap_or_default().into();
}

pub(crate) fn capture_arrivals(eng: &Engine) -> serde_json::Value {
    eng.resource::<SteamState>().borrow().io.capture()
}

pub(crate) fn restore_arrivals(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<SteamState>().borrow().io.restore(value);
}

/// Take what Steam sent this tick: call results wake their tokens, callbacks
/// are the `steam_event` event.
pub(crate) fn pump_steam_system(eng: &Engine, _: f32) {
    let arrivals = {
        let state = eng.resource::<SteamState>();
        let mut state = state.borrow_mut();
        let SteamState {
            steam,
            io,
            pending,
            inbox,
            ..
        } = &mut *state;
        if let Some(steam) = steam
            && !io.start(eng, |report| steam.dispatch(pending, inbox, report))
        {
            steam.discard();
        }
        state.io.drain()
    };
    let mut finished = Vec::new();
    let mut events = Vec::new();
    for arrival in arrivals {
        match arrival {
            Arrival::Answer { request, answer } => {
                let mut pairs = vec![(crate::REQUEST.to_string(), id_value(request))];
                if let Ok(Value::Map(fields)) = balaur_core::engine_api::from_json(&answer) {
                    pairs.extend(fields);
                }
                finished.push((request, Value::Map(pairs)));
            }
            Arrival::Event { event } => {
                if let Ok(value) = balaur_core::engine_api::from_json(&event) {
                    events.push(value);
                }
            }
        }
    }
    {
        let snapshot = eng.resource::<SteamSnapshot>();
        let mut snapshot = snapshot.borrow_mut();
        // copy: the snapshot keeps what each waiter and listener is handed.
        snapshot.answers = finished.iter().map(|(_, value)| value.clone()).collect();
        snapshot.events.clone_from(&events);
    }
    for event in events {
        balaur_core::events::emit(eng, crate::STEAM_EVENT, event);
    }
    balaur_core::handler::wake_all(eng, finished);
}
