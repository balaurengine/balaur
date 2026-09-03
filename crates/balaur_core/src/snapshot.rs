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
//! Nodes come and go, so the `nodes` source records the live set and puts it
//! back: it frees what was spawned after the snapshot and respawns what was
//! freed, with its name, parent, components and script. It is registered
//! first, so every later source finds the entity it is about to write to.
//!
//! Everything is keyed by [`StableId`] rather than by entity index, because a
//! respawned node is a new entity. The recorded index is kept as a fallback
//! for a node that carries no id, which is a tree built by hand in a test
//! rather than one loaded from a scene.

use anyhow::{Context, Result};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::components::StableId;
use crate::engine::Engine;
use crate::scene::{collect_subtree, Name, Parent, ScriptAttachment};

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
    // First: it decides which entities exist, and every source below writes
    // into them.
    app.add_snapshot_source("nodes", save_nodes, load_nodes);
    app.add_snapshot_source("transforms", save_transforms, load_transforms);
    // The clock, so restoring a tick puts the tick number back too: a
    // rollback that re-ran tick 40 while the engine still counted 47 would
    // hand scripts a number the first run never saw.
    app.add_snapshot_source(
        "clock",
        |eng| serde_json::json!({ "tick": eng.tick(), "time": eng.time() }),
        |eng, value| {
            let tick = value.get("tick").and_then(serde_json::Value::as_u64);
            let time = value.get("time").and_then(serde_json::Value::as_f64);
            if let (Some(tick), Some(time)) = (tick, time) {
                eng.set_clock(tick, time);
            }
        },
    );
    // Script instances, through the host's own save/load contract. Core never
    // learns the language; NodeId carries no serde, so it travels as bits.
    app.add_snapshot_source(
        "scripts",
        |eng| {
            let Some(host) = eng.script_host() else {
                return serde_json::Value::Null;
            };
            let world = eng.world();
            let states: Vec<ScriptFrame> = host
                .save_state()
                .into_iter()
                .map(|(node, value)| ScriptFrame {
                    id: crate::entity_of(node)
                        .ok()
                        .and_then(|e| crate::ids::of(&world, e)),
                    entity: node.0,
                    value,
                })
                .collect();
            serde_json::to_value(states).unwrap_or(serde_json::Value::Null)
        },
        |eng, value| {
            let Some(host) = eng.script_host() else {
                return;
            };
            let Ok(frames) = serde_json::from_value::<Vec<ScriptFrame>>(value.clone()) else {
                return;
            };
            let world = eng.world();
            let root = eng.root();
            let states: Vec<_> = frames
                .into_iter()
                .filter_map(|frame| {
                    let entity = resolve(&world, root, frame.id.as_deref(), frame.entity)?;
                    Some((crate::node_id_of(entity), frame.value))
                })
                .collect();
            drop(world);
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

/// Keyed by stable id where a node has one, because a respawned node is a
/// new entity. The index is the fallback for a node that carries no id.
#[derive(Serialize, Deserialize)]
struct TransformFrame {
    id: Option<String>,
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
                id: crate::ids::of(&world, entity),
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
    let root = eng.root();
    for frame in frames {
        let Some(entity) = resolve(&world, root, frame.id.as_deref(), frame.entity) else {
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

/// The entity a frame refers to: by stable id where it has one, and by the
/// recorded index otherwise.
fn resolve(world: &hecs::World, root: Entity, id: Option<&str>, entity: u64) -> Option<Entity> {
    match id {
        Some(id) => crate::ids::find(world, root, id),
        None => hecs::Entity::from_bits(entity),
    }
}

/// One script instance's state, keyed the way transforms are.
#[derive(Serialize, Deserialize)]
struct ScriptFrame {
    id: Option<String>,
    entity: u64,
    value: balaur_script::Value,
}

/// One node as the snapshot remembers it: enough to build it again.
///
/// Components travel as the TOML they were declared with, not as whatever a
/// subsystem derived from them, so a respawn goes back in through the same
/// door a scene file uses.
#[derive(Serialize, Deserialize)]
struct NodeFrame {
    id: String,
    name: String,
    /// The parent's stable id; absent when the parent is the root.
    parent: Option<String>,
    script: Option<String>,
    components: Vec<(String, String)>,
}

/// The live node set, and the id counter that must mint the same ids again.
#[derive(Default, Serialize, Deserialize)]
struct NodesFrame {
    next_id: u64,
    nodes: Vec<NodeFrame>,
}

fn save_nodes(eng: &Engine) -> serde_json::Value {
    let root = eng.root();
    let entities = collect_subtree(&eng.world(), root);
    let mut nodes = Vec::new();
    for entity in entities {
        if entity == root {
            continue;
        }
        // The component hooks read the world themselves, so this borrow ends
        // before they are called.
        let Some((id, name, parent, script)) = ({
            let world = eng.world();
            crate::ids::of(&world, entity).map(|id| {
                let name = world
                    .get::<&Name>(entity)
                    .map_or_else(|_| String::new(), |n| n.0.clone());
                let parent = world
                    .get::<&Parent>(entity)
                    .ok()
                    .map(|p| p.0)
                    .filter(|&p| p != root)
                    .and_then(|p| crate::ids::of(&world, p));
                let script = world
                    .get::<&ScriptAttachment>(entity)
                    .ok()
                    .map(|s| s.path.clone());
                (id, name, parent, script)
            })
        }) else {
            continue;
        };
        let components = crate::components::present_on(eng, entity)
            .into_iter()
            .filter_map(|name| {
                let value = crate::components::get(eng, entity, &name)?;
                Some((name, toml::to_string(&value).ok()?))
            })
            .collect();
        nodes.push(NodeFrame {
            id,
            name,
            parent,
            script,
            components,
        });
    }
    let next_id = eng
        .try_resource::<crate::ids::IdAllocator>()
        .map_or(0, |a| a.borrow().next);
    serde_json::to_value(NodesFrame { next_id, nodes }).unwrap_or(serde_json::Value::Null)
}

fn load_nodes(eng: &Engine, value: &serde_json::Value) {
    let frame: NodesFrame = match serde_json::from_value(value.clone()) {
        Ok(frame) => frame,
        Err(e) => {
            tracing::error!(error = %e, "restoring the node set");
            return;
        }
    };
    let root = eng.root();
    let wanted: crate::DetHashSet<&str> = frame.nodes.iter().map(|n| n.id.as_str()).collect();
    free_spawned_since(eng, root, &wanted);
    // Frames are in the order `collect_subtree` produced, which puts a parent
    // before its children, so a parent is back before anything asks for it.
    for node in &frame.nodes {
        respawn(eng, root, node);
    }
    if let Some(allocator) = eng.try_resource::<crate::ids::IdAllocator>() {
        allocator.borrow_mut().next = frame.next_id;
    }
}

/// Free every node the snapshot does not name. A node with no id is left
/// alone: nothing recorded it, so nothing can say it is new.
fn free_spawned_since(eng: &Engine, root: Entity, wanted: &crate::DetHashSet<&str>) {
    let extra: Vec<Entity> = {
        let world = eng.world();
        collect_subtree(&world, root)
            .into_iter()
            .filter(|&e| e != root)
            .filter(|&e| crate::ids::of(&world, e).is_some_and(|id| !wanted.contains(id.as_str())))
            .collect()
    };
    for entity in extra {
        // Its parent may have taken it already.
        if !eng.world().contains(entity) {
            continue;
        }
        if let Some(host) = eng.script_host() {
            let subtree = collect_subtree(&eng.world(), entity);
            for e in subtree {
                host.detach(crate::node_id_of(e));
            }
        }
        crate::scene::free_subtree(&mut eng.world_mut(), entity);
    }
}

/// Put one node back, if it is not already there.
fn respawn(eng: &Engine, root: Entity, node: &NodeFrame) {
    let parent = {
        let world = eng.world();
        if crate::ids::find(&world, root, &node.id).is_some() {
            return;
        }
        match &node.parent {
            None => root,
            Some(parent) => {
                let Some(entity) = crate::ids::find(&world, root, parent) else {
                    tracing::error!(node = %node.id, parent = %parent, "respawning under a parent that is gone");
                    return;
                };
                entity
            }
        }
    };
    let entity = {
        let mut world = eng.world_mut();
        let entity = crate::scene::spawn_node(&mut world, &node.name, parent);
        let _ = world.insert_one(entity, StableId(node.id.clone()));
        entity
    };
    for (name, text) in &node.components {
        let Ok(params) = toml::from_str::<toml::Value>(text) else {
            continue;
        };
        if let Err(e) = crate::components::add(eng, entity, name, Some(&params)) {
            tracing::error!(error = %e, component = %name, node = %node.id, "restoring a component");
        }
    }
    if let Some(path) = &node.script {
        if let Some(host) = eng.script_host() {
            if let Err(e) = host.attach(crate::node_id_of(entity), path) {
                tracing::error!(error = %e, node = %node.id, "reattaching a script");
            }
        }
    }
}
