//! Named sounds: `audio/events.toml`, played by name rather than by path.
//!
//! ```toml
//! [hit]
//! files = ["sfx/hit1.wav", "sfx/hit2.wav", "sfx/hit3.wav"]
//! bus = "sfx"
//! volume = 0.9
//! ```
//!
//! `audio.play_event("hit")` is what a script says; which file, at what level,
//! through which bus is what a sound designer says. That is the whole point:
//! the two can be tuned without touching each other.
//!
//! **Variations are taken in turn, not at random.** Two reasons, and the
//! second is the load-bearing one: a rotation never plays the same sample
//! twice running, which is what variations exist to avoid; and drawing from
//! the engine's RNG would make what a player hears part of the simulation's
//! random stream, so a muted game and a loud one would diverge.

use std::cell::RefCell;
use std::collections::BTreeMap;

use balaur_core::Engine;

/// One named sound.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Event {
    /// The variations, taken in turn. One file is a sound with no variation.
    pub files: Vec<String>,
    /// The bus this plays through; empty is `master`.
    pub bus: String,
    pub volume: f32,
    pub pitch: f32,
    /// Restart when it ends — for an ambience declared as an event.
    #[serde(rename = "loop")]
    pub looped: bool,
}

impl Default for Event {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            bus: String::new(),
            volume: 1.0,
            pitch: 1.0,
            looped: false,
        }
    }
}

/// Every declared event, and where each rotation has got to.
#[derive(Default)]
pub struct Events {
    events: BTreeMap<String, Event>,
    /// Next variation per event. Presentation state: it is not in a snapshot
    /// and not in the digest, because which of three footsteps played is not
    /// something a replay has to agree about.
    turn: RefCell<BTreeMap<String, usize>>,
    loaded: bool,
}

impl Events {
    /// A declared event by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Event> {
        self.events.get(name)
    }

    /// Every event, in name order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.events.keys().cloned().collect()
    }

    /// The next file for an event, advancing its rotation.
    #[must_use]
    pub fn next_file(&self, name: &str) -> Option<String> {
        let event = self.events.get(name)?;
        if event.files.is_empty() {
            return None;
        }
        let mut turn = self.turn.borrow_mut();
        let at = turn.entry(name.to_string()).or_insert(0);
        let file = event.files[*at % event.files.len()].clone();
        *at = at.wrapping_add(1);
        Some(file)
    }
}

/// Read `audio/events.toml` once, the first time anything asks.
pub fn ensure_loaded(eng: &Engine) {
    let events = eng.resource::<Events>();
    if events.borrow().loaded {
        return;
    }
    let read = read(eng);
    let mut events = events.borrow_mut();
    events.events = read;
    events.loaded = true;
}

/// The events file, or nothing. A project with no events is the normal case.
fn read(eng: &Engine) -> BTreeMap<String, Event> {
    let Ok(source) = balaur_core::project::scene_text(eng, "audio/events.toml") else {
        return BTreeMap::new();
    };
    match toml::from_str::<BTreeMap<String, Event>>(&source) {
        Ok(events) => events,
        Err(err) => {
            tracing::warn!("audio/events.toml: {err}; no events declared");
            BTreeMap::new()
        }
    }
}
