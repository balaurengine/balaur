//! Rig modifiers: bones posed again after the clip has had its say.
//!
//! Godot's `SkeletonModification2D` and `SkeletonModification3D`, as two
//! components over one set of solvers. `look_at` turns a bone so its child
//! points at a node; `two_bone_ik` bends a root, middle, tip chain so the tip
//! reaches one; `fabrik` and `ccdik` reach with a chain of any length; and
//! `jiggle` lets a chain lag behind the pose on a spring. All five run in
//! `Stage::Update` after the animation system, so a clip poses the rig and a
//! modifier has the last word, every frame, from the transforms as they are
//! now (composed from locals, never last frame's globals).
//!
//! `jiggle` is the one kind with memory. It steps on the same 1/60 tick the
//! playhead does rather than on the frame's `dt`, and its points ride the
//! animation snapshot, so a rollback puts them back where they were.
//!
//! The solvers are dimension-agnostic where they can be: FABRIK and the
//! spring move points in `Vec3` and 2D passes them with `z = 0`, so there is
//! one reaching algorithm rather than two that drift apart. What does differ
//! per dimension is how a solved point is turned back into a bone's rotation
//! — an angle about z in 2D, the shortest arc in 3D — and that is the only
//! thing written twice.
//!
//! Every transcendental is `libm`'s, so the pose is the same on every
//! platform.

use crate::keys as k;
use crate::player::{AnimationState, FIXED_DT, MAX_SUBSTEPS};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, as_f64};
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{self, Children, Parent, Transform};
use balaur_core::skeleton::{Bone, affine_2d, affine_3d, quat_about_z, quat_from_euler};
use balaur_plugin::Registry;
use glamx::{Mat3, Mat4, Quat, Vec2, Vec3};

/// The shortest bone [`two_bone_ik`] will solve. Below it the reach clamp
/// inverts and there is no elbow angle to find anyway.
const MIN_BONE: f32 = 1e-5;

/// How far down a chain a modifier will walk when `chain` is left at zero.
/// A cycle cannot happen in a scene tree, but a rig deep enough to matter
/// here is already past what a solver converges on.
const MAX_CHAIN: usize = 64;

/// The five modifiers, written once for the schema, the matcher and the
/// read-back.
const LOOK_AT: &str = "look_at";
const TWO_BONE_IK: &str = "two_bone_ik";
const FABRIK: &str = "fabrik";
const CCDIK: &str = "ccdik";
const JIGGLE: &str = "jiggle";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    LookAt,
    TwoBoneIk,
    Fabrik,
    Ccdik,
    Jiggle,
}

impl Kind {
    fn parse(text: Option<&str>) -> Result<Self> {
        match text {
            None | Some(LOOK_AT) => Ok(Self::LookAt),
            Some(TWO_BONE_IK) => Ok(Self::TwoBoneIk),
            Some(FABRIK) => Ok(Self::Fabrik),
            Some(CCDIK) => Ok(Self::Ccdik),
            Some(JIGGLE) => Ok(Self::Jiggle),
            Some(other) => Err(anyhow!("unknown modifier kind '{other}'")),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::LookAt => LOOK_AT,
            Self::TwoBoneIk => TWO_BONE_IK,
            Self::Fabrik => FABRIK,
            Self::Ccdik => CCDIK,
            Self::Jiggle => JIGGLE,
        }
    }

    /// Whether the kind reaches for a target node. `jiggle` is the one that
    /// does not: it follows the pose it was given.
    const fn wants_target(self) -> bool {
        !matches!(self, Self::Jiggle)
    }
}

/// What a `modifier2d` or `modifier3d` component wrote on the node. One
/// struct for both: the keys are the same and only the solver differs.
#[derive(Clone, Debug)]
pub struct Params {
    kind: Kind,
    /// Node path to the point to aim at, relative to the node.
    target: String,
    /// Node path to the driven bone, relative to the node; empty is the node.
    bone: String,
    /// How many bones the chain holds, counting the driven one. Zero walks
    /// to the deepest tip.
    chain: usize,
    /// Solver passes for `fabrik` and `ccdik`.
    iterations: u32,
    /// How close to the target ends a `fabrik` or `ccdik` solve early.
    tolerance: f32,
    /// How far a `ccdik` bone may turn from its rest, radians. Zero is free.
    angle_limit: f32,
    /// How hard a `jiggle` bone is pulled back to the pose.
    stiffness: f32,
    /// How much of a `jiggle` bone's speed survives a tick, `0..1`.
    damping: f32,
    /// What `gravity` weighs against `stiffness` on a `jiggle` bone.
    mass: f32,
    gravity: Vec3,
    use_gravity: bool,
    flip: bool,
    enabled: bool,
}

/// A 2D rig modifier, over `bone2d`.
///
/// Shared rather than owned: the system reads every modifier's params once a
/// frame, and the paths in them are strings nobody should be copying sixty
/// times a second.
#[derive(Clone, Debug)]
pub struct Modifier2d(std::sync::Arc<Params>);

/// A 3D rig modifier, over `bone3d`.
#[derive(Clone, Debug)]
pub struct Modifier3d(std::sync::Arc<Params>);

