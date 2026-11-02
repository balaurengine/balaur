//! Audio buses: a tree of gains and effects a sound plays through.
//!
//! ```toml
//! [audio.buses]
//! sfx = { volume_linear = 0.9 }
//! music = { volume_linear = 0.6, limit = true, limit_threshold_db = -3.0 }
//! ui = { volume_linear = 1.0, parent = "sfx" }
//! cave = { parent = "sfx", low_pass_hz = 800.0, reverb_time = 0.25, reverb_level = 0.4 }
//! ```
//!
//! A sound's gain is its own volume times every bus's up to the root, so a
//! player pulling the music slider moves every piece of music at once and
//! nothing else. `master` exists whether or not the project declares it,
//! because there has to be a name for "everything".
//!
//! A bus may carry effects: the two filters (`low_pass_hz`,
//! `low_pass_q`, `high_pass_hz`, `high_pass_q`), the distortion
//! (`distortion_gain`, `distortion_threshold`), the reverb (`reverb_time`,
//! `reverb_level`) and automatic gain control (`auto_gain`, `auto_gain_target`,
//! `auto_gain_attack_time`, `auto_gain_release_time`, `auto_gain_max`,
//! `auto_gain_floor`). They act on the bus's whole mix, once, rather than on
//! each sound: a cave's echo is the cave's, whatever plays in it.
//!
//! A bus may also carry rodio's limiter (`limit`), which holds the sum of what
//! plays through it, after its effects, under `limit_threshold_db`. A bus
//! with a limiter or an effect switched on is mixed on its own before it
//! joins its parent, so what it applies hears that bus alone. Both are built
//! when the device opens; `audio.bus_effects` and `audio.bus_limit` read them
//! back. A bus does not own a sound: routing is a property of the playback,
//! which is what lets one file be a footstep on `sfx` and a menu click on
//! `ui`.

use std::collections::BTreeMap;

use balaur_core::Engine;

/// The bus every chain ends at, declared or not.
pub const MASTER: &str = "master";

/// How deep a parent chain may go before it is a cycle. A project with eight
/// nested buses has a different problem.
const MAX_DEPTH: usize = 8;

/// One bus: its own gain, what it feeds into, its effects and its limiter.
#[derive(Clone, Debug)]
pub struct Bus {
    pub volume: f32,
    /// Empty means `master`; `master`'s own parent is empty and stays there.
    pub parent: String,
    /// `None` when none is switched on.
    pub effects: Option<Effects>,
    pub limit: Option<Limiter>,
}

/// rodio's `LimitSettings` in a bus's own keys.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limiter {
    pub threshold_db: f32,
    pub knee_db: f32,
    pub attack_time: f32,
    pub release_time: f32,
}

/// Automatic gain control: rodio's `automatic_gain_control` with every knob it
/// takes, and the floor its filter adds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoGain {
    pub on: bool,
    pub target: f32,
    pub attack_time: f32,
    pub release_time: f32,
    pub max: f32,
    pub floor: f32,
}

impl Default for AutoGain {
    fn default() -> Self {
        Self {
            on: false,
            target: 1.0,
            attack_time: 4.0,
            release_time: 0.0,
            max: 7.0,
            floor: 0.0,
        }
    }
}

/// What a bus does to everything it mixes before that joins its parent.
/// Each is one of rodio's adapters, or its formula where the adapter will not
/// take a bus's mix; kept here as numbers so a headless run reads back what a
/// device would be given.
///
/// A bus's rather than a sound's: a filter or a reverb on the mix runs once
/// for everything on it, and a sound that wants one of its own plays on a
/// bus of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effects {
    /// 0 is off.
    pub low_pass_hz: f32,
    pub low_pass_q: f32,
    /// 0 is off.
    pub high_pass_hz: f32,
    pub high_pass_q: f32,
    /// 0 is off.
    pub reverb_time: f32,
    pub reverb_level: f32,
    pub distortion_gain: f32,
    /// 0 is off.
    pub distortion_threshold: f32,
    pub auto_gain: AutoGain,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            low_pass_hz: 0.0,
            low_pass_q: DEFAULT_Q,
            high_pass_hz: 0.0,
            high_pass_q: DEFAULT_Q,
            reverb_time: 0.0,
            reverb_level: 0.0,
            distortion_gain: 1.0,
            distortion_threshold: 0.0,
            auto_gain: AutoGain::default(),
        }
    }
}

