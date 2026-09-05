//! `character2d`: the 2D half of `crate::character`.
//!
//! Same controller, same properties, one axis fewer. `up` is a `vec2`, and a
//! platformer's is `[0, 1]`.

use crate::rapier2d::control::{
    CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use crate::rapier2d::prelude::QueryFilter;
use crate::scalar::{self, Pose2, Rotation2, Vector2};
use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Transform, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::FIXED_DT;
use crate::character::shared_character_schema;
use crate::dim2::PhysicsState2d;
use crate::vocabulary::{self as v, component as c, keys as k, map};
use glamx::EulerRot;

pub struct Character2d(pub toml::Value);

crate::shared::character::functions!(
    state = PhysicsState2d,
    vector = Vector2,
    value = Vec2,
    array = a2
);

pub(crate) fn move_character(eng: &Engine, entity: Entity, translation: Vector2) -> Result<Value> {
    let params = {
        let world = eng.world();
        let character = world
            .get::<&Character2d>(entity)
            .map_err(|_| anyhow!("node has no character2d"))?;
        character.0.clone()
    };
    let up = scalar::v2a(crate::vocabulary::vec2(&params, k::UP, [0.0, 1.0]));
    let up = if up.length_squared() < 1.0e-12 {
        Vector2::Y
    } else {
        up.normalize()
    };
    let controller = controller_of(&params, up);
    let push = crate::vocabulary::boolean(&params, k::PUSH_BODIES, true);

    let (movement, collisions) = {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let handle = crate::dim2::collider::first_collider(state, entity)
            .map_err(|_| anyhow!("a character needs a collider2d to move with"))?;
        let (shape, pose) = {
            let collider = &state.world.colliders[handle];
            (collider.shared_shape().clone(), *collider.position())
        };
        let mut collisions = Vec::new();
        let filter = QueryFilter::default().exclude_collider(handle);
        let movement = controller.move_shape(
            scalar::real(FIXED_DT),
            &state.world.query_pipeline_with_filter(filter),
            shape.as_ref(),
            &pose,
            translation,
            |collision| collisions.push(collision),
        );
        if push && !collisions.is_empty() {
            let mass = state
                .bodies
                .get(&entity)
                .map_or(1.0, |body| state.world.bodies[*body].mass().max(1.0));
            let dispatcher = state.world.narrow_phase.query_dispatcher();
            let mut queries = state.world.broad_phase.as_query_pipeline_mut(
                dispatcher,
                &mut state.world.bodies,
                &mut state.world.colliders,
                filter,
            );
            controller.solve_character_collision_impulses(
                scalar::real(FIXED_DT),
                &mut queries,
                shape.as_ref(),
                mass,
                collisions.iter(),
            );
        }
        (movement, collisions)
    };

    let pose = {
        let world = eng.world();
        let transform = world.get::<&mut Transform>(entity);
        let Ok(mut transform) = transform else {
            return Ok(Value::Nil);
        };
        transform.position.x += scalar::f32_of(movement.translation.x);
        transform.position.y += scalar::f32_of(movement.translation.y);
        // The node's own rotation, not identity: a character authored at an
        // angle would otherwise snap upright the first time it moved.
        let (angle, _, _) = transform.rotation.to_euler(EulerRot::ZYX);
        Pose2::from_parts(
            scalar::v2(transform.position.x, transform.position.y),
            Rotation2::from_angle(scalar::real(angle)),
        )
    };
    {
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
        state.grounded.insert(entity, movement.grounded);
    }
    Ok(map([
        (k::X, Value::Num(f64::from(movement.translation.x))),
        (k::Y, Value::Num(f64::from(movement.translation.y))),
        (k::GROUNDED, Value::Bool(movement.grounded)),
        (k::SLIDING, Value::Bool(movement.is_sliding_down_slope)),
        (k::COLLISIONS, collision_list(eng, &collisions)),
    ]))
}

pub(crate) fn install_character2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("move_character", &[c::CHARACTER_2D], "", "Move the character by an offset, sliding along walls, climbing steps and staying on the ground: returns `#{ x, y, grounded, sliding, collisions }`. Call it from fixed_update."),
        ("is_grounded", &[c::CHARACTER_2D], "", "Whether the last move ended with ground under the character's feet."),
    ]);
    m.function(
        "move_character",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            move_character(eng, entity_of(node)?, scalar::v2(x, y))
        },
    );
    // A reader, as in 3D: a zero-translation sweep would still snap to ground
    // and write the transform, so asking would move the character.
    m.function("is_grounded", |eng: &Engine, node: NodeId| {
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
            (k::UP, r#"{ type = "vec2", default = [0.0, 1.0], description = "Which way is up for this character: the axis it stands along and measures slopes against" }"#),
        ]),
        shared,
    ]
    .join("\n");
    reg.register_component(
        c::CHARACTER_2D,
        ComponentDef {
            doc: "Moves a node the way a 2D player expects: `physics2d.move_character` slides it along walls, steps it up ledges, keeps it off slopes that are too steep and holds it to the ground over a crest. Needs a `collider2d`.",
            schema: ComponentDef::parse_schema(c::CHARACTER_2D, &schema),
            tags: &["2d", "physics"],
            expects: &[c::COLLIDER_2D],
            apply: Box::new(|eng, entity, params| {
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
