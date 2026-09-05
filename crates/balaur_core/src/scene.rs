//! Godot-like scene tree layered on top of the ECS.
//!
//! A "node" is nothing more than an entity carrying the tree components
//! below. Plugins attach their own components to the same entity, so the
//! node abstraction costs nothing on the data plane.

use glamx::{Quat, Vec3};
use hecs::{Entity, World};

use crate::engine::Engine;

pub struct Name(pub String);
pub struct Parent(pub Entity);
pub struct Children(pub Vec<Entity>);

/// A parent's children by name, so a path segment is one lookup rather than
/// a scan of every sibling.
///
/// Kept by the functions in this module that add, remove, move or rename a
/// child; a parent without one (a world built by hand in a test) is scanned.
#[derive(Default)]
pub struct NameIndex(pub crate::collections::DetHashMap<String, NameSlot>);

/// One name among a parent's children: the earliest in tree order bearing
/// it, which is what [`find_node`] answers, and how many do, so losing one of
/// a unique name is a removal and nothing more.
#[derive(Clone, Copy)]
pub struct NameSlot {
    pub first: Entity,
    pub count: u32,
}

/// Which script file drives this node, if any. The live instance is kept
/// by the script host, keyed by entity.
pub struct ScriptAttachment {
    pub path: String,
}

/// What the node's `script` key set over the script's exported defaults.
///
/// Kept because `init` reads them: a node put back by a snapshot has to
/// re-attach with the tuned values, not with the exports.
pub struct ScriptProps(pub Vec<(String, balaur_script::Value)>);

/// Record what a node was attached with, replacing whatever it carried.
pub fn remember_script_props(
    eng: &Engine,
    entity: Entity,
    props: &[(String, balaur_script::Value)],
) {
    let mut world = eng.world_mut();
    if props.is_empty() {
        let _ = world.remove_one::<ScriptProps>(entity);
        return;
    }
    let _ = world.insert_one(entity, ScriptProps(props.to_vec()));
}

/// What [`remember_script_props`] recorded, empty for a node that set none.
#[must_use]
pub fn script_props(world: &World, entity: Entity) -> Vec<(String, balaur_script::Value)> {
    world
        .get::<&ScriptProps>(entity)
        .map(|p| p.0.clone())
        .unwrap_or_default()
}

/// Local (parent-relative) transform, TRS convention.
#[derive(Clone, Copy)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const fn identity() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Every component in TRS order, the shape a digest hashes and a
    /// snapshot stores.
    #[must_use]
    pub fn trs(&self) -> [f32; 10] {
        [
            self.position.x,
            self.position.y,
            self.position.z,
            self.rotation.x,
            self.rotation.y,
            self.rotation.z,
            self.rotation.w,
            self.scale.x,
            self.scale.y,
            self.scale.z,
        ]
    }
}

/// Whether a node draws, and which layer it draws on.
///
/// Nothing in physics reads either field: a hidden collider still collides,
/// which is what a game hiding a sprite for a frame expects.
#[derive(Clone, Copy)]
pub struct Appearance {
    pub visible: bool,
    pub z_index: i32,
    /// Add `z_index` to the parent's rather than replacing it, so moving a
    /// subtree between layers keeps the order inside it.
    pub z_relative: bool,
}

impl Appearance {
    pub const fn identity() -> Self {
        Self {
            visible: true,
            z_index: 0,
            z_relative: true,
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self::identity()
    }
}

/// The names a node is filed under, for a query: `door`, `enemy`. The same
/// word `presets.toml` uses for components, so one vocabulary classifies
/// both. Kept sorted and unique, so two runs list them alike.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags(pub Vec<String>);

impl Tags {
    pub fn add(&mut self, tag: &str) -> bool {
        match self.0.binary_search_by(|t| t.as_str().cmp(tag)) {
            Ok(_) => false,
            Err(at) => {
                self.0.insert(at, tag.to_string());
                true
            }
        }
    }

    pub fn remove(&mut self, tag: &str) -> bool {
        match self.0.binary_search_by(|t| t.as_str().cmp(tag)) {
            Ok(at) => {
                self.0.remove(at);
                true
            }
            Err(_) => false,
        }
    }

