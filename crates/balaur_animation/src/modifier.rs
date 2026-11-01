//! Rig modifiers: bones posed again after the clip has had its say.
//!
//! Godot's `SkeletonModification2D` and `SkeletonModification3D`, as two
//! components over one set of solvers. `look_at` turns a bone so its child
//! points at a node; `two_bone_ik` bends a root, middle, tip chain so the tip
//! reaches one; `fabrik` and `ccdik` reach with a chain of any length;
//! `jiggle` lets a chain lag behind the pose on a spring; and `follow` moves
//! the node after its target by `offset` and `lag`. All six run in
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

use crate::modifier_solve::{
    MIN_BONE, aim_at_angle, aim_at_dir_3d, aim_at_point_2d, aim_at_point_3d, aim_local_2d,
    aim_local_3d, angle_of, chain_points, clamp_angle_2d, clamp_angle_3d, rotation_3d, scene_order,
    segment_lengths, tip_of, two_bone_ik_2d, two_bone_ik_3d,
};
use crate::player::{AnimationState, fixed_dt, max_substeps};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Enum, Written};
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{self, Parent, Transform};
use balaur_plugin::Registry;
use glamx::{Quat, Vec2, Vec3};
use std::sync::Arc;

pub(crate) use crate::modifier_solve::{chain_of, origin_2d, origin_3d, pose_2d, pose_3d};

/// What a modifier does to the rig: aim one bone, bend two, reach with a
/// chain, let a chain lag, or trail a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Kind {
    LookAt,
    TwoBoneIk,
    Fabrik,
    Ccdik,
    Jiggle,
    Follow,
}

