//! Rig modifiers: a bone aimed at a target after the clip has posed it.
//!
//! Godot's `SkeletonModification2D`, as one component. `look_at` turns a
//! bone so its child points at a node; `two_bone_ik` bends a root, middle,
//! tip chain so the tip reaches one — the analytic two-bone solve, with
//! `flip` choosing which way the elbow goes. Both run in `Stage::Update`
//! after the animation system, so a clip poses the rig and a modifier has
//! the last word, every frame, from the transforms as they are now (composed
//! from locals, never last frame's globals). Every transcendental is
//! `libm`'s, so the pose is the same on every platform.

use crate::keys as k;
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{self, Children, Parent, Transform};
use balaur_core::skeleton::{Bone, affine_2d, quat_about_z};
use balaur_plugin::Registry;
use glamx::{Mat3, Vec2};

/// The shortest bone [`two_bone_ik`] will solve. Below it the reach clamp
/// inverts and there is no elbow angle to find anyway.
const MIN_BONE: f32 = 1e-5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    LookAt,
    TwoBoneIk,
}

/// What the `modifier2d` component wrote on the node.
#[derive(Clone, Debug)]
pub struct Modifier2d {
    kind: Kind,
    /// Node path to the point to aim at, relative to the node.
    target: String,
    /// Node path to the driven bone, relative to the node; empty is the node.
    bone: String,
    flip: bool,
    enabled: bool,
}

/// The two modifiers, written once for the schema, the matcher and the
/// read-back.
const LOOK_AT: &str = "look_at";
const TWO_BONE_IK: &str = "two_bone_ik";

fn schema() -> String {
    ComponentDef::schema(&[
        (
            k::KIND,
            &format!(
                r#"{{ type = "enum", default = "{}", options = ["{}", "{}"], description = "Aim one bone at the target, or bend a root, middle, tip chain so the tip reaches it" }}"#,
                LOOK_AT, LOOK_AT, TWO_BONE_IK
            ),
        ),
        (
            k::TARGET,
            r#"{ type = "string", default = "", description = "Node path to the point to aim at, relative to this node" }"#,
        ),
        (
            k::BONE,
            r#"{ type = "string", default = "", description = "Node path to the driven bone, relative to this node; empty means this node. For two_bone_ik, the root of the chain" }"#,
        ),
        (
            k::FLIP,
            r#"{ type = "bool", default = false, description = "Bend the two-bone chain the other way" }"#,
        ),
        (
            k::ENABLED,
            r#"{ type = "bool", default = true, description = "Whether the modifier runs; off leaves the clip's pose alone" }"#,
        ),
    ])
}

/// The `modifier2d` component: writes one [`Modifier2d`] on the node.
pub(crate) fn register_modifier2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "modifier2d",
        ComponentDef {
            doc: "Aims a 2D bone at a target node every frame, after the clip has posed the rig: \
                  `look_at` turns one bone toward the target, `two_bone_ik` bends a root, middle \
                  and tip chain so the tip reaches it.",
            schema: ComponentDef::parse_schema("modifier2d", &schema()),
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::ANIMATION,
            ],
            expects: &[],
            apply: Box::new(apply_modifier2d),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Modifier2d>(entity);
                Ok(())
            }),
            get: Box::new(modifier2d_of),
        },
    );
}

fn apply_modifier2d(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
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
    let kind = match params.get(k::KIND).and_then(toml::Value::as_str) {
        None | Some(LOOK_AT) => Kind::LookAt,
        Some(TWO_BONE_IK) => Kind::TwoBoneIk,
        Some(other) => return Err(anyhow!("unknown modifier2d kind '{other}'")),
    };
    let modifier = Modifier2d {
        kind,
        target: text(k::TARGET),
        bone: text(k::BONE),
        flip: flag(k::FLIP, false),
        enabled: flag(k::ENABLED, true),
    };
    eng.world_mut()
        .insert_one(entity, modifier)
        .map_err(|_| anyhow!("node is dead"))
}

fn modifier2d_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let m = world.get::<&Modifier2d>(entity).ok()?;
    let mut out = toml::map::Map::new();
    let kind = match m.kind {
        Kind::LookAt => LOOK_AT,
        Kind::TwoBoneIk => TWO_BONE_IK,
    };
    out.insert(k::KIND.into(), toml::Value::String(kind.into()));
    out.insert(k::TARGET.into(), toml::Value::String(m.target.clone()));
    out.insert(k::BONE.into(), toml::Value::String(m.bone.clone()));
    out.insert(k::FLIP.into(), toml::Value::Boolean(m.flip));
    out.insert(k::ENABLED.into(), toml::Value::Boolean(m.enabled));
    Some(toml::Value::Table(out))
}

/// A node's world pose composed from local transforms, so a bone this frame
/// has already moved sees the move.
fn pose_2d(world: &World, entity: Entity) -> Mat3 {
    let mut chain = vec![entity];
    let mut current = entity;
    while let Ok(parent) = world.get::<&Parent>(current) {
        current = parent.0;
        chain.push(current);
    }
    let mut matrix = Mat3::IDENTITY;
    for e in chain.into_iter().rev() {
        if let Ok(t) = world.get::<&Transform>(e) {
            matrix *= affine_2d(t.position, t.rotation, t.scale);
        }
    }
    matrix
}

