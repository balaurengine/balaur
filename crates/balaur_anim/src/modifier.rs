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
use crate::modifier_solve::{
    MIN_BONE, aim_at_angle, aim_at_dir_3d, aim_at_point_2d, aim_at_point_3d, aim_local_2d,
    aim_local_3d, angle_of, chain_points, clamp_angle_2d, clamp_angle_3d, rotation_3d, scene_order,
    segment_lengths, tip_of, two_bone_ik_2d, two_bone_ik_3d,
};
use crate::player::{AnimationState, FIXED_DT, MAX_SUBSTEPS};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, as_f64};
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{self, Parent, Transform};
use balaur_plugin::Registry;
use glamx::{Quat, Vec3};

pub(crate) use crate::modifier_solve::{chain_of, origin_2d, origin_3d, pose_2d, pose_3d};

/// The five modifiers, written once for the schema, the matcher and the
/// read-back.
const LOOK_AT: &str = "look_at";
const TWO_BONE_IK: &str = "two_bone_ik";
const FABRIK: &str = "fabrik";
const CCDIK: &str = "ccdik";
const JIGGLE: &str = "jiggle";
const FOLLOW: &str = "follow";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    LookAt,
    TwoBoneIk,
    Fabrik,
    Ccdik,
    Jiggle,
    Follow,
}

impl Kind {
    fn parse(text: Option<&str>) -> Result<Self> {
        match text {
            None | Some(LOOK_AT) => Ok(Self::LookAt),
            Some(TWO_BONE_IK) => Ok(Self::TwoBoneIk),
            Some(FABRIK) => Ok(Self::Fabrik),
            Some(CCDIK) => Ok(Self::Ccdik),
            Some(JIGGLE) => Ok(Self::Jiggle),
            Some(FOLLOW) => Ok(Self::Follow),
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
            Self::Follow => FOLLOW,
        }
    }

    /// Whether the kind reaches for a target node. `jiggle` is the one that
    /// does not: it follows the pose it was given.
    const fn wants_target(self) -> bool {
        !matches!(self, Self::Jiggle)
    }

    /// Whether where the kind lands depends on where it was, which is what
    /// makes it owe fixed ticks rather than reading the frame's own `dt`.
    const fn has_memory(self) -> bool {
        matches!(self, Self::Jiggle | Self::Follow)
    }
}

/// What a `modifier2d` or `modifier3d` component wrote on the node. One
/// struct for both: the keys are the same and only the solver differs.
#[derive(Clone, Debug)]
pub struct Params {
    kind: Kind,
    /// Node path to the point to aim at, relative to the node.
    pub(crate) target: String,
    /// Node path to the driven bone, relative to the node; empty is the node.
    pub(crate) bone: String,
    /// How many bones the chain holds, counting the driven one. Zero walks
    /// to the deepest tip.
    pub(crate) chain: usize,
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
    /// Seconds a `follow` node takes to close most of the gap. Zero snaps.
    lag: f32,
    /// Where a `follow` node sits relative to its target, in world units.
    offset: Vec3,
    flip: bool,
    enabled: bool,
}

/// A 2D rig modifier, over `bone2d`.
///
/// Shared rather than owned: the system reads every modifier's params once a
/// frame, and the paths in them are strings nobody should be copying sixty
/// times a second.
#[derive(Clone, Debug)]
pub struct Modifier2d(pub(crate) std::sync::Arc<Params>);

