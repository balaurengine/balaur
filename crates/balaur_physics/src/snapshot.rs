//! The 3D world's snapshot frame, and the pieces the 2D frame and the
//! vehicle and ragdoll rows share: node keys, and the world as bincode.

use balaur_core::Engine;
use balaur_core::collections::DetHashMap;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use serde::Deserialize as _;

use crate::rapier3d::pipeline::PhysicsWorld;
use crate::rapier3d::prelude::{ColliderHandle, RigidBodyHandle};
use crate::vocabulary::component as c;
use crate::{PhysicsState3d, events, joint, softbody, vehicle};

/// The rapier world plus the maps tying it to entities.
///
/// The whole world rather than a per-body summary: rapier's own
/// `serde-serialize` skips exactly the workspace fields a snapshot must not
/// carry (the pipeline, the CCD solver), and reconstructing islands and
/// contact state by hand would be a second physics engine.
#[derive(serde::Deserialize)]
struct PhysicsFrame3d {
    #[serde(with = "world_bytes")]
    world: PhysicsWorld,
    bodies: Vec<(NodeKey, RigidBodyHandle)>,
    colliders: Vec<(NodeKey, Vec<ColliderHandle>)>,
    body_authored: Vec<(NodeKey, crate::shared::body::Authored)>,
    joints: Vec<(NodeKey, joint::JointRef3d)>,
    soft_bodies: Vec<(NodeKey, softbody::SoftRef3d)>,
    soft_params: Vec<(NodeKey, toml::Value)>,
    collider_params: Vec<(NodeKey, toml::Value)>,
    joint_params: Vec<(NodeKey, toml::Value)>,
    surfaces: Vec<(ColliderHandle, events::Surface)>,
    wheel_inputs: Vec<(NodeKey, vehicle::WheelInput3d)>,
    vehicles: Vec<(NodeKey, vehicle::VehicleFrame3d)>,
    follows: Vec<(NodeKey, crate::follow::FollowRef3d)>,
    grounded: Vec<(NodeKey, bool)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

/// The save side borrows: a rollback ring holds many of these, and
/// `PhysicsWorld` is not `Clone` precisely because copying one is expensive.
#[derive(serde::Serialize)]
struct PhysicsFrameRef3d<'a> {
    #[serde(with = "world_bytes")]
    world: &'a PhysicsWorld,
    bodies: Vec<(NodeKey, RigidBodyHandle)>,
    colliders: Vec<(NodeKey, Vec<ColliderHandle>)>,
    body_authored: Vec<(NodeKey, crate::shared::body::Authored)>,
    joints: Vec<(NodeKey, joint::JointRef3d)>,
    soft_bodies: Vec<(NodeKey, softbody::SoftRef3d)>,
    soft_params: Vec<(NodeKey, toml::Value)>,
    collider_params: Vec<(NodeKey, toml::Value)>,
    joint_params: Vec<(NodeKey, toml::Value)>,
    surfaces: Vec<(ColliderHandle, events::Surface)>,
    wheel_inputs: Vec<(NodeKey, vehicle::WheelInput3d)>,
    vehicles: Vec<(NodeKey, vehicle::VehicleFrame3d)>,
    follows: Vec<(NodeKey, crate::follow::FollowRef3d)>,
    grounded: Vec<(NodeKey, bool)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

/// A rapier world inside a snapshot frame, as base64 bincode.
///
/// JSON cannot carry one: a voxel grid's map is keyed by vectors, and a
/// one-way collider's `user_data` runs past 64 bits.
pub(crate) mod world_bytes {
    use base64::Engine as _;
    use serde::Deserialize as _;

    pub(crate) fn serialize<T: serde::Serialize, S: serde::Serializer>(
        world: &T,
        out: S,
    ) -> Result<S::Ok, S::Error> {
        let bytes = bincode::serde::encode_to_vec(world, bincode::config::standard())
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        out.serialize_str(&base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes))
    }

