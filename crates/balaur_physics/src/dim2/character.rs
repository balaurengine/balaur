//! `character2d`: the 2D half of `crate::character`.
//!
//! Same controller, same properties, one axis fewer. `up` is a `vec2`, and a
//! platformer's is `[0, 1]`.

use crate::rapier2d::control::{
    CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use crate::rapier2d::prelude::QueryFilter;
use crate::scalar::{self, Pose2, Rotation2, Vector2};
use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{entity_of, Engine, Transform};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::character::SHARED_CHARACTER_SCHEMA;
use crate::dim2::PhysicsState2d;
use crate::vocabulary::map;
use crate::FIXED_DT;
use glamx::EulerRot;

pub struct Character2d(pub toml::Value);

fn controller_of(params: &toml::Value, up: Vector2) -> KinematicCharacterController {
    let relative = crate::vocabulary::text(params, "lengths", "absolute") == "relative";
    let length = |value: f32| {
        let value = scalar::real(value);
        if relative {
            CharacterLength::Relative(value)
        } else {
            CharacterLength::Absolute(value)
        }
    };
    let autostep_height = crate::vocabulary::f(params, "autostep", 0.3);
    KinematicCharacterController {
        up,
        offset: length(crate::vocabulary::f(params, "offset", 0.01)),
        slide: crate::vocabulary::boolean(params, "slide", true),
        autostep: (autostep_height > 0.0).then(|| CharacterAutostep {
            max_height: length(autostep_height),
            min_width: length(crate::vocabulary::f(params, "autostep_min_width", 0.2)),
            include_dynamic_bodies: crate::vocabulary::boolean(params, "autostep_dynamic", false),
        }),
        max_slope_climb_angle: scalar::real(
            crate::vocabulary::f(params, "max_climb_angle", 45.0).to_radians(),
        ),
        min_slope_slide_angle: scalar::real(
            crate::vocabulary::f(params, "min_slide_angle", 30.0).to_radians(),
        ),
        snap_to_ground: {
            let distance = crate::vocabulary::f(params, "snap_to_ground", 0.2);
            (distance > 0.0).then(|| length(distance))
        },
        normal_nudge_factor: scalar::real(crate::vocabulary::f(params, "normal_nudge", 0.0001)),
    }
}

pub(crate) fn move_character(eng: &Engine, entity: Entity, translation: Vector2) -> Result<Value> {
    let params = {
        let world = eng.world();
        let character = world
            .get::<&Character2d>(entity)
            .map_err(|_| anyhow!("node has no character2d"))?;
        character.0.clone()
    };
    let up = scalar::v2a(crate::vocabulary::vec2(&params, "up", [0.0, 1.0]));
    let up = if up.length_squared() < 1.0e-12 {
        Vector2::Y
    } else {
        up.normalize()
    };
    let controller = controller_of(&params, up);
    let push = crate::vocabulary::boolean(&params, "push_bodies", true);

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
        ("x", Value::Num(f64::from(movement.translation.x))),
        ("y", Value::Num(f64::from(movement.translation.y))),
        ("grounded", Value::Bool(movement.grounded)),
        ("sliding", Value::Bool(movement.is_sliding_down_slope)),
        ("collisions", collision_list(eng, &collisions)),
    ]))
}

fn collision_list(eng: &Engine, collisions: &[CharacterCollision]) -> Value {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let mut out: Vec<(u64, Value)> = Vec::new();
    for collision in collisions {
        let Some(other) = state
            .world
            .colliders
            .get(collision.handle)
            .and_then(|collider| Entity::from_bits(collider.user_data as u64))
        else {
            continue;
        };
        let point = collision.hit.witness1;
        let normal = collision.hit.normal1;
        out.push((
            other.to_bits().get(),
            map([
                ("node", Value::Node(other.to_bits().get())),
                ("point", Value::Vec2(scalar::a2(point))),
                ("normal", Value::Vec2(scalar::a2(normal))),
                (
                    "remaining",
                    Value::Vec2(scalar::a2(collision.translation_remaining)),
                ),
            ]),
        ));
    }
    out.sort_by_key(|(bits, _)| *bits);
    Value::List(out.into_iter().map(|(_, value)| value).collect())
}

pub(crate) fn install_character2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("move_character", &["character2d"], "", "Move the character by an offset, sliding along walls, climbing steps and staying on the ground: returns `#{ x, y, grounded, sliding, collisions }`. Call it from fixed_update."),
        ("is_grounded", &["character2d"], "", "Whether the last move ended with ground under the character's feet."),
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
    let schema = format!(
        r#"up = {{ type = "vec2", default = [0.0, 1.0], description = "Which way is up for this character: the axis it stands along and measures slopes against" }}
{SHARED_CHARACTER_SCHEMA}"#
    );
    reg.register_component(
        "character2d",
        ComponentDef {
            doc: "Moves a node the way a 2D player expects: `physics2d.move_character` slides it along walls, steps it up ledges, keeps it off slopes that are too steep and holds it to the ground over a crest. Needs a `collider2d`.",
            schema: ComponentDef::parse_schema("character2d", &schema),
            tags: &["2d", "physics"],
            expects: &["collider2d"],
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