/// A jiggle chain's dynamic points and their speeds, one per solved bone,
/// and the two rotations that keep the spring from chasing itself.
///
/// Kept out of the component so re-applying the component — which the editor
/// does on every inspector edit — does not throw the motion away.
#[derive(Clone, Debug, Default)]
pub struct Jiggle {
    pub(crate) points: Vec<Vec3>,
    pub(crate) velocities: Vec<Vec3>,
    /// The pose the clip left, per bone. A spring pulls toward *this*, never
    /// toward where the spring itself put the bone last tick — that pair
    /// agree at any angle, including upside down, and the chain would stay
    /// wherever it was first flung.
    pub(crate) incoming: Vec<Quat>,
    /// What this modifier wrote last tick. A bone still holding it was not
    /// touched by a clip since, so `incoming` still stands; a bone holding
    /// anything else has been posed and `incoming` is replaced.
    pub(crate) written: Vec<Quat>,
}

fn schema() -> String {
    let kinds = ComponentDef::options(&[LOOK_AT, TWO_BONE_IK, FABRIK, CCDIK, JIGGLE]);
    // Down, at about two thirds of earth's: a chain that hangs rather than
    // drops. A 2D rig reads the third number as nothing, so both dimensions
    // take the same one.
    let gravity = "[0.0, -6.0, 0.0]";
    ComponentDef::schema(&[
        (
            k::KIND,
            &format!(
                r#"{{ type = "enum", default = "{LOOK_AT}", options = [{kinds}], description = "Aim one bone at the target, bend a two-bone chain to it, reach with a chain of any length ({FABRIK} or {CCDIK}), or let a chain lag behind the pose ({JIGGLE})" }}"#
            ),
        ),
        (
            k::TARGET,
            r#"{ type = "string", default = "", description = "Node path to the point to aim at, relative to this node. Unused by jiggle" }"#,
        ),
        (
            k::BONE,
            r#"{ type = "string", default = "", description = "Node path to the driven bone, relative to this node; empty means this node. For a chain solver, its root" }"#,
        ),
        (
            k::CHAIN,
            r#"{ type = "int", default = 0, description = "How many bones the chain holds, counting the driven one; 0 walks to the deepest tip" }"#,
        ),
        (
            k::ITERATIONS,
            r#"{ type = "int", default = 10, description = "Solver passes for fabrik and ccdik" }"#,
        ),
        (
            k::TOLERANCE,
            r#"{ type = "float", default = 0.01, description = "How close to the target ends a fabrik or ccdik solve early" }"#,
        ),
        (
            k::ANGLE_LIMIT,
            r#"{ type = "float", default = 0.0, description = "How far a ccdik bone may turn from its rest, in radians; 0 leaves it free" }"#,
        ),
        (
            k::STIFFNESS,
            r#"{ type = "float", default = 3.0, description = "How hard a jiggle bone is pulled back to the pose" }"#,
        ),
        (
            k::DAMPING,
            r#"{ type = "float", default = 0.75, description = "How much of a jiggle bone's speed survives a tick, 0 to 1" }"#,
        ),
        (
            k::MASS,
            r#"{ type = "float", default = 0.75, description = "What gravity weighs against stiffness on a jiggle bone" }"#,
        ),
        (
            k::GRAVITY,
            &format!(
                r#"{{ type = "vec3", default = {gravity}, description = "Pull on a jiggle bone while `use_gravity` is on" }}"#
            ),
        ),
        (
            k::USE_GRAVITY,
            r#"{ type = "bool", default = false, description = "Whether a jiggle chain is pulled by `gravity`" }"#,
        ),
        (
            k::FLIP,
            r#"{ type = "bool", default = false, description = "Bend a two-bone chain the other way" }"#,
        ),
        (
            k::ENABLED,
            r#"{ type = "bool", default = true, description = "Whether the modifier runs; off leaves the clip's pose alone" }"#,
        ),
    ])
}

const DOC_2D: &str = "Poses 2D bones after the clip has run, every frame: `look_at` turns one bone \
                      toward a target node, `two_bone_ik` bends a root, middle and tip chain so \
                      the tip reaches it, `fabrik` and `ccdik` reach with a chain of any length, \
                      and `jiggle` lets a chain trail the pose on a spring.";

const DOC_3D: &str = "The 3D twin of `modifier2d`, over `bone3d`: `look_at`, `two_bone_ik`, \
                      `fabrik`, `ccdik` and `jiggle`, posing bones after the clip has run. A \
                      chain solver turns each bone by the shortest arc onto the solved point, so \
                      a bone's twist about its own aim is left as the clip wrote it.";

/// The `modifier2d` component: writes one [`Modifier2d`] on the node.
pub(crate) fn register_modifier2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "modifier2d",
        ComponentDef {
            doc: DOC_2D,
            schema: ComponentDef::parse_schema("modifier2d", &schema()),
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::ANIMATION,
            ],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let params = params_of(params)?;
                eng.world_mut()
                    .insert_one(entity, Modifier2d(std::sync::Arc::new(params)))
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Modifier2d>(entity);
                forget_jiggle(eng, entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let m = world.get::<&Modifier2d>(entity).ok()?;
                Some(table_of(&m.0))
            }),
        },
    );
}

/// The `modifier3d` component: writes one [`Modifier3d`] on the node.
pub(crate) fn register_modifier3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "modifier3d",
        ComponentDef {
            doc: DOC_3D,
            schema: ComponentDef::parse_schema("modifier3d", &schema()),
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::ANIMATION,
            ],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let params = params_of(params)?;
                eng.world_mut()
                    .insert_one(entity, Modifier3d(std::sync::Arc::new(params)))
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Modifier3d>(entity);
                forget_jiggle(eng, entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let m = world.get::<&Modifier3d>(entity).ok()?;
                Some(table_of(&m.0))
            }),
        },
    );
}