    pub(crate) fn deserialize<'de, T: serde::de::DeserializeOwned, D: serde::Deserializer<'de>>(
        input: D,
    ) -> Result<T, D::Error> {
        let text = String::deserialize(input)?;
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(text)
            .map_err(<D::Error as serde::de::Error>::custom)?;
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
            .map(|(world, _)| world)
            .map_err(<D::Error as serde::de::Error>::custom)
    }
}

/// A frame as the snapshot framework holds it, saying why when one cannot be
/// written rather than handing back a `Null` a restore silently skips.
pub(crate) fn frame_value(frame: impl serde::Serialize, world: &str) -> serde_json::Value {
    serde_json::to_value(frame).unwrap_or_else(|e| {
        tracing::error!(error = %e, "saving the {world} physics world");
        serde_json::Value::Null
    })
}

/// How a snapshot names a node: its [`balaur_core::ids`] id, and its entity
/// bits for a tree built by hand. Entity bits alone would not survive a
/// respawn, which mints a new entity for the same node.
pub(crate) type NodeKey = (String, u64);

pub(crate) fn key_of(world: &balaur_core::hecs::World, entity: Entity) -> NodeKey {
    (
        balaur_core::ids::of(world, entity).unwrap_or_default(),
        entity.to_bits().get(),
    )
}

/// The map a snapshot row belongs to, as keys a respawn cannot invalidate.
pub(crate) fn keyed<V: Clone>(
    world: &balaur_core::hecs::World,
    map: &DetHashMap<Entity, V>,
) -> Vec<(NodeKey, V)> {
    map.iter()
        .map(|(entity, value)| (key_of(world, *entity), value.clone()))
        .collect()
}

/// The node a key names now, which is a different entity after a respawn.
pub(crate) fn resolve_key(eng: &Engine, key: &NodeKey) -> Option<Entity> {
    let root = eng.root();
    let world = eng.world();
    if !key.0.is_empty()
        && let Some(entity) = balaur_core::ids::find(&world, root, &key.0)
    {
        return Some(entity);
    }
    let entity = Entity::from_bits(key.1)?;
    world.contains(entity).then_some(entity)
}

/// A component's params as a frame keeps them: each table without the keys
/// still at their schema default, which [`filled`] writes back on restore.
pub(crate) fn authored(
    eng: &Engine,
    world: &balaur_core::hecs::World,
    map: &DetHashMap<Entity, toml::Value>,
    component: &str,
) -> Vec<(NodeKey, toml::Value)> {
    let defaults = defaults_of(eng, component);
    map.iter()
        .map(|(entity, params)| (key_of(world, *entity), trimmed(params, &defaults)))
        .collect()
}

/// The rows a frame kept, with every key it left out at its default again.
pub(crate) fn filled(
    eng: &Engine,
    rows: Vec<(NodeKey, toml::Value)>,
    component: &str,
) -> Vec<(NodeKey, toml::Value)> {
    let defaults = defaults_of(eng, component);
    rows.into_iter()
        .map(|(key, mut params)| {
            if let Some(table) = params.as_table_mut() {
                for (name, value) in &defaults {
                    table.entry(name.clone()).or_insert_with(|| value.clone());
                }
            }
            (key, params)
        })
        .collect()
}

/// Every property a component declares, at its default, as an `add` would
/// fill it in.
fn defaults_of(eng: &Engine, component: &str) -> toml::map::Map<String, toml::Value> {
    let registry = eng.resource::<balaur_core::components::ComponentRegistry>();
    let registry = registry.borrow();
    registry
        .def(component)
        .and_then(|def| balaur_core::components::merge_defaults(&def.schema, None).ok())
        .and_then(|params| params.as_table().cloned())
        .unwrap_or_default()
}

/// `params` without its defaults. A table missing a key the schema defaults
/// is kept whole, so the restore's fill cannot add what it never held.
fn trimmed(params: &toml::Value, defaults: &toml::map::Map<String, toml::Value>) -> toml::Value {
    let Some(table) = params.as_table() else {
        return params.clone();
    };
    if defaults.keys().any(|name| !table.contains_key(name)) {
        return params.clone();
    }
    toml::Value::Table(
        table
            .iter()
            .filter(|(name, value)| !defaults.get(*name).is_some_and(|d| same(d, value)))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
    )
}

