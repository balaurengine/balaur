//! Save and restore the whole simulation, for rollback.
//!
//! Replay re-simulates a session from its inputs. Rollback also has to *go
//! back*: a late input for tick N means restoring the world as it was at N
//! and stepping forward again. That needs real state, not a hash and not an
//! input log.
//!
//! Sources register the way digest and replay sources do — core owns
//! transforms, components and the RNG; physics contributes its world; the
//! script host contributes each instance's fields.
//!
//! **What is not covered yet:** entities created or destroyed since the
//! snapshot. Restoring writes over the state of nodes that exist; it does
//! not respawn a node that has since been freed, nor free one that has since
//! been spawned. Rolling back across a spawn is the next piece.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// State a subsystem owns, in a form that survives being put down and picked
/// back up.
pub type SaveFn = Box<dyn Fn(&Engine) -> serde_json::Value>;
/// The inverse of a [`SaveFn`].
pub type LoadFn = Box<dyn Fn(&Engine, &serde_json::Value)>;

/// Every subsystem that owns simulation state, in registration order.
#[derive(Default)]
pub struct SnapshotRegistry(pub Vec<(String, SaveFn, LoadFn)>);

/// One moment of the simulation, keyed by source name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot(pub serde_json::Map<String, serde_json::Value>);

impl Snapshot {
    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("encoding a snapshot")
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("decoding a snapshot")
    }
}

/// Ask every registered source for its state.
pub fn capture(eng: &Engine) -> Snapshot {
    let mut out = serde_json::Map::new();
    if let Some(sources) = eng.try_resource::<SnapshotRegistry>() {
        for (name, save, _) in &sources.borrow().0 {
            out.insert(name.clone(), save(eng));
        }
    }
    Snapshot(out)
}

/// Put every registered source back the way it was.
///
/// A source missing from the snapshot is left alone, so a snapshot taken
/// before a plugin was added still restores what it does cover.
pub fn restore(eng: &Engine, snapshot: &Snapshot) {
    if let Some(sources) = eng.try_resource::<SnapshotRegistry>() {
        for (name, _, load) in &sources.borrow().0 {
            if let Some(value) = snapshot.0.get(name) {
                load(eng, value);
            }
        }
    }
}

/// A bounded history of recent states, oldest dropped first.
///
/// What rollback keeps: the last N ticks, so a late input for any of them
/// can be answered by restoring and re-simulating.
pub struct SnapshotRing {
    frames: std::collections::VecDeque<(u64, Snapshot)>,
    capacity: usize,
}

impl SnapshotRing {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: std::collections::VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, tick: u64, snapshot: Snapshot) {
        if self.frames.len() == self.capacity {
            self.frames.pop_front();
        }
        self.frames.push_back((tick, snapshot));
    }

    /// The state at `tick`, if it is still in the window.
    #[must_use]
    pub fn get(&self, tick: u64) -> Option<&Snapshot> {
        self.frames
            .iter()
            .find(|(at, _)| *at == tick)
            .map(|(_, snapshot)| snapshot)
    }

    /// The oldest tick still restorable — how far back rollback can reach.
    #[must_use]
    pub fn earliest(&self) -> Option<u64> {
        self.frames.front().map(|(tick, _)| *tick)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// What core itself owns: every node's local transform, and the RNG stream.
///
/// Deliberately not components. A component's `apply` may have side effects —
/// re-adding `body` rebuilds the rigid body and throws away its velocity — so
/// the subsystem that owns the state restores it, exactly as with digests.
pub(crate) fn build_core_sources(app: &mut crate::app::App) {
    app.add_snapshot_source("transforms", save_transforms, load_transforms);
    // Script instances, through the host's own save/load contract. Core never
    // learns the language; NodeId carries no serde, so it travels as bits.
    app.add_snapshot_source(
        "scripts",
        |eng| {
            let Some(host) = eng.script_host() else {
                return serde_json::Value::Null;
            };
            let states: Vec<(u64, balaur_script::Value)> = host
                .save_state()
                .into_iter()
                .map(|(node, value)| (node.0, value))
                .collect();
            serde_json::to_value(states).unwrap_or(serde_json::Value::Null)
        },
        |eng, value| {
            let Some(host) = eng.script_host() else {
                return;
            };
            let Ok(states) =
                serde_json::from_value::<Vec<(u64, balaur_script::Value)>>(value.clone())
            else {
                return;
            };
            let states: Vec<_> = states
                .into_iter()
                .map(|(bits, value)| (balaur_script::NodeId(bits), value))
                .collect();
            host.load_state(&states);
        },
    );
    app.add_snapshot_source(
        "rng",
        |eng| serde_json::json!(crate::rng::with_rng(eng, |rng| rng.state())),
        |eng, value| {
            if let Some(state) = value.as_u64() {
                crate::rng::with_rng(eng, |rng| *rng = crate::rng::Pcg32::from_state(state));
            }
        },
    );
}

/// Keyed by entity bits: a rollback restores the world it left, in the same
/// process, so the handles are the same handles.
#[derive(Serialize, Deserialize)]
struct TransformFrame {
    entity: u64,
    trs: [f32; 10],
}

fn save_transforms(eng: &Engine) -> serde_json::Value {
    let world = eng.world();
    let frames: Vec<TransformFrame> = crate::scene::collect_subtree(&world, eng.root())
        .into_iter()
        .filter_map(|entity| {
            let t = world.get::<&crate::scene::Transform>(entity).ok()?;
            Some(TransformFrame {
                entity: entity.to_bits().get(),
                trs: [
                    t.position.x,
                    t.position.y,
                    t.position.z,
                    t.rotation.x,
                    t.rotation.y,
                    t.rotation.z,
                    t.rotation.w,
                    t.scale.x,
                    t.scale.y,
                    t.scale.z,
                ],
            })
        })
        .collect();
    serde_json::to_value(frames).unwrap_or(serde_json::Value::Null)
}

fn load_transforms(eng: &Engine, value: &serde_json::Value) {
    let frames: Vec<TransformFrame> = match serde_json::from_value(value.clone()) {
        Ok(frames) => frames,
        Err(e) => {
            tracing::error!(error = %e, "restoring transforms");
            return;
        }
    };
    let world = eng.world();
    for frame in frames {
        let Some(entity) = hecs::Entity::from_bits(frame.entity) else {
            continue;
        };
        let Ok(mut t) = world.get::<&mut crate::scene::Transform>(entity) else {
            continue;
        };
        let v = frame.trs;
        t.position = glamx::Vec3::new(v[0], v[1], v[2]);
        t.rotation = glamx::Quat::from_xyzw(v[3], v[4], v[5], v[6]);
        t.scale = glamx::Vec3::new(v[7], v[8], v[9]);
    }
}