/// A 3D rig modifier, over `bone3d`.
#[derive(Clone, Debug)]
pub struct Modifier3d(pub(crate) std::sync::Arc<Params>);

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
    let kinds = ComponentDef::options(&[LOOK_AT, TWO_BONE_IK, FABRIK, CCDIK, JIGGLE, FOLLOW]);
    // Down, at about two thirds of earth's: a chain that hangs rather than
    // drops. A 2D rig reads the third number as nothing, so both dimensions
    // take the same one.
    let gravity = "[0.0, -6.0, 0.0]";
    ComponentDef::schema(&[
        (
            k::KIND,
            &format!(
                r#"{{ type = "enum", default = "{LOOK_AT}", options = [{kinds}], description = "Aim one bone at the target, bend a two-bone chain to it, reach with a chain of any length ({FABRIK} or {CCDIK}), let a chain lag behind the pose ({JIGGLE}), or trail the target at an offset ({FOLLOW})" }}"#
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
            k::LAG,
            r#"{ type = "float", default = 0.0, description = "Seconds a follow node takes to close most of the gap to its target; 0 pins it there" }"#,
        ),
        (
            k::OFFSET,
            r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where a follow node sits relative to its target, in world units" }"#,
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

const DOC_2D: &str = "Poses 2D bones toward `target` after the clip runs. `kind` is `look_at`, `two_bone_ik`, `fabrik`, `ccdik`, `jiggle` or `follow`; `follow` moves the node by `offset` and `lag`.";

const DOC_3D: &str = "Poses `bone3d` nodes toward `target` after the clip runs. `kind` is `look_at`, `two_bone_ik`, `fabrik`, `ccdik`, `jiggle` or `follow`; `follow` moves the node by `offset` and `lag`.";

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
        // A lag below zero would grow the gap instead of closing it.
        lag: number(k::LAG, 0.0).max(0.0),
        offset: vector(params, k::OFFSET, Vec3::ZERO),
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
    put(k::LAG, toml::Value::Float(f64::from(m.lag)));
    put(
        k::OFFSET,
        toml::Value::Array(
            [m.offset.x, m.offset.y, m.offset.z]
                .into_iter()
                .map(|v| toml::Value::Float(f64::from(v)))
                .collect(),
        ),
    );
    put(k::FLIP, toml::Value::Boolean(m.flip));
    put(k::ENABLED, toml::Value::Boolean(m.enabled));
    toml::Value::Table(out)
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
        // A direction to aim along, not a joint position: held to the bone's
        // own circle, a point flung past the origin would stay upside down.
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
        // Two modifiers on one bone have to land the same way twice, and the
        // scene's reading order does not move when another node is added.
        work.sort_by(|a, b| (a.3, &a.0).cmp(&(b.3, &b.0)));
        work.into_iter()
            .map(|(_, e, m, dim3)| (e, m, dim3))
            .collect()
    };
    let steps = fixed_steps(eng, dt, &work);
    for (entity, m, dim3) in work.drain(..) {
        run_one(eng, entity, &m, dim3, steps);
    }
}

/// How many fixed ticks the modifiers that remember owe this frame, advanced
/// once for the whole system rather than once per modifier.
///
/// The accumulator only moves when one of them is in the scene, so a scene
/// with none carries no residual into the frame the first one appears.
fn fixed_steps(eng: &Engine, dt: f32, work: &[(Entity, std::sync::Arc<Params>, bool)]) -> u32 {
    if !work.iter().any(|(_, m, _)| m.kind.has_memory()) {
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
        (Kind::Follow, _) => follow_point(&world, bone, point + m.offset, m.lag, dim3, steps),
        (Kind::Jiggle, _) => unreachable!("handled above"),
    }
}

/// Move a node toward a point, closing the same share of the gap per fixed
/// tick so the path it takes does not change with the frame rate.
///
/// The node's own transform is the memory, so nothing is kept beside the
/// scene and a rollback puts a follower back where the snapshot had it.
fn follow_point(world: &World, node: Entity, goal: Vec3, lag: f32, dim3: bool, steps: u32) {
    let here = if dim3 {
        origin_3d(&pose_3d(world, node))
    } else {
        origin_2d(&pose_2d(world, node)).extend(0.0)
    };
    if !goal.is_finite() || !here.is_finite() {
        return;
    }
    // Exponential, so `steps` ticks at once land where that many one at a
    // time would: a frame that hitched does not overshoot.
    let share = if lag <= 0.0 {
        1.0
    } else {
        1.0 - libm::expf(-(steps as f32) * FIXED_DT / lag)
    };
    let want = here + (goal - here) * share;
    // A world point is written as a local one, because a follower may hang
    // under a parent that is itself moving.
    let local = match world.get::<&Parent>(node).map(|p| p.0) {
        Ok(parent) if dim3 => pose_3d(world, parent).inverse().transform_point3(want),
        Ok(parent) => {
            let placed = pose_2d(world, parent).inverse() * want.truncate().extend(1.0);
            Vec3::new(placed.x, placed.y, want.z)
        }
        Err(_) => want,
    };
    let Ok(mut transform) = world.get::<&mut Transform>(node) else {
        return;
    };
    transform.position.x = local.x;
    transform.position.y = local.y;
    // A 2D follower keeps whatever depth it was given: z is the draw order.
    if dim3 {
        transform.position.z = local.z;
    }
}

// The editor's gizmo asks these two; they read the same walk the solver
// makes, so they keep the module's public path.
pub use crate::gizmo::{chain_of_node, target_of};
