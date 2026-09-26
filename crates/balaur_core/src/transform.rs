//! The `transform` component: a node's local position, rotation and scale.
//!
//! The data is [`crate::scene::Transform`], which the tree has carried since
//! before the component registry existed. Registering it buys what every
//! other component gets: a `[nodes.transform]` scene key, `node.transform` in
//! a script, a generated inspector section, and a node that may have none.

use anyhow::{Result, anyhow};
use glamx::{EulerRot, Quat, Vec3};
use hecs::Entity;

use crate::app::App;
use crate::components::ComponentDef;
use crate::scene::Transform;

/// The component's name, and the scene key that applies it.
pub const COMPONENT: &str = "transform";

pub(crate) mod k {
    pub(crate) const POSITION: &str = "position";
    pub(crate) const ROTATION_EULER: &str = "rotation_euler";
    pub(crate) const SCALE: &str = "scale";
    pub(crate) const SKEW: &str = "skew";
}

fn schema() -> String {
    ComponentDef::schema(&[
        (
            k::POSITION,
            r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where the node sits in its parent's space" }"#,
        ),
        // Radians on the file and in a script, degrees in the inspector: the
        // `unit` key is what a row is drawn in, not what anything stores.
        (
            k::ROTATION_EULER,
            r#"{ type = "vec3", default = [0.0, 0.0, 0.0], unit = "degrees", description = "Local rotation as euler angles in radians, x then y then z" }"#,
        ),
        (
            k::SCALE,
            r#"{ type = "vec3", default = [1.0, 1.0, 1.0], description = "Size relative to the parent's" }"#,
        ),
        (
            k::SKEW,
            r#"{ type = "float", default = 0.0, unit = "degrees", description = "A 2D shear in radians: how far the y axis leans past square with the x axis; children lean with it" }"#,
        ),
    ])
}

/// Euler angles in the order a scene file writes them, which is the order
/// `node.rotation_euler()` answers in.
fn euler_of(rotation: Quat) -> [f32; 3] {
    let (yaw, pitch, roll) = rotation.to_euler(EulerRot::ZYX);
    [roll, pitch, yaw]
}

fn rotation_of(euler: Vec3) -> Quat {
    Quat::from_euler(EulerRot::ZYX, euler.z, euler.y, euler.x)
}

fn transform_of(params: &toml::Value) -> Transform {
    let vec3 = |key| Vec3::from_array(crate::components::prop_vec3(params, key));
    Transform {
        position: vec3(k::POSITION),
        rotation: rotation_of(vec3(k::ROTATION_EULER)),
        scale: vec3(k::SCALE),
        skew: crate::components::prop_f32(params, k::SKEW),
    }
}

fn apply(eng: &crate::Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let next = transform_of(params);
    let mut world = eng.world_mut();
    if let Ok(mut held) = world.get::<&mut Transform>(entity) {
        *held = next;
        return Ok(());
    }
    world
        .insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))
}

fn get(eng: &crate::Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let held = world.get::<&Transform>(entity).ok()?;
    let numbers = |v: [f32; 3]| {
        toml::Value::Array(
            v.iter()
                .map(|n| toml::Value::Float(f64::from(*n)))
                .collect(),
        )
    };
    let mut out = toml::map::Map::new();
    out.insert(k::POSITION.into(), numbers(held.position.to_array()));
    out.insert(k::ROTATION_EULER.into(), numbers(euler_of(held.rotation)));
    out.insert(k::SCALE.into(), numbers(held.scale.to_array()));
    out.insert(k::SKEW.into(), toml::Value::Float(f64::from(held.skew)));
    Some(toml::Value::Table(out))
}

/// One property, without building the other three: what a script reading
/// `node.transform.position` every frame asks for.
fn read_property(eng: &crate::Engine, entity: Entity, key: &str) -> Option<toml::Value> {
    let world = eng.world();
    let held = world.get::<&Transform>(entity).ok()?;
    let numbers = |v: [f32; 3]| {
        toml::Value::Array(
            v.iter()
                .map(|n| toml::Value::Float(f64::from(*n)))
                .collect(),
        )
    };
    match key {
        k::POSITION => Some(numbers(held.position.to_array())),
        k::ROTATION_EULER => Some(numbers(euler_of(held.rotation))),
        k::SCALE => Some(numbers(held.scale.to_array())),
        k::SKEW => Some(toml::Value::Float(f64::from(held.skew))),
        _ => None,
    }
}

/// One property written into the live transform. `false` is a node without
/// one, or a value the schema would have refused.
fn write_property(eng: &crate::Engine, entity: Entity, key: &str, value: &toml::Value) -> bool {
    let vec3 = || {
        let list = value.as_array()?;
        let [x, y, z] = list.as_slice() else {
            return None;
        };
        let number = |v: &toml::Value| crate::components::as_f64(v).map(|n| n as f32);
        Some(Vec3::new(number(x)?, number(y)?, number(z)?))
    };
    let world = eng.world_mut();
    let Ok(mut held) = world.get::<&mut Transform>(entity) else {
        return false;
    };
    match key {
        k::POSITION => match vec3() {
            Some(v) => held.position = v,
            None => return false,
        },
        k::ROTATION_EULER => match vec3() {
            Some(v) => held.rotation = rotation_of(v),
            None => return false,
        },
        k::SCALE => match vec3() {
            Some(v) => held.scale = v,
            None => return false,
        },
        k::SKEW => match crate::components::as_f64(value) {
            Some(n) => held.skew = n as f32,
            None => return false,
        },
        _ => return false,
    }
    true
}

/// Give `entity` a transform if it has none, for a component that poses the
/// node it is on.
///
/// A body, a bone or a character writes the node's transform every step, so
/// one attached to a node the scene left bare would move something that is not
/// there. Called from those components' `apply`, which is the moment the node
/// acquires a reason to have one.
pub fn ensure(eng: &crate::Engine, entity: Entity) {
    let mut world = eng.world_mut();
    if world.get::<&Transform>(entity).is_ok() {
        return;
    }
    let _ = world.insert_one(entity, Transform::identity());
    crate::components::mark_present(&mut world, entity, crate::components::TRANSFORM_BIT);
}

pub(crate) fn register_transform_component(app: &mut App) {
    crate::components::answers_property(&app.engine, COMPONENT, Box::new(read_property));
    crate::components::writes_property(&app.engine, COMPONENT, Box::new(write_property));
    app.register_component(
        COMPONENT,
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "The node's `position`, `rotation_euler`, `scale` and `skew` in its parent's space. A node without one sits at its parent.",
            schema: ComponentDef::parse_schema(COMPONENT, &schema()),
            tags: &["2d", "3d"],
            expects: &[],
            apply: Box::new(apply),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Transform>(entity);
                Ok(())
            }),
            get: Box::new(get),
        },
    );
}