fn angle_of(m: &Mat3) -> f32 {
    libm::atan2f(m.x_axis.y, m.x_axis.x)
}

fn origin_of(m: &Mat3) -> Vec2 {
    Vec2::new(m.z_axis.x, m.z_axis.y)
}

fn first_child_bone(world: &World, entity: Entity) -> Option<Entity> {
    let children = world.get::<&Children>(entity).ok()?;
    children
        .0
        .iter()
        .copied()
        .find(|&child| world.get::<&Bone>(child).is_ok())
}

/// The direction a bone points along in its own frame: toward its first
/// child bone, else its gizmo `angle`.
fn aim_local(world: &World, bone: Entity) -> f32 {
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
    let local = wanted - aim_local(world, bone) - parent_angle;
    if let Ok(mut t) = world.get::<&mut Transform>(bone) {
        t.rotation = quat_about_z(local);
    }
}

fn look_at(world: &World, bone: Entity, target: Vec2) {
    let to = target - origin_of(&pose_2d(world, bone));
    aim_at_angle(world, bone, libm::atan2f(to.y, to.x));
}

/// The analytic two-bone solve: the root turns to put the middle joint on
/// the circle both segments can reach, then the middle turns to put the tip
/// on the target. Out of reach, the chain straightens toward it.
fn two_bone_ik(world: &World, root: Entity, target: Vec2, flip: bool) {
    let (Some(mid), Some(tip)) = (
        first_child_bone(world, root),
        first_child_bone(world, root).and_then(|mid| first_child_bone(world, mid)),
    ) else {
        tracing::debug!("two_bone_ik needs a root, middle and tip bone");
        return;
    };
    let (r, m, t) = (
        origin_of(&pose_2d(world, root)),
        origin_of(&pose_2d(world, mid)),
        origin_of(&pose_2d(world, tip)),
    );
    let l1 = (m - r).length();
    let l2 = (t - m).length();
    // The clamp below has `min > max` for anything shorter, and `f32::clamp`
    // panics on that: a bone 5e-6 from its parent is what reaches it.
    if !(l1 > MIN_BONE && l2 > MIN_BONE) {
        return;
    }
    let to = target - r;
    let d = to.length().clamp((l1 - l2).abs() + 1e-5, l1 + l2 - 1e-5);
    let base = libm::atan2f(to.y, to.x);
    let sign = if flip { -1.0 } else { 1.0 };
    let cos_root = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
    let cos_mid = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0);
    let root_aim = base + sign * libm::acosf(cos_root);
    let mid_aim = root_aim - sign * (std::f32::consts::PI - libm::acosf(cos_mid));
    aim_at_angle(world, root, root_aim);
    aim_at_angle(world, mid, mid_aim);
}

/// Every modifier, in a fixed order, from the transforms as they are now.
pub(crate) fn modify_system(eng: &Engine, _dt: f32) {
    // Nothing has moved under a held game, so there is nothing to re-pose.
    if eng.frozen_root().is_some() {
        return;
    }
    let world = eng.world();
    let mut modifiers: Vec<(Entity, Modifier2d)> = world
        .query::<(Entity, &Modifier2d)>()
        .iter()
        .filter(|(_, m)| m.enabled && !m.target.is_empty())
        .map(|(e, m)| (e, m.clone()))
        .collect();
    // Entity order is reproducible for one binary; a set order is what makes
    // two modifiers on one bone land the same way twice.
    modifiers.sort_by_key(|(e, _)| e.to_bits());
    for (entity, m) in modifiers {
        let Some(target) = scene::find_node(&world, entity, &m.target) else {
            tracing::debug!(target = m.target, "modifier2d target names no node");
            continue;
        };
        let bone = if m.bone.is_empty() {
            Some(entity)
        } else {
            scene::find_node(&world, entity, &m.bone)
        };
        let Some(bone) = bone else {
            tracing::debug!(bone = m.bone, "modifier2d bone names no node");
            continue;
        };
        let point = origin_of(&pose_2d(&world, target));
        // A target whose transform went non-finite would write NaN rotations
        // into the rig and keep them there; the clip's pose stands instead.
        if !point.x.is_finite() || !point.y.is_finite() {
            tracing::debug!(target = m.target, "modifier2d target is not a finite point");
            continue;
        }
        match m.kind {
            Kind::LookAt => look_at(&world, bone, point),
            Kind::TwoBoneIk => two_bone_ik(&world, bone, point, m.flip),
        }
    }
}

/// Where the modifier's target is, for a tool that draws the reach.
#[must_use]
pub fn target_of(eng: &Engine, entity: Entity) -> Option<Vec2> {
    let world = eng.world();
    let m = world.get::<&Modifier2d>(entity).ok()?;
    let target = scene::find_node(&world, entity, &m.target)?;
    Some(origin_of(&pose_2d(&world, target)))
}