/// A modifier's points go with it: a node that swapped `jiggle` for `look_at`
/// and back should start from the pose, not from where it was swinging.
fn forget_jiggle(eng: &Engine, entity: Entity) {
    if let Some(state) = eng.try_resource::<AnimationState>() {
        state.borrow_mut().jiggle.shift_remove(&entity);
    }
}

fn params_of(params: &toml::Value) -> Result<Params> {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let flag = |key: &str, default: bool| {
        params
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    let number = |key: &str, default: f32| {
        params
            .get(key)
            .and_then(as_f64)
            .map_or(default, |v| v as f32)
    };
    let count = |key: &str, default: u32| {
        params
            .get(key)
            .and_then(toml::Value::as_integer)
            .map_or(default, |v| v.clamp(0, i64::from(u32::MAX)) as u32)
    };
    Ok(Params {
        kind: Kind::parse(params.get(k::KIND).and_then(toml::Value::as_str))?,
        target: text(k::TARGET),
        bone: text(k::BONE),
        chain: count(k::CHAIN, 0) as usize,
        iterations: count(k::ITERATIONS, 10),
        tolerance: number(k::TOLERANCE, 0.01),
        angle_limit: number(k::ANGLE_LIMIT, 0.0),
        stiffness: number(k::STIFFNESS, 3.0),
        // A tick that kept more speed than it had would wind the spring up
        // instead of settling it, so the read is clamped rather than trusted.
        damping: number(k::DAMPING, 0.75).clamp(0.0, 1.0),
        mass: number(k::MASS, 0.75),
        gravity: vector(params, k::GRAVITY, Vec3::new(0.0, -6.0, 0.0)),
        use_gravity: flag(k::USE_GRAVITY, false),
        flip: flag(k::FLIP, false),
        enabled: flag(k::ENABLED, true),
    })
}

fn vector(params: &toml::Value, key: &str, default: Vec3) -> Vec3 {
    let Some(list) = params.get(key).and_then(toml::Value::as_array) else {
        return default;
    };
    let at = |i: usize| list.get(i).and_then(as_f64).map_or(0.0, |v| v as f32);
    Vec3::new(at(0), at(1), at(2))
}

fn table_of(m: &Params) -> toml::Value {
    let mut out = toml::map::Map::new();
    let mut put = |key: &str, value: toml::Value| {
        out.insert(key.to_string(), value);
    };
    put(k::KIND, toml::Value::String(m.kind.name().into()));
    put(k::TARGET, toml::Value::String(m.target.clone()));
    put(k::BONE, toml::Value::String(m.bone.clone()));
    put(
        k::CHAIN,
        toml::Value::Integer(i64::try_from(m.chain).unwrap_or(0)),
    );
    put(k::ITERATIONS, toml::Value::Integer(i64::from(m.iterations)));
    put(k::TOLERANCE, toml::Value::Float(f64::from(m.tolerance)));
    put(k::ANGLE_LIMIT, toml::Value::Float(f64::from(m.angle_limit)));
    put(k::STIFFNESS, toml::Value::Float(f64::from(m.stiffness)));
    put(k::DAMPING, toml::Value::Float(f64::from(m.damping)));
    put(k::MASS, toml::Value::Float(f64::from(m.mass)));
    put(
        k::GRAVITY,
        toml::Value::Array(
            [m.gravity.x, m.gravity.y, m.gravity.z]
                .into_iter()
                .map(|v| toml::Value::Float(f64::from(v)))
                .collect(),
        ),
    );
    put(k::USE_GRAVITY, toml::Value::Boolean(m.use_gravity));
    put(k::FLIP, toml::Value::Boolean(m.flip));
    put(k::ENABLED, toml::Value::Boolean(m.enabled));
    toml::Value::Table(out)
}

// ---------------------------------------------------------------- poses

/// A node's 2D world pose composed from local transforms, so a bone this
/// frame has already moved sees the move.
fn pose_2d(world: &World, entity: Entity) -> Mat3 {
    let mut matrix = Mat3::IDENTITY;
    for e in ancestry(world, entity) {
        if let Ok(t) = world.get::<&Transform>(e) {
            matrix *= affine_2d(t.position, t.rotation, t.scale);
        }
    }
    matrix
}

/// The 3D twin of [`pose_2d`].
fn pose_3d(world: &World, entity: Entity) -> Mat4 {
    let mut matrix = Mat4::IDENTITY;
    for e in ancestry(world, entity) {
        if let Ok(t) = world.get::<&Transform>(e) {
            matrix *= affine_3d(t.position, t.rotation, t.scale);
        }
    }
    matrix
}

/// A node's world rotation alone, composed the same way. Scale would skew an
/// aim direction and translation cannot turn one, so neither is wanted here.
fn rotation_3d(world: &World, entity: Entity) -> Quat {
    let mut q = Quat::IDENTITY;
    for e in ancestry(world, entity) {
        if let Ok(t) = world.get::<&Transform>(e) {
            q *= t.rotation;
        }
    }
    q
}

/// Where a node sits in the scene, as the child index at each step down from
/// the root: the order the scene file reads in, and one that stays put when a
/// node elsewhere is added or removed.
fn scene_order(world: &World, entity: Entity) -> Vec<u32> {
    let line = ancestry(world, entity);
    line.windows(2)
        .map(|step| {
            world
                .get::<&scene::Children>(step[0])
                .ok()
                .and_then(|kids| kids.0.iter().position(|child| *child == step[1]))
                .unwrap_or(0) as u32
        })
        .collect()
}

/// `entity` and every ancestor, root first — the order a pose composes in.
fn ancestry(world: &World, entity: Entity) -> Vec<Entity> {
    let mut chain = vec![entity];
    let mut current = entity;
    while let Ok(parent) = world.get::<&Parent>(current) {
        current = parent.0;
        chain.push(current);
        if chain.len() > MAX_CHAIN * 4 {
            break;
        }
    }
    chain.reverse();
    chain
}

fn angle_of(m: &Mat3) -> f32 {
    libm::atan2f(m.x_axis.y, m.x_axis.x)
}

fn origin_2d(m: &Mat3) -> Vec2 {
    Vec2::new(m.z_axis.x, m.z_axis.y)
}

fn origin_3d(m: &Mat4) -> Vec3 {
    m.w_axis.truncate()
}

fn first_child_bone(world: &World, entity: Entity) -> Option<Entity> {
    let children = world.get::<&Children>(entity).ok()?;
    children
        .0
        .iter()
        .copied()
        .find(|&child| world.get::<&Bone>(child).is_ok())
}

/// The bones a chain solver works on: `root` and its first-child bones, at
/// most `len` of them, or as far as the rig goes when `len` is zero.
fn chain_of(world: &World, root: Entity, len: usize) -> Vec<Entity> {
    let cap = if len == 0 {
        MAX_CHAIN
    } else {
        len.min(MAX_CHAIN)
    };
    let mut out = Vec::new();
    let mut current = Some(root);
    while let Some(bone) = current {
        out.push(bone);
        if out.len() >= cap {
            break;
        }
        current = first_child_bone(world, bone);
    }
    out
}

// ------------------------------------------------------------ 2D aiming

/// The direction a bone points along in its own frame: toward its first
/// child bone, else its gizmo `angle`.
fn aim_local_2d(world: &World, bone: Entity) -> f32 {
    match first_child_bone(world, bone) {
        Some(child) => {
            let p = world
                .get::<&Transform>(child)
                .map(|t| t.position)
                .unwrap_or_default();
            libm::atan2f(p.y, p.x)
        }
        None => world.get::<&Bone>(bone).map_or(0.0, |b| b.angle),
    }
}

/// Turn `bone` so that its aim points along `wanted` in world space.
fn aim_at_angle(world: &World, bone: Entity, wanted: f32) {
    let parent_angle = world
        .get::<&Parent>(bone)
        .ok()
        .map_or(0.0, |p| angle_of(&pose_2d(world, p.0)));
    let local = wanted - aim_local_2d(world, bone) - parent_angle;
    if let Ok(mut t) = world.get::<&mut Transform>(bone) {
        t.rotation = quat_about_z(local);
    }
}

/// Point `bone`'s aim at a world point, doing nothing when the point is the
/// bone's own origin and names no direction.
fn aim_at_point_2d(world: &World, bone: Entity, point: Vec2) {
    let to = point - origin_2d(&pose_2d(world, bone));
    if to.length_squared() > MIN_BONE * MIN_BONE {
        aim_at_angle(world, bone, libm::atan2f(to.y, to.x));
    }
}

/// Hold a bone within `limit` radians of its rest rotation about z.
fn clamp_angle_2d(world: &World, bone: Entity, limit: f32) {
    if limit <= 0.0 {
        return;
    }
    let rest = world.get::<&Bone>(bone).map_or(0.0, |b| b.rest_rotation.z);
    let Ok(mut t) = world.get::<&mut Transform>(bone) else {
        return;
    };
    let current = balaur_core::skeleton::angle_about_z(t.rotation);
    let delta = wrap_pi(current - rest);
    if delta.abs() > limit {
        t.rotation = quat_about_z(rest + limit.copysign(delta));
    }
}

/// An angle folded onto `-pi..pi`, so "how far from rest" is the short way
/// round rather than a number that grew with the turns.
fn wrap_pi(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut a = angle;
    while a > PI {
        a -= TAU;
    }
    while a < -PI {
        a += TAU;
    }
    a
}

// ------------------------------------------------------------ 3D aiming

/// The direction a bone points along in its own frame, as a unit vector:
/// toward its first child bone, else `+X`, which is what a bone with no
/// child and no 2D gizmo angle has to mean.
fn aim_local_3d(world: &World, bone: Entity) -> Vec3 {
    let toward = first_child_bone(world, bone)
        .and_then(|child| world.get::<&Transform>(child).ok().map(|t| t.position))
        .unwrap_or(Vec3::X);
    if toward.length_squared() > MIN_BONE * MIN_BONE {
        toward.normalize()
    } else {
        Vec3::X
    }
}

/// Turn `bone` by the shortest arc so its aim points along `wanted` in world
/// space, leaving its twist about that aim as the clip wrote it.
fn aim_at_dir_3d(world: &World, bone: Entity, wanted: Vec3) {
    if wanted.length_squared() <= MIN_BONE * MIN_BONE {
        return;
    }
    let parent = world
        .get::<&Parent>(bone)
        .ok()
        .map_or(Quat::IDENTITY, |p| rotation_3d(world, p.0));
    // The aim is stated in the bone's own frame, so the wanted direction is
    // carried into the parent's before the arc between them is taken.
    let local_wanted = parent.inverse() * wanted.normalize();
    let rotation = Quat::from_rotation_arc(aim_local_3d(world, bone), local_wanted);
    if let Ok(mut t) = world.get::<&mut Transform>(bone) {
        t.rotation = rotation;
    }
}

fn aim_at_point_3d(world: &World, bone: Entity, point: Vec3) {
    let to = point - origin_3d(&pose_3d(world, bone));
    aim_at_point_dir(world, bone, to);
}

fn aim_at_point_dir(world: &World, bone: Entity, to: Vec3) {
    if to.length_squared() > MIN_BONE * MIN_BONE {
        aim_at_dir_3d(world, bone, to);
    }
}

/// Hold a bone within `limit` radians of its rest rotation.
fn clamp_angle_3d(world: &World, bone: Entity, limit: f32) {
    if limit <= 0.0 {
        return;
    }
    let rest = world
        .get::<&Bone>(bone)
        .map_or(Quat::IDENTITY, |b| quat_from_euler(b.rest_rotation));
    let Ok(mut t) = world.get::<&mut Transform>(bone) else {
        return;
    };
    let delta = rest.inverse() * t.rotation;
    // `w` past one by a rounding step is what makes `acos` return NaN and
    // freeze the bone at a rotation nothing can clamp afterwards.
    let angle = 2.0 * libm::acosf(delta.w.abs().clamp(0.0, 1.0));
    if angle <= limit {
        return;
    }
    let axis = Vec3::new(delta.x, delta.y, delta.z);
    if axis.length_squared() <= MIN_BONE * MIN_BONE {
        return;
    }
    let axis = axis.normalize() * delta.w.signum();
    t.rotation = rest * Quat::from_axis_angle(axis, limit);
}

// ------------------------------------------------------------- solvers

/// The joints a chain reaches with: every bone's origin, and the last bone's
/// tip when it has one, so there is a segment per bone rather than per gap.
fn chain_points(world: &World, chain: &[Entity], dim3: bool) -> Vec<Vec3> {
    let origin = |bone: Entity| {
        if dim3 {
            origin_3d(&pose_3d(world, bone))
        } else {
            origin_2d(&pose_2d(world, bone)).extend(0.0)
        }
    };
    let mut points: Vec<Vec3> = chain.iter().map(|&bone| origin(bone)).collect();
    if let Some(&last) = chain.last()
        && let Some(tip) = tip_of(world, last, dim3)
    {
        points.push(tip);
    }
    points
}

/// Where a chain's last bone ends: its first child bone if it has one, else
/// its gizmo length along its aim. A bone with neither ends the chain at its
/// own origin and simply does not get a segment.
fn tip_of(world: &World, bone: Entity, dim3: bool) -> Option<Vec3> {
    if let Some(child) = first_child_bone(world, bone) {
        return Some(if dim3 {
            origin_3d(&pose_3d(world, child))
        } else {
            origin_2d(&pose_2d(world, child)).extend(0.0)
        });
    }
    let length = world.get::<&Bone>(bone).map_or(0.0, |b| b.length);
    if length <= MIN_BONE {
        return None;
    }
    if dim3 {
        let pose = pose_3d(world, bone);
        let dir = rotation_3d(world, bone) * aim_local_3d(world, bone);
        Some(origin_3d(&pose) + dir * length)
    } else {
        let pose = pose_2d(world, bone);
        let angle = angle_of(&pose) + aim_local_2d(world, bone);
        let (s, c) = libm::sincosf(angle);
        Some((origin_2d(&pose) + Vec2::new(c, s) * length).extend(0.0))
    }
}

fn segment_lengths(points: &[Vec3]) -> Vec<f32> {
    points.windows(2).map(|w| (w[1] - w[0]).length()).collect()
}

/// FABRIK: pull the chain onto the target from the tip back, then put the
/// root where it belongs and push forward, keeping every segment its own
/// length. Converges from any pose and never needs an inverse Jacobian.
///
/// Dimension-agnostic: 2D hands it points with `z = 0` and gets them back the
/// same way, because every step is a lerp along a segment.
fn fabrik(points: &mut [Vec3], lengths: &[f32], target: Vec3, iterations: u32, tolerance: f32) {
    let n = points.len();
    if n < 2 || lengths.len() + 1 != n {
        return;
    }
    let root = points[0];
    let reach: f32 = lengths.iter().sum();
    // Out of reach there is nothing to iterate: the answer is the straight
    // line at the target, and iterating only finds it slowly.
    let to = target - root;
    if to.length() >= reach {
        let dir = if to.length_squared() > MIN_BONE * MIN_BONE {
            to.normalize()
        } else {
            Vec3::X
        };
        for i in 1..n {
            points[i] = points[i - 1] + dir * lengths[i - 1];
        }
        return;
    }
    for _ in 0..iterations.max(1) {
        if (points[n - 1] - target).length() <= tolerance {
            break;
        }
        points[n - 1] = target;
        for i in (0..n - 1).rev() {
            points[i] = along(points[i + 1], points[i], lengths[i]);
        }
        points[0] = root;
        for i in 1..n {
            points[i] = along(points[i - 1], points[i], lengths[i - 1]);
        }
    }
}

/// The point `length` away from `from`, toward `toward`. A zero-length step
/// keeps the direction it had rather than exploding into a NaN.
fn along(from: Vec3, toward: Vec3, length: f32) -> Vec3 {
    let d = toward - from;
    if d.length_squared() <= MIN_BONE * MIN_BONE {
        return from + Vec3::X * length;
    }
    from + d.normalize() * length
}

/// The analytic two-bone solve: the root turns to put the middle joint on
/// the circle both segments can reach, then the middle turns to put the tip
/// on the target. Out of reach, the chain straightens toward it.
fn two_bone_ik_2d(world: &World, root: Entity, target: Vec2, flip: bool) {
    let (Some(mid), Some(tip)) = (
        first_child_bone(world, root),
        first_child_bone(world, root).and_then(|mid| first_child_bone(world, mid)),
    ) else {
        tracing::debug!("two_bone_ik needs a root, middle and tip bone");
        return;
    };
    let (r, m, t) = (
        origin_2d(&pose_2d(world, root)),
        origin_2d(&pose_2d(world, mid)),
        origin_2d(&pose_2d(world, tip)),
    );
    let l1 = (m - r).length();
    let l2 = (t - m).length();
    // The clamp below has `min > max` for anything shorter, and `f32::clamp`
    // panics on that: a bone 5e-6 from its parent is what reaches it.
    if !(l1 > MIN_BONE && l2 > MIN_BONE) {
        return;
    }
    let to = target - r;
    let base = libm::atan2f(to.y, to.x);
    let sign = if flip { -1.0 } else { 1.0 };
    let (root_aim, mid_aim) = two_bone_angles(l1, l2, to.length(), base, sign);
    aim_at_angle(world, root, root_aim);
    aim_at_angle(world, mid, mid_aim);
}

/// The two aim angles a two-bone chain takes, shared by both dimensions: 3D
/// solves in the plane the chain and the target span and uses these there.
fn two_bone_angles(l1: f32, l2: f32, distance: f32, base: f32, sign: f32) -> (f32, f32) {
    let d = distance.clamp((l1 - l2).abs() + 1e-5, l1 + l2 - 1e-5);
    let cos_root = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
    let cos_mid = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0);
    let root_aim = base + sign * libm::acosf(cos_root);
    let mid_aim = root_aim - sign * (std::f32::consts::PI - libm::acosf(cos_mid));
    (root_aim, mid_aim)
}