impl Effects {
    /// Whether any of them is switched on. A bus with none is mixed straight
    /// into its parent, with no chain to run.
    #[must_use]
    pub fn any_on(&self) -> bool {
        self.auto_gain.on
            || self.low_pass_hz > 0.0
            || self.high_pass_hz > 0.0
            || self.distortion_threshold > 0.0
            || self.reverb_time > 0.0
    }
}

/// rodio's own Q for `low_pass` and `high_pass` (`blt.rs`).
pub const DEFAULT_Q: f32 = 0.5;

/// The least Q a filter takes: at 0 its turn divides by zero.
pub const MIN_Q: f32 = 0.01;

/// rodio caps automatic gain control's attack and release here (`source/mod.rs`).
pub const MAX_AUTO_GAIN_TIME: f32 = 10.0;

/// The longest a reverb's echo trails. The echo holds every sample of that
/// span, per channel, for as long as the device is open.
pub const MAX_REVERB_TIME: f32 = 10.0;

/// Where a bus's mix goes and what is done to it on the way, for the device
/// to build its mixers from.
#[derive(Clone, Debug)]
pub struct Route {
    pub name: String,
    /// Resolved: `master` where the bus names none; empty for `master`.
    pub parent: String,
    pub effects: Option<Effects>,
    pub limit: Option<Limiter>,
}

impl Route {
    /// Whether the bus is mixed on its own before it joins its parent: an
    /// effect or a limiter has to hear this bus's mix alone. Any other bus's
    /// sounds join the nearest mixer above it.
    #[must_use]
    pub fn mixed_alone(&self) -> bool {
        self.effects.is_some() || self.limit.is_some()
    }
}

/// The bus one feeds: an empty parent means `master`, which every chain ends
/// at whether or not the project spelled it.
fn parent_of(bus: &Bus) -> &str {
    if bus.parent.is_empty() {
        MASTER
    } else {
        &bus.parent
    }
}

/// Every declared bus, and the live volumes a game has since set.
pub struct Buses {
    buses: BTreeMap<String, Bus>,
    loaded: bool,
}

impl Default for Buses {
    fn default() -> Self {
        let mut buses = BTreeMap::new();
        buses.insert(
            MASTER.to_string(),
            Bus {
                volume: 1.0,
                parent: String::new(),
                effects: None,
                limit: None,
            },
        );
        Self {
            buses,
            loaded: false,
        }
    }
}

impl Buses {
    /// The gain a sound on this bus is multiplied by: its bus's volume and
    /// every one above it, `master` included.
    ///
    /// A name nobody declared is 1.0 rather than 0.0 — a typo should leave a
    /// sound audible and findable, not silently delete it.
    #[must_use]
    pub fn gain(&self, name: &str) -> f32 {
        let mut at = if name.is_empty() { MASTER } else { name };
        if !self.buses.contains_key(at) {
            return 1.0;
        }
        let mut gain = 1.0;
        for _ in 0..MAX_DEPTH {
            let Some(bus) = self.buses.get(at) else { break };
            gain *= bus.volume;
            if at == MASTER {
                break;
            }
            at = parent_of(bus);
        }
        gain
    }

    /// Whether a sound on `name` passes through `ancestor` — the question a
    /// slider asks: does moving this bus move that sound?
    #[must_use]
    pub fn feeds(&self, name: &str, ancestor: &str) -> bool {
        let ancestor = if ancestor.is_empty() {
            MASTER
        } else {
            ancestor
        };
        let mut at = if name.is_empty() { MASTER } else { name };
        for _ in 0..MAX_DEPTH {
            if at == ancestor {
                return self.buses.contains_key(at);
            }
            let Some(bus) = self.buses.get(at) else {
                return false;
            };
            if at == MASTER {
                return false;
            }
            at = parent_of(bus);
        }
        false
    }

