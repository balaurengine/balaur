//! Physical bones: a rig walked into bodies and joints, and blended back.
//!
//! Godot's *Create Physical Skeleton*. [`build_2d`] and [`build_3d`] read a
//! rig's bones, and for every bone long enough to hold a shape they spawn a
//! body with a capsule along it, hinged to its parent bone's body. The bodies
//! are ordinary nodes carrying ordinary `body2d` / `collider2d` / `joint2d`
//! components, which is what makes a ragdoll savable, undoable in the editor
//! and visible in the inspector rather than a thing only this module knows
//! about.
//!
//! They are spawned under a container at the scene root rather than under the
//! rig, because physics writes a simulated pose straight into a node's
//! `Transform` and reads it back the same way — that is world space, so a
//! body under a moved parent would drift by the parent's transform every
//! step. The container's own transform is identity and stays that way.
//!
//! [`blend_system`] is the way back: the `ragdoll` component on the rig root
//! carries a weight, and each frame every bone is moved from the pose the
//! clip just wrote toward the pose its body ended up in. At `0` the clip wins
//! outright and the bodies simulate unseen, at `1` the rig is limp, and
//! between the two a hit can push a walk around without ending it.

use anyhow::{Result, anyhow};
use balaur_core::components::{ComponentDef, as_f64};
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, GlobalTransform, Parent, Transform};
use balaur_core::skeleton::{self, Bone};
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};
use glamx::{Quat, Vec3};

use crate::vocabulary::{component as c, keys as k, words as w};

/// The `ragdoll` component's own keys.
pub(crate) const BLEND: &str = "blend";
pub(crate) const BODIES: &str = "bodies";

/// The component a built ragdoll leaves on the rig root.
const RAGDOLL: &str = "ragdoll";

/// The suffix a container node takes, after the rig's own name.
const CONTAINER_SUFFIX: &str = "_ragdoll";

/// A bone shorter than this has no room for a capsule and is skipped: a
/// tip bone with no gizmo length is the usual one.
const MIN_BONE: f32 = 1e-4;

/// How thick a bone's capsule is, as a fraction of its length.
const DEFAULT_THICKNESS: f32 = 0.25;

/// What `ragdoll` was asked to build.
struct Recipe {
    thickness: f32,
    density: f32,
    friction: f32,
    /// The hinge's low and high, in radians; equal values mean no limit.
    limits: [f32; 2],
    blend: f32,
}

