//! The `cloner` component: a node drawn many times over, in one call.
//!
//! Where the copies go is `balaur_core::cloner`, which knows nothing about
//! drawing. What this adds is the tree: the cloner's whole subtree is the
//! template, and every node in it is given the world poses its copies draw
//! at. Physics, scripts and the outliner still see one node.

use anyhow::{Result, anyhow};
use balaur_core::cloner::{Clone3d, Cloner, MAX_CLONES, Mode, keys as ck, words as cw};
use balaur_core::components::{ComponentDef, as_f64};
use balaur_core::hecs::Entity;
use balaur_core::scene::{GlobalTransform, collect_subtree};
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};
use glamx::{EulerRot, Mat4, Quat, Vec3};

use crate::{Renderable2d, Renderable3d};

/// Where a node draws its copies: one world matrix and tint each. A laid-out
/// cloner's first copy is where the node already is; an empty list draws
/// nothing.
///
/// Written onto every drawn node under a cloner once the poses have settled
/// for the tick, and read by a backend, which turns them into instances.
pub struct Clones(pub Vec<Placed>);

/// Whether a node's cloner lists no copies, so the node draws nothing.
#[cfg(feature = "window")]
pub(crate) fn emptied(world: &balaur_core::hecs::World, entity: Entity) -> bool {
    world
        .get::<&Clones>(entity)
        .is_ok_and(|clones| clones.0.is_empty())
}

/// One copy of a drawn node, in the world.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub at: Mat4,
    pub tint: [f32; 4],
}

/// The number a params table holds at `key`.
fn number(params: &toml::Value, key: &str, fallback: f32) -> f32 {
    params
        .get(key)
        .and_then(as_f64)
        .map_or(fallback, |v| v as f32)
}

/// A three-number array, or the fallback for each slot it does not carry.
fn triple(params: &toml::Value, key: &str, fallback: [f32; 3]) -> [f32; 3] {
    let mut out = fallback;
    if let Some(row) = params.get(key).and_then(toml::Value::as_array) {
        for (slot, value) in out.iter_mut().zip(row) {
            if let Some(number) = as_f64(value) {
                *slot = number as f32;
            }
        }
    }
    out
}

/// A listed copy as a table holds it, the `transform` component's keys and a
/// tint; a missing key is the identity's.
fn copy_from_table(table: &toml::Value) -> Clone3d {
    let euler = triple(table, ck::ROTATION_EULER, [0.0; 3]);
    let tint = table
        .get(ck::TINT)
        .and_then(toml::Value::as_array)
        .map_or([1.0; 4], |row| {
            let mut out = [1.0; 4];
            for (slot, value) in out.iter_mut().zip(row) {
                if let Some(number) = as_f64(value) {
                    *slot = number as f32;
                }
            }
            out
        });
    Clone3d {
        position: Vec3::from_array(triple(table, ck::POSITION, [0.0; 3])),
        rotation: Quat::from_euler(EulerRot::ZYX, euler[2], euler[1], euler[0]),
        scale: Vec3::from_array(triple(table, ck::SCALE, [1.0; 3])),
        tint,
    }
}

fn copy_to_table(copy: &Clone3d) -> toml::Value {
    let floats = |values: &[f32]| {
        toml::Value::Array(
            values
                .iter()
                .map(|v| toml::Value::Float(f64::from(*v)))
                .collect(),
        )
    };
    let (yaw, pitch, roll) = copy.rotation.to_euler(EulerRot::ZYX);
    let mut map = toml::map::Map::new();
    map.insert(ck::POSITION.into(), floats(&copy.position.to_array()));
    map.insert(ck::ROTATION_EULER.into(), floats(&[roll, pitch, yaw]));
    map.insert(ck::SCALE.into(), floats(&copy.scale.to_array()));
    map.insert(ck::TINT.into(), floats(&copy.tint));
    toml::Value::Table(map)
}

/// The cloner a params table describes.
fn cloner_from_params(params: &toml::Value) -> Result<Cloner> {
    let word = params
        .get(ck::KIND)
        .and_then(toml::Value::as_str)
        .unwrap_or(cw::LINEAR);
    let mode = Mode::from_word(word).ok_or_else(|| anyhow!("unknown cloner mode '{word}'"))?;
    let counts = triple(params, ck::COUNTS, [3.0, 1.0, 3.0]);
    Ok(Cloner {
        mode,
        count: number(params, ck::COUNT, 4.0).max(1.0) as u32,
        counts: counts.map(|n| n.max(1.0) as u32),
        step: Vec3::from_array(triple(params, ck::STEP, [1.0, 0.0, 0.0])),
        radius: number(params, ck::RADIUS, 2.0),
        angle: number(params, ck::ANGLE_DEGREES, 0.0),
        seed: number(params, ck::SEED, 0.0).max(0.0) as u64,
        random: number(params, ck::RANDOM, 0.0).clamp(0.0, 1.0),
        copies: params
            .get(ck::COPIES)
            .and_then(toml::Value::as_array)
            .map(|rows| rows.iter().take(MAX_CLONES).map(copy_from_table).collect())
            .unwrap_or_default(),
    })
}