/// The 3D two-bone solve. The chain bends in the plane holding the root, the
/// target and the pole — `flip` picks the other side of it — and the two
/// angles are the same law of cosines the 2D solve uses.
fn two_bone_ik_3d(world: &World, root: Entity, target: Vec3, flip: bool) {
    let (Some(mid), Some(tip)) = (
        first_child_bone(world, root),
        first_child_bone(world, root).and_then(|mid| first_child_bone(world, mid)),
    ) else {
        tracing::debug!("two_bone_ik needs a root, middle and tip bone");
        return;
    };
    let (r, m, t) = (
        origin_3d(&pose_3d(world, root)),
        origin_3d(&pose_3d(world, mid)),
        origin_3d(&pose_3d(world, tip)),
    );
    let l1 = (m - r).length();
    let l2 = (t - m).length();
    if !(l1 > MIN_BONE && l2 > MIN_BONE) {
        return;
    }
    let to = target - r;
    if to.length_squared() <= MIN_BONE * MIN_BONE {
        return;
    }
    let axis = to.normalize();
    // The bend plane: the elbow's current offset from the root-to-target line
    // is what keeps a solved knee pointing where the clip had it. A chain
    // already straight has no such offset, so any perpendicular will do.
    let offset = (m - r) - axis * (m - r).dot(axis);
    let bend = if offset.length_squared() > MIN_BONE * MIN_BONE {
        offset.normalize()
    } else {
        axis.any_orthonormal_vector()
    };
    let bend = if flip { -bend } else { bend };
    let (root_aim, mid_aim) = two_bone_angles(l1, l2, to.length(), 0.0, 1.0);
    let dir = |angle: f32| {
        let (s, c) = libm::sincosf(angle);
        axis * c + bend * s
    };
    aim_at_dir_3d(world, root, dir(root_aim));
    aim_at_dir_3d(world, mid, dir(mid_aim));
}

