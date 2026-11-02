//! `character3d`: the way a player moves, which is not the way a crate falls.
//!
//! A character is a kinematic body that sweeps its shape along a desired
//! translation, slides along what it meets, climbs steps, refuses slopes that
//! are too steep and slides down ones that are, and stays glued to the ground
//! over a crest. Rapier's `KinematicCharacterController` does all of it; this
//! is the component that spells it and the call that runs it.
//!
//! `move_character` reads the query pipeline, so it belongs in `fixed_update`
//!: the binding says so, and so does the docs page.

use crate::rapier3d::control::{
    CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController,
};
use crate::rapier3d::parry::query::ShapeCastStatus;
use crate::rapier3d::prelude::{
    Collider, ColliderHandle, InteractionGroups, QueryFilter, QueryFilterFlags, RigidBodyHandle,
    SharedShape,
};
use crate::scalar::{self, Pose, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Transform, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId, Value};

use crate::PhysicsState3d;
use crate::properties::{LengthMode, NodePath, SweepIgnore};
use crate::vocabulary::{component as c, keys as k, map};
use balaur_core::fixed_dt;

// Held on the node itself: rapier keeps no character state, and the
// controller is rebuilt from this at every move.
/// A 3D character controller: `physics3d.move_character` slides the node along walls and steps it up ledges. Needs a `collider3d`; a `kinematic` `body3d` lets it push bodies.
#[derive(Clone, Component)]
#[component(
    name = "character3d",
    requires = [balaur_core::transform::COMPONENT],
    tags = [ balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
    expects = [c::COLLIDER_3D],
)]
pub struct Character3d {
    /// Which way is up for this character: the axis it stands along and measures slopes against
    #[property(default = [0.0, 1.0, 0.0])]
    pub up_direction: [f32; 3],
    /// Slide along what is in the way instead of stopping dead against it
    #[property(default = true)]
    pub slide: bool,
    /// Push dynamic bodies the character walks into, rather than passing through them
    #[property(default = true)]
    pub push_bodies: bool,
    /// Whether floor_snap_length, step_height, step_min_width and safe_margin are world units or fractions of the character's shape: relative measures the width across up_direction for step_min_width, and the height along it for the rest
    #[property(default = LengthMode::Absolute)]
    pub lengths: LengthMode,
    /// The steepest slope the character may walk up; radians in the file, degrees in the inspector
    #[property(
        default = std::f32::consts::FRAC_PI_4,
        min = 0.0,
        max = 90.0,
        unit = "degrees",
        group = "floor"
    )]
    pub floor_max_angle: f32,
    /// The shallowest slope the character slides back down; radians in the file, degrees in the inspector
    #[property(
        default = std::f32::consts::FRAC_PI_6,
        min = 0.0,
        max = 90.0,
        unit = "degrees",
        group = "floor"
    )]
    pub min_slide_angle: f32,
    /// How far below its feet the character looks for ground to stay stuck to over a crest; 0 turns snapping off
    #[property(default = 0.2, min = 0.0, group = "floor")]
    pub floor_snap_length: f32,
    /// The tallest step the character climbs without jumping; 0 turns stepping off
    #[property(default = 0.3, min = 0.0, group = "steps")]
    pub step_height: f32,
    /// How much clear ground a step needs on top before it may be climbed
    #[property(default = 0.2, min = 0.0, group = "steps")]
    pub step_min_width: f32,
    /// Climb onto dynamic bodies too, not only static and kinematic ones
    #[property(group = "steps")]
    pub step_on_dynamic: bool,
    /// A gap kept between the character and everything else, so the solver never has to push it out of a wall
    #[property(default = 0.01, min = 0.0, group = "contacts")]
    pub safe_margin: f32,
    /// A tiny push along the contact normal that stops the character catching on seams
    #[property(default = 0.0001, min = 0.0, group = "contacts")]
    pub normal_nudge: f32,
    /// The mass a push is worked out with; 0 takes the body's mass, or the colliders' when there is no body
    #[property(min = 0.0, group = "contacts")]
    pub push_mass: f32,
    /// What a move passes through: static takes colliders with no body too. The character's own collision_layer and collision_mask filter the rest
    #[property(flags, default = vec![SweepIgnore::Sensors], group = "filtering")]
    pub ignore: Vec<SweepIgnore>,
    /// Nodes a move passes through: their colliders, and every collider on their body
    #[property(group = "filtering")]
    pub ignore_nodes: Vec<NodePath>,
}

crate::shared::character::functions!(
    state = PhysicsState3d,
    character = Character3d,
    vector = Vector,
    pose = Pose,
    value = Vec3,
    array = a3,
    collider = c::COLLIDER_3D
);

