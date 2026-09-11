//! Godot-like scene tree layered on top of the ECS.
//!
//! A "node" is nothing more than an entity carrying the tree components
//! below. Plugins attach their own components to the same entity, so the
//! node abstraction costs nothing on the data plane.

use std::cell::RefCell;

use glamx::{Quat, Vec3, Vec4};
use hecs::{Entity, World};
use smol_str::SmolStr;

use crate::engine::Engine;

pub struct Name(pub String);
pub struct Parent(pub Entity);
pub struct Children(pub Vec<Entity>);

/// Bumped whenever the tree's shape changes: a node attached, detached, moved
/// among its siblings, or renamed.
///
/// Presentation reads it to skip rebuilding what it walked last frame.
/// Nothing in the simulation branches on it, so it is outside the digest —
/// and being process-wide, a second engine in the same test only makes the
/// number move more often, which costs a rebuild and never a wrong picture.
static SHAPE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What the tree's shape is at now. A reader that sees the same number twice
/// saw no node added, freed, moved or renamed in between.
#[must_use]
pub fn shape_revision() -> u64 {
    SHAPE.load(std::sync::atomic::Ordering::Relaxed)
}

fn shape_changed() {
    SHAPE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Bumped whenever a node's world visibility or tint changes, so a layer that
/// caches what it drew knows when the tree hid, showed or faded something.
/// Outside the digest for the same reason [`SHAPE`] is.
static APPEARANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What the tree's appearance is at now. The same number twice means no node
/// was hidden, shown or re-tinted in between.
#[must_use]
pub fn appearance_revision() -> u64 {
    APPEARANCE.load(std::sync::atomic::Ordering::Relaxed)
}

/// A parent's children by name, so a path segment is one lookup rather than
/// a scan of every sibling.
///
/// Kept by the functions in this module that add, remove, move or rename a
/// child; a parent without one (a world built by hand in a test) is scanned.
///
/// Keyed on an inline string: a name of 23 bytes or fewer sits in the entry
/// itself, so a probe compares bytes where it stands rather than following a
/// pointer into the heap once per segment of a path.
#[derive(Default)]
pub struct NameIndex(pub crate::collections::DetHashMap<SmolStr, NameSlot>);

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
    /// A 2D shear: how far the y axis is turned past square with the x axis,
    /// in radians, before the scale. Zero on everything that is not a 2D node
    /// asking for one, and the composition takes the plain path when it is.
    pub skew: f32,
}

impl Transform {
    pub const fn identity() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            skew: 0.0,
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

/// A `material` asset reference, interned so [`Appearance`] stays `Copy`.
///
/// Ids are handed out per process, in the order references are first seen,
/// so anything saved or hashed stores [`MaterialId::reference`] instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MaterialId(u32);

/// Every reference interned so far; index 0 is the empty one.
static MATERIALS: std::sync::LazyLock<std::sync::RwLock<MaterialTable>> =
    std::sync::LazyLock::new(|| {
        std::sync::RwLock::new(MaterialTable {
            references: vec![std::sync::Arc::from("")],
            ids: std::collections::HashMap::new(),
        })
    });

struct MaterialTable {
    references: Vec<std::sync::Arc<str>>,
    ids: std::collections::HashMap<std::sync::Arc<str>, u32>,
}

impl MaterialId {
    /// No material: the node takes its parent's.
    pub const NONE: Self = Self(0);

    /// The id for `reference`, the same one every time; empty is [`Self::NONE`].
    #[must_use]
    pub fn intern(reference: &str) -> Self {
        if reference.is_empty() {
            return Self::NONE;
        }
        if let Some(&id) = MATERIALS
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ids
            .get(reference)
        {
            return Self(id);
        }
        let mut table = MATERIALS
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(&id) = table.ids.get(reference) {
            return Self(id);
        }
        let id = u32::try_from(table.references.len()).expect("fewer than 2^32 materials");
        let shared: std::sync::Arc<str> = std::sync::Arc::from(reference);
        table.references.push(shared.clone());
        table.ids.insert(shared, id);
        Self(id)
    }