/// Cyclic coordinate descent: turn each bone in turn, tip end first, so the
/// chain's tip swings onto the target, and repeat. Slower to converge than
/// FABRIK and the only one of the two that can hold a per-bone angle limit,
/// which is why both are here.
fn ccdik(world: &World, chain: &[Entity], target: Vec3, p: &Params, dim3: bool) {
    if chain.is_empty() {
        return;
    }
    let tip = |world: &World| tip_of(world, chain[chain.len() - 1], dim3);
    for _ in 0..p.iterations.max(1) {
        let Some(end) = tip(world) else { return };
        if (end - target).length() <= p.tolerance {
            return;
        }
        for &bone in chain.iter().rev() {
            let Some(end) = tip(world) else { return };
            let pivot = if dim3 {
                origin_3d(&pose_3d(world, bone))
            } else {
                origin_2d(&pose_2d(world, bone)).extend(0.0)
            };
            let (from, to) = (end - pivot, target - pivot);
            if from.length_squared() <= MIN_BONE * MIN_BONE
                || to.length_squared() <= MIN_BONE * MIN_BONE
            {
                continue;
            }
            if dim3 {
                let turn = Quat::from_rotation_arc(from.normalize(), to.normalize());
                let aim = rotation_3d(world, bone) * aim_local_3d(world, bone);
                aim_at_dir_3d(world, bone, turn * aim);
                clamp_angle_3d(world, bone, p.angle_limit);
            } else {
                let delta = libm::atan2f(to.y, to.x) - libm::atan2f(from.y, from.x);
                let aim = angle_of(&pose_2d(world, bone)) + aim_local_2d(world, bone);
                aim_at_angle(world, bone, aim + delta);
                clamp_angle_2d(world, bone, p.angle_limit);
            }
        }
    }
}

