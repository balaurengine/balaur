//! Audio buses: a tree of gains a sound plays through.
//!
//! ```toml
//! [audio.buses]
//! sfx = { volume_linear = 0.9 }
//! music = { volume_linear = 0.6, limit = true, limit_threshold_db = -3.0 }
//! ui = { volume_linear = 1.0, parent = "sfx" }
//! ```
//!
//! A sound's gain is its own volume times every bus's up to the root, so a
//! player pulling the music slider moves every piece of music at once and
//! nothing else. `master` exists whether or not the project declares it,
//! because there has to be a name for "everything".
//!
//! A bus may carry rodio's limiter (`limit`), which holds the sum of what
//! plays through it under `limit_threshold_db`. A limited bus is mixed on its
//! own before it joins its parent, so the limiter hears that bus alone. The
//! limiters are built when the device opens. A bus does not own a sound:
//! routing is a property of the playback, which is what lets one file be a
//! footstep on `sfx` and a menu click on `ui`.

use std::collections::BTreeMap;

use balaur_core::Engine;

/// The bus every chain ends at, declared or not.
pub const MASTER: &str = "master";

/// How deep a parent chain may go before it is a cycle. A project with eight
/// nested buses has a different problem.
const MAX_DEPTH: usize = 8;

/// One bus: its own gain, what it feeds into, and its limiter.
#[derive(Clone, Debug)]
pub struct Bus {
    pub volume: f32,
    /// Empty means `master`; `master`'s own parent is empty and stays there.
    pub parent: String,
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

/// Where a bus's mix goes, for the device to build its mixers from.
#[derive(Clone, Debug)]
pub struct Route {
    pub name: String,
    /// Resolved: `master` where the bus names none; empty for `master`.
    pub parent: String,
    pub limit: Option<Limiter>,
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
    // A field's name is the key a project writes; the defaults are rodio's
    // `LimitSettings::default()`.
    #[derive(serde::Deserialize)]
    #[serde(default)]
    struct Declared {
        volume_linear: f32,
        parent: String,
        limit: bool,
        limit_threshold_db: f32,
        limit_knee_db: f32,
        limit_attack_time: f32,
        limit_release_time: f32,
    }
    impl Default for Declared {
        fn default() -> Self {
            Self {
                volume_linear: 1.0,
                parent: String::new(),
                limit: false,
                limit_threshold_db: -1.0,
                limit_knee_db: 4.0,
                limit_attack_time: 0.005,
                limit_release_time: 0.1,
            }
        }
    }
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
        .map(|(name, one)| {
            (
                name,
                Bus {
                    volume: one.volume_linear.max(0.0),
                    parent: one.parent,
                    limit: one.limit.then_some(Limiter {
                        threshold_db: one.limit_threshold_db,
                        knee_db: one.limit_knee_db,
                        attack_time: one.limit_attack_time,
                        release_time: one.limit_release_time,
                    }),
                },
            )
        })
        .collect()
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