    /// One bus's own volume, without its parents'.
    #[must_use]
    pub fn volume(&self, name: &str) -> f32 {
        self.buses.get(name).map_or(1.0, |bus| bus.volume)
    }

    /// Set one bus's own volume. A bus nobody declared is created at that
    /// volume under `master`, so a game may build its mix in script alone.
    pub fn set_volume(&mut self, name: &str, volume: f32) {
        let volume = volume.max(0.0);
        self.buses
            .entry(name.to_string())
            .and_modify(|bus| bus.volume = volume)
            .or_insert(Bus {
                volume,
                parent: String::new(),
                effects: None,
                limit: None,
            });
    }

    /// Every bus, in name order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.buses.keys().cloned().collect()
    }

    /// One bus's limiter, `None` for a bus with none or a name nobody declared.
    #[must_use]
    pub fn limit(&self, name: &str) -> Option<Limiter> {
        let name = if name.is_empty() { MASTER } else { name };
        self.buses.get(name).and_then(|bus| bus.limit)
    }

    /// One bus's effects, `None` for a bus with none switched on or a name
    /// nobody declared.
    #[must_use]
    pub fn effects(&self, name: &str) -> Option<Effects> {
        let name = if name.is_empty() { MASTER } else { name };
        self.buses.get(name).and_then(|bus| bus.effects)
    }

    /// Every bus and where it feeds, for the device's mixers.
    #[must_use]
    pub fn routes(&self) -> Vec<Route> {
        self.buses
            .iter()
            .map(|(name, bus)| Route {
                name: name.clone(),
                parent: if name == MASTER {
                    String::new()
                } else {
                    parent_of(bus).to_string()
                },
                effects: bus.effects,
                limit: bus.limit,
            })
            .collect()
    }
}

/// Read `[audio.buses]` once, the first time anything asks.
///
/// Lazy for the reason every project table here is: the manifest is read when
/// the project loads, which is after every plugin has been built.
pub fn ensure_loaded(eng: &Engine) {
    let buses = eng.resource::<Buses>();
    if buses.borrow().loaded {
        return;
    }
    let declared = declared(eng);
    let mut buses = buses.borrow_mut();
    buses.loaded = true;
    for (name, bus) in declared {
        buses.buses.insert(name, bus);
    }
    validate(&mut buses);
}

/// The `[audio.buses]` table, or nothing.
fn declared(eng: &Engine) -> BTreeMap<String, Bus> {
    // Resolved, so a platform may mix its buses differently: a phone's
    // speaker wants less of the bass bus than a desktop's headphones.
    let table = balaur_core::settings::table(eng, crate::vocabulary::settings::BUSES);
    let parsed: BTreeMap<String, Declared> = match toml::Value::Table(table).try_into() {
        Ok(parsed) => parsed,
        Err(err) => {
            tracing::warn!("project.toml [audio.buses]: {err}; no buses declared");
            return BTreeMap::new();
        }
    };
    parsed
        .into_iter()
        .map(|(name, one)| (name, one.into_bus()))
        .collect()
}

/// One `[audio.buses]` entry as a project writes it. A field's name is the
/// key; the defaults are rodio's, `LimitSettings::default()` for the limiter
/// and `Effects::default()` for the rest.
#[derive(serde::Deserialize)]
#[serde(default)]
struct Declared {
    volume_linear: f32,
    parent: String,
    low_pass_hz: f32,
    low_pass_q: f32,
    high_pass_hz: f32,
    high_pass_q: f32,
    reverb_time: f32,
    reverb_level: f32,
    distortion_gain: f32,
    distortion_threshold: f32,
    auto_gain: bool,
    auto_gain_target: f32,
    auto_gain_attack_time: f32,
    auto_gain_release_time: f32,
    auto_gain_max: f32,
    auto_gain_floor: f32,
    limit: bool,
    limit_threshold_db: f32,
    limit_knee_db: f32,
    limit_attack_time: f32,
    limit_release_time: f32,
}