/// Equal to the bit: `-0.0` is not the default `0.0`, so it stays in the frame.
fn same(a: &toml::Value, b: &toml::Value) -> bool {
    match (a, b) {
        (toml::Value::Float(x), toml::Value::Float(y)) => x.to_bits() == y.to_bits(),
        (toml::Value::Array(x), toml::Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (toml::Value::Table(x), toml::Value::Table(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(name, value)| y.get(name).is_some_and(|other| same(value, other)))
        }
        _ => a == b,
    }
}

pub(crate) fn resolved<V>(eng: &Engine, rows: Vec<(NodeKey, V)>) -> DetHashMap<Entity, V> {
    rows.into_iter()
        .filter_map(|(key, value)| Some((resolve_key(eng, &key)?, value)))
        .collect()
}

fn save_physics(eng: &Engine) -> serde_json::Value {
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let world = eng.world();
    let frame = PhysicsFrameRef3d {
        world: &state.world,
        bodies: keyed(&world, &state.bodies),
        colliders: keyed(&world, &state.colliders),
        body_authored: keyed(&world, &state.body_authored),
        joints: keyed(&world, &state.joints),
        soft_bodies: keyed(&world, &state.soft_bodies),
        soft_params: authored(eng, &world, &state.soft_params, c::SOFTBODY_3D),
        collider_params: authored(eng, &world, &state.collider_params, c::COLLIDER_3D),
        joint_params: authored(eng, &world, &state.joint_params, c::JOINT_3D),
        surfaces: state.surfaces.iter().map(|(h, s)| (*h, *s)).collect(),
        wheel_inputs: keyed(&world, &state.wheel_inputs),
        vehicles: vehicle::frame_rows(&world, &state.vehicles),
        follows: keyed(&world, &state.follows),
        grounded: keyed(&world, &state.grounded),
        shape_revision: state.shape_revision,
        paused: state.paused,
        sleeping_allowed: state.sleeping_allowed,
    };
    frame_value(frame, "3D")
}

fn load_physics(eng: &Engine, value: &serde_json::Value) {
    let frame: PhysicsFrame3d = match PhysicsFrame3d::deserialize(value) {
        Ok(frame) => frame,
        Err(e) => {
            tracing::error!(error = %e, "restoring the physics world");
            return;
        }
    };
    let bodies = resolved(eng, frame.bodies);
    let colliders = resolved(eng, frame.colliders);
    let body_authored = resolved(eng, frame.body_authored);
    let joints = resolved(eng, frame.joints);
    let soft_bodies = resolved(eng, frame.soft_bodies);
    let soft_params = resolved(eng, filled(eng, frame.soft_params, c::SOFTBODY_3D));
    let collider_params = resolved(eng, filled(eng, frame.collider_params, c::COLLIDER_3D));
    let joint_params = resolved(eng, filled(eng, frame.joint_params, c::JOINT_3D));
    let wheel_inputs = resolved(eng, frame.wheel_inputs);
    let vehicles = vehicle::resolved_rows(eng, frame.vehicles);
    let follows = resolved(eng, frame.follows);
    let grounded = resolved(eng, frame.grounded);
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    state.world = frame.world;
    state.shape_revision = frame.shape_revision;
    state.paused = frame.paused;
    state.sleeping_allowed = frame.sleeping_allowed;
    state.bodies = bodies;
    state.colliders = colliders;
    state.body_authored = body_authored;
    state.joints = joints;
    state.soft_bodies = soft_bodies;
    state.soft_params = soft_params;
    state.collider_params = collider_params;
    state.joint_params = joint_params;
    state.surfaces = frame.surfaces.into_iter().collect();
    state.wheel_inputs = wheel_inputs;
    state.vehicles = vehicles;
    state.follows = follows;
    state.grounded = grounded;
    super::restamp_collider_owners(&mut state);
}

pub(crate) fn build_physics_snapshot(reg: &mut Registry<'_>) {
    reg.add_snapshot_source("physics", save_physics, load_physics);
}
