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
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Transform, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId, Value};

use crate::dim2::PhysicsState2d;
use crate::properties::{ExcludeKind, LengthMode, NodePath};
use crate::vocabulary::words;
use crate::vocabulary::{component as c, keys as k, map};
use balaur_core::fixed_dt;
use glamx::EulerRot;

/// A 2D character controller: `node.character2d.move_by` slides the node along walls and steps it up ledges. Needs a `collider2d`; a `kinematic` `body2d` lets it push bodies.
#[derive(Clone, Component)]
#[component(
    name = "character2d",
    requires = [balaur_core::transform::COMPONENT],
    tags = [ balaur_core::components::tag::DIM_2D, balaur_core::components::tag::PHYSICS],
    expects = [c::COLLIDER_2D],
)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is its own property, not a state machine"
)]
pub struct Character2d {
    /// Which way is up for this character: the axis it stands along and measures slopes against
    #[property(default = [0.0, 1.0])]
    pub up_direction: [f32; 2],
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
    /// Stop at sensors; off, a move passes through them
    #[property(group = "filtering")]
    pub hit_sensors: bool,
    /// Stop at colliders that are not sensors; off, a move passes through them
    #[property(default = true, group = "filtering")]
    pub hit_solids: bool,
    /// The body kinds a move passes through: static takes colliders with no body too. The character's own collision_layer and collision_mask filter the rest
    #[property(flags, group = "filtering")]
    pub exclude_kinds: Vec<ExcludeKind>,
    /// Nodes a move passes through: their colliders, and every collider on their body
    #[property(group = "filtering")]
    pub exclude: Vec<NodePath>,
}

crate::shared::character::functions!(
    state = PhysicsState2d,
    character = Character2d,
    vector = Vector2,
    pose = Pose2,
    value = Vec2,
    array = a2,
    collider = c::COLLIDER_2D
);

pub(crate) fn move_character(eng: &Engine, entity: Entity, translation: Vector2) -> Result<Value> {
    let character = {
        let world = eng.world();
        let character = world
            .get::<&Character2d>(entity)
            .map_err(|_| anyhow!("node has no character2d"))?;
        Character2d::clone(&character)
    };
    let up = scalar::v2a(character.up_direction);
    let up = if up.length_squared() < 1.0e-12 {
        Vector2::Y
    } else {
        up.normalize()
    };
    let controller = controller_of(&character, up);
    let push = character.push_bodies;

    let excluded = excluded_nodes(eng, entity, &character.exclude);

    let (movement, collisions) = {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let (shape, pose, groups) = sweep_shape(state, entity)?;
        // Its own layers, never its own body, and nothing its filter excludes, as in 3D.
        let skipped = Skipped::of(&excluded, state);
        let passes = |_: ColliderHandle, collider: &Collider| skipped.passes(collider);
        let filter = QueryFilter::from(filter_flags(&character))
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

    if !apply_movement(eng, entity, movement.translation, movement.grounded) {
        return Ok(Value::Nil);
    }
    Ok(map([
        (k::APPLIED, Value::Vec2(scalar::a2(movement.translation))),
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
        transform.position.x += translation.x;
        transform.position.y += translation.y;
        // The node's own rotation, not identity: a character authored at an
        // angle would otherwise snap upright the first time it moved.
        let (angle, _, _) = transform.rotation.to_euler(EulerRot::ZYX);
        Pose2::from_parts(
            scalar::v2(transform.position.x, transform.position.y),
            Rotation2::from_angle(angle),
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

pub(crate) fn install_character2d_api(m: &mut Module<Engine>) {
    m.records(&[
        (
            "CharacterCollision",
            "{ node: node, point: Vec2, normal: Vec2, own_point: Vec2, own_normal: Vec2, position: Vec2, applied: Vec2, remaining: Vec2, distance: float, status: string, subshape: int }",
            "One thing a move bumped into: the hit on the obstacle and on the character in world space, where the character stood, how much of the move was done and left, how far it swept, how the sweep ended (`converged`, `out_of_iterations`, `failed`, `penetrating`) and which part of the obstacle's shape it met.",
        ),
        (
            "CharacterMove",
            "{ applied: Vec2, on_floor: bool, sliding: bool, collisions: [CharacterCollision] }",
            "What a move did: the offset `applied`, which is how far the character went, whether it ended on the ground or sliding down a slope, and what it bumped into.",
        ),
    ]);
    m.describe(&[
        (words::functions::MOVE_CHARACTER_BY, &[c::CHARACTER_2D], "(node: node, offset: Vec2) -> CharacterMove", "Move the character by an offset, sliding along walls, climbing steps and staying on the ground. Every solid collider of the character is swept. Call it from fixed_update."),
        (words::functions::IS_ON_FLOOR, &[c::CHARACTER_2D], "", "Whether the last move ended with ground under the character's feet."),
    ]);
    m.function(
        words::functions::MOVE_CHARACTER_BY,
        |eng: &Engine, (node, offset): (NodeId, Value)| {
            let offset = scalar::vec2_arg("an offset", &offset)?;
            move_character(eng, entity_of(node)?, offset)
        },
    );
    // A reader, as in 3D: a zero-translation sweep would still snap to ground
    // and write the transform, so asking would move the character.
    m.function(
        words::functions::IS_ON_FLOOR,
        |eng: &Engine, node: NodeId| {
            let entity = entity_of(node)?;
            let state = eng.resource::<PhysicsState2d>();
            let grounded = state.borrow().grounded.get(&entity).copied();
            Ok(grounded.unwrap_or(false))
        },
    );
}

/// `character2d`: what was asked, held on the node; rapier keeps no character
/// state, so a move builds its controller from this.
pub(crate) fn register_character2d_component(reg: &mut Registry<'_>) {
    reg.add_component::<Character2d>();
}