/// One jiggle tick: each bone's point is pulled back toward the tip the clip
/// posed, held at the bone's own length, and the bone is aimed at it.
///
/// Root to tip, because aiming a bone moves every origin below it: a pass the
/// other way would spring against a chain that had not moved yet.
fn jiggle_step(world: &World, chain: &[Entity], p: &Params, dim3: bool, state: &mut Jiggle) {
    if state.points.len() != chain.len() {
        state.points.clear();
        state.velocities.clear();
        state.incoming.clear();
        state.written.clear();
    }
    // The whole chain goes back on the clip's pose before anything is aimed,
    // so a bone's target is where the clip put it and not where the spring
    // did. A bone the clip has since moved keeps its new pose instead.
    for (i, &bone) in chain.iter().enumerate() {
        let Ok(mut t) = world.get::<&mut Transform>(bone) else {
            continue;
        };
        match (state.incoming.get(i), state.written.get(i)) {
            (Some(&clip), Some(&written)) if t.rotation == written => t.rotation = clip,
            _ => {
                while state.incoming.len() <= i {
                    state.incoming.push(t.rotation);
                    state.written.push(t.rotation);
                }
                state.incoming[i] = t.rotation;
            }
        }
    }
    let keep = (1.0 - p.damping).clamp(0.0, 1.0);
    for (i, &bone) in chain.iter().enumerate() {
        let origin = if dim3 {
            origin_3d(&pose_3d(world, bone))
        } else {
            origin_2d(&pose_2d(world, bone)).extend(0.0)
        };
        let Some(rest_tip) = tip_of(world, bone, dim3) else {
            continue;
        };
        if i >= state.points.len() {
            state.points.push(rest_tip);
            state.velocities.push(Vec3::ZERO);
        }
        let mut point = state.points[i];
        let mut velocity = state.velocities[i];
        let mut force = (rest_tip - point) * p.stiffness;
        if p.use_gravity {
            force += p.gravity * p.mass;
        }
        velocity = (velocity + force * FIXED_DT) * keep;
        point += velocity * FIXED_DT;
        // The point is a direction to aim along, not a joint position, so it
        // is left where the spring puts it. Holding it on the bone's own
        // circle would look tidier and has one fixed point too many: a point
        // flung to the far side of the origin sits exactly opposite the pose,
        // where the pull is along the radius the projection cancels, and the
        // bone stays upside down for good.
        let _ = origin;
        // A non-finite point would be aimed at once and then hold the bone
        // there for the rest of the session; the pose is the safe fallback.
        if !point.is_finite() || !velocity.is_finite() {
            point = rest_tip;
            velocity = Vec3::ZERO;
        }
        state.points[i] = point;
        state.velocities[i] = velocity;
        if dim3 {
            aim_at_point_3d(world, bone, point);
        } else {
            aim_at_point_2d(world, bone, point.truncate());
        }
        if let Ok(t) = world.get::<&Transform>(bone) {
            while state.written.len() <= i {
                state.written.push(t.rotation);
            }
            state.written[i] = t.rotation;
        }
    }
    state.points.truncate(chain.len());
    state.velocities.truncate(chain.len());
    state.incoming.truncate(chain.len());
    state.written.truncate(chain.len());
}

