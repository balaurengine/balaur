//! The ragdoll blend: a rig's bones moved toward the bodies built for them.
//!
//! The bodies are ordinary nodes carrying ordinary `body2d` / `collider2d` /
//! `joint2d` components, one per bone and named after it, under a container
//! the `ragdoll` component on the rig root names. The editor's Create Physical
//! Skeleton builds them into a scene. The container sits at the world origin,
//! because physics writes a simulated pose straight into a node's `Transform`
//! as a world pose.
//!
//! The component carries a weight, and each frame every bone is moved from the
//! pose the clip just wrote toward the pose its body ended up in. At `0` the
//! clip wins outright and the bodies simulate unseen, at `1` the rig is limp,
//! and between the two a hit can push a walk around without ending it.
//!
//! A clip writes only what it keys, so the pose a bone had before the blend
//! goes back on it at the start of the next frame: otherwise an unkeyed hip
//! would stay wherever it fell.

use balaur_core::Engine;
use balaur_core::collections::DetHashMap;
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Parent, Transform};
use balaur_geometry::skeleton;
use balaur_plugin::Registry;
use glamx::{Quat, Vec3};

/// A node's world pose composed from local transforms rather than read from
/// `GlobalTransform`.
///
/// The blend runs after the step, before the scene sync has caught up with
/// what the step moved.
fn world_pose(world: &balaur_core::hecs::World, entity: Entity) -> (Vec3, Quat) {
    let mut chain = vec![entity];
    let mut current = entity;
    while let Ok(parent) = world.get::<&Parent>(current) {
        current = parent.0;
        chain.push(current);
    }
    let (mut position, mut rotation, mut scale) = (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
    for e in chain.into_iter().rev() {
        if let Ok(t) = world.get::<&Transform>(e) {
            position += rotation * (t.position * scale);
            rotation *= t.rotation;
            scale *= t.scale;
        }
    }
    (position, rotation)
}

/// Drives a rig's bones from `bodies`, one body per bone and named after it, which the editor's Create Physical Skeleton builds. `influence` is how much of the simulated pose the bones take, 0 to 1.
#[derive(Clone, Debug, Component)]
#[component(name = "ragdoll", tags = [balaur_core::components::tag::PHYSICS])]
pub struct Ragdoll {
    /// The node holding the bodies, one per bone, named after it
    #[property(node)]
    pub bodies: String,
    /// How much of the simulated pose the bones take
    #[property(default = 1.0, min = 0.0, max = 1.0)]
    pub influence: f32,
}

impl Ragdoll {
    /// How much of the simulated pose the bones take, held to `0..1`.
    fn blend(&self) -> f32 {
        self.influence.clamp(0.0, 1.0)
    }
}

/// `ragdoll`: held on the rig root, and read by the blend each frame.
pub(crate) fn register_ragdoll_component(reg: &mut Registry<'_>) {
    reg.add_component::<Ragdoll>();
}

/// One bone's local pose, as a snapshot row keeps it.
type Pose = ([f32; 3], [f32; 4]);

/// What the blend found on a bone and what it left there.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Blended {
    incoming: Pose,
    written: Pose,
}

/// Every bone the blend wrote this frame.
#[derive(Default)]
pub(crate) struct BlendMemory(pub(crate) DetHashMap<Entity, Blended>);

/// Put back the pose each blended bone had before the blend, ahead of the
/// clip. A bone something else moved since keeps that move.
pub(crate) fn restore_system(eng: &Engine, _dt: f32) {
    if eng.frozen_root().is_some() || eng.paused() {
        return;
    }
    let memory = std::mem::take(&mut eng.resource::<BlendMemory>().borrow_mut().0);
    let world = eng.world();
    for (bone, pose) in memory {
        let Ok(mut t) = world.get::<&mut Transform>(bone) else {
            continue;
        };
        if t.position == Vec3::from_array(pose.written.0)
            && t.rotation == Quat::from_array(pose.written.1)
        {
            t.position = Vec3::from_array(pose.incoming.0);
            t.rotation = Quat::from_array(pose.incoming.1);
        }
    }
}

pub(crate) fn save_memory(eng: &Engine) -> serde_json::Value {
    let memory = eng.resource::<BlendMemory>();
    let rows = crate::keyed(&eng.world(), &memory.borrow().0);
    serde_json::to_value(rows).unwrap_or(serde_json::Value::Null)
}

pub(crate) fn load_memory(eng: &Engine, value: &serde_json::Value) {
    let rows = match serde_json::from_value(value.clone()) {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "restoring the ragdoll blend");
            return;
        }
    };
    let memory = crate::resolved(eng, rows);
    eng.resource::<BlendMemory>().borrow_mut().0 = memory;
}

/// Move every bone from the pose the clip wrote toward the pose its body
/// ended up in.
///
/// Runs after the step, so the bodies have moved; the bone's `Transform` at
/// this moment is still what the animation system wrote this frame, which is
/// exactly the other end of the blend. A weight of zero writes nothing at
/// all, so an armed but unused ragdoll costs one compare per rig.
pub(crate) fn blend_system(eng: &Engine, _dt: f32) {
    if eng.frozen_root().is_some() || eng.paused() {
        return;
    }
    let memory = eng.resource::<BlendMemory>();
    let mut memory = memory.borrow_mut();
    let world = eng.world();
    let rigs: Vec<(Entity, Ragdoll)> = world
        .query::<(Entity, &Ragdoll)>()
        .iter()
        .filter(|(_, r)| r.blend() > 0.0 && !r.bodies.is_empty())
        .filter(|(e, _)| !balaur_core::parked::is_parked(eng, *e))
        .map(|(e, r)| (e, r.clone()))
        .collect();
    for (rig, ragdoll) in rigs {
        let Some(container) = scene::find_node(&world, rig, &ragdoll.bodies) else {
            tracing::debug!(bodies = ragdoll.bodies, "ragdoll bodies name no node");
            continue;
        };
        // Parents first, so a bone's parent pose is the one blended this frame.
        for bone in skeleton::bones_under(&world, rig) {
            let Ok(name) = world.get::<&scene::Name>(bone) else {
                continue;
            };
            let Some(body) = scene::find_node(&world, container, &name.0) else {
                continue;
            };
            drop(name);
            // Composed rather than read off `GlobalTransform`, which the
            // scene sync has not caught up with since the step.
            let (wanted_position, wanted_rotation) = world_pose(&world, body);
            let parent = world.get::<&Parent>(bone).ok().map(|p| p.0);
            let (parent_position, parent_rotation) =
                parent.map_or((Vec3::ZERO, Quat::IDENTITY), |p| world_pose(&world, p));
            let inverse = parent_rotation.inverse();
            let local_position = inverse * (wanted_position - parent_position);
            let local_rotation = inverse * wanted_rotation;
            if let Ok(mut t) = world.get::<&mut Transform>(bone) {
                let incoming = (t.position.to_array(), t.rotation.to_array());
                t.position = t.position.lerp(local_position, ragdoll.blend());
                t.rotation = t.rotation.slerp(local_rotation, ragdoll.blend());
                let written = (t.position.to_array(), t.rotation.to_array());
                memory.0.insert(bone, Blended { incoming, written });
            }
        }
    }
}