impl Kind {
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

/// Poses 2D bones toward `target` after the clip runs. `kind` is `look_at`, `two_bone_ik`, `fabrik`, `ccdik`, `jiggle` or `follow`; `follow` moves the node by `offset` and `lag`.
#[derive(Clone, Debug, Component)]
#[component(name = "modifier2d", on_remove = modifier2d_removed)]
pub struct Modifier2d {
    /// Aim one bone at the target, bend a two-bone chain to it, reach with a chain of any length (fabrik or ccdik), let a chain lag behind the pose (jiggle), or trail the target at an offset (follow)
    #[property(default = Kind::LookAt)]
    pub kind: Kind,
    /// Whether the modifier runs; off leaves the clip's pose alone
    #[property(default = true)]
    pub enabled: bool,
    /// The driven bone; empty means this node. For a chain solver, its root
    #[property(node)]
    pub bone: String,
    /// The point to aim at. Unused by jiggle
    #[property(node)]
    pub target: String,
    /// How many bones the chain holds, counting the driven one; 0 walks to the deepest tip
    #[property(group = "chain")]
    pub chain_count: i64,
    /// Solver passes for fabrik and ccdik
    #[property(default = 10, group = "chain")]
    pub iterations: i64,
    /// How close to the target ends a fabrik or ccdik solve early
    #[property(default = 0.01, group = "chain")]
    pub tolerance: f64,
    /// How far a ccdik bone may turn from its rest, in radians; 0 leaves it free
    #[property(group = "chain")]
    pub angle_limit: f32,
    /// Bend a two-bone chain the other way
    #[property(group = "chain")]
    pub flip_bend_direction: bool,
    /// How hard a jiggle bone is pulled back to the pose
    #[property(default = 3.0, group = "jiggle")]
    pub stiffness: f32,
    /// How much of a jiggle bone's speed survives a tick, 0 to 1
    #[property(default = 0.75, group = "jiggle")]
    pub damping: f32,
    /// What gravity weighs against stiffness on a jiggle bone
    #[property(default = 0.75, group = "jiggle")]
    pub mass: f32,
    /// Whether a jiggle chain is pulled by `gravity`
    #[property(group = "jiggle")]
    pub use_gravity: bool,
    /// Pull on a jiggle bone while `use_gravity` is on
    #[property(default = Vec2::new(0.0, -JIGGLE_GRAVITY), group = "jiggle")]
    pub gravity: Vec2,
    /// Where a follow node sits relative to its target, in world units
    #[property(group = "follow")]
    pub offset: Vec2,
    /// Seconds a follow node takes to close most of the gap to its target; 0 pins it there
    #[property(group = "follow")]
    pub lag: f32,
}

/// Poses `bone3d` nodes toward `target` after the clip runs. `kind` is `look_at`, `two_bone_ik`, `fabrik`, `ccdik`, `jiggle` or `follow`; `follow` moves the node by `offset` and `lag`.
#[derive(Clone, Debug, Component)]
#[component(name = "modifier3d", on_remove = modifier3d_removed)]
pub struct Modifier3d {
    /// Aim one bone at the target, bend a two-bone chain to it, reach with a chain of any length (fabrik or ccdik), let a chain lag behind the pose (jiggle), or trail the target at an offset (follow)
    #[property(default = Kind::LookAt)]
    pub kind: Kind,
    /// Whether the modifier runs; off leaves the clip's pose alone
    #[property(default = true)]
    pub enabled: bool,
    /// The driven bone; empty means this node. For a chain solver, its root
    #[property(node)]
    pub bone: String,
    /// The point to aim at. Unused by jiggle
    #[property(node)]
    pub target: String,
    /// How many bones the chain holds, counting the driven one; 0 walks to the deepest tip
    #[property(group = "chain")]
    pub chain_count: i64,
    /// Solver passes for fabrik and ccdik
    #[property(default = 10, group = "chain")]
    pub iterations: i64,
    /// How close to the target ends a fabrik or ccdik solve early
    #[property(default = 0.01, group = "chain")]
    pub tolerance: f64,
    /// How far a ccdik bone may turn from its rest, in radians; 0 leaves it free
    #[property(group = "chain")]
    pub angle_limit: f32,
    /// Bend a two-bone chain the other way
    #[property(group = "chain")]
    pub flip_bend_direction: bool,
    /// How hard a jiggle bone is pulled back to the pose
    #[property(default = 3.0, group = "jiggle")]
    pub stiffness: f32,
    /// How much of a jiggle bone's speed survives a tick, 0 to 1
    #[property(default = 0.75, group = "jiggle")]
    pub damping: f32,
    /// What gravity weighs against stiffness on a jiggle bone
    #[property(default = 0.75, group = "jiggle")]
    pub mass: f32,
    /// Whether a jiggle chain is pulled by `gravity`
    #[property(group = "jiggle")]
    pub use_gravity: bool,
    /// Pull on a jiggle bone while `use_gravity` is on
    #[property(default = Vec3::new(0.0, -JIGGLE_GRAVITY, 0.0), group = "jiggle")]
    pub gravity: Vec3,
    /// Where a follow node sits relative to its target, in world units
    #[property(group = "follow")]
    pub offset: Vec3,
    /// Seconds a follow node takes to close most of the gap to its target; 0 pins it there
    #[property(group = "follow")]
    pub lag: f32,
}

/// A jiggle chain's pull when it names none: down, at about two thirds of
/// earth's, so a chain hangs rather than drops.
const JIGGLE_GRAVITY: f32 = 6.0;

/// What the solver reads of a `modifier2d` or a `modifier3d`, worked out once
/// a write lands: counts clamped, a spring that settles, a lag that closes,
/// and the vectors in three dimensions whichever twin it came from.
#[derive(Clone, Debug)]
pub struct Params {
    kind: Kind,
    /// Node path to the point to aim at, relative to the node.
    pub(crate) target: String,
    /// Node path to the driven bone, relative to the node; empty is the node.
    pub(crate) bone: String,
    /// How many bones the chain holds, counting the driven one. Zero walks
    /// to the deepest tip.
    pub(crate) chain_count: usize,
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
    flip_bend_direction: bool,
    enabled: bool,
}

/// [`Params`] from either twin: they hold the same fields, a 2D one's
/// vectors a dimension short.
macro_rules! params_of {
    ($asked:ident, $gravity:expr, $offset:expr) => {
        Params {
            kind: $asked.kind,
            target: $asked.target.clone(),
            bone: $asked.bone.clone(),
            chain_count: count($asked.chain_count) as usize,
            iterations: count($asked.iterations),
            tolerance: $asked.tolerance as f32,
            angle_limit: $asked.angle_limit,
            stiffness: $asked.stiffness,
            // A tick that kept more speed than it had would wind the spring
            // up instead of settling it, so the read is clamped rather than
            // trusted.
            damping: $asked.damping.clamp(0.0, 1.0),
            mass: $asked.mass,
            gravity: $gravity,
            use_gravity: $asked.use_gravity,
            // A lag below zero would grow the gap instead of closing it.
            lag: $asked.lag.max(0.0),
            offset: $offset,
            flip_bend_direction: $asked.flip_bend_direction,
            enabled: $asked.enabled,
        }
    };
}

/// A count as the solver takes it: none below zero.
fn count(asked: i64) -> u32 {
    asked.clamp(0, i64::from(u32::MAX)) as u32
}

/// A 2D modifier's [`Params`], beside the [`Modifier2d`] it was worked out
/// from.
///
/// Shared rather than owned: the system reads every modifier's params once a
/// frame, and the paths in them are strings nobody should be copying sixty
/// times a second.
#[derive(Clone, Debug)]
pub(crate) struct Params2d(pub(crate) Arc<Params>);

/// A 3D modifier's [`Params`], beside the [`Modifier3d`] it was worked out
/// from.
#[derive(Clone, Debug)]
pub(crate) struct Params3d(pub(crate) Arc<Params>);

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

/// What a removed `Modifier2d` leaves behind, taken with it.
fn modifier2d_removed(eng: &Engine, entity: Entity) {
    {
        let _ = eng.remove_one::<Params2d>(entity);
    }
    forget_jiggle(eng, entity);
}

/// The `modifier2d` component: a [`Modifier2d`] on the node, and the
/// [`Params2d`] the solver reads beside it.
pub(crate) fn register_modifier2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        <Modifier2d as Component>::NAME,
        ComponentDef {
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::ANIMATION,
            ],
            ..ComponentDef::of::<Modifier2d>(Some(written_2d))
        },
    );
}

