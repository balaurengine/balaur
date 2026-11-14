//! What a script reads about the machine's audio: whether an output device is
//! open, and the outputs the platform lists. The machine is asked once a tick,
//! a recording keeps the tick's answers, and a replay answers from them
//! without asking it.

use balaur_core::Engine;
use balaur_core::replay;
use serde::{Deserialize, Serialize};

use crate::AudioState;
use crate::vocabulary::words;

/// What `audio.device_ready` and `audio.devices` answered on one tick, as a
/// recording keeps it. A read the tick did not make is left out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAnswers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<String>>,
}

/// A machine described rather than asked: what it sets is answered where the
/// OS would be, so a test can run with speakers or with none.
#[derive(Default)]
pub struct Machine(pub DeviceAnswers);

/// The answers this tick has given. A replay's are its recording's, whatever
/// the tick.
#[derive(Default)]
struct Answered {
    tick: u64,
    answers: DeviceAnswers,
}

pub(crate) fn declare(reg: &mut balaur_plugin::Registry<'_>) {
    reg.insert_resource(Answered::default());
    reg.insert_resource(Machine::default());
    reg.add_replay_source(words::replay::AUDIO_DEVICE, capture, restore);
    // The header carries what `init` read, before the first recorded tick.
    reg.add_replay_setup(words::replay::AUDIO_DEVICE, capture, restore);
}

/// `audio.device_ready`.
pub(crate) fn ready(eng: &Engine) -> bool {
    if let Some(ready) = given(eng).ready {
        return ready;
    }
    if replay::is_playing(eng) {
        return false;
    }
    let ready = ask_ready(eng);
    keep(eng, |answers| answers.ready = Some(ready));
    ready
}

/// `audio.devices`.
pub(crate) fn devices(eng: &Engine) -> Vec<String> {
    if let Some(devices) = given(eng).devices {
        return devices;
    }
    if replay::is_playing(eng) {
        return Vec::new();
    }
    let devices = ask_devices(eng);
    keep(eng, |answers| answers.devices = Some(devices.clone()));
    devices
}

/// A replay's answers, or what a live run has asked this tick.
fn given(eng: &Engine) -> DeviceAnswers {
    let answered = eng.resource::<Answered>();
    let answered = answered.borrow();
    if replay::is_playing(eng) || answered.tick == eng.tick() {
        answered.answers.clone()
    } else {
        DeviceAnswers::default()
    }
}

/// Hold an answer for the rest of the tick, and in the header when the tick
/// is the one a recording started in.
fn keep(eng: &Engine, set: impl FnOnce(&mut DeviceAnswers)) {
    let answered = eng.resource::<Answered>();
    let mut answered = answered.borrow_mut();
    let tick = eng.tick();
    if answered.tick != tick {
        *answered = Answered {
            tick,
            answers: DeviceAnswers::default(),
        };
    }
    set(&mut answered.answers);
    let value = serde_json::to_value(&answered.answers).unwrap_or_default();
    replay::amend_setup(eng, words::replay::AUDIO_DEVICE, value);
}

fn ask_ready(eng: &Engine) -> bool {
    if let Some(ready) = eng.resource::<Machine>().borrow().0.ready {
        return ready;
    }
    crate::ensure_loaded(eng);
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    state.open_if_needed();
    state.device.is_some()
}

fn ask_devices(eng: &Engine) -> Vec<String> {
    let described = eng.resource::<Machine>().borrow().0.devices.clone();
    described.unwrap_or_else(crate::devices)
}

fn capture(eng: &Engine) -> serde_json::Value {
    serde_json::to_value(given(eng)).unwrap_or_default()
}

fn restore(eng: &Engine, value: &serde_json::Value) {
    match DeviceAnswers::deserialize(value) {
        Ok(answers) => {
            *eng.resource::<Answered>().borrow_mut() = Answered {
                tick: eng.tick(),
                answers,
            };
        }
        Err(e) => tracing::error!(error = %e, "replaying the audio device"),
    }
}
