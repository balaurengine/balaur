//! `character2d`: the 2D half of `crate::character`.
//!
//! Same controller, same properties, one axis fewer. `up_direction` is a `vec2`, and a
//! platformer's is `[0, 1]`.

use crate::rapier2d::control::{
    CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use crate::rapier2d::parry::query::ShapeCastStatus;
use crate::rapier2d::prelude::{
    Collider, ColliderHandle, InteractionGroups, QueryFilter, QueryFilterFlags, RigidBodyHandle,
    SharedShape,
};
use crate::scalar::{self, Pose2, Real, Rotation2, Vector2};
use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Transform, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::character::shared_character_schema;
use crate::dim2::PhysicsState2d;
use crate::vocabulary::{self as v, component as c, keys as k, map};
use balaur_core::fixed_dt;
use glamx::EulerRot;

pub struct Character2d(pub toml::Value);

crate::shared::character::functions!(
    state = PhysicsState2d,
    vector = Vector2,
    pose = Pose2,
    value = Vec2,
    array = a2,
    collider = c::COLLIDER_2D
);

pub(crate) fn move_character(eng: &Engine, entity: Entity, translation: Vector2) -> Result<Value> {
    let params = {
        let world = eng.world();
        let character = world
            .get::<&Character2d>(entity)
            .map_err(|_| anyhow!("node has no character2d"))?;
        character.0.clone()
    };
    let up = scalar::v2a(crate::vocabulary::vec2(
        &params,
        k::UP_DIRECTION,
        [0.0, 1.0],
    ));
    let up = if up.length_squared() < 1.0e-12 {
        Vector2::Y
    } else {
        up.normalize()
    };
    let controller = controller_of(&params, up);
    let push = crate::vocabulary::boolean(&params, k::PUSH_BODIES, true);

    let ignored = ignored_nodes(eng, entity, &params);

    let (movement, collisions) = {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let (shape, pose, groups) = sweep_shape(state, entity)?;
        // Its own layers, never its own body, and nothing `ignore` names, as in 3D.
        let skipped = Skipped::of(&ignored, state);
        let passes = |_: ColliderHandle, collider: &Collider| skipped.passes(collider);
        let filter = QueryFilter::from(ignore_flags(&params))
            .groups(groups)
            .predicate(&passes);
        let mut collisions = Vec::new();
        let movement = controller.move_shape(
            scalar::real(fixed_dt()),
            &state.world.query_pipeline_with_filter(filter),
            shape.as_ref(),
            &pose,
            translation,
            |collision| collisions.push(collision),
        );
        if push && !collisions.is_empty() {
            let mass = push_mass(state, entity, &params);
            let dispatcher = state.world.narrow_phase.query_dispatcher();
            let mut queries = state.world.broad_phase.as_query_pipeline_mut(
                dispatcher,
                &mut state.world.bodies,
                &mut state.world.colliders,
                filter,
            );
            controller.solve_character_collision_impulses(
                scalar::real(fixed_dt()),
                &mut queries,
                shape.as_ref(),
                mass,
                collisions.iter(),
            );
        }
        (movement, collisions)
    };

    if !apply_movement(eng, entity, movement.translation, movement.grounded) {
        return Ok(Value::Nil);
    }
    Ok(map([
        (k::X, Value::Num(f64::from(movement.translation.x))),
        (k::Y, Value::Num(f64::from(movement.translation.y))),
        (k::ON_FLOOR, Value::Bool(movement.grounded)),
        (k::SLIDING, Value::Bool(movement.is_sliding_down_slope)),
        (k::COLLISIONS, collision_list(eng, &collisions)),
    ]))
}

/// Write the effective translation onto the node, and onto its body or its
/// standalone colliders; `false` when the node has no transform to move.
fn apply_movement(eng: &Engine, entity: Entity, translation: Vector2, grounded: bool) -> bool {
    let pose = {
        let world = eng.world();
        let transform = world.get::<&mut Transform>(entity);
        let Ok(mut transform) = transform else {
            return false;
        };
        transform.position.x += scalar::f32_of(translation.x);
        transform.position.y += scalar::f32_of(translation.y);
        // The node's own rotation, not identity: a character authored at an
        // angle would otherwise snap upright the first time it moved.
        let (angle, _, _) = transform.rotation.to_euler(EulerRot::ZYX);
        Pose2::from_parts(
            scalar::v2(transform.position.x, transform.position.y),
            Rotation2::from_angle(scalar::real(angle)),
        )
    };
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    if let Some(handle) = state.bodies.get(&entity).copied() {
        state.world.bodies[handle].set_next_kinematic_position(pose);
    } else {
        // No body: nothing else moves a standalone collider, and a sweep
        // from where the character used to be walks through walls.
        let handles = state.colliders.get(&entity).cloned().unwrap_or_default();
        for handle in handles {
            if let Some(collider) = state.world.colliders.get_mut(handle) {
                collider.set_position(pose);
            }
        }
        state.queries_ready = false;
    }
    state.grounded.insert(entity, grounded);
    true
}

pub(crate) fn install_character2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("move_character", &[c::CHARACTER_2D], "", "Move the character by an offset, sliding along walls, climbing steps and staying on the ground: returns `#{ x, y, on_floor, sliding, collisions }`, each collision carrying the same fields as in 3D. Every solid collider of the character is swept. Call it from fixed_update."),
        ("is_on_floor", &[c::CHARACTER_2D], "", "Whether the last move ended with ground under the character's feet."),
    ]);
    m.function(
        "move_character",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            move_character(eng, entity_of(node)?, scalar::v2(x, y))
        },
    );
    // A reader, as in 3D: a zero-translation sweep would still snap to ground
    // and write the transform, so asking would move the character.
    m.function("is_on_floor", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState2d>();
        let grounded = state.borrow().grounded.get(&entity).copied();
        Ok(grounded.unwrap_or(false))
    });
}

pub(crate) fn register_character2d_component(reg: &mut Registry<'_>) {
    let shared = shared_character_schema();
    let schema = [
        v::schema(&[
            (k::UP_DIRECTION, r#"{ type = "vec2", default = [0.0, 1.0], description = "Which way is up for this character: the axis it stands along and measures slopes against" }"#),
        ]),
        shared,
    ]
    .join("\n");
    reg.register_component(
        c::CHARACTER_2D,
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "A 2D character controller: `physics2d.move_character` slides the node along walls and steps it up ledges. Needs a `collider2d`; a `kinematic` `body2d` lets it push bodies.",
            schema: ComponentDef::parse_schema(c::CHARACTER_2D, &schema),
            tags: &[balaur_core::components::tag::DIM_2D, balaur_core::components::tag::PHYSICS],
            expects: &[c::COLLIDER_2D],
            apply: Box::new(|eng, entity, params| {
                // Moving a character writes the node's transform.
                balaur_core::transform::ensure(eng, entity);
                let _ = eng
                    .world_mut()
                    .insert_one(entity, Character2d(params.clone()));
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Character2d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let character = world.get::<&Character2d>(entity).ok()?;
                Some(character.0.clone())
            }),
        },
    );
}