impl Default for Declared {
    fn default() -> Self {
        let effects = Effects::default();
        let auto_gain = effects.auto_gain;
        Self {
            volume_linear: 1.0,
            parent: String::new(),
            low_pass_hz: effects.low_pass_hz,
            low_pass_q: effects.low_pass_q,
            high_pass_hz: effects.high_pass_hz,
            high_pass_q: effects.high_pass_q,
            reverb_time: effects.reverb_time,
            reverb_level: effects.reverb_level,
            distortion_gain: effects.distortion_gain,
            distortion_threshold: effects.distortion_threshold,
            auto_gain: auto_gain.on,
            auto_gain_target: auto_gain.target,
            auto_gain_attack_time: auto_gain.attack_time,
            auto_gain_release_time: auto_gain.release_time,
            auto_gain_max: auto_gain.max,
            auto_gain_floor: auto_gain.floor,
            limit: false,
            limit_threshold_db: -1.0,
            limit_knee_db: 4.0,
            limit_attack_time: 0.005,
            limit_release_time: 0.1,
        }
    }
}

impl Declared {
    fn into_bus(self) -> Bus {
        Bus {
            volume: self.volume_linear.max(0.0),
            effects: self.effects(),
            limit: self.limit.then_some(Limiter {
                threshold_db: self.limit_threshold_db,
                knee_db: self.limit_knee_db,
                attack_time: self.limit_attack_time,
                release_time: self.limit_release_time,
            }),
            parent: self.parent,
        }
    }

    /// Clamped here, where the numbers are read, so what `audio.bus_effects`
    /// reads back is what the device is given.
    fn effects(&self) -> Option<Effects> {
        let effects = Effects {
            low_pass_hz: self.low_pass_hz.max(0.0),
            low_pass_q: self.low_pass_q.max(MIN_Q),
            high_pass_hz: self.high_pass_hz.max(0.0),
            high_pass_q: self.high_pass_q.max(MIN_Q),
            reverb_time: seconds(self.reverb_time).min(MAX_REVERB_TIME),
            reverb_level: self.reverb_level.max(0.0),
            distortion_gain: self.distortion_gain.max(0.0),
            distortion_threshold: self.distortion_threshold.clamp(0.0, 1.0),
            auto_gain: AutoGain {
                on: self.auto_gain,
                target: self.auto_gain_target.max(0.0),
                attack_time: seconds(self.auto_gain_attack_time).min(MAX_AUTO_GAIN_TIME),
                release_time: seconds(self.auto_gain_release_time).min(MAX_AUTO_GAIN_TIME),
                max: self.auto_gain_max.max(0.0),
                floor: self.auto_gain_floor.max(0.0),
            },
        };
        effects.any_on().then_some(effects)
    }
}

/// A time key: below zero, or not a finite number, is none.
fn seconds(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// Break a parent that does not resolve, so `gain` cannot loop.
///
/// A cycle is reported and cut rather than refused: the rest of the mix is
/// fine, and a game that would not start over a mis-typed parent is worse
/// than one that plays it at the wrong level and says so.
fn validate(buses: &mut Buses) {
    let names: Vec<String> = buses.buses.keys().cloned().collect();
    for name in names {
        let mut at = name.clone();
        let mut seen = vec![at.clone()];
        let mut ended = false;
        for _ in 0..MAX_DEPTH {
            let Some(parent) = buses.buses.get(&at).map(|b| b.parent.clone()) else {
                ended = true;
                break;
            };
            if parent.is_empty() {
                ended = true;
                break;
            }
            if !buses.buses.contains_key(&parent) {
                tracing::warn!(
                    "audio bus '{at}' names a parent '{parent}' nothing declares; feeding it to master"
                );
                detach(buses, &at);
                ended = true;
                break;
            }
            if seen.contains(&parent) {
                tracing::warn!("audio bus '{name}' feeds a cycle through '{parent}'; cutting it");
                detach(buses, &at);
                ended = true;
                break;
            }
            seen.push(parent.clone());
            at = parent;
        }
        if !ended {
            tracing::warn!(
                "audio bus '{name}' is nested more than {MAX_DEPTH} deep; the chain above '{at}' is ignored"
            );
        }
    }
}

/// Feed a bus straight to `master`, which is what cutting a bad parent means.
fn detach(buses: &mut Buses, name: &str) {
    if let Some(bus) = buses.buses.get_mut(name) {
        bus.parent = String::new();
    }
}
