//! `character3d`: the way a player moves, which is not the way a crate falls.
//!
//! A character is a kinematic body that sweeps its shape along a desired
//! translation, slides along what it meets, climbs steps, refuses slopes that
//! are too steep and slides down ones that are, and stays glued to the ground
//! over a crest. Rapier's `KinematicCharacterController` does all of it; this
//! is the component that spells it and the call that runs it.
//!
//! `move_character` reads the query pipeline, so it belongs in `fixed_update`
//! — the binding says so, and so does the docs page.

use crate::rapier3d::control::{
    CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use crate::rapier3d::prelude::QueryFilter;
use crate::scalar::{self, Vector};
use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Transform, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::vocabulary::{map, words};
use crate::{FIXED_DT, PhysicsState};

/// The schema both dimensions share. `up` is the one property whose shape
/// differs, so each adds its own.
pub(crate) fn shared_character_schema() -> String {
    let modes = crate::vocabulary::options(words::LENGTH_MODES);
    let absolute = words::ABSOLUTE;
    format!(
        r#"
offset = {{ type = "float", default = 0.01, min = 0.0, description = "A gap kept between the character and everything else, so the solver never has to push it out of a wall" }}
slide = {{ type = "bool", default = true, description = "Slide along what is in the way instead of stopping dead against it" }}
autostep = {{ type = "float", default = 0.3, min = 0.0, description = "The tallest step the character climbs without jumping; 0 turns stepping off" }}
autostep_min_width = {{ type = "float", default = 0.2, min = 0.0, description = "How much clear ground a step needs on top before it may be climbed" }}
autostep_dynamic = {{ type = "bool", default = false, description = "Climb onto dynamic bodies too, not only static and kinematic ones" }}
max_climb_angle = {{ type = "float", default = 45.0, min = 0.0, max = 90.0, description = "The steepest slope the character may walk up, in degrees" }}
min_slide_angle = {{ type = "float", default = 30.0, min = 0.0, max = 90.0, description = "The shallowest slope the character slides back down, in degrees" }}
snap_to_ground = {{ type = "float", default = 0.2, min = 0.0, description = "How far below its feet the character looks for ground to stay stuck to over a crest; 0 turns snapping off" }}
normal_nudge = {{ type = "float", default = 0.0001, min = 0.0, description = "A tiny push along the contact normal that stops the character catching on seams" }}
push_bodies = {{ type = "bool", default = true, description = "Push dynamic bodies the character walks into, rather than passing through them" }}
lengths = {{ type = "enum", default = "{absolute}", options = [{modes}], description = "Whether offset, autostep and snap_to_ground are in world units or as a fraction of the character's own height" }}
"#
    )
}

crate::shared::character::functions!(
    state = PhysicsState,
    vector = Vector,
    value = Vec3,
    array = a3
);

/// Move the character by `translation`, and say what happened.
///
/// The effective translation is written to the node's transform: a character
/// *is* its node, and a caller that had to apply the result itself would get
/// it wrong the first time and every time after.
pub(crate) fn move_character(eng: &Engine, entity: Entity, translation: Vector) -> Result<Value> {
    let params = {
        let world = eng.world();
        let character = world
            .get::<&Character3d>(entity)
            .map_err(|_| anyhow!("node has no character3d"))?;
        character.0.clone()
    };
    let up = scalar::v3a(crate::vocabulary::vec3(&params, "up", [0.0, 1.0, 0.0]));
    let up = if up.length_squared() < 1.0e-12 {
        Vector::Y
    } else {
        up.normalize()
    };
    let controller = controller_of(&params, up);
    let push = crate::vocabulary::boolean(&params, "push_bodies", true);

    let (movement, collisions) = {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let handle = crate::collider::first_collider(state, entity)
            .map_err(|_| anyhow!("a character needs a collider3d to move with"))?;
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
        // Pushing what it walks into is the difference between a character
        // that shoves a crate and one that scrapes past it.
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

    apply_movement(eng, entity, movement.translation);
    {
        let state = eng.resource::<PhysicsState>();
        state
            .borrow_mut()
            .grounded
            .insert(entity, movement.grounded);
    }
    Ok(map([
        ("x", Value::Num(f64::from(movement.translation.x))),
        ("y", Value::Num(f64::from(movement.translation.y))),
        ("z", Value::Num(f64::from(movement.translation.z))),
        ("grounded", Value::Bool(movement.grounded)),
        ("sliding", Value::Bool(movement.is_sliding_down_slope)),
        ("collisions", collision_list(eng, &collisions)),
    ]))
}

/// Write the effective translation onto the node, and onto its body when it
/// has one, so the next step starts where the character actually is.
fn apply_movement(eng: &Engine, entity: Entity, translation: Vector) {
    let pose = {
        let world = eng.world();
        let transform = world.get::<&mut Transform>(entity);
        let Ok(mut transform) = transform else {
            return;
        };
        transform.position += scalar::position_of(translation);
        scalar::pose_of(transform.position, transform.rotation)
    };
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    if let Some(handle) = state.bodies.get(&entity).copied() {
        state.world.bodies[handle].set_next_kinematic_position(pose);
        return;
    }
    // No body: the collider is standalone world geometry, which nothing else
    // moves. Without this the character's own shape stays where it started and
    // every sweep is cast from the wrong place — it walks through walls.
    let handles = state.colliders.get(&entity).cloned().unwrap_or_default();
    for handle in handles {
        if let Some(collider) = state.world.colliders.get_mut(handle) {
            collider.set_position(pose);
        }
    }
    state.queries_ready = false;
}

pub(crate) fn install_character_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("move_character", &["character3d"], "", "Move the character by an offset, sliding along walls, climbing steps and staying on the ground: returns `#{ x, y, z, grounded, sliding, collisions }`. Call it from fixed_update — it reads the world the step just wrote."),
        ("is_grounded", &["character3d"], "", "Whether the last move ended with ground under the character's feet."),
    ]);
    m.function(
        "move_character",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            move_character(eng, entity_of(node)?, scalar::v3(x, y, z))
        },
    );
    // A reader, not a move: sweeping a zero translation would still snap to
    // ground, write the transform and push bodies, so asking would simulate.
    m.function("is_grounded", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState>();
        let grounded = state.borrow().grounded.get(&entity).copied();
        Ok(grounded.unwrap_or(false))
    });
}

