//! The `body3d` component and the script calls that drive one.
//!
//! Split from `lib.rs` under `MAX_FILE_LINES`: bodies, colliders and the
//! plugin itself are three subjects, and only the plugin needs all three.

use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine};
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::Vec3;
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, RigidBodyHandle};

use crate::collider::{add_collider, apply_collider, get_collider_params, overlaps};
use crate::{node_pose, PhysicsState};


pub(crate) fn add_body(eng: &Engine, entity: Entity, kind: &str) -> Result<()> {
    let builder = match kind {
        "dynamic" => RigidBodyBuilder::dynamic(),
        "static" => RigidBodyBuilder::fixed(),
        "kinematic" => RigidBodyBuilder::kinematic_position_based(),
        other => return Err(anyhow!("unknown body kind '{other}'")),
    };
    let pose = node_pose(eng, entity)?;
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let builder = if state.sleeping_allowed {
        builder
    } else {
        builder.can_sleep(false)
    };
    let handle = state.world.insert_body(builder.pose(pose));
    state.bodies.insert(entity, handle);
    Ok(())
}


pub(crate) fn with_body<R>(
    eng: &Engine,
    entity: Entity,
    f: impl FnOnce(&mut PhysicsState, RigidBodyHandle) -> R,
) -> Result<R> {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let handle = state
        .bodies
        .get(&entity)
        .copied()
        .ok_or_else(|| anyhow!("node has no rigid body"))?;
    Ok(f(&mut state, handle))
}


pub(crate) fn remove_body_and_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    // Attached colliders die with the body inside rapier.
    state.colliders.swap_remove(&entity);
    if let Some(handle) = state.bodies.swap_remove(&entity) {
        state.world.remove_body(handle);
    }
}


/// Body and collider creation, impulses, velocity access and overlap queries.
pub(crate) fn install_body_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("add_body", &["body3d"], "", "Give the node a rigid body of the given kind (`BODY_DYNAMIC`, `BODY_STATIC`, `BODY_KINEMATIC`)."),
        ("add_ball_collider", &["collider3d"], "", "Attach a sphere collider of the given radius."),
        ("add_cuboid_collider", &["collider3d"], "", "Attach a box collider from its three half-extents."),
        ("apply_impulse", &["body3d"], "", "Add an instant change in momentum, as if the body were struck."),
        ("set_linear_velocity", &["body3d"], "", "Set how fast the body travels, in units per second."),
        ("linear_velocity", &["body3d"], "", "How fast the body is travelling, in units per second."),
        ("overlaps", &["collider3d"], "", "The nodes this one currently intersects; rapier reports a pair only when one of the two colliders is a sensor."),
        ("set_gravity", &[], "", "Set the 3D world's gravity, in units per second squared."),
    ]);
    m.function(
        "add_body",
        |eng: &Engine, (node, kind): (NodeId, String)| add_body(eng, entity_of(node)?, &kind),
    );
    m.function(
        "add_ball_collider",
        |eng: &Engine, (node, radius): (NodeId, f32)| {
            add_collider(eng, entity_of(node)?, ColliderBuilder::ball(radius))
        },
    );
    m.function(
        "add_cuboid_collider",
        |eng: &Engine, (node, hx, hy, hz): (NodeId, f32, f32, f32)| {
            add_collider(eng, entity_of(node)?, ColliderBuilder::cuboid(hx, hy, hz))
        },
    );
    m.function(
        "apply_impulse",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(Vec3::new(x, y, z), true);
            })
        },
    );
    m.function(
        "set_linear_velocity",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_linvel(Vec3::new(x, y, z), true);
            })
        },
    );
    m.function("linear_velocity", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            let v = state.world.bodies[handle].linvel();
            (v.x, v.y, v.z)
        })
    });
    // Sensor pairs only: rapier's narrow phase reports an intersection only
    // when at least one of the two colliders is a sensor.
    m.function("overlaps", |eng: &Engine, node: NodeId| {
        Ok(overlaps(eng, entity_of(node)?)
            .into_iter()
            .map(balaur_core::node_id_of)
            .collect::<Vec<_>>())
    });
    // The 2D world has `physics2d.set_gravity`.
    // No reader by design: add `physics3d.gravity` when a caller needs it back.
    m.function("set_gravity", |eng: &Engine, (x, y, z): (f32, f32, f32)| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().world.gravity = Vec3::new(x, y, z);
        Ok(())
    });
}

/// The `body3d` key. Not backed by a component type: it writes into
/// [`crate::PhysicsState`].
///
/// The `body = "dynamic"` shorthand keeps working via the schema's
/// `shorthand` marker.
pub(crate) fn register_body_component(app: &mut App) {
    app.register_component(
        "body3d",
        ComponentDef {
            doc: "Makes the node a 3D rigid body rapier simulates: `dynamic` falls and responds to forces, `static` never moves, `kinematic` is moved by script or animation and pushes what it meets. On its own a body has no shape; add a `collider3d` for it to collide with anything.",
            schema: ComponentDef::parse_schema(
                "body3d",
                r#"kind = { type = "enum", default = "dynamic", options = ["dynamic", "static", "kinematic"], shorthand = true, description = "How physics drives the node: simulated, immovable, or moved by script" }"#,
            ),
            tags: &["3d", "physics"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("dynamic")
                    .to_string();
                // Recreate the body, preserving any collider.
                let collider = get_collider_params(eng, entity);
                remove_body_and_colliders(eng, entity);
                add_body(eng, entity, &kind)?;
                if let Some(params) = collider {
                    apply_collider(eng, entity, &params)?;
                }
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                // Removing the body keeps the collider, as static geometry.
                let collider = get_collider_params(eng, entity);
                remove_body_and_colliders(eng, entity);
                if let Some(params) = collider {
                    apply_collider(eng, entity, &params)?;
                }
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let state = eng.resource::<PhysicsState>();
                let state = state.borrow();
                let handle = state.bodies.get(&entity)?;
                let kind = match state.world.bodies[*handle].body_type() {
                    rapier3d::prelude::RigidBodyType::Dynamic => "dynamic",
                    rapier3d::prelude::RigidBodyType::Fixed => "static",
                    _ => "kinematic",
                };
                Some(toml::Value::Table(toml::map::Map::from_iter([(
                    "kind".to_string(),
                    toml::Value::String(kind.to_string()),
                )])))
            }),
        },
    );
}