/// Move the character by `translation`, and say what happened.
///
/// The effective translation is written to the node's transform: a character
/// *is* its node, and a caller that had to apply the result itself would get
/// it wrong the first time and every time after.
pub(crate) fn move_character(eng: &Engine, entity: Entity, translation: Vector) -> Result<Value> {
    let character = {
        let world = eng.world();
        let character = world
            .get::<&Character3d>(entity)
            .map_err(|_| anyhow!("node has no character3d"))?;
        Character3d::clone(&character)
    };
    let up = scalar::v3a(character.up_direction);
    let up = if up.length_squared() < 1.0e-12 {
        Vector::Y
    } else {
        up.normalize()
    };
    let controller = controller_of(&character, up);
    let push = character.push_bodies;

    let ignored = ignored_nodes(eng, entity, &character.ignore_nodes);

    let (movement, collisions) = {
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let (shape, pose, groups) = sweep_shape(state, entity)?;
        // Its own layers, never its own body, and nothing `ignore` names.
        let skipped = Skipped::of(&ignored, state);
        let passes = |_: ColliderHandle, collider: &Collider| skipped.passes(collider);
        let filter = QueryFilter::from(ignore_flags(&character.ignore))
            .groups(groups)
            .predicate(&passes);
        let mut collisions = Vec::new();
        let movement = controller.move_shape(
            fixed_dt(),
            &state.world.query_pipeline_with_filter(filter),
            shape.as_ref(),
            &pose,
            translation,
            |collision| collisions.push(collision),
        );
        // Pushing what it walks into is the difference between a character
        // that shoves a crate and one that scrapes past it.
        if push && !collisions.is_empty() {
            let mass = push_mass(state, entity, character.push_mass);
            let dispatcher = state.world.narrow_phase.query_dispatcher();
            let mut queries = state.world.broad_phase.as_query_pipeline_mut(
                dispatcher,
                &mut state.world.bodies,
                &mut state.world.colliders,
                filter,
            );
            controller.solve_character_collision_impulses(
                fixed_dt(),
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
        let state = eng.resource::<PhysicsState3d>();
        state
            .borrow_mut()
            .grounded
            .insert(entity, movement.grounded);
    }
    Ok(map([
        (k::X, Value::Num(f64::from(movement.translation.x))),
        (k::Y, Value::Num(f64::from(movement.translation.y))),
        (k::Z, Value::Num(f64::from(movement.translation.z))),
        (k::ON_FLOOR, Value::Bool(movement.grounded)),
        (k::SLIDING, Value::Bool(movement.is_sliding_down_slope)),
        (k::COLLISIONS, collision_list(eng, &collisions)),
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
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    if let Some(handle) = state.bodies.get(&entity).copied() {
        state.world.bodies[handle].set_next_kinematic_position(pose);
        return;
    }
    // No body: the collider is standalone world geometry, which nothing else
    // moves. Without this the character's own shape stays where it started and
    // every sweep is cast from the wrong place. It walks through walls.
    let handles = state.colliders.get(&entity).cloned().unwrap_or_default();
    for handle in handles {
        if let Some(collider) = state.world.colliders.get_mut(handle) {
            collider.set_position(pose);
        }
    }
    state.queries_ready = false;
}

pub(crate) fn install_character_api(m: &mut Module<Engine>) {
    m.describe(&[
        ("move_character", &[c::CHARACTER_3D], "", "Move the character by an offset, sliding along walls, climbing steps and staying on the ground: returns `{ x, y, z, on_floor, sliding, collisions }`, each collision `{ node, point, normal, own_point, own_normal, position, applied, remaining, distance, status, subshape }`: the hit on the obstacle and on the character in world space, where the character stood, how much of the move was done and left, how far it swept, how the sweep ended (`converged`, `out_of_iterations`, `failed`, `penetrating`) and which part of the obstacle's shape it met. Every solid collider of the character is swept. Call it from fixed_update. It reads the world the step just wrote."),
        ("is_on_floor", &[c::CHARACTER_3D], "", "Whether the last move ended with ground under the character's feet."),
    ]);
    m.function(
        "move_character",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            move_character(eng, entity_of(node)?, scalar::v3(x, y, z))
        },
    );
    // A reader, not a move: sweeping a zero translation would still snap to
    // ground, write the transform and push bodies, so asking would simulate.
    m.function("is_on_floor", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let grounded = state.borrow().grounded.get(&entity).copied();
        Ok(grounded.unwrap_or(false))
    });
}

/// The character component writes no rapier state of its own: it is read at
/// the moment a script moves the node, and the collider does the rest.
pub(crate) fn register_character_component(reg: &mut Registry<'_>) {
    reg.add_component::<Character3d>();
}