/// A character's settings, held on the node itself.
///
/// The one physics component backed by a real component type (N16): rapier
/// keeps no character state — the controller is rebuilt per move — so there is
/// nothing in the world to write it into.
pub struct Character3d(pub toml::Value);

/// The character component writes no rapier state of its own: it is read at
/// the moment a script moves the node, and the collider does the rest.
pub(crate) fn register_character_component(reg: &mut Registry<'_>) {
    let shared = shared_character_schema();
    let schema = format!(
        r#"up = {{ type = "vec3", default = [0.0, 1.0, 0.0], description = "Which way is up for this character: the axis it stands along and measures slopes against" }}
{shared}"#
    );
    reg.register_component(
        "character3d",
        ComponentDef {
            doc: "Moves a node the way a player expects rather than the way physics would: `physics3d.move_character` slides it along walls, steps it up ledges, keeps it off slopes that are too steep and holds it to the ground over a crest. Needs a `collider3d`; a `body3d` of kind kinematic lets it push what it walks into.",
            schema: ComponentDef::parse_schema("character3d", &schema),
            tags: &["3d", "physics"],
            expects: &["collider3d"],
            // Every property is read at move time, so applying one is
            // remembering it and nothing else.
            apply: Box::new(|eng, entity, params| {
                let _ = eng.world_mut().insert_one(entity, Character3d(params.clone()));
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Character3d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let character = world.get::<&Character3d>(entity).ok()?;
                Some(character.0.clone())
            }),
        },
    );
}