    /// The reference this id was interned from; empty for [`Self::NONE`].
    #[must_use]
    pub fn reference(self) -> std::sync::Arc<str> {
        MATERIALS
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .references[self.0 as usize]
            .clone()
    }

    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// Whether a node draws, what it is tinted by, which layer it draws on, and
/// the material it and its subtree draw with.
///
/// Nothing in physics reads any field: a hidden collider still collides,
/// which is what a game hiding a sprite for a frame expects.
#[derive(Clone, Copy)]
pub struct Appearance {
    pub visible: bool,
    /// A colour multiplied into whatever the node draws, and into every
    /// descendant's, so one key fades a whole rig. A renderable's own `color`
    /// is the node's alone; this is the one that inherits.
    pub tint: Vec4,
    pub z_index: i32,
    /// Add `z_index` to the parent's rather than replacing it, so moving a
    /// subtree between layers keeps the order inside it.
    pub z_relative: bool,
    /// The material this node and every descendant naming none draw with.
    /// [`MaterialId::NONE`] takes the parent's.
    pub material: MaterialId,
}

impl Appearance {
    pub const fn identity() -> Self {
        Self {
            visible: true,
            tint: Vec4::ONE,
            z_index: 0,
            z_relative: true,
            material: MaterialId::NONE,
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
    /// Every `Appearance::tint` from the root down, multiplied channel by
    /// channel. What a renderer multiplies its own colour by.
    pub tint: Vec4,
    pub z_index: i32,
    /// The nearest material from this node up; what a renderer draws with.
    pub material: MaterialId,
}

impl GlobalAppearance {
    pub const fn identity() -> Self {
        Self {
            visible: true,
            tint: Vec4::ONE,
            z_index: 0,
            material: MaterialId::NONE,
        }
    }

    fn mul(self, local: Appearance) -> Self {
        Self {
            visible: self.visible && local.visible,
            tint: self.tint * local.tint,
            z_index: if local.z_relative {
                self.z_index.saturating_add(local.z_index)
            } else {
                local.z_index
            },
            material: if local.material.is_none() {
                self.material
            } else {
                local.material
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
    /// The world shear, in the same terms as [`Transform::skew`]: every 2D
    /// basis is exactly a rotation, a shear and a scale, so a skewed parent's
    /// children are placed exactly rather than approximately.
    pub skew: f32,
}

impl GlobalTransform {
    pub const fn identity() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            skew: 0.0,
        }
    }

    /// The 2D affine matrix this pose places with: rotation, shear, scale
    /// and translation, in that order, the way Godot's `Transform2D` does.
    #[must_use]
    pub fn affine_2d(&self) -> glamx::Mat3 {
        let basis = linear_2d(
            z_angle(self.rotation),
            self.skew,
            self.scale.x,
            self.scale.y,
        );
        glamx::Mat3::from_cols(
            basis.x_axis.extend(0.0),
            basis.y_axis.extend(0.0),
            Vec3::new(self.position.x, self.position.y, 1.0),
        )
    }

    fn mul(&self, local: &Transform) -> Self {
        let flat = |q: Quat| q.x == 0.0 && q.y == 0.0;
        if (self.skew == 0.0 && local.skew == 0.0) || !flat(self.rotation) || !flat(local.rotation)
        {
            return Self {
                position: self.position + self.rotation * (local.position * self.scale),
                rotation: self.rotation * local.rotation,
                scale: self.scale * local.scale,
                skew: 0.0,
            };
        }
        // A shear anywhere above makes the basis a general 2D matrix, so it
        // is composed as one and taken back apart.
        let parent = linear_2d(
            z_angle(self.rotation),
            self.skew,
            self.scale.x,
            self.scale.y,
        );
        let own = linear_2d(
            z_angle(local.rotation),
            local.skew,
            local.scale.x,
            local.scale.y,
        );
        let at = self.position.truncate() + parent * local.position.truncate();
        let (angle, skew, sx, sy) = decompose_2d(parent * own);
        Self {
            position: Vec3::new(
                at.x,
                at.y,
                self.position.z + local.position.z * self.scale.z,
            ),
            rotation: Quat::from_rotation_z(angle),
            scale: Vec3::new(sx, sy, self.scale.z * local.scale.z),
            skew,
        }
    }
}

/// The angle a rotation about z turns by. Only asked of one that is.
fn z_angle(rotation: Quat) -> f32 {
    2.0 * libm::atan2f(rotation.z, rotation.w)
}

/// Rotation, then a shear turning the y axis `skew` further, then scale.
/// Every 2D linear map is exactly one of these.
fn linear_2d(angle: f32, skew: f32, sx: f32, sy: f32) -> glamx::Mat2 {
    let (x_sin, x_cos) = (libm::sinf(angle), libm::cosf(angle));
    let (y_sin, y_cos) = (libm::sinf(angle + skew), libm::cosf(angle + skew));
    glamx::Mat2::from_cols(
        glamx::Vec2::new(x_cos * sx, x_sin * sx),
        glamx::Vec2::new(-y_sin * sy, y_cos * sy),
    )
}

/// [`linear_2d`] taken apart: the rotation is the x axis's direction, and
/// the y axis read in that frame gives the shear and its scale, negative for
/// a mirror.
fn decompose_2d(m: glamx::Mat2) -> (f32, f32, f32, f32) {
    let angle = libm::atan2f(m.x_axis.y, m.x_axis.x);
    let sx = libm::hypotf(m.x_axis.x, m.x_axis.y);
    let (sin, cos) = (libm::sinf(-angle), libm::cosf(-angle));
    let across = cos * m.y_axis.x - sin * m.y_axis.y;
    let up = sin * m.y_axis.x + cos * m.y_axis.y;
    let sy = libm::hypotf(across, up).copysign(up);
    if sy == 0.0 {
        return (angle, 0.0, sx, 0.0);
    }
    (angle, libm::atan2f(-across / sy, up / sy), sx, sy)
}

/// The components every node has, whatever else it carries, plus whatever the
/// caller adds after them.
///
/// One tuple rather than a spawn and then an insert: adding a component
/// afterwards moves the entity to another archetype, and that move was the
/// single hottest function of a script adding children. A local `Transform` is
/// one of the extras rather than one of these, because a node may have none;
/// its `GlobalTransform` is here either way, so a bare node still has a world
/// position and every reader of one is untouched.
macro_rules! node_bundle {
    ($name:expr $(, $extra:expr)* $(,)?) => {
        (
            Name($name.to_string()),
            GlobalTransform::identity(),
            Appearance::identity(),
            GlobalAppearance::identity(),
            Children(Vec::new()),
            NameIndex::default(),
            $($extra,)*
        )
    };
}

pub(crate) fn spawn_root(world: &mut World) -> Entity {
    world.spawn(node_bundle!("Root", Transform::identity()))
}

/// Spawn a new node under `parent`.
pub fn spawn_node(world: &mut World, name: &str, parent: Entity) -> Entity {
    let entity = world.spawn(node_bundle!(name, Parent(parent), Transform::identity()));
    attach(world, parent, name, entity);
    entity
}

/// [`spawn_node`] without a local `Transform`, for a node that only groups or
/// only draws.
///
/// What a scene file naming no `[nodes.transform]` gets: the component is
/// absent rather than at its defaults, the same way an absent `[nodes.sprite]`
/// means no sprite. Spawning it bare rather than removing one afterwards is
/// what keeps the archetype move out of loading a scene.
pub fn spawn_node_bare(world: &mut World, name: &str, parent: Entity) -> Entity {
    let entity = world.spawn(node_bundle!(name, Parent(parent)));
    attach(world, parent, name, entity);
    entity
}

/// [`spawn_node`] with a stable id, in one spawn.
pub fn spawn_node_with_id(world: &mut World, name: &str, parent: Entity, id: String) -> Entity {
    let entity = world.spawn(node_bundle!(
        name,
        Parent(parent),
        Transform::identity(),
        crate::components::StableId(id)
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
    shape_changed();
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
        && world.get::<&NameIndex>(parent).is_ok_and(|index| {
            index
                .0
                .get(name.0.as_str())
                .is_some_and(|slot| slot.count > 1)
        })
    {
        reindex(world, parent, &name.0);
    }
}

/// Rename a node, keeping its parent's [`NameIndex`] right.
pub fn rename(world: &World, entity: Entity, name: &str) {
    shape_changed();
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
    shape_changed();
    if let Ok(mut children) = world.get::<&mut Children>(parent) {
        children.0.push(child);
    }
    if let Ok(mut index) = world.get::<&mut NameIndex>(parent) {
        if let Some(slot) = index.0.get_mut(name) {
            slot.count += 1;
        } else {
            index.0.insert(
                SmolStr::new(name),
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
    shape_changed();
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
                SmolStr::new(name),
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

/// Resolve a `A/B/C` path relative to `from` by matching child names; `.`
/// is the node itself and `..` climbs to the parent, as a Godot NodePath does.
pub fn find_node(world: &World, from: Entity, path: &str) -> Option<Entity> {
    let mut current = from;
    for segment in path.split('/').filter(|s| !s.is_empty() && *s != ".") {
        if segment == ".." {
            current = world.get::<&Parent>(current).ok()?.0;
            continue;
        }
        current = named_child(world, current, segment)?;
    }
    Some(current)
}

fn named_child(world: &World, parent: Entity, name: &str) -> Option<Entity> {
    match world.get::<&NameIndex>(parent) {
        Ok(index) => {
            let found = index.0.get(name)?.first;
            debug_assert!(
                world.get::<&Name>(found).is_ok_and(|n| n.0 == name)
                    && world.get::<&Parent>(found).is_ok_and(|p| p.0 == parent),
                "the name index of {parent:?} is stale for {name:?}"
            );
            Some(found)
        }
        Err(_) => child_named(world, parent, name),
    }
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

thread_local! {
    /// The frontier [`propagate_transforms`] walks, kept between frames: the
    /// pass runs every frame over every node, and a fresh stack per node was
    /// an allocation per node.
    static PROPAGATE_STACK: RefCell<Vec<(Entity, GlobalTransform, GlobalAppearance)>> =
        const { RefCell::new(Vec::new()) };
}

/// Recompute every `GlobalTransform` and `GlobalAppearance` from the root down.
pub fn propagate_transforms(world: &mut World, root: Entity) {
    PROPAGATE_STACK.with_borrow_mut(|stack| {
        stack.clear();
        stack.push((
            root,
            GlobalTransform::identity(),
            GlobalAppearance::identity(),
        ));
        while let Some((entity, parent_global, parent_appearance)) = stack.pop() {
            let global = match world.get::<&Transform>(entity) {
                Ok(local) => parent_global.mul(&local),
                Err(_) => parent_global,
            };
            if let Ok(mut slot) = world.get::<&mut GlobalTransform>(entity) {
                *slot = global;
            }
            let appearance = match world.get::<&Appearance>(entity) {
                Ok(local) => parent_appearance.mul(*local),
                Err(_) => parent_appearance,
            };
            if let Ok(mut slot) = world.get::<&mut GlobalAppearance>(entity) {
                if slot.visible != appearance.visible || slot.tint != appearance.tint {
                    APPEARANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                *slot = appearance;
            }
            if let Ok(children) = world.get::<&Children>(entity) {
                // Reversed, so popping visits siblings in the order they are
                // listed. A subtree's answer does not depend on it; a log does.
                stack.extend(children.0.iter().rev().map(|&c| (c, global, appearance)));
            }
        }
    });
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
        skew: 0.0,
    };
    // A shear on either side is only undone exactly through the matrices.
    let local = if parent_global.skew != 0.0 || child_global.skew != 0.0 {
        sheared_local(&parent_global, &child_global, local.position.z)
    } else {
        local
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

/// The local transform that puts a node at `child` under `parent`, when a
/// shear is in either and the plain division would drop it.
fn sheared_local(parent: &GlobalTransform, child: &GlobalTransform, z: f32) -> Transform {
    let local = parent.affine_2d().inverse() * child.affine_2d();
    let basis = glamx::Mat2::from_cols(local.x_axis.truncate(), local.y_axis.truncate());
    let (angle, skew, sx, sy) = decompose_2d(basis);
    let scale_z = if parent.scale.z.abs() > f32::EPSILON {
        child.scale.z / parent.scale.z
    } else {
        1.0
    };
    Transform {
        position: Vec3::new(local.z_axis.x, local.z_axis.y, z),
        rotation: Quat::from_rotation_z(angle),
        scale: Vec3::new(sx, sy, scale_z),
        skew,
    }
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
    // Said here rather than left to `detach`: this unlinks the children by
    // rewriting each parent's list itself, so `detach` never runs.
    shape_changed();
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