    #[must_use]
    pub fn has(&self, tag: &str) -> bool {
        self.0.binary_search_by(|t| t.as_str().cmp(tag)).is_ok()
    }
}

/// Every node carrying `tag`, in tree order.
#[must_use]
pub fn tagged(world: &World, root: Entity, tag: &str) -> Vec<Entity> {
    collect_subtree(world, root)
        .into_iter()
        .filter(|&e| world.get::<&Tags>(e).is_ok_and(|t| t.has(tag)))
        .collect()
}

/// World-space appearance, recomputed beside `GlobalTransform`.
#[derive(Clone, Copy)]
pub struct GlobalAppearance {
    pub visible: bool,
    pub z_index: i32,
}

impl GlobalAppearance {
    pub const fn identity() -> Self {
        Self {
            visible: true,
            z_index: 0,
        }
    }

    fn mul(self, local: Appearance) -> Self {
        Self {
            visible: self.visible && local.visible,
            z_index: if local.z_relative {
                self.z_index.saturating_add(local.z_index)
            } else {
                local.z_index
            },
        }
    }
}

impl Default for GlobalAppearance {
    fn default() -> Self {
        Self::identity()
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
    pub const fn identity() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    fn mul(&self, local: &Transform) -> Self {
        Self {
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
        Appearance::identity(),
        GlobalAppearance::identity(),
        Children(Vec::new()),
        NameIndex::default(),
    ))
}

/// Spawn a new node under `parent`.
pub fn spawn_node(world: &mut World, name: &str, parent: Entity) -> Entity {
    let entity = world.spawn((
        Name(name.to_string()),
        Transform::identity(),
        GlobalTransform::identity(),
        Appearance::identity(),
        GlobalAppearance::identity(),
        Children(Vec::new()),
        NameIndex::default(),
        Parent(parent),
    ));
    attach(world, parent, name, entity);
    entity
}

/// [`spawn_node`] with a stable id, in one spawn.
///
/// Adding the id afterwards moved the entity to another archetype, and that
/// move was the single hottest function of a script adding children.
pub fn spawn_node_with_id(world: &mut World, name: &str, parent: Entity, id: String) -> Entity {
    let entity = world.spawn((
        Name(name.to_string()),
        Transform::identity(),
        GlobalTransform::identity(),
        Appearance::identity(),
        GlobalAppearance::identity(),
        Children(Vec::new()),
        NameIndex::default(),
        Parent(parent),
        crate::components::StableId(id),
    ));
    attach(world, parent, name, entity);
    entity
}

/// Spawn a node as `parent`'s child number `index`, clamped to the end.
///
/// Where a snapshot puts a freed node back: the digest walks the tree in
/// order, so a node restored as the last sibling reads as a divergence made
/// of nothing but ordering.
pub fn spawn_node_at(world: &mut World, name: &str, parent: Entity, index: usize) -> Entity {
    let entity = spawn_node(world, name, parent);
    move_child_to(world, parent, entity, index);
    entity
}

/// Move `entity` to `index` among `parent`'s children, clamped to the end.
///
/// What makes a snapshot restore reproduce the tree order the digest walks
/// rather than only the set.
pub fn move_child_to(world: &World, parent: Entity, entity: Entity, index: usize) {
    {
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
    // Among siblings sharing its name, the earliest may now be another; a
    // unique name stays on its one holder wherever it moves.
    if let Ok(name) = world.get::<&Name>(entity)
        && world
            .get::<&NameIndex>(parent)
            .is_ok_and(|index| index.0.get(&name.0).is_some_and(|slot| slot.count > 1))
    {
        reindex(world, parent, &name.0);
    }
}

/// Rename a node, keeping its parent's [`NameIndex`] right.
pub fn rename(world: &World, entity: Entity, name: &str) {
    let old = {
        let Ok(mut current) = world.get::<&mut Name>(entity) else {
            return;
        };
        if current.0 == name {
            return;
        }
        std::mem::replace(&mut current.0, name.to_string())
    };
    let Ok(parent) = world.get::<&Parent>(entity).map(|p| p.0) else {
        return;
    };
    unindex(world, parent, &old, entity);
    index_in_place(world, parent, name, entity);
}

/// Append `child`, named `name`, to `parent`'s children and index it.
fn attach(world: &World, parent: Entity, name: &str, child: Entity) {
    if let Ok(mut children) = world.get::<&mut Children>(parent) {
        children.0.push(child);
    }
    if let Ok(mut index) = world.get::<&mut NameIndex>(parent) {
        if let Some(slot) = index.0.get_mut(name) {
            slot.count += 1;
        } else {
            index.0.insert(
                name.to_string(),
                NameSlot {
                    first: child,
                    count: 1,
                },
            );
        }
    }
}

/// Take `child` out of `parent`'s children and its name out of the index.
fn detach(world: &World, parent: Entity, child: Entity) {
    if let Ok(mut children) = world.get::<&mut Children>(parent) {
        children.0.retain(|&c| c != child);
    }
    if let Ok(name) = world.get::<&Name>(child) {
        unindex(world, parent, &name.0, child);
    }
}

/// Count `child` as a holder of `name` under `parent` wherever it sits in
/// the order, unlike [`attach`], whose child is always last.
fn index_in_place(world: &World, parent: Entity, name: &str, child: Entity) {
    let shared = {
        let Ok(mut index) = world.get::<&mut NameIndex>(parent) else {
            return;
        };
        if let Some(slot) = index.0.get_mut(name) {
            slot.count += 1;
            true
        } else {
            index.0.insert(
                name.to_string(),
                NameSlot {
                    first: child,
                    count: 1,
                },
            );
            false
        }
    };
    if shared {
        reindex(world, parent, name);
    }
}

/// Drop one holder of `name` from `parent`'s index; when it was the earliest
/// of several, the next in order takes its place. The child must already be
/// out of the parent's list.
fn unindex(world: &World, parent: Entity, name: &str, child: Entity) {
    let lost_first = {
        let Ok(mut index) = world.get::<&mut NameIndex>(parent) else {
            return;
        };
        let Some(slot) = index.0.get_mut(name) else {
            return;
        };
        slot.count -= 1;
        if slot.count == 0 {
            index.0.swap_remove(name);
            return;
        }
        slot.first == child
    };
    if lost_first {
        reindex(world, parent, name);
    }
}

/// Point `name`'s slot under `parent` at the earliest child bearing it.
fn reindex(world: &World, parent: Entity, name: &str) {
    let Ok(mut index) = world.get::<&mut NameIndex>(parent) else {
        return;
    };
    let Some(slot) = index.0.get_mut(name) else {
        return;
    };
    if let Some(first) = child_named(world, parent, name) {
        slot.first = first;
    }
}

/// The earliest of `parent`'s children named `name`, by scanning them.
fn child_named(world: &World, parent: Entity, name: &str) -> Option<Entity> {
    let children = world.get::<&Children>(parent).ok()?;
    children
        .0
        .iter()
        .copied()
        .find(|&c| world.get::<&Name>(c).is_ok_and(|n| n.0 == name))
}

/// Resolve a `A/B/C` path relative to `from` by matching child names; a
/// `..` segment climbs to the parent, as a Godot NodePath does.
pub fn find_node(world: &World, from: Entity, path: &str) -> Option<Entity> {
    let mut current = from;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if segment == ".." {
            current = world.get::<&Parent>(current).ok()?.0;
            continue;
        }
        current = match world.get::<&NameIndex>(current) {
            Ok(index) => {
                let found = index.0.get(segment)?.first;
                debug_assert!(
                    world.get::<&Name>(found).is_ok_and(|n| n.0 == segment)
                        && world.get::<&Parent>(found).is_ok_and(|p| p.0 == current),
                    "the name index of {current:?} is stale for {segment:?}"
                );
                found
            }
            Err(_) => child_named(world, current, segment)?,
        };
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

/// Recompute every `GlobalTransform` and `GlobalAppearance` from the root down.
pub fn propagate_transforms(world: &mut World, root: Entity) {
    let identity = GlobalTransform::identity();
    let visible = GlobalAppearance::identity();
    propagate_recursive(world, root, &identity, visible);
}

fn propagate_recursive(
    world: &mut World,
    entity: Entity,
    parent_global: &GlobalTransform,
    parent_appearance: GlobalAppearance,
) {
    let global = match world.get::<&Transform>(entity) {
        Ok(local) => parent_global.mul(&local),
        Err(_) => *parent_global,
    };
    if let Ok(mut slot) = world.get::<&mut GlobalTransform>(entity) {
        *slot = global;
    }
    let appearance = match world.get::<&Appearance>(entity) {
        Ok(local) => parent_appearance.mul(*local),
        Err(_) => parent_appearance,
    };
    if let Ok(mut slot) = world.get::<&mut GlobalAppearance>(entity) {
        *slot = appearance;
    }
    let children: Vec<Entity> = match world.get::<&Children>(entity) {
        Ok(children) => children.0.clone(),
        Err(_) => return,
    };
    for child in children {
        propagate_recursive(world, child, &global, appearance);
    }
}

/// A node's world appearance composed from local ones, current this instant
/// rather than as of the last scene sync.
#[must_use]
pub fn composed_appearance(world: &World, entity: Entity) -> GlobalAppearance {
    let mut chain = vec![entity];
    let mut current = entity;
    while let Ok(parent) = world.get::<&Parent>(current) {
        current = parent.0;
        chain.push(current);
    }
    let mut appearance = GlobalAppearance::identity();
    for e in chain.into_iter().rev() {
        if let Ok(local) = world.get::<&Appearance>(e) {
            appearance = appearance.mul(*local);
        }
    }
    appearance
}

/// A node's world transform composed from local ones, current this instant
/// rather than as of the last scene sync.
#[must_use]
pub fn composed_global(world: &World, entity: Entity) -> GlobalTransform {
    let mut chain = vec![entity];
    let mut current = entity;
    while let Ok(parent) = world.get::<&Parent>(current) {
        current = parent.0;
        chain.push(current);
    }
    let mut global = GlobalTransform::identity();
    for e in chain.into_iter().rev() {
        if let Ok(local) = world.get::<&Transform>(e) {
            global = global.mul(&local);
        }
    }
    global
}

/// Move `entity` under `new_parent`, keeping where it is in the world: the
/// local transform is rewritten so nothing on screen moves. Refused when
/// the new parent is the node itself or one of its descendants.
///
/// # Errors
/// If either node is dead, or the move would make a cycle.
pub fn reparent(world: &mut World, entity: Entity, new_parent: Entity) -> anyhow::Result<()> {
    if entity == new_parent || collect_subtree(world, entity).contains(&new_parent) {
        anyhow::bail!("a node cannot be moved under itself");
    }
    if !world.contains(new_parent) {
        anyhow::bail!("the new parent is dead");
    }
    let child_global = composed_global(world, entity);
    let parent_global = composed_global(world, new_parent);
    let safe = |s: f32| if s.abs() > f32::EPSILON { s } else { 1.0 };
    let inverse_rotation = parent_global.rotation.inverse();
    let offset = inverse_rotation * (child_global.position - parent_global.position);
    let local = Transform {
        position: Vec3::new(
            offset.x / safe(parent_global.scale.x),
            offset.y / safe(parent_global.scale.y),
            offset.z / safe(parent_global.scale.z),
        ),
        rotation: inverse_rotation * child_global.rotation,
        scale: Vec3::new(
            child_global.scale.x / safe(parent_global.scale.x),
            child_global.scale.y / safe(parent_global.scale.y),
            child_global.scale.z / safe(parent_global.scale.z),
        ),
    };
    if let Ok(old_parent) = world.get::<&Parent>(entity).map(|p| p.0) {
        detach(world, old_parent, entity);
    }
    let name = world
        .get::<&Name>(entity)
        .map(|n| n.0.clone())
        .unwrap_or_default();
    attach(world, new_parent, &name, entity);
    world
        .insert(entity, (Parent(new_parent), local))
        .map_err(|_| anyhow::anyhow!("node is dead"))?;
    Ok(())
}

/// Collect a subtree in despawn order (children before parents is not
/// required by hecs, but callers also use this to tear down script
/// instances and plugin state).
pub fn collect_subtree(world: &World, entity: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    collect_subtree_into(world, entity, &mut out);
    out
}

/// [`collect_subtree`] appending to `out`, for a caller with many to gather;
/// a leaf costs no allocation.
pub fn collect_subtree_into(world: &World, entity: Entity, out: &mut Vec<Entity>) {
    out.push(entity);
    let mut stack: Vec<Entity> = match world.get::<&Children>(entity) {
        Ok(children) if !children.0.is_empty() => children.0.clone(),
        _ => return,
    };
    while let Some(e) = stack.pop() {
        out.push(e);
        if let Ok(children) = world.get::<&Children>(e) {
            stack.extend(children.0.iter().copied());
        }
    }
}

/// Whether `entity` is `root` or somewhere below it.
pub fn is_within(world: &World, entity: Entity, root: Entity) -> bool {
    let mut current = entity;
    loop {
        if current == root {
            return true;
        }
        match world.get::<&Parent>(current).ok().map(|p| p.0) {
            Some(parent) => current = parent,
            None => return false,
        }
    }
}

/// Free a node the way a running engine must: detach every script instance
/// under it, run every component's `remove` hook, then despawn.
///
/// [`free_subtree`] is the raw half and leaves plugin state behind — physics
/// keys its bodies, colliders and joints by entity and learns of a
/// destruction from nowhere else, so a stale handle answers raycasts and then
/// panics. Every path that destroys a node at run time goes through here.
pub fn free_node(eng: &Engine, entity: Entity) {
    let subtree = collect_subtree(&eng.world(), entity);
    if let Some(host) = eng.script_host() {
        for &e in &subtree {
            host.detach(crate::node_id_of(e));
        }
    }
    for &e in &subtree {
        crate::components::remove_present(eng, e);
    }
    free_subtree(&mut eng.world_mut(), entity);
}

/// [`free_node`] for many nodes at once, which is what a frame's queued frees
/// are.
///
/// One pass over each parent's children rather than one per freed node:
/// fifty thousand siblings freed one at a time is fifty thousand scans of
/// a fifty-thousand-entry list, and a frame that frees a whole container of
/// them is ordinary.
pub fn free_nodes(eng: &Engine, entities: &[Entity]) {
    let mut subtree = Vec::with_capacity(entities.len());
    {
        let world = eng.world();
        for &entity in entities {
            collect_subtree_into(&world, entity, &mut subtree);
        }
    }
    if let Some(host) = eng.script_host() {
        for &e in &subtree {
            host.detach(crate::node_id_of(e));
        }
    }
    for &e in &subtree {
        crate::components::remove_present(eng, e);
    }
    let mut world = eng.world_mut();
    let doomed: crate::collections::DetHashSet<Entity> = entities.iter().copied().collect();
    let mut parents: Vec<Entity> = entities
        .iter()
        .filter_map(|&e| world.get::<&Parent>(e).ok().map(|p| p.0))
        .collect();
    parents.sort_unstable_by_key(|e| e.to_bits());
    parents.dedup();
    for parent in parents {
        let (emptied, gone) = {
            let Ok(mut children) = world.get::<&mut Children>(parent) else {
                continue;
            };
            let gone: Vec<Entity> = children
                .0
                .iter()
                .copied()
                .filter(|c| doomed.contains(c))
                .collect();
            children.0.retain(|c| !doomed.contains(c));
            (children.0.is_empty(), gone)
        };
        // A parent losing every child drops its index whole rather than
        // one name at a time.
        if emptied {
            if let Ok(mut index) = world.get::<&mut NameIndex>(parent) {
                index.0.clear();
            }
            continue;
        }
        for child in gone {
            if let Ok(name) = world.get::<&Name>(child) {
                unindex(&world, parent, &name.0, child);
            }
        }
    }
    for e in subtree {
        let _ = world.despawn(e);
    }
}

/// Despawn a node and its whole subtree, unlinking it from its parent.
pub fn free_subtree(world: &mut World, entity: Entity) {
    if let Ok(parent) = world.get::<&Parent>(entity).map(|p| p.0) {
        detach(world, parent, entity);
    }
    for e in collect_subtree(world, entity) {
        let _ = world.despawn(e);
    }
}
