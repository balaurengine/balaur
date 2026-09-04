//! Audio buses: a tree of gains a sound plays through.
//!
//! ```toml
//! [audio.buses]
//! sfx = { volume = 0.9 }
//! music = { volume = 0.6 }
//! ui = { volume = 1.0, parent = "sfx" }
//! ```
//!
//! A sound's gain is its own volume times every bus's up to the root, so a
//! player pulling the music slider moves every piece of music at once and
//! nothing else. `master` exists whether or not the project declares it,
//! because there has to be a name for "everything".
//!
//! This is the mixing a game asks for. It is not a mixing *graph* — no
//! effects, no sends — and a bus does not own a sound: routing is a property
//! of the playback, which is what lets one file be a footstep on `sfx` and a
//! menu click on `ui`.

use std::collections::BTreeMap;

use balaur_core::Engine;

/// The bus every chain ends at, declared or not.
pub const MASTER: &str = "master";

/// How deep a parent chain may go before it is a cycle. A project with eight
/// nested buses has a different problem.
const MAX_DEPTH: usize = 8;

/// One bus: its own gain and what it feeds into.
#[derive(Clone, Debug)]
pub struct Bus {
    pub volume: f32,
    /// Empty means `master`; `master`'s own parent is empty and stays there.
    pub parent: String,
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
        let ancestor = if ancestor.is_empty() { MASTER } else { ancestor };
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
            });
    }

    /// Every bus, in name order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.buses.keys().cloned().collect()
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
    #[derive(serde::Deserialize)]
    struct Declared {
        #[serde(default = "one")]
        volume: f32,
        #[serde(default)]
        parent: String,
    }
    fn one() -> f32 {
        1.0
    }
    #[derive(serde::Deserialize, Default)]
    struct Audio {
        #[serde(default)]
        buses: BTreeMap<String, Declared>,
    }
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(default)]
        audio: Audio,
    }
    let Some(source) = balaur_core::project::manifest_source(eng) else {
        return BTreeMap::new();
    };
    let parsed = match toml::from_str::<Manifest>(&source) {
        Ok(manifest) => manifest.audio.buses,
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
                    volume: one.volume.max(0.0),
                    parent: one.parent,
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
                tracing::warn!("audio bus '{at}' names a parent '{parent}' nothing declares; feeding it to master");
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
