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
use crate::scene::{Children, Name, Parent, ScriptAttachment, collect_subtree};

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

    /// Record the world at `tick`, replacing what was there.
    ///
    /// Replacing matters: a rollback re-runs ticks that are already in the
    /// ring, and appending them again would fill it with duplicates, so a
    /// ring of thirty-two would hold far fewer than thirty-two distinct
    /// ticks and drop the oldest long before it had to.
    pub fn push(&mut self, tick: u64, snapshot: Snapshot) {
        if let Some(slot) = self.frames.iter_mut().find(|(at, _)| *at == tick) {
            slot.1 = snapshot;
            return;
        }
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
    app.add_snapshot_source("appearance", save_appearance, load_appearance);
    app.add_snapshot_source("tags", save_tags, load_tags);
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
                trs: t.trs(),
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

/// Keyed the way `TransformFrame` is.
#[derive(Serialize, Deserialize)]
struct AppearanceFrame {
    id: Option<String>,
    entity: u64,
    visible: bool,
    z_index: i32,
    z_relative: bool,
}

fn save_appearance(eng: &Engine) -> serde_json::Value {
    let world = eng.world();
    let frames: Vec<AppearanceFrame> = crate::scene::collect_subtree(&world, eng.root())
        .into_iter()
        .filter_map(|entity| {
            let a = world.get::<&crate::scene::Appearance>(entity).ok()?;
            Some(AppearanceFrame {
                id: crate::ids::of(&world, entity),
                entity: entity.to_bits().get(),
                visible: a.visible,
                z_index: a.z_index,
                z_relative: a.z_relative,
            })
        })
        .collect();
    serde_json::to_value(frames).unwrap_or(serde_json::Value::Null)
}

fn load_appearance(eng: &Engine, value: &serde_json::Value) {
    let frames: Vec<AppearanceFrame> = match serde_json::from_value(value.clone()) {
        Ok(frames) => frames,
        Err(e) => {
            tracing::error!(error = %e, "restoring appearance");
            return;
        }
    };
    let world = eng.world();
    let root = eng.root();
    for frame in frames {
        let Some(entity) = resolve(&world, root, frame.id.as_deref(), frame.entity) else {
            continue;
        };
        let Ok(mut a) = world.get::<&mut crate::scene::Appearance>(entity) else {
            continue;
        };
        a.visible = frame.visible;
        a.z_index = frame.z_index;
        a.z_relative = frame.z_relative;
    }
}

#[derive(Serialize, Deserialize)]
struct TagsFrame {
    id: Option<String>,
    entity: u64,
    tags: Vec<String>,
}

fn save_tags(eng: &Engine) -> serde_json::Value {
    let world = eng.world();
    let frames: Vec<TagsFrame> = crate::scene::collect_subtree(&world, eng.root())
        .into_iter()
        .filter_map(|entity| {
            let tags = world.get::<&crate::scene::Tags>(entity).ok()?;
            Some(TagsFrame {
                id: crate::ids::of(&world, entity),
                entity: entity.to_bits().get(),
                tags: tags.0.clone(),
            })
        })
        .collect();
    serde_json::to_value(frames).unwrap_or(serde_json::Value::Null)
}

fn load_tags(eng: &Engine, value: &serde_json::Value) {
    let Ok(frames) = serde_json::from_value::<Vec<TagsFrame>>(value.clone()) else {
        return;
    };
    let root = eng.root();
    // Every node's tags are replaced: a tag added after the snapshot goes.
    let all: Vec<Entity> = crate::scene::collect_subtree(&eng.world(), root);
    let mut world = eng.world_mut();
    for entity in all {
        let _ = world.remove_one::<crate::scene::Tags>(entity);
    }
    for frame in frames {
        let Some(entity) = resolve(&world, root, frame.id.as_deref(), frame.entity) else {
            continue;
        };
        let _ = world.insert_one(entity, crate::scene::Tags(frame.tags));
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
    /// Where among the parent's children it sat. The digest walks the tree in
    /// order, so a node put back at the end is a divergence of its own.
    #[serde(default)]
    index: usize,
    script: Option<String>,
    /// What the node's `script` key set over the script's exports, so a
    /// respawn's `init` reads the tuned values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    props: Vec<(String, balaur_script::Value)>,
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
        let Some((id, name, parent, index, script, props)) = ({
            let world = eng.world();
            crate::ids::of(&world, entity).map(|id| {
                let name = world
                    .get::<&Name>(entity)
                    .map_or_else(|_| String::new(), |n| n.0.clone());
                let above = world.get::<&Parent>(entity).ok().map(|p| p.0);
                let index = above
                    .and_then(|p| world.get::<&Children>(p).ok())
                    .and_then(|c| c.0.iter().position(|&child| child == entity))
                    .unwrap_or(0);
                let parent = above
                    .filter(|&p| p != root)
                    .and_then(|p| crate::ids::of(&world, p));
                let script = world
                    .get::<&ScriptAttachment>(entity)
                    .ok()
                    .map(|s| s.path.clone());
                let props = crate::scene::script_props(&world, entity);
                (id, name, parent, index, script, props)
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
            index,
            script,
            props,
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
    for node in respawn_order(&frame.nodes) {
        respawn(eng, root, node);
    }
    if let Some(allocator) = eng.try_resource::<crate::ids::IdAllocator>() {
        allocator.borrow_mut().next = frame.next_id;
    }
}

/// Frame order for a respawn: parents first, then siblings by index.
///
/// `collect_subtree` pops a stack, so it hands the last child back first, and
/// an insert at a recorded index only lands right once the earlier siblings
/// are there. Three siblings freed in one tick came back reversed.
fn respawn_order(nodes: &[NodeFrame]) -> Vec<&NodeFrame> {
    let at: crate::DetHashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();
    let mut depth = vec![0usize; nodes.len()];
    for (i, slot) in depth.iter_mut().enumerate() {
        let mut cursor = i;
        while let Some(parent) = nodes[cursor].parent.as_deref() {
            let Some(&above) = at.get(parent) else { break };
            *slot += 1;
            cursor = above;
            // A decoded frame is the one place a parent chain can cycle.
            if *slot > nodes.len() {
                break;
            }
        }
    }
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| (depth[i], nodes[i].index));
    order.into_iter().map(|i| &nodes[i]).collect()
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
        crate::scene::free_node(eng, entity);
    }
}

/// Put a node that survived back where the snapshot had it.
///
/// `set_parent` and a rename are simulation state a script can write, so a
/// node still being alive is not the same as it being unchanged: the digest
/// walks the tree in order, and a node left under its new parent is a desync.
fn restore_placement(eng: &Engine, entity: Entity, parent: Entity, node: &NodeFrame) {
    let mut world = eng.world_mut();
    if let Ok(mut name) = world.get::<&mut Name>(entity)
        && name.0 != node.name {
            name.0.clone_from(&node.name);
        }
    if world.get::<&Parent>(entity).ok().map(|p| p.0) == Some(parent) {
        return;
    }
    if let Err(e) = crate::scene::reparent(&mut world, entity, parent) {
        tracing::error!(error = %e, node = %node.id, "moving a node back under its parent");
        return;
    }
    // `reparent` appends; the recorded index is where it actually sat.
    move_child_to(&mut world, parent, entity, node.index);
}

/// Move `entity` to `index` among `parent`'s children, which is what makes a
/// restore reproduce the tree order the digest walks rather than only the set.
fn move_child_to(world: &mut hecs::World, parent: Entity, entity: Entity, index: usize) {
    let Ok(mut children) = world.get::<&mut Children>(parent) else {
        return;
    };
    let Some(at) = children.0.iter().position(|&c| c == entity) else {
        return;
    };
    let moved = children.0.remove(at);
    let to = index.min(children.0.len());
    children.0.insert(to, moved);
}

/// Put one node back, if it is not already there.
fn respawn(eng: &Engine, root: Entity, node: &NodeFrame) {
    let (existing, parent) = {
        let world = eng.world();
        let existing = crate::ids::find(&world, root, &node.id);
        let parent = match &node.parent {
            None => root,
            Some(parent) => {
                let Some(entity) = crate::ids::find(&world, root, parent) else {
                    tracing::error!(node = %node.id, parent = %parent, "respawning under a parent that is gone");
                    return;
                };
                entity
            }
        };
        (existing, parent)
    };
    if let Some(entity) = existing {
        restore_placement(eng, entity, parent, node);
        return;
    }
    let entity = {
        let mut world = eng.world_mut();
        let entity = crate::scene::spawn_node_at(&mut world, &node.name, parent, node.index);
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
    if let Some(path) = &node.script
        && let Some(host) = eng.script_host() {
            crate::scene::remember_script_props(eng, entity, &node.props);
            if let Err(e) = host.attach_with_props(crate::node_id_of(entity), path, &node.props) {
                tracing::error!(error = %e, node = %node.id, "reattaching a script");
            }
        }
}
