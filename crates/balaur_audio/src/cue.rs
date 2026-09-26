//! Named sounds: `audio/cues.toml`, played by name rather than by path.
//!
//! ```toml
//! [hit]
//! files = ["sfx/hit1.wav", "sfx/hit2.wav", "sfx/hit3.wav"]
//! bus = "sfx"
//! volume_linear = 0.9
//! ```
//!
//! `audio.play_cue("hit")` is what a script says; which file, at what level,
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
pub struct Cue {
    /// The variations, taken in turn. One file is a sound with no variation.
    pub files: Vec<String>,
    /// The bus this plays through; empty is `master`.
    pub bus: String,
    pub volume_linear: f32,
    pub pitch_scale: f32,
    /// Restart when it ends — for an ambience declared as a cue.
    #[serde(rename = "loop")]
    pub looped: bool,
    /// How far the sound carries, when a caller plays it at a position: full
    /// volume inside `min_distance`, silent past `max_distance`, and
    /// `doppler_level` of the closing speed in its pitch.
    pub min_distance: f32,
    pub max_distance: f32,
    pub doppler_level: f32,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            bus: String::new(),
            volume_linear: 1.0,
            pitch_scale: 1.0,
            looped: false,
            min_distance: 1.0,
            max_distance: 50.0,
            doppler_level: 0.0,
        }
    }
}

/// Every declared cue, and where each rotation has got to.
#[derive(Default)]
pub struct Cues {
    cues: BTreeMap<String, Cue>,
    /// Next variation per cue. Presentation state: it is not in a snapshot
    /// and not in the digest, because which of three footsteps played is not
    /// something a replay has to agree about.
    turn: RefCell<BTreeMap<String, usize>>,
    loaded: bool,
}

impl Cues {
    /// A declared cue by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Cue> {
        self.cues.get(name)
    }

    /// Every cue, in name order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.cues.keys().cloned().collect()
    }

    /// The next file for a cue, advancing its rotation.
    #[must_use]
    pub fn next_file(&self, name: &str) -> Option<String> {
        let cue = self.cues.get(name)?;
        if cue.files.is_empty() {
            return None;
        }
        let mut turn = self.turn.borrow_mut();
        let at = turn.entry(name.to_string()).or_insert(0);
        let file = cue.files[*at % cue.files.len()].clone();
        *at = at.wrapping_add(1);
        Some(file)
    }
}

/// Read `audio/cues.toml` once, the first time anything asks.
pub fn ensure_loaded(eng: &Engine) {
    let cues = eng.resource::<Cues>();
    if cues.borrow().loaded {
        return;
    }
    let read = read(eng);
    let mut cues = cues.borrow_mut();
    cues.cues = read;
    cues.loaded = true;
}

/// The cues file, or nothing. A project with no cues is the normal case.
fn read(eng: &Engine) -> BTreeMap<String, Cue> {
    let Ok(source) = balaur_core::project::scene_text(eng, "audio/cues.toml") else {
        return BTreeMap::new();
    };
    match toml::from_str::<BTreeMap<String, Cue>>(&source) {
        Ok(cues) => cues,
        Err(err) => {
            tracing::warn!("audio/cues.toml: {err}; no cues declared");
            BTreeMap::new()
        }
    }
}