/// What a removed `Modifier3d` leaves behind, taken with it.
fn modifier3d_removed(eng: &Engine, entity: Entity) {
    {
        let _ = eng.remove_one::<Params3d>(entity);
    }
    forget_jiggle(eng, entity);
}

/// The `modifier3d` component: a [`Modifier3d`] on the node, and the
/// [`Params3d`] the solver reads beside it.
pub(crate) fn register_modifier3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        <Modifier3d as Component>::NAME,
        ComponentDef {
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::ANIMATION,
            ],
            ..ComponentDef::of::<Modifier3d>(Some(written_3d))
        },
    );
}

/// The solver's [`Params2d`], worked out again from what the node now asks.
fn written_2d(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let params = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Modifier2d>(entity) else {
            return Ok(());
        };
        params_of!(asked, asked.gravity.extend(0.0), asked.offset.extend(0.0))
    };
    eng.insert_one(entity, Params2d(Arc::new(params)))
        .map_err(|_| anyhow!("node is dead"))
}

/// The solver's [`Params3d`], worked out again from what the node now asks.
fn written_3d(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let params = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Modifier3d>(entity) else {
            return Ok(());
        };
        params_of!(asked, asked.gravity, asked.offset)
    };
    eng.insert_one(entity, Params3d(Arc::new(params)))
        .map_err(|_| anyhow!("node is dead"))
}

/// A modifier's points go with it: a node that swapped `jiggle` for `look_at`
/// and back should start from the pose, not from where it was swinging.
fn forget_jiggle(eng: &Engine, entity: Entity) {
    if let Some(state) = eng.try_resource::<AnimationState>() {
        state.borrow_mut().jiggle.shift_remove(&entity);
    }
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
        let step = fixed_dt();
        velocity = (velocity + force * step) * keep;
        point += velocity * step;
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
    let paused = balaur_core::process::pause(eng);
    let mut work: Vec<(Entity, Arc<Params>, bool)> = {
        let world = eng.world();
        let mut work: Vec<(Vec<u32>, Entity, Arc<Params>, bool)> = world
            .query::<(Entity, &Params2d)>()
            .iter()
            .map(|(e, m)| (e, Arc::clone(&m.0), false))
            .chain(
                world
                    .query::<(Entity, &Params3d)>()
                    .iter()
                    .map(|(e, m)| (e, Arc::clone(&m.0), true)),
            )
            .filter(|(_, m, _)| m.enabled && !(m.kind.wants_target() && m.target.is_empty()))
            .filter(|(e, _, _)| balaur_core::process::ticks(&world, *e, paused))
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
fn fixed_steps(eng: &Engine, dt: f32, work: &[(Entity, Arc<Params>, bool)]) -> u32 {
    if !work.iter().any(|(_, m, _)| m.kind.has_memory()) {
        return 0;
    }
    let Some(state) = eng.try_resource::<AnimationState>() else {
        return 0;
    };
    let mut state = state.borrow_mut();
    state.honour_step_restart(eng);
    // The same clamp the playhead takes: a frame that hitched must not be
    // paid back in a hundred ticks of spring at once.
    state.jiggle_accumulator =
        (state.jiggle_accumulator + dt).min(fixed_dt() * max_substeps() as f32);
    let mut steps = 0;
    while state.jiggle_accumulator >= fixed_dt() {
        state.jiggle_accumulator -= fixed_dt();
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
        let chain = chain_of(&world, bone, m.chain_count);
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
        (Kind::TwoBoneIk, false) => {
            two_bone_ik_2d(&world, bone, point.truncate(), m.flip_bend_direction);
        }
        (Kind::TwoBoneIk, true) => two_bone_ik_3d(&world, bone, point, m.flip_bend_direction),
        (Kind::Fabrik, _) => {
            let chain = chain_of(&world, bone, m.chain_count);
            let mut points = chain_points(&world, &chain, dim3);
            let lengths = segment_lengths(&points);
            fabrik(&mut points, &lengths, point, m.iterations, m.tolerance);
            apply_points(&world, &chain, &points, dim3);
        }
        (Kind::Ccdik, _) => {
            let chain = chain_of(&world, bone, m.chain_count);
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
        1.0 - libm::expf(-(steps as f32) * fixed_dt() / lag)
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
