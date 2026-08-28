//! Godot-like scene tree layered on top of the ECS.
//!
//! A "node" is nothing more than an entity carrying the tree components
//! below. Plugins attach their own components to the same entity, so the
//! node abstraction costs nothing on the data plane.

use glamx::{Quat, Vec3};
use hecs::{Entity, World};

pub struct Name(pub String);
pub struct Parent(pub Entity);
pub struct Children(pub Vec<Entity>);

/// Which script file drives this node, if any. The live Lua instance is kept
/// by the script host, keyed by entity.
pub struct ScriptAttachment {
    pub path: String,
}

/// Local (parent-relative) transform, TRS convention.
#[derive(Clone, Copy)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub fn identity() -> Self {
        Transform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

/// World-space transform, recomputed every frame after the update stages.
/// Composition ignores shear (scale is combined component-wise), matching the
/// usual game engine convention.
#[derive(Clone, Copy)]
pub struct GlobalTransform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl GlobalTransform {
    pub fn identity() -> Self {
        GlobalTransform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    fn mul(&self, local: &Transform) -> GlobalTransform {
        GlobalTransform {
            position: self.position + self.rotation * (local.position * self.scale),
            rotation: self.rotation * local.rotation,
            scale: self.scale * local.scale,
        }
    }
}

pub(crate) fn spawn_root(world: &mut World) -> Entity {
    world.spawn((
        Name("Root".to_string()),
        Transform::identity(),
        GlobalTransform::identity(),
        Children(Vec::new()),
    ))
}

/// Spawn a new node under `parent`.
pub fn spawn_node(world: &mut World, name: &str, parent: Entity) -> Entity {
    let entity = world.spawn((
        Name(name.to_string()),
        Transform::identity(),
        GlobalTransform::identity(),
        Children(Vec::new()),
        Parent(parent),
    ));
    if let Ok(mut children) = world.get::<&mut Children>(parent) {
        children.0.push(entity);
    }
    entity
}

/// Resolve a `A/B/C` path relative to `from` by matching child names.
pub fn find_node(world: &World, from: Entity, path: &str) -> Option<Entity> {
    let mut current = from;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        let children = world.get::<&Children>(current).ok()?;
        let mut next = None;
        for &child in &children.0 {
            if let Ok(name) = world.get::<&Name>(child) {
                if name.0 == segment {
                    next = Some(child);
                    break;
                }
            }
        }
        drop(children);
        current = next?;
    }
    Some(current)
}

/// Absolute path of a node from the root, for debugging and editor display.
pub fn node_path(world: &World, entity: Entity) -> String {
    let mut segments = Vec::new();
    let mut current = entity;
    loop {
        match world.get::<&Name>(current) {
            Ok(name) => segments.push(name.0.clone()),
            Err(_) => break,
        }
        match world.get::<&Parent>(current) {
            Ok(parent) => current = parent.0,
            Err(_) => break,
        }
    }
    segments.reverse();
    segments.join("/")
}

/// Recompute every `GlobalTransform` from the root down.
pub fn propagate_transforms(world: &mut World, root: Entity) {
    let identity = GlobalTransform::identity();
    propagate_recursive(world, root, &identity);
}

fn propagate_recursive(world: &mut World, entity: Entity, parent_global: &GlobalTransform) {
    let global = match world.get::<&Transform>(entity) {
        Ok(local) => parent_global.mul(&local),
        Err(_) => *parent_global,
    };
    if let Ok(mut slot) = world.get::<&mut GlobalTransform>(entity) {
        *slot = global;
    }
    let children: Vec<Entity> = match world.get::<&Children>(entity) {
        Ok(children) => children.0.clone(),
        Err(_) => return,
    };
    for child in children {
        propagate_recursive(world, child, &global);
    }
}

/// Collect a subtree in despawn order (children before parents is not
/// required by hecs, but callers also use this to tear down script
/// instances and plugin state).
pub fn collect_subtree(world: &World, entity: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![entity];
    while let Some(e) = stack.pop() {
        out.push(e);
        if let Ok(children) = world.get::<&Children>(e) {
            stack.extend(children.0.iter().copied());
        }
    }
    out
}

/// Despawn a node and its whole subtree, unlinking it from its parent.
pub fn free_subtree(world: &mut World, entity: Entity) {
    if let Ok(parent) = world.get::<&Parent>(entity).map(|p| p.0) {
        if let Ok(mut children) = world.get::<&mut Children>(parent) {
            children.0.retain(|&c| c != entity);
        }
    }
    for e in collect_subtree(world, entity) {
        let _ = world.despawn(e);
    }
}