/// Write a solved point list back onto the chain, root to tip.
fn apply_points(world: &World, chain: &[Entity], points: &[Vec3], dim3: bool) {
    for (i, &bone) in chain.iter().enumerate() {
        let Some(&next) = points.get(i + 1) else {
            break;
        };
        if dim3 {
            aim_at_point_3d(world, bone, next);
        } else {
            aim_at_point_2d(world, bone, next.truncate());
        }
    }
}

// -------------------------------------------------------------- system

/// Every modifier, in a fixed order, from the transforms as they are now.
pub(crate) fn modify_system(eng: &Engine, dt: f32) {
    // Nothing has moved under a held game, so there is nothing to re-pose.
    if eng.frozen_root().is_some() {
        return;
    }
    let mut work: Vec<(Entity, std::sync::Arc<Params>, bool)> = {
        let world = eng.world();
        let mut work: Vec<(Vec<u32>, Entity, std::sync::Arc<Params>, bool)> = world
            .query::<(Entity, &Modifier2d)>()
            .iter()
            .map(|(e, m)| (e, std::sync::Arc::clone(&m.0), false))
            .chain(
                world
                    .query::<(Entity, &Modifier3d)>()
                    .iter()
                    .map(|(e, m)| (e, std::sync::Arc::clone(&m.0), true)),
            )
            .filter(|(_, m, _)| m.enabled && !(m.kind.wants_target() && m.target.is_empty()))
            .map(|(e, m, dim3)| (scene_order(&world, e), e, m, dim3))
            .collect();
        // A set order is what makes two modifiers on one bone land the same
        // way twice. The scene's own reading order, not the entity's number:
        // that would move when a node somewhere else in the scene is added,
        // and the same rig would then solve differently.
        work.sort_by(|a, b| (a.3, &a.0).cmp(&(b.3, &b.0)));
        work.into_iter()
            .map(|(_, e, m, dim3)| (e, m, dim3))
            .collect()
    };
    let steps = jiggle_steps(eng, dt, &work);
    for (entity, m, dim3) in work.drain(..) {
        run_one(eng, entity, &m, dim3, steps);
    }
}