impl Recipe {
    fn of(opts: Option<&Value>) -> Self {
        let mut out = Self {
            thickness: DEFAULT_THICKNESS,
            density: 1.0,
            friction: 0.5,
            limits: [0.0, 0.0],
            blend: 1.0,
        };
        let Some(Value::Map(entries)) = opts else {
            return out;
        };
        let number = |v: &Value| match v {
            Value::Num(n) => Some(*n as f32),
            Value::Int(n) => Some(*n as f32),
            _ => None,
        };
        for (key, value) in entries {
            match key.as_str() {
                "thickness" => out.thickness = number(value).unwrap_or(out.thickness).max(0.001),
                k::DENSITY => out.density = number(value).unwrap_or(out.density).max(0.0),
                k::FRICTION => out.friction = number(value).unwrap_or(out.friction).max(0.0),
                BLEND => out.blend = number(value).unwrap_or(out.blend).clamp(0.0, 1.0),
                k::LIMITS => {
                    if let Value::List(pair) = value
                        && pair.len() >= 2
                    {
                        out.limits = [
                            number(&pair[0]).unwrap_or(0.0),
                            number(&pair[1]).unwrap_or(0.0),
                        ];
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// One bone's segment in world space: where it starts, where it ends, and
/// which bone it hangs from.
struct Segment {
    bone: Entity,
    parent: Option<Entity>,
    from: Vec3,
    to: Vec3,
}

impl Segment {
    fn length(&self) -> f32 {
        (self.to - self.from).length()
    }

    fn middle(&self) -> Vec3 {
        (self.from + self.to) * 0.5
    }
}

/// Every bone under `rig` as a world-space segment, tip bones included when
/// their gizmo `length` says how far they reach.
fn segments(eng: &Engine, rig: Entity, dim3: bool) -> Vec<Segment> {
    let world = eng.world();
    let bones = skeleton::bones_under(&world, rig);
    let origin = |e: Entity| {
        world
            .get::<&GlobalTransform>(e)
            .map(|g| g.position)
            .unwrap_or_default()
    };
    let rotation = |e: Entity| {
        world
            .get::<&GlobalTransform>(e)
            .map(|g| g.rotation)
            .unwrap_or(Quat::IDENTITY)
    };
    let child_bone = |e: Entity| {
        world
            .get::<&scene::Children>(e)
            .ok()
            .and_then(|children| {
                children
                    .0
                    .iter()
                    .copied()
                    .find(|&child| world.get::<&Bone>(child).is_ok())
            })
    };
    bones
        .iter()
        .filter_map(|&bone| {
            let from = origin(bone);
            let to = match child_bone(bone) {
                Some(child) => origin(child),
                None => {
                    let b = world.get::<&Bone>(bone).ok()?;
                    if b.length <= MIN_BONE {
                        return None;
                    }
                    // A tip bone points along its gizmo angle in 2D and
                    // along its own x axis in 3D, which is where a rig
                    // authored here and one imported each put it.
                    let dir = if dim3 {
                        rotation(bone) * Vec3::X
                    } else {
                        let (s, c) = (libm::sinf(b.angle), libm::cosf(b.angle));
                        rotation(bone) * Vec3::new(c, s, 0.0)
                    };
                    from + dir * b.length
                }
            };
            let parent = world
                .get::<&Parent>(bone)
                .ok()
                .map(|p| p.0)
                .filter(|&p| bones.contains(&p));
            Some(Segment {
                bone,
                parent,
                from,
                to,
            })
        })
        .filter(|s| s.length() > MIN_BONE)
        .collect()
}

/// Build a 2D ragdoll for the rig at `rig`, answering the body nodes it made.
///
/// # Errors
/// When the node carries no bones, or a component will not apply.
pub fn build_2d(eng: &Engine, rig: Entity, opts: Option<&Value>) -> Result<Vec<Entity>> {
    build(eng, rig, opts, false)
}

/// The 3D twin of [`build_2d`].
///
/// # Errors
/// As [`build_2d`].
pub fn build_3d(eng: &Engine, rig: Entity, opts: Option<&Value>) -> Result<Vec<Entity>> {
    build(eng, rig, opts, true)
}

fn build(eng: &Engine, rig: Entity, opts: Option<&Value>, dim3: bool) -> Result<Vec<Entity>> {
    let recipe = Recipe::of(opts);
    let segments = segments(eng, rig, dim3);
    if segments.is_empty() {
        return Err(anyhow!(
            "this node carries no bones long enough to build a ragdoll from; a tip bone needs a \
             `length` before it has a shape"
        ));
    }
    let rig_name = eng
        .world()
        .get::<&scene::Name>(rig)
        .map(|n| n.0.clone())
        .unwrap_or_default();
    let container = {
        let mut world = eng.world_mut();
        let root = eng.root();
        scene::spawn_node(&mut world, &format!("{rig_name}{CONTAINER_SUFFIX}"), root)
    };
    let mut made = Vec::with_capacity(segments.len());
    // Bodies first, every one of them, and joints after: a joint names the
    // node at its other end, and that node has to exist to be named.
    for segment in &segments {
        let node = spawn_body(eng, container, segment, &recipe, dim3)?;
        made.push((segment.bone, node));
    }
    for segment in &segments {
        let Some(parent) = segment.parent else { continue };
        let (Some(&(_, node)), Some(&(_, other))) = (
            made.iter().find(|(bone, _)| *bone == segment.bone),
            made.iter().find(|(bone, _)| *bone == parent),
        ) else {
            continue;
        };
        add_joint(eng, node, other, segment, &recipe, dim3)?;
    }
    let bodies = relative_path(&eng.world(), rig, container);
    let mut params = toml::map::Map::new();
    params.insert(BLEND.into(), toml::Value::Float(f64::from(recipe.blend)));
    params.insert(BODIES.into(), toml::Value::String(bodies));
    balaur_core::components::add(eng, rig, RAGDOLL, Some(&toml::Value::Table(params)))?;
    Ok(made.into_iter().map(|(_, node)| node).collect())
}

/// One bone's body: a capsule along the segment, at its middle.
fn spawn_body(
    eng: &Engine,
    container: Entity,
    segment: &Segment,
    recipe: &Recipe,
    dim3: bool,
) -> Result<Entity> {
    let name = eng
        .world()
        .get::<&scene::Name>(segment.bone)
        .map(|n| n.0.clone())
        .unwrap_or_else(|_| "Bone".to_string());
    let node = {
        let mut world = eng.world_mut();
        scene::spawn_node(&mut world, &name, container)
    };
    let length = segment.length();
    let along = (segment.to - segment.from) / length;
    // A capsule stands along its own y, so the body is turned to put y on
    // the bone rather than the capsule being described some other way.
    let rotation = Quat::from_rotation_arc(Vec3::Y, along);
    {
        let world = eng.world();
        if let Ok(mut t) = world.get::<&mut Transform>(node) {
            t.position = segment.middle();
            t.rotation = rotation;
        }
    }
    let body = if dim3 { c::BODY_3D } else { c::BODY_2D };
    let collider = if dim3 { c::COLLIDER_3D } else { c::COLLIDER_2D };
    let mut kind = toml::map::Map::new();
    kind.insert(k::KIND.into(), toml::Value::String(w::DYNAMIC.into()));
    balaur_core::components::add(eng, node, body, Some(&toml::Value::Table(kind)))?;
    let mut shape = toml::map::Map::new();
    shape.insert(k::KIND.into(), toml::Value::String(w::CAPSULE.into()));
    shape.insert(
        k::RADIUS.into(),
        toml::Value::Float(f64::from(length * recipe.thickness)),
    );
    // The capsule's height is the straight part between its two caps, so a
    // bone's own length minus what the caps already cover.
    shape.insert(
        k::HEIGHT.into(),
        toml::Value::Float(f64::from((length - 2.0 * length * recipe.thickness).max(0.0))),
    );
    shape.insert(
        k::DENSITY.into(),
        toml::Value::Float(f64::from(recipe.density)),
    );
    shape.insert(
        k::FRICTION.into(),
        toml::Value::Float(f64::from(recipe.friction)),
    );
    balaur_core::components::add(eng, node, collider, Some(&toml::Value::Table(shape)))?;
    Ok(node)
}

/// The hinge holding a bone's body to its parent bone's, anchored where the
/// two actually meet rather than at either body's middle.
fn add_joint(
    eng: &Engine,
    node: Entity,
    other: Entity,
    segment: &Segment,
    recipe: &Recipe,
    dim3: bool,
) -> Result<()> {
    let local = |body: Entity, point: Vec3| -> Vec3 {
        let world = eng.world();
        let Ok(t) = world.get::<&Transform>(body) else {
            return Vec3::ZERO;
        };
        t.rotation.inverse() * (point - t.position)
    };
    let mine = local(node, segment.from);
    let theirs = local(other, segment.from);
    let path = relative_path(&eng.world(), node, other);
    let mut params = toml::map::Map::new();
    let kind = if dim3 { w::SPHERICAL } else { w::REVOLUTE };
    params.insert(k::KIND.into(), toml::Value::String(kind.into()));
    params.insert(k::BODY.into(), toml::Value::String(path));
    params.insert(k::ANCHOR.into(), vector(mine, dim3));
    params.insert(k::OTHER_ANCHOR.into(), vector(theirs, dim3));
    if recipe.limits[0] != recipe.limits[1] {
        params.insert(
            k::LIMITS.into(),
            toml::Value::Array(vec![
                toml::Value::Float(f64::from(recipe.limits[0])),
                toml::Value::Float(f64::from(recipe.limits[1])),
            ]),
        );
    }
    let joint = if dim3 { c::JOINT_3D } else { c::JOINT_2D };
    balaur_core::components::add(eng, node, joint, Some(&toml::Value::Table(params)))
}

fn vector(v: Vec3, dim3: bool) -> toml::Value {
    let mut out = vec![
        toml::Value::Float(f64::from(v.x)),
        toml::Value::Float(f64::from(v.y)),
    ];
    if dim3 {
        out.push(toml::Value::Float(f64::from(v.z)));
    }
    toml::Value::Array(out)
}

/// The path `find_node` walks from `from` to reach `to`: as many `..` as it
/// takes to climb to the two nodes' common ancestor, then the names down.
///
/// Written here rather than taken from `scene` because a ragdoll is the
/// first thing to need one: every other node path in the engine is authored
/// by hand or is absolute from the root.
fn relative_path(world: &balaur_core::hecs::World, from: Entity, to: Entity) -> String {
    let ancestry = |mut e: Entity| {
        let mut chain = vec![e];
        while let Ok(parent) = world.get::<&Parent>(e) {
            e = parent.0;
            chain.push(e);
        }
        chain.reverse();
        chain
    };
    let (here, there) = (ancestry(from), ancestry(to));
    let shared = here
        .iter()
        .zip(&there)
        .take_while(|(a, b)| a == b)
        .count();
    let up = here.len().saturating_sub(shared);
    let down = there[shared..].iter().filter_map(|&e| {
        world.get::<&scene::Name>(e).ok().map(|n| n.0.clone())
    });
    std::iter::repeat_n("..".to_string(), up)
        .chain(down)
        .collect::<Vec<_>>()
        .join("/")
}

/// What a built ragdoll leaves on the rig root.
#[derive(Clone, Debug)]
pub struct Ragdoll {
    /// How much of the simulated pose the bones take, `0..1`.
    pub blend: f32,
    /// Node path to the container holding the bodies, relative to the rig.
    pub bodies: String,
}

pub(crate) fn register_ragdoll_component(reg: &mut Registry<'_>) {
    reg.register_component(
        RAGDOLL,
        ComponentDef {
            doc: "Drives a rig's bones from the bodies `physics2d.ragdoll` or `physics3d.ragdoll` \
                  built for it. `blend` is how much of the simulated pose the bones take: 0 leaves \
                  the clip in charge while the bodies simulate unseen, 1 goes limp, and anything \
                  between lets a hit push an animation around without ending it.",
            schema: ComponentDef::parse_schema(
                RAGDOLL,
                &ComponentDef::schema(&[
                    (BLEND, r#"{ type = "float", default = 1.0, min = 0.0, max = 1.0, description = "How much of the simulated pose the bones take" }"#),
                    (BODIES, r#"{ type = "node", default = "", description = "The node holding the bodies, one per bone, named after it" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::PHYSICS],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let ragdoll = Ragdoll {
                    blend: params
                        .get(BLEND)
                        .and_then(as_f64)
                        .map_or(1.0, |v| (v as f32).clamp(0.0, 1.0)),
                    bodies: params
                        .get(BODIES)
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                };
                eng.world_mut()
                    .insert_one(entity, ragdoll)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Ragdoll>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let r = world.get::<&Ragdoll>(entity).ok()?;
                let mut out = toml::map::Map::new();
                out.insert(BLEND.into(), toml::Value::Float(f64::from(r.blend)));
                out.insert(BODIES.into(), toml::Value::String(r.bodies.clone()));
                Some(toml::Value::Table(out))
            }),
        },
    );
}

/// Move every bone from the pose the clip wrote toward the pose its body
/// ended up in.
///
/// Runs after the step, so the bodies have moved; the bone's `Transform` at
/// this moment is still what the animation system wrote this frame, which is
/// exactly the other end of the blend. A weight of zero writes nothing at
/// all, so an armed but unused ragdoll costs one compare per rig.
pub(crate) fn blend_system(eng: &Engine, _dt: f32) {
    if eng.frozen_root().is_some() {
        return;
    }
    let world = eng.world();
    let rigs: Vec<(Entity, Ragdoll)> = world
        .query::<(Entity, &Ragdoll)>()
        .iter()
        .filter(|(_, r)| r.blend > 0.0 && !r.bodies.is_empty())
        .map(|(e, r)| (e, r.clone()))
        .collect();
    for (rig, ragdoll) in rigs {
        let Some(container) = scene::find_node(&world, rig, &ragdoll.bodies) else {
            tracing::debug!(bodies = ragdoll.bodies, "ragdoll bodies name no node");
            continue;
        };
        for bone in skeleton::bones_under(&world, rig) {
            let Ok(name) = world.get::<&scene::Name>(bone) else {
                continue;
            };
            let Some(body) = scene::find_node(&world, container, &name.0) else {
                continue;
            };
            // Where the body's middle puts the bone's own origin: the body
            // was built centred on the segment, so the origin is half a bone
            // back along the capsule's axis.
            let (Ok(body_global), Ok(bone_global)) = (
                world.get::<&GlobalTransform>(body),
                world.get::<&GlobalTransform>(bone),
            ) else {
                continue;
            };
            let half = (bone_global.position - body_global.position).length();
            let wanted_position = body_global.position + body_global.rotation * Vec3::NEG_Y * half;
            let wanted_rotation = body_global.rotation;
            drop(body_global);
            drop(bone_global);
            drop(name);
            // The bone's local frame: what the parent's world pose leaves.
            let parent = world.get::<&Parent>(bone).ok().map(|p| p.0);
            let (parent_position, parent_rotation) = parent
                .and_then(|p| world.get::<&GlobalTransform>(p).ok())
                .map_or((Vec3::ZERO, Quat::IDENTITY), |g| (g.position, g.rotation));
            let inverse = parent_rotation.inverse();
            let local_position = inverse * (wanted_position - parent_position);
            let local_rotation = inverse * wanted_rotation;
            if let Ok(mut t) = world.get::<&mut Transform>(bone) {
                t.position = t.position.lerp(local_position, ragdoll.blend);
                t.rotation = t.rotation.slerp(local_rotation, ragdoll.blend);
            }
        }
    }
}

/// `ragdoll` and `ragdoll_blend`, on both `physics2d` and `physics3d`.
pub(crate) fn install_ragdoll_api(m: &mut dyn Bindings<Engine>, dim3: bool) {
    let doc = if dim3 {
        "Build a 3D ragdoll from the rig under this node: a body and a capsule per bone, hinged \
         to its parent's. The options table takes `thickness` (capsule radius as a fraction of \
         bone length), `density`, `friction`, `limits` and `blend`. Answers the body nodes it made."
    } else {
        "Build a 2D ragdoll from the rig under this node: a body and a capsule per bone, hinged \
         to its parent's. The options table takes `thickness` (capsule radius as a fraction of \
         bone length), `density`, `friction`, `limits` and `blend`. Answers the body nodes it made."
    };
    m.describe(&[("ragdoll", &[], "(node: node, opts: table) -> list", doc)]);
    m.function(
        "ragdoll",
        move |eng: &Engine, (node, opts): (NodeId, Option<Value>)| {
            let rig = entity_of(node)?;
            let made = if dim3 {
                build_3d(eng, rig, opts.as_ref())?
            } else {
                build_2d(eng, rig, opts.as_ref())?
            };
            Ok(Value::List(
                made.into_iter()
                    .map(|e| Value::Node(balaur_core::node_id_of(e).0))
                    .collect(),
            ))
        },
    );
}

/// `ragdoll_blend`, on `physics` rather than on either dimension: a blend
/// weight means the same thing in both, and a verb spelled twice is a verb
/// a reader has to check twice.
pub(crate) fn install_blend_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "ragdoll_blend",
        &[RAGDOLL],
        "(node: node, weight: float)",
        "How much of the ragdoll's simulated pose the rig's bones take, 0 to 1: 0 leaves the clip \
         in charge while the bodies simulate unseen, 1 goes limp. Tween it to fall over and get \
         back up.",
    )]);
    m.function(
        "ragdoll_blend",
        |eng: &Engine, (node, weight): (NodeId, f32)| {
            let entity = entity_of(node)?;
            let mut params = toml::map::Map::new();
            params.insert(
                BLEND.into(),
                toml::Value::Float(f64::from(weight.clamp(0.0, 1.0))),
            );
            balaur_core::components::patch(eng, entity, RAGDOLL, &toml::Value::Table(params))
        },
    );
}