/// A cloner read back as its params.
fn cloner_to_params(cloner: &Cloner) -> toml::Value {
    let float = |v: f32| toml::Value::Float(f64::from(v));
    let integer = |v: u32| toml::Value::Integer(i64::from(v));
    let mut map = toml::map::Map::new();
    map.insert(
        ck::KIND.into(),
        toml::Value::String(cloner.mode.word().into()),
    );
    map.insert(ck::COUNT.into(), integer(cloner.count));
    map.insert(
        ck::COUNTS.into(),
        toml::Value::Array(cloner.counts.iter().map(|n| integer(*n)).collect()),
    );
    map.insert(
        ck::STEP.into(),
        toml::Value::Array(cloner.step.to_array().iter().map(|v| float(*v)).collect()),
    );
    map.insert(ck::RADIUS.into(), float(cloner.radius));
    map.insert(ck::ANGLE_DEGREES.into(), float(cloner.angle));
    map.insert(ck::SEED.into(), integer(cloner.seed as u32));
    map.insert(ck::RANDOM.into(), float(cloner.random));
    map.insert(
        ck::COPIES.into(),
        toml::Value::Array(cloner.copies.iter().map(copy_to_table).collect()),
    );
    toml::Value::Table(map)
}

/// The `cloner` component: draws the node's subtree many times over.
pub(crate) fn register_cloner_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "cloner",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "Draws the node's subtree many times; physics and scripts still see one node. `kind` is `linear`, `radial` or `grid`, or `list` for the `copies` a scene or a script places and tints one by one; `seed` and `random` scatter the copies.",
            schema: ComponentDef::parse_schema(
                "cloner",
                &ComponentDef::schema(&[
                    (ck::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How the copies are laid out" }}"#, cw::LINEAR, crate::vocabulary::options(cw::KINDS))),
                    (ck::COUNT, r#"{ type = "int", default = 4, min = 1, description = "How many copies, when kind is linear or radial" }"#),
                    (ck::COUNTS, r#"{ type = "vec3", default = [3, 1, 3], description = "How many along each axis, when kind is grid" }"#),
                    (ck::STEP, r#"{ type = "vec3", default = [1.0, 0.0, 0.0], description = "The gap between copies, when kind is linear or grid" }"#),
                    (ck::RADIUS, r#"{ type = "float", default = 2.0, description = "How far out the ring sits, when kind is radial" }"#),
                    (ck::ANGLE_DEGREES, r#"{ type = "float", default = 0.0, description = "Degrees between copies on a ring; zero closes the ring evenly" }"#),
                    (ck::SEED, r#"{ type = "int", default = 0, min = 0, description = "The seed the scatter runs off; zero scatters nothing" }"#),
                    (ck::RANDOM, r#"{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "How far a copy may wander in position, turn and size" }"#),
                    (ck::COPIES, r#"{ type = "list", of = { type = "record", fields = { position = { type = "vec3", default = [0.0, 0.0, 0.0] }, rotation_euler = { type = "vec3", default = [0.0, 0.0, 0.0] }, scale = { type = "vec3", default = [1.0, 1.0, 1.0] }, tint = { type = "color", default = [1.0, 1.0, 1.0, 1.0] } } }, default = [], description = "The copies, when kind is list: each placed in the node's own space with the transform component's keys, and tinted over the node's colour. An empty list draws nothing" }"#),
                ]),
            ),
            tags: &["render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let cloner = cloner_from_params(params)?;
                let mut world = eng.world_mut();
                if let Ok(mut existing) = world.get::<&mut Cloner>(entity) {
                    *existing = cloner;
                    return Ok(());
                }
                world
                    .insert_one(entity, cloner)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Cloner>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let cloner = world.get::<&Cloner>(entity).ok()?;
                Some(cloner_to_params(&cloner))
            }),
        },
    );
}

fn matrix(at: &GlobalTransform) -> Mat4 {
    Mat4::from_scale_rotation_translation(at.scale, at.rotation, at.position)
}

/// Give every drawn node under a cloner the world poses its copies sit at.
///
/// A copy's transform is written in the cloner's own space, so turning or
/// scaling the cloner turns and scales the whole arrangement with it.
pub(crate) fn resolve_cloners_system(eng: &Engine, _dt: f32) {
    let owners: Vec<(Entity, Cloner)> = {
        let world = eng.world();
        let mut owners = Vec::new();
        for (entity, cloner) in &mut world.query::<(Entity, &Cloner)>() {
            owners.push((entity, cloner.clone()));
        }
        owners
    };
    let mut written: Vec<(Entity, Vec<Placed>)> = Vec::new();
    for (owner, cloner) in owners {
        let clones = cloner.clones();
        let world = eng.world();
        let Ok(at) = world.get::<&GlobalTransform>(owner) else {
            continue;
        };
        let cloner_pose = matrix(&at);
        let inverse = cloner_pose.inverse();
        let placements: Vec<(Mat4, [f32; 4])> = clones
            .iter()
            .map(|clone| {
                let local = Mat4::from_scale_rotation_translation(
                    clone.scale,
                    clone.rotation,
                    clone.position,
                );
                (cloner_pose * local * inverse, clone.tint)
            })
            .collect();
        for entity in collect_subtree(&world, owner) {
            let drawn = world.get::<&Renderable3d>(entity).is_ok()
                || world.get::<&Renderable2d>(entity).is_ok();
            if !drawn {
                continue;
            }
            let Ok(pose) = world.get::<&GlobalTransform>(entity) else {
                continue;
            };
            let here = matrix(&pose);
            let placed = placements
                .iter()
                .map(|(a, tint)| Placed {
                    at: *a * here,
                    tint: *tint,
                })
                .collect();
            written.push((entity, placed));
        }
    }
    let cloned: Vec<Entity> = written.iter().map(|(entity, _)| *entity).collect();
    let mut world = eng.world_mut();
    for (entity, placements) in written {
        if let Ok(mut existing) = world.get::<&mut Clones>(entity) {
            existing.0 = placements;
        } else {
            let _ = world.insert_one(entity, Clones(placements));
        }
    }
    // A node whose cloner was removed goes back to drawing once.
    let mut stale: Vec<Entity> = Vec::new();
    for (entity, _) in &mut world.query::<(Entity, &Clones)>() {
        if !cloned.contains(&entity) {
            stale.push(entity);
        }
    }
    for entity in stale {
        let _ = world.remove_one::<Clones>(entity);
    }
}

/// A copy a script hands over, with the keys a listed copy takes.
fn copy_from_value(value: &Value) -> Clone3d {
    let Value::Map(fields) = value else {
        return Clone3d::default();
    };
    let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let three = |key: &str, fallback: [f32; 3]| match field(key) {
        Some(Value::Vec3(v)) => *v,
        Some(Value::Vec2([x, y])) => [*x, *y, fallback[2]],
        Some(Value::List(items)) => {
            let mut out = fallback;
            for (slot, item) in out.iter_mut().zip(items) {
                if let Value::Num(n) = item {
                    *slot = *n as f32;
                } else if let Value::Int(n) = item {
                    *slot = *n as f32;
                }
            }
            out
        }
        _ => fallback,
    };
    let euler = three(ck::ROTATION_EULER, [0.0; 3]);
    let tint = match field(ck::TINT) {
        Some(Value::Color(c)) => *c,
        Some(list @ Value::List(_)) => crate::draw_2d::color_of(list).unwrap_or([1.0; 4]),
        _ => [1.0; 4],
    };
    Clone3d {
        position: Vec3::from_array(three(ck::POSITION, [0.0; 3])),
        rotation: Quat::from_euler(EulerRot::ZYX, euler[2], euler[1], euler[0]),
        scale: Vec3::from_array(three(ck::SCALE, [1.0; 3])),
        tint,
    }
}

/// The script surface: where a cloner's copies sit, so an editor's "bake to
/// nodes" can spawn real children at them, and one listed copy set in place.
pub(crate) fn install_cloner_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "clones",
            &["cloner"],
            "(node: node) -> list",
            "Where the node's cloner puts each copy, in the node's own space, as `#{ position, rotation, scale }`; an empty list when the node has no cloner. What a bake-to-nodes command spawns from.",
        ),
        (
            "set_copy",
            &["cloner"],
            "(node: node, index: int, copy: table)",
            "Place and tint one listed copy, `#{ position, rotation_euler, scale, tint }`, without writing the whole list: the list grows with plain copies up to `index`. What a script moving every copy each frame calls.",
        ),
    ]);
    m.function(
        "set_copy",
        |eng: &Engine, (node, index, copy): (NodeId, i64, Value)| {
            let index =
                usize::try_from(index).map_err(|_| anyhow!("a copy's index is 0 or more"))?;
            if index >= MAX_CLONES {
                return Err(anyhow!("a cloner holds at most {MAX_CLONES} copies"));
            }
            let entity = entity_of(node)?;
            let world = eng.world_mut();
            let mut cloner = world
                .get::<&mut Cloner>(entity)
                .map_err(|_| anyhow!("the node has no cloner"))?;
            if cloner.copies.len() <= index {
                cloner.copies.resize(index + 1, Clone3d::default());
            }
            cloner.copies[index] = copy_from_value(&copy);
            Ok(())
        },
    );
    m.function("clones", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let Ok(cloner) = world.get::<&Cloner>(entity_of(node)?) else {
            return Ok(Value::List(Vec::new()));
        };
        Ok(Value::List(
            cloner
                .clones()
                .into_iter()
                .map(|clone| {
                    let (axis, angle) = clone.rotation.to_axis_angle();
                    Value::Map(vec![
                        (
                            "position".to_string(),
                            Value::Vec3(clone.position.to_array()),
                        ),
                        ("axis".to_string(), Value::Vec3(axis.to_array())),
                        (
                            "angle".to_string(),
                            Value::Num(f64::from(angle.to_degrees())),
                        ),
                        ("scale".to_string(), Value::Vec3(clone.scale.to_array())),
                    ])
                })
                .collect(),
        ))
    });
}