/// How many fixed ticks the jiggle springs owe this frame, advanced once for
/// the whole system rather than once per modifier.
///
/// The accumulator only moves when something is actually jiggling, so a scene
/// with no springs in it does not carry a residual into the frame one appears.
fn jiggle_steps(eng: &Engine, dt: f32, work: &[(Entity, std::sync::Arc<Params>, bool)]) -> u32 {
    if !work.iter().any(|(_, m, _)| m.kind == Kind::Jiggle) {
        return 0;
    }
    let Some(state) = eng.try_resource::<AnimationState>() else {
        return 0;
    };
    let mut state = state.borrow_mut();
    // The same clamp the playhead takes: a frame that hitched must not be
    // paid back in a hundred ticks of spring at once.
    state.jiggle_accumulator = (state.jiggle_accumulator + dt).min(FIXED_DT * MAX_SUBSTEPS as f32);
    let mut steps = 0;
    while state.jiggle_accumulator >= FIXED_DT {
        state.jiggle_accumulator -= FIXED_DT;
        steps += 1;
    }
    steps
}

fn run_one(eng: &Engine, entity: Entity, m: &Params, dim3: bool, steps: u32) {
    let world = eng.world();
    let bone = if m.bone.is_empty() {
        Some(entity)
    } else {
        scene::find_node(&world, entity, &m.bone)
    };
    let Some(bone) = bone else {
        tracing::debug!(bone = m.bone, "modifier bone names no node");
        return;
    };
    if m.kind == Kind::Jiggle {
        if steps == 0 {
            return;
        }
        let chain = chain_of(&world, bone, m.chain);
        // The spring's own state, taken out of the map for the tick: the
        // solver writes transforms, and holding the resource borrow across
        // that is what a component `apply` hook would panic on.
        let state = eng.resource::<AnimationState>();
        let mut points = state
            .borrow_mut()
            .jiggle
            .shift_remove(&entity)
            .unwrap_or_default();
        for _ in 0..steps {
            jiggle_step(&world, &chain, m, dim3, &mut points);
        }
        state.borrow_mut().jiggle.insert(entity, points);
        return;
    }
    let Some(target) = scene::find_node(&world, entity, &m.target) else {
        tracing::debug!(target = m.target, "modifier target names no node");
        return;
    };
    let point = if dim3 {
        origin_3d(&pose_3d(&world, target))
    } else {
        origin_2d(&pose_2d(&world, target)).extend(0.0)
    };
    // A target whose transform went non-finite would write NaN rotations
    // into the rig and keep them there; the clip's pose stands instead.
    if !point.is_finite() {
        tracing::debug!(target = m.target, "modifier target is not a finite point");
        return;
    }
    match (m.kind, dim3) {
        (Kind::LookAt, false) => aim_at_point_2d(&world, bone, point.truncate()),
        (Kind::LookAt, true) => aim_at_point_3d(&world, bone, point),
        (Kind::TwoBoneIk, false) => two_bone_ik_2d(&world, bone, point.truncate(), m.flip),
        (Kind::TwoBoneIk, true) => two_bone_ik_3d(&world, bone, point, m.flip),
        (Kind::Fabrik, _) => {
            let chain = chain_of(&world, bone, m.chain);
            let mut points = chain_points(&world, &chain, dim3);
            let lengths = segment_lengths(&points);
            fabrik(&mut points, &lengths, point, m.iterations, m.tolerance);
            apply_points(&world, &chain, &points, dim3);
        }
        (Kind::Ccdik, _) => {
            let chain = chain_of(&world, bone, m.chain);
            ccdik(&world, &chain, point, m, dim3);
        }
        (Kind::Jiggle, _) => unreachable!("handled above"),
    }
}

/// Where the modifier's target is, for a tool that draws the reach.
#[must_use]
pub fn target_of(eng: &Engine, entity: Entity) -> Option<Vec2> {
    let world = eng.world();
    let (target, dim3) = match world.get::<&Modifier2d>(entity) {
        Ok(m) => (m.0.target.clone(), false),
        Err(_) => (
            world.get::<&Modifier3d>(entity).ok()?.0.target.clone(),
            true,
        ),
    };
    let target = scene::find_node(&world, entity, &target)?;
    Some(if dim3 {
        origin_3d(&pose_3d(&world, target)).truncate()
    } else {
        origin_2d(&pose_2d(&world, target))
    })
}

/// The bones a modifier drives, for a tool that draws the chain it solves.
///
/// The editor's gizmo needs the same walk the solver makes — a `chain` of two
/// on a rig five deep draws two bones, not five — and this is that walk.
#[must_use]
pub fn chain_of_node(eng: &Engine, entity: Entity) -> Vec<Entity> {
    let world = eng.world();
    let (bone_path, chain) = match world.get::<&Modifier2d>(entity) {
        Ok(m) => (m.0.bone.clone(), m.0.chain),
        Err(_) => match world.get::<&Modifier3d>(entity) {
            Ok(m) => (m.0.bone.clone(), m.0.chain),
            Err(_) => return Vec::new(),
        },
    };
    let bone = if bone_path.is_empty() {
        Some(entity)
    } else {
        scene::find_node(&world, entity, &bone_path)
    };
    bone.map(|bone| chain_of(&world, bone, chain))
        .unwrap_or_default()
}
