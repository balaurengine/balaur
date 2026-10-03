//! The words both dimensions use, and the readers that turn them into
//! numbers.
//!
//! rapier2d and rapier3d are separate crates with incompatible types, so the
//! calls into them are written twice (see `dim2`). Everything *around* those
//! calls — schema property text, option lists, options-table reading, result
//! ordering — is `toml` and `Value` and belongs here, written once.

mod layers;
mod read;
mod soft;

pub(crate) use layers::{bits, flags, layer_bit, layer_bits, layer_names, layer_options, names};
pub(crate) use read::{Opts, axis, boolean, color, f, flag, indices, map, text, vec2, vec3};

/// The closed sets of words a scene file, a script table and the inspector all
/// spell. Written once here so a matcher, a schema's `options` list and the
/// read-back cannot disagree about what a word is.
pub(crate) mod words {
    pub(crate) use super::soft::words::*;

    pub(crate) const DYNAMIC: &str = "dynamic";
    pub(crate) const STATIC: &str = "static";
    pub(crate) const KINEMATIC: &str = "kinematic";
    pub(crate) const KINEMATIC_VELOCITY: &str = "kinematic_velocity";
    /// A soft body's cluster proxy, which rapier makes itself: it is read back
    /// but never accepted, so it stays out of `BODY_KINDS`.
    pub(crate) const SOFT_FRAME: &str = "soft_frame";
    /// The body kinds both dimensions accept (N14).
    pub(crate) const BODY_KINDS: &[&str] = &[DYNAMIC, STATIC, KINEMATIC, KINEMATIC_VELOCITY];

    pub(crate) const AVERAGE: &str = "average";
    pub(crate) const MIN: &str = "min";
    pub(crate) const MULTIPLY: &str = "multiply";
    pub(crate) const MAX: &str = "max";
    pub(crate) const CLAMPED_SUM: &str = "clamped_sum";
    pub(crate) const GEOMETRIC_MEAN: &str = "geometric_mean";
    /// How two surfaces' friction or bounciness are combined.
    pub(crate) const COMBINE_RULES: &[&str] =
        &[AVERAGE, MIN, MULTIPLY, MAX, CLAMPED_SUM, GEOMETRIC_MEAN];

    pub(crate) const SPHERE: &str = "sphere";
    pub(crate) const BOX: &str = "box";
    pub(crate) const CIRCLE: &str = "circle";
    pub(crate) const RECTANGLE: &str = "rectangle";
    pub(crate) const CAPSULE: &str = "capsule";
    pub(crate) const CYLINDER: &str = "cylinder";
    pub(crate) const CONE: &str = "cone";
    pub(crate) const TRIANGLE: &str = "triangle";
    pub(crate) const SEGMENT: &str = "segment";
    pub(crate) const WORLD_BOUNDARY: &str = "world_boundary";
    pub(crate) const TRIANGLE_MESH: &str = "triangle_mesh";
    pub(crate) const CONVEX_HULL: &str = "convex_hull";
    pub(crate) const CONVEX_DECOMPOSITION: &str = "convex_decomposition";
    pub(crate) const EXACT: &str = "exact";
    pub(crate) const VHACD: &str = "vhacd";
    pub(crate) const POLYLINE: &str = "polyline";
    pub(crate) const HEIGHTFIELD: &str = "heightfield";
    pub(crate) const VOXELS: &str = "voxels";
    pub(crate) const VOXELIZED_MESH: &str = "voxelized_mesh";
    pub(crate) const FIT: &str = "fit";
    pub(crate) const CONVEX_MESH: &str = "convex_mesh";
    pub(crate) const CONVEX_POLYGON: &str = "convex_polygon";
    pub(crate) const VOXELIZED_POINTS: &str = "voxelized_points";
    /// The 3D collider shapes, in the order the inspector offers them.
    pub(crate) const SHAPES: &[&str] = &[
        SPHERE,
        BOX,
        CAPSULE,
        CYLINDER,
        CONE,
        TRIANGLE,
        SEGMENT,
        WORLD_BOUNDARY,
        TRIANGLE_MESH,
        CONVEX_HULL,
        CONVEX_DECOMPOSITION,
        POLYLINE,
        HEIGHTFIELD,
        VOXELS,
        VOXELIZED_MESH,
        FIT,
        CONVEX_MESH,
        VOXELIZED_POINTS,
    ];
    /// The 2D shapes: a circle and a rectangle, where 3D has a sphere and a box.
    pub(crate) const SHAPES_2D: &[&str] = &[
        CIRCLE,
        RECTANGLE,
        CAPSULE,
        TRIANGLE,
        SEGMENT,
        WORLD_BOUNDARY,
        TRIANGLE_MESH,
        CONVEX_HULL,
        CONVEX_DECOMPOSITION,
        POLYLINE,
        HEIGHTFIELD,
        VOXELS,
        VOXELIZED_MESH,
        FIT,
        CONVEX_POLYGON,
        VOXELIZED_POINTS,
    ];

    pub(crate) const SOLID: &str = "solid";
    pub(crate) const SURFACE: &str = "surface";
    /// Whether voxelizing a mesh fills its inside or only its shell.
    pub(crate) const FILL_MODES: &[&str] = &[SOLID, SURFACE];

    pub(crate) const AABB: &str = "aabb";
    pub(crate) const OBB: &str = "obb";
    /// The shapes a mesh can be fitted to, when a collider's kind is `fit`.
    pub(crate) const FIT_MODES: &[&str] = &[CONVEX_HULL, AABB, OBB, CONVEX_DECOMPOSITION];
    /// The 2D fits: rapier2d's mesh converter has no decomposition.
    pub(crate) const FIT_MODES_2D: &[&str] = &[CONVEX_HULL, AABB, OBB];

    /// How a concave 2D polygon is cut into convex pieces: exactly, over a
    /// triangulation, or approximately, over a voxel grid.
    pub(crate) const DECOMPOSITION_METHODS: &[&str] = &[EXACT, VHACD, VOXELS];
    /// How a 3D convex_decomposition is cut: into hulls, or into voxel parts.
    pub(crate) const DECOMPOSITION_METHODS_3D: &[&str] = &[VHACD, VOXELS];

    pub(crate) const EITHER: &str = "either";
    /// Whether a pair needs both colliders' layers to accept it, or either's.
    pub(crate) const TEST_MODES: &[&str] = &[BOTH, EITHER];

    pub(crate) const CHAIN: &str = "chain";
    pub(crate) const OUTLINE: &str = "outline";
    pub(crate) const MESH: &str = "mesh";
    /// Which edges a polyline collider takes from its mesh.
    pub(crate) const EDGE_MODES: &[&str] = &[CHAIN, MESH];
    pub(crate) const EDGE_MODES_2D: &[&str] = &[CHAIN, OUTLINE, MESH];

    pub(crate) const SIMPLIFIED: &str = "simplified";
    pub(crate) const PER_CONTACT: &str = "per_contact";
    /// How the 3D solver treats friction: one cone per four contacts, or one
    /// per contact.
    pub(crate) const FRICTION_MODELS: &[&str] = &[SIMPLIFIED, PER_CONTACT];

    pub(crate) const FIXED: &str = "fixed";
    pub(crate) const HINGE: &str = "hinge";
    pub(crate) const SLIDER: &str = "slider";
    pub(crate) const BALL_SOCKET: &str = "ball_socket";
    pub(crate) const ROPE: &str = "rope";
    pub(crate) const SPRING: &str = "spring";
    pub(crate) const GROOVE: &str = "groove";
    pub(crate) const GENERIC: &str = "generic";
    /// The 3D joints. `ball_socket` needs three angular axes, so 2D has none.
    pub(crate) const JOINT_KINDS: &[&str] =
        &[FIXED, HINGE, SLIDER, BALL_SOCKET, ROPE, SPRING, GENERIC];
    /// The 2D joints. `groove` is Godot's, and 2D-only.
    pub(crate) const JOINT_KINDS_2D: &[&str] =
        &[FIXED, HINGE, SLIDER, ROPE, SPRING, GROOVE, GENERIC];

    pub(crate) const BOTH: &str = "both";

    pub(crate) const OFF: &str = "off";
    pub(crate) const VELOCITY: &str = "velocity";
    pub(crate) const POSITION: &str = "position";
    /// What a joint's motor drives towards, if anything.
    pub(crate) const MOTOR_MODES: &[&str] = &[OFF, VELOCITY, POSITION];

    /// What rapier picks when the author does not: a spring's own default, the
    /// generator's own choice.
    pub(crate) const AUTO: &str = "auto";
    pub(crate) const ACCELERATION: &str = "acceleration";
    pub(crate) const FORCE: &str = "force";
    /// Whether a motor's strength ignores mass; `auto` is force for a spring's
    /// spring and acceleration for every other motor, as rapier builds them.
    pub(crate) const MOTOR_MODELS: &[&str] = &[AUTO, ACCELERATION, FORCE];

    pub(crate) const SENSORS: &str = "sensors";
    pub(crate) const SOLIDS: &str = "solids";
    /// What a character's or a vehicle's sweep passes through.
    pub(crate) const IGNORES: &[&str] = &[STATIC, KINEMATIC, DYNAMIC, SENSORS, SOLIDS];
    pub(crate) const PD: &str = "pd";
    pub(crate) const PID: &str = "pid";
    /// Rapier's two controllers: proportional-derivative, and the same with an
    /// integral that keeps pulling against a steady push.
    pub(crate) const FOLLOW_KINDS: &[&str] = &[PD, PID];

    pub(crate) const ABSOLUTE: &str = "absolute";
    pub(crate) const RELATIVE: &str = "relative";
    /// Whether a character's lengths are world units or a fraction of it.
    pub(crate) const LENGTH_MODES: &[&str] = &[ABSOLUTE, RELATIVE];

    pub(crate) const X: &str = "x";
    pub(crate) const Y: &str = "y";
    pub(crate) const Z: &str = "z";
    pub(crate) const NEGATIVE_X: &str = "-x";
    pub(crate) const NEGATIVE_Y: &str = "-y";
    pub(crate) const NEGATIVE_Z: &str = "-z";
    /// A chassis's own axes, either way along, as `up_axis` and `forward_axis`
    /// name them.
    pub(crate) const AXES: &[&str] = &[X, Y, Z, NEGATIVE_X, NEGATIVE_Y, NEGATIVE_Z];
    /// The world axes a 3D body or generic joint may lock, and the 2D pair.
    pub(crate) const LOCK_AXES: &[&str] = &[X, Y, Z];
    pub(crate) const LOCK_AXES_2D: &[&str] = &[X, Y];
    /// The axes a capsule may lie along, in 3D and in 2D.
    pub(crate) const CAPSULE_AXES: &[&str] = &[X, Y, Z];
    pub(crate) const CAPSULE_AXES_2D: &[&str] = &[X, Y];
    pub(crate) const ROTATION_X: &str = "rotation_x";
    pub(crate) const ROTATION_Y: &str = "rotation_y";
    pub(crate) const ROTATION_Z: &str = "rotation_z";
    pub(crate) const ROTATION: &str = "rotation";
    /// Degrees of freedom: a joint's own in an `axes` record and a joint
    /// call, the world's in `solve_ik`'s `constrain`.
    pub(crate) const JOINT_AXES: &[&str] = &[X, Y, Z, ROTATION_X, ROTATION_Y, ROTATION_Z];
    pub(crate) const JOINT_AXES_2D: &[&str] = &[X, Y, ROTATION];

    pub(crate) const COLLISION: &str = "collision";
    pub(crate) const CONTACT_FORCE: &str = "contact_force";

    pub(crate) const DYNAMIC_DYNAMIC: &str = "dynamic_dynamic";
    pub(crate) const DYNAMIC_KINEMATIC: &str = "dynamic_kinematic";
    pub(crate) const DYNAMIC_STATIC: &str = "dynamic_static";
    pub(crate) const KINEMATIC_KINEMATIC: &str = "kinematic_kinematic";
    pub(crate) const KINEMATIC_STATIC: &str = "kinematic_static";
    pub(crate) const STATIC_STATIC: &str = "static_static";
    /// The pairs a collider is tested against unless it asks for more.
    pub(crate) const DEFAULT_COLLISIONS: &[&str] =
        &[DYNAMIC_DYNAMIC, DYNAMIC_KINEMATIC, DYNAMIC_STATIC];

    pub(crate) const ENABLED: &str = "enabled";
    pub(crate) const DISABLED: &str = "disabled";
    pub(crate) const BODY_DISABLED: &str = "body_disabled";
    /// A joint whose other end is not in the scene yet.
    pub(crate) const WAITING: &str = "waiting";

    pub(crate) const NONE: &str = "none";
    pub(crate) const MOTORS: &str = "motors";
    pub(crate) const FOLLOW: &str = "follow";
    /// What pulls a ragdoll's bodies, beyond gravity and contacts.
    pub(crate) const RAGDOLL_DRIVES: &[&str] = &[NONE, MOTORS, FOLLOW];

    pub(crate) const DRAW_COLLIDERS: &str = "colliders";
    pub(crate) const DRAW_AABBS: &str = "aabbs";
    pub(crate) const DRAW_AXES: &str = "axes";
    pub(crate) const DRAW_IMPULSE_JOINTS: &str = "impulse_joints";
    pub(crate) const DRAW_MULTIBODY_JOINTS: &str = "multibody_joints";
    pub(crate) const DRAW_CONTACTS: &str = "contacts";
    pub(crate) const DRAW_SOLVER_CONTACTS: &str = "solver_contacts";
    pub(crate) const DRAW_SOFT_BODIES: &str = "soft_bodies";
    pub(crate) const DRAW_PSEUDO_NORMALS: &str = "pseudo_normals";
    pub(crate) const DRAW_SOFT_VOLUME_CONTACTS: &str = "soft_volume_contacts";
    pub(crate) const DRAW_SOFT_STRESS: &str = "soft_stress";

    /// How a character's sweep ended at a hit, as parry's shape cast reports it.
    pub(crate) const CONVERGED: &str = "converged";
    pub(crate) const OUT_OF_ITERATIONS: &str = "out_of_iterations";
    pub(crate) const FAILED: &str = "failed";
    pub(crate) const PENETRATING: &str = "penetrating";
}

/// Every property, options-table and result key the physics components and
/// calls spell, so a schema line and the reader behind it name the same key.
pub(crate) mod keys {
    pub(crate) use super::soft::keys::*;

    pub(crate) const A: &str = "a";
    pub(crate) const AABB_COLOR: &str = "aabb_color";
    pub(crate) const ALLOWED_LINEAR_ERROR: &str = "allowed_linear_error";
    pub(crate) const ALLOW_FAST_ROTATION: &str = "allow_fast_rotation";
    pub(crate) const ANCHOR: &str = "anchor";
    pub(crate) const ANCHOR_ROTATION: &str = "anchor_rotation";
    pub(crate) const ANGLE: &str = "angle";
    pub(crate) const ANGULAR_DAMPING: &str = "angular_damping";
    pub(crate) const APPLIED: &str = "applied";
    pub(crate) const APPROXIMATE_HULLS: &str = "approximate_hulls";
    pub(crate) const ARMATURE: &str = "armature";
    pub(crate) const AT: &str = "at";
    pub(crate) const AXES: &str = "axes";
    pub(crate) const AXES_LENGTH: &str = "axes_length";
    pub(crate) const AXIS: &str = "axis";
    pub(crate) const AXLE: &str = "axle";
    pub(crate) const ARTICULATION: &str = "articulation";
    pub(crate) const ARTICULATION_ANCHOR_COLOR: &str = "articulation_anchor_color";
    pub(crate) const ARTICULATION_SEPARATION_COLOR: &str = "articulation_separation_color";
    pub(crate) const B: &str = "b";
    pub(crate) const BODY: &str = "body";
    pub(crate) const BORDER_SUBDIVISIONS: &str = "border_subdivisions";
    pub(crate) const BODIES: &str = "bodies";
    pub(crate) const BRAKE: &str = "brake";
    pub(crate) const BREAK_FORCE: &str = "break_force";
    pub(crate) const BREAK_TORQUE: &str = "break_torque";
    pub(crate) const BROAD_PHASE_MS: &str = "broad_phase_ms";
    pub(crate) const C: &str = "c";
    pub(crate) const CAN_SLEEP: &str = "can_sleep";
    pub(crate) const CCD_BROAD_PHASE_MS: &str = "ccd_broad_phase_ms";
    pub(crate) const CCD_MS: &str = "ccd_ms";
    pub(crate) const CCD_NARROW_PHASE_MS: &str = "ccd_narrow_phase_ms";
    pub(crate) const CCD_SOLVER_MS: &str = "ccd_solver_ms";
    pub(crate) const CCD_SUBSTEPS: &str = "ccd_substeps";
    pub(crate) const CCD_SUBSTEP_COUNT: &str = "ccd_substep_count";
    pub(crate) const CCD_TIME_OF_IMPACT_MS: &str = "ccd_time_of_impact_ms";
    pub(crate) const CELLS: &str = "cells";
    pub(crate) const CENTER: &str = "center";
    pub(crate) const CENTER_OF_MASS: &str = "center_of_mass";
    pub(crate) const COLLIDERS: &str = "colliders";
    pub(crate) const COLLIDE_CONNECTED: &str = "collide_connected";
    pub(crate) const COLLISIONS: &str = "collisions";
    pub(crate) const COLLISION_DETECTION_MS: &str = "collision_detection_ms";
    pub(crate) const COLLISION_LAYER: &str = "collision_layer";
    pub(crate) const COLLISION_MARGIN: &str = "collision_margin";
    pub(crate) const COLLISION_MASK: &str = "collision_mask";
    pub(crate) const COLLISION_TEST: &str = "collision_test";
    pub(crate) const CONNECTED_ANCHOR: &str = "connected_anchor";
    pub(crate) const CONNECTED_ANCHOR_ROTATION: &str = "connected_anchor_rotation";
    pub(crate) const CONNECTED_AXIS: &str = "connected_axis";
    pub(crate) const CONNECTED_BODY: &str = "connected_body";
    pub(crate) const CONNECTED_COMPONENTS: &str = "connected_components";
    pub(crate) const CONSTRAIN: &str = "constrain";
    pub(crate) const CONSTRAINT_COUNT: &str = "constraint_count";
    pub(crate) const CONTACT_CLUSTERING: &str = "contact_clustering";
    pub(crate) const CONTACT_COUNT: &str = "contact_count";
    pub(crate) const CONTACT_DAMPING: &str = "contact_damping";
    pub(crate) const CONTACT_DEPTH_COLOR: &str = "contact_depth_color";
    pub(crate) const CONTACT_FORCE_THRESHOLD: &str = "contact_force_threshold";
    pub(crate) const CONTACT_FREQUENCY_HZ: &str = "contact_frequency_hz";
    pub(crate) const CONTACT_NORMAL: &str = "contact_normal";
    pub(crate) const CONTACT_NORMAL_COLOR: &str = "contact_normal_color";
    pub(crate) const CONTACT_NORMAL_LENGTH: &str = "contact_normal_length";
    pub(crate) const CONTACT_PAIRS: &str = "contact_pairs";
    pub(crate) const CONTACT_PAIR_COUNT: &str = "contact_pair_count";
    pub(crate) const CONTACT_POINT: &str = "contact_point";
    pub(crate) const CONTACT_RECYCLE_DISTANCE: &str = "contact_recycle_distance";
    pub(crate) const CONTACT_RECYCLING: &str = "contact_recycling";
    pub(crate) const CONTINUOUS_COLLISION: &str = "continuous_collision";
    pub(crate) const COORDINATES: &str = "coordinates";
    pub(crate) const COUPLED_ROTATION: &str = "coupled_rotation";
    pub(crate) const COUPLED_TRANSLATION: &str = "coupled_translation";
    pub(crate) const DISTANCE: &str = "distance";
    pub(crate) const DROP_BAD_TOPOLOGY: &str = "drop_bad_topology";
    pub(crate) const DROP_DEGENERATE_TRIANGLES: &str = "drop_degenerate_triangles";
    pub(crate) const DROP_DUPLICATE_TRIANGLES: &str = "drop_duplicate_triangles";
    pub(crate) const DYNAMIC_COLOR: &str = "dynamic_color";
    pub(crate) const EDGES: &str = "edges";
    pub(crate) const FILL_CAVITIES: &str = "fill_cavities";
    pub(crate) const FIX_SELF_INTERSECTIONS: &str = "fix_self_intersections";
    pub(crate) const FLOOR_SNAP_LENGTHS: &str = "floor_snap_lengths";
    /// A contact force's or a broken joint's size, in an event payload.
    pub(crate) const FORCE: &str = "force";
    pub(crate) const DAMPING: &str = "damping";
    pub(crate) const DAMPING_COMPRESSION: &str = "damping_compression";
    pub(crate) const DAMPING_RELAXATION: &str = "damping_relaxation";
    pub(crate) const DENSITY: &str = "density";
    pub(crate) const DIRECTION: &str = "direction";
    pub(crate) const DISABLED_TINT: &str = "disabled_tint";
    pub(crate) const DOMINANCE: &str = "dominance";
    pub(crate) const DRIVE: &str = "drive";
    /// A ragdoll's `follow3d`/`follow2d` table, with `drive = "follow"`.
    pub(crate) const FOLLOW: &str = "follow";
    pub(crate) const EDGE_NORMAL_COLOR: &str = "edge_normal_color";
    pub(crate) const EDGE_RADIUS: &str = "edge_radius";
    pub(crate) const ENABLED: &str = "enabled";
    pub(crate) const ENGINE_FORCE: &str = "engine_force";
    pub(crate) const EVENTS: &str = "events";
    pub(crate) const EXCLUDE: &str = "exclude";
    pub(crate) const EXCLUDE_BODY: &str = "exclude_body";
    pub(crate) const FILL: &str = "fill";
    pub(crate) const FILTER: &str = "filter";
    pub(crate) const FINAL_BROAD_PHASE_MS: &str = "final_broad_phase_ms";
    pub(crate) const FIT: &str = "fit";
    pub(crate) const FIX_INTERNAL_EDGES: &str = "fix_internal_edges";
    pub(crate) const FLOOR_MAX_ANGLE: &str = "floor_max_angle";
    pub(crate) const FLOOR_SNAP_LENGTH: &str = "floor_snap_length";
    pub(crate) const FORWARD_AXIS: &str = "forward_axis";
    pub(crate) const FORWARD_IMPULSE: &str = "forward_impulse";
    pub(crate) const FRICTION: &str = "friction";
    pub(crate) const FRICTION_COMBINE: &str = "friction_combine";
    pub(crate) const FRICTION_IN_BIAS_PASS: &str = "friction_in_bias_pass";
    pub(crate) const FRICTION_MODEL: &str = "friction_model";
    pub(crate) const FRICTION_SLIP: &str = "friction_slip";
    pub(crate) const GEAR_OFFSET: &str = "gear_offset";
    pub(crate) const GEAR_RATIO: &str = "gear_ratio";
    pub(crate) const GEAR_WITH: &str = "gear_with";
    pub(crate) const GRAVITY_2D: &str = "gravity_2d";
    pub(crate) const GRAVITY_3D: &str = "gravity_3d";
    pub(crate) const GRAVITY_SCALE: &str = "gravity_scale";
    pub(crate) const GROUND: &str = "ground";
    pub(crate) const GYROSCOPIC_FORCES: &str = "gyroscopic_forces";
    pub(crate) const HEIGHT: &str = "height";
    pub(crate) const HEIGHTFIELD: &str = "heightfield";
    pub(crate) const HIT_FROM_INSIDE: &str = "hit_from_inside";
    pub(crate) const HIT_SENSORS: &str = "hit_sensors";
    pub(crate) const HIT_SOLIDS: &str = "hit_solids";
    pub(crate) const HULL_DOWNSAMPLING: &str = "hull_downsampling";
    pub(crate) const IGNORE: &str = "ignore";
    pub(crate) const IGNORE_NODES: &str = "ignore_nodes";
    pub(crate) const IMPULSE: &str = "impulse";
    pub(crate) const INDICES: &str = "indices";
    pub(crate) const INERTIA: &str = "inertia";
    pub(crate) const INERTIA_ROTATION: &str = "inertia_rotation";
    pub(crate) const INITIAL_ANGULAR_VELOCITY: &str = "initial_angular_velocity";
    pub(crate) const INITIAL_LINEAR_VELOCITY: &str = "initial_linear_velocity";
    pub(crate) const INSIDE: &str = "inside";
    pub(crate) const INTERNAL_ITERATIONS: &str = "internal_iterations";
    pub(crate) const IN_CONTACT: &str = "in_contact";
    pub(crate) const ISLAND_CONSTRAINTS_MS: &str = "island_constraints_ms";
    pub(crate) const ISLAND_CONSTRUCTION_MS: &str = "island_construction_ms";
    pub(crate) const ITERATIONS: &str = "iterations";
    pub(crate) const JOINT_ANCHOR_COLOR: &str = "joint_anchor_color";
    pub(crate) const JOINT_SEPARATION_COLOR: &str = "joint_separation_color";
    pub(crate) const KEEP_COLLINEAR: &str = "keep_collinear";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const KINEMATIC_COLOR: &str = "kinematic_color";
    pub(crate) const KINEMATIC_LINK: &str = "kinematic_link";
    pub(crate) const LENGTH_UNIT: &str = "length_unit";
    pub(crate) const LIMITS: &str = "limits";
    pub(crate) const LIMIT_IMPULSES: &str = "limit_impulses";
    pub(crate) const LINEAR_DAMPING: &str = "linear_damping";
    pub(crate) const LINK_DAMPING: &str = "link_damping";
    pub(crate) const LOCK_ROTATION: &str = "lock_rotation";
    pub(crate) const LOCK_TRANSLATION: &str = "lock_translation";
    pub(crate) const MASS: &str = "mass";
    pub(crate) const MAX: &str = "max";
    pub(crate) const MAX_CONCAVITY: &str = "max_concavity";
    pub(crate) const MAX_CONVEX_HULLS: &str = "max_convex_hulls";
    pub(crate) const MAX_CORRECTIVE_VELOCITY: &str = "max_corrective_velocity";
    pub(crate) const MAX_DISTANCE: &str = "max_distance";
    pub(crate) const MAX_FORCE: &str = "max_force";
    pub(crate) const MAX_LINEAR_VELOCITY: &str = "max_linear_velocity";
    pub(crate) const MAX_LENGTH: &str = "max_length";
    pub(crate) const MAX_TIME: &str = "max_time";
    pub(crate) const MERGE_VERTICES: &str = "merge_vertices";
    pub(crate) const MESH: &str = "mesh";
    pub(crate) const METHOD: &str = "method";
    pub(crate) const MIN: &str = "min";
    pub(crate) const MIN_CCD_SECONDS: &str = "min_ccd_seconds";
    pub(crate) const MIN_SLIDE_ANGLE: &str = "min_slide_angle";
    pub(crate) const MOTOR: &str = "motor";
    pub(crate) const MOTOR_IMPULSES: &str = "motor_impulses";
    pub(crate) const MOTOR_MAX_FORCE: &str = "motor_max_force";
    pub(crate) const MOTOR_MODEL: &str = "motor_model";
    pub(crate) const MOTOR_TARGET: &str = "motor_target";
    pub(crate) const MOTOR_TARGET_VELOCITY: &str = "motor_target_velocity";
    pub(crate) const NARROW_PHASE_MS: &str = "narrow_phase_ms";
    pub(crate) const NODE: &str = "node";
    pub(crate) const NODES: &str = "nodes";
    pub(crate) const NORMAL: &str = "normal";
    pub(crate) const NORMAL_LENGTH: &str = "normal_length";
    pub(crate) const NORMAL_NUDGE: &str = "normal_nudge";
    pub(crate) const NORMALS: &str = "normals";
    pub(crate) const OFFSET: &str = "offset";
    pub(crate) const ONE_WAY_ANGLE: &str = "one_way_angle";
    pub(crate) const OTHER: &str = "other";
    pub(crate) const OFFSET_ROTATION: &str = "offset_rotation";
    pub(crate) const ONE_WAY: &str = "one_way";
    pub(crate) const ONE_WAY_AXIS: &str = "one_way_axis";
    pub(crate) const ONLY: &str = "only";
    pub(crate) const ON_FLOOR: &str = "on_floor";
    pub(crate) const ORIENTED: &str = "oriented";
    pub(crate) const ORIGIN: &str = "origin";
    pub(crate) const OVERLAP: &str = "overlap";
    pub(crate) const OWN_NORMAL: &str = "own_normal";
    pub(crate) const OWN_POINT: &str = "own_point";
    pub(crate) const PASSIVE_REST: &str = "passive_rest";
    pub(crate) const PASSIVE_STIFFNESS: &str = "passive_stiffness";
    /// A world's entry in what `physics` reports for both, named for its module.
    pub(crate) const PHYSICS_2D: &str = "physics2d";
    pub(crate) const PHYSICS_3D: &str = "physics3d";
    pub(crate) const PIECES: &str = "pieces";
    pub(crate) const PLANE_DOWNSAMPLING: &str = "plane_downsampling";
    pub(crate) const REVOLUTION_BIAS: &str = "revolution_bias";
    pub(crate) const SLEEP_ANGULAR_THRESHOLD: &str = "sleep_angular_threshold";
    pub(crate) const SLEEP_READY_TINT: &str = "sleep_ready_tint";
    pub(crate) const SLEEP_THRESHOLD: &str = "sleep_threshold";
    pub(crate) const SOLVER_TEST: &str = "solver_test";
    pub(crate) const STARTED: &str = "started";
    pub(crate) const START_ASLEEP: &str = "start_asleep";
    pub(crate) const STATIC_COLOR: &str = "static_color";
    pub(crate) const SURFACE_VELOCITY: &str = "surface_velocity";
    pub(crate) const SUSPENSION: &str = "suspension";
    pub(crate) const SYMMETRY_BIAS: &str = "symmetry_bias";
    pub(crate) const TOPOLOGY: &str = "topology";
    /// The particle pairs a tear cut, in a `tear` payload.
    pub(crate) const TORN_EDGES: &str = "edges";
    pub(crate) const POINT: &str = "point";
    pub(crate) const POINTS: &str = "points";
    pub(crate) const POSITION: &str = "position";
    pub(crate) const PREDICATE: &str = "predicate";
    pub(crate) const PREDICTION_DISTANCE: &str = "prediction_distance";
    pub(crate) const PUSH_BODIES: &str = "push_bodies";
    pub(crate) const PUSH_MASS: &str = "push_mass";
    pub(crate) const RADIUS: &str = "radius";
    pub(crate) const RAY_ORIGIN: &str = "ray_origin";
    pub(crate) const REMAINING: &str = "remaining";
    /// Whether a collision ended because a collider went away.
    pub(crate) const REMOVED: &str = "removed";
    pub(crate) const RESOLUTION: &str = "resolution";
    pub(crate) const RESTITUTION: &str = "restitution";
    pub(crate) const RESTITUTION_COMBINE: &str = "restitution_combine";
    pub(crate) const REST_LENGTH: &str = "rest_length";
    pub(crate) const ROTATION: &str = "rotation";
    pub(crate) const SAFE_MARGIN: &str = "safe_margin";
    pub(crate) const SAFE_MARGIN_LENGTHS: &str = "safe_margin_lengths";
    pub(crate) const SCALE: &str = "scale";
    pub(crate) const SELF_COLLISION: &str = "self_collision";
    pub(crate) const SENSOR: &str = "sensor";
    pub(crate) const SHAPE: &str = "shape";
    pub(crate) const SIDE_FRICTION: &str = "side_friction";
    pub(crate) const SIDE_IMPULSE: &str = "side_impulse";
    pub(crate) const SIZE: &str = "size";
    pub(crate) const SLEEPING_TINT: &str = "sleeping_tint";
    pub(crate) const SLIDE: &str = "slide";
    pub(crate) const SLIDING: &str = "sliding";
    pub(crate) const SOFT_BODIES: &str = "soft_bodies";
    pub(crate) const SOFT_BODY_COLOR: &str = "soft_body_color";
    pub(crate) const SOFT_CONTACT_STIFFENING: &str = "soft_contact_stiffening";
    pub(crate) const SOFT_FRAME_COLOR: &str = "soft_frame_color";
    pub(crate) const SOFT_LINEAR_TOLERANCE: &str = "soft_linear_tolerance";
    pub(crate) const SOFT_LOADED_COLOR: &str = "soft_loaded_color";
    pub(crate) const SOFT_MAX_DENSE_DOFS: &str = "soft_max_dense_dofs";
    pub(crate) const SOFT_MAX_EXTRA_SUBSTEPS: &str = "soft_max_extra_substeps";
    pub(crate) const SOFT_MAX_LINEAR_ITERATIONS: &str = "soft_max_linear_iterations";
    pub(crate) const SOFT_RECOVERY: &str = "soft_recovery";
    pub(crate) const SOFT_RESWEEP_STRAIN: &str = "soft_resweep_strain";
    pub(crate) const SOFT_SLACK_COLOR: &str = "soft_slack_color";
    pub(crate) const SOFTNESS_DAMPING_RATIO: &str = "softness_damping_ratio";
    pub(crate) const SOFTNESS_HZ: &str = "softness_hz";
    pub(crate) const SOLVER_ITERATIONS: &str = "solver_iterations";
    pub(crate) const SOLVER_LAYER: &str = "solver_layer";
    pub(crate) const SOLVER_MASK: &str = "solver_mask";
    pub(crate) const SOLVER_MS: &str = "solver_ms";
    pub(crate) const SPECULATIVE_DISTANCE: &str = "speculative_distance";
    pub(crate) const STABILIZATION_ITERATIONS: &str = "stabilization_iterations";
    pub(crate) const STANDALONE_COLOR: &str = "standalone_color";
    pub(crate) const STATIC_CONTACT_DAMPING: &str = "static_contact_damping";
    pub(crate) const STATIC_CONTACT_FREQUENCY_HZ: &str = "static_contact_frequency_hz";
    pub(crate) const STATUS: &str = "status";
    pub(crate) const STEERING: &str = "steering";
    pub(crate) const STEP_HEIGHT: &str = "step_height";
    pub(crate) const STEP_HEIGHT_LENGTHS: &str = "step_height_lengths";
    pub(crate) const STEP_MIN_WIDTH: &str = "step_min_width";
    pub(crate) const STEP_MIN_WIDTH_LENGTHS: &str = "step_min_width_lengths";
    pub(crate) const STEP_MS: &str = "step_ms";
    pub(crate) const STEP_ON_DYNAMIC: &str = "step_on_dynamic";
    pub(crate) const STIFFNESS: &str = "stiffness";
    pub(crate) const STOP_AT_PENETRATION: &str = "stop_at_penetration";
    pub(crate) const SUBDIVISIONS: &str = "subdivisions";
    pub(crate) const SUBSHAPE: &str = "subshape";
    pub(crate) const SUSPENSION_DIRECTION: &str = "suspension_direction";
    pub(crate) const SUSPENSION_FORCE: &str = "suspension_force";
    pub(crate) const SUSPENSION_LENGTH: &str = "suspension_length";
    pub(crate) const SUSPENSION_MAX_FORCE: &str = "suspension_max_force";
    pub(crate) const SUSPENSION_STIFFNESS: &str = "suspension_stiffness";
    pub(crate) const SUSPENSION_TRAVEL: &str = "suspension_travel";
    pub(crate) const THICKNESS: &str = "thickness";
    /// `[physics] threads`: how many the solver may take.
    pub(crate) const THREADS: &str = "threads";
    pub(crate) const TIME_TO_SLEEP: &str = "time_to_sleep";
    pub(crate) const TOLERANCE: &str = "tolerance";
    pub(crate) const TORQUE: &str = "torque";
    pub(crate) const TOTAL_FORCE: &str = "total_force";
    pub(crate) const TWO_SIDED_EDGES: &str = "two_sided_edges";
    pub(crate) const UPDATE_MS: &str = "update_ms";
    pub(crate) const UP_AXIS: &str = "up_axis";
    pub(crate) const TARGET_LINEAR_VELOCITY: &str = "target_linear_velocity";
    pub(crate) const TARGET_ANGULAR_VELOCITY: &str = "target_angular_velocity";
    pub(crate) const POSITION_GAIN: &str = "position_gain";
    pub(crate) const VELOCITY_GAIN: &str = "velocity_gain";
    pub(crate) const INTEGRAL_GAIN: &str = "integral_gain";
    pub(crate) const ROTATION_GAIN: &str = "rotation_gain";
    pub(crate) const SPIN_GAIN: &str = "spin_gain";
    pub(crate) const ROTATION_INTEGRAL_GAIN: &str = "rotation_integral_gain";
    pub(crate) const TRANSLATION_AXES: &str = "translation_axes";
    pub(crate) const ROTATION_AXES: &str = "rotation_axes";
    pub(crate) const FOLLOW_ROTATION: &str = "follow_rotation";
    pub(crate) const UP_DIRECTION: &str = "up_direction";
    pub(crate) const USER_CHANGES_MS: &str = "user_changes_ms";
    pub(crate) const VELOCITIES: &str = "velocities";
    pub(crate) const VELOCITY: &str = "velocity";
    pub(crate) const VELOCITY_A: &str = "velocity_a";
    pub(crate) const VELOCITY_ASSEMBLY_BODIES_MS: &str = "velocity_assembly_bodies_ms";
    pub(crate) const VELOCITY_ASSEMBLY_CONSTRAINTS_MS: &str = "velocity_assembly_constraints_ms";
    pub(crate) const VELOCITY_ASSEMBLY_MS: &str = "velocity_assembly_ms";
    pub(crate) const VELOCITY_B: &str = "velocity_b";
    pub(crate) const VELOCITY_RESOLUTION_MS: &str = "velocity_resolution_ms";
    pub(crate) const VELOCITY_UPDATE_MS: &str = "velocity_update_ms";
    pub(crate) const VELOCITY_WRITEBACK_MS: &str = "velocity_writeback_ms";
    pub(crate) const VERTEX_NORMAL_COLOR: &str = "vertex_normal_color";
    pub(crate) const VOLUME: &str = "volume";
    pub(crate) const VOLUME_GRADIENT_COLOR: &str = "volume_gradient_color";
    pub(crate) const VOLUME_NORMAL_COLOR: &str = "volume_normal_color";
    pub(crate) const VOXELS: &str = "voxels";
    pub(crate) const VOXEL_SIZE: &str = "voxel_size";
    pub(crate) const WARMSTART: &str = "warmstart";
    pub(crate) const WARMSTART_JOINTS: &str = "warmstart_joints";
    pub(crate) const WORLD_2D: &str = "2d";
    pub(crate) const WORLD_3D: &str = "3d";
    pub(crate) const X: &str = "x";
    pub(crate) const Y: &str = "y";
    pub(crate) const Z: &str = "z";
}

/// The component keys, as the registry and every `describe` entry spell them.
pub(crate) mod component {
    pub(crate) const BODY_3D: &str = "body3d";
    pub(crate) const BODY_2D: &str = "body2d";
    pub(crate) const COLLIDER_3D: &str = "collider3d";
    pub(crate) const COLLIDER_2D: &str = "collider2d";
    pub(crate) const TILE_COLLISION: &str = "tile_collision";
    pub(crate) const JOINT_3D: &str = "joint3d";
    pub(crate) const JOINT_2D: &str = "joint2d";
    pub(crate) const CHARACTER_3D: &str = "character3d";
    pub(crate) const CHARACTER_2D: &str = "character2d";
    pub(crate) const WHEEL_3D: &str = "wheel3d";
    pub(crate) const VEHICLE_3D: &str = "vehicle3d";
    pub(crate) const FOLLOW_3D: &str = "follow3d";
    pub(crate) const FOLLOW_2D: &str = "follow2d";
    pub(crate) const SOFTBODY_3D: &str = "softbody3d";
    pub(crate) const SOFTBODY_2D: &str = "softbody2d";
    /// What a 2D node can be drawn by: a soft body bends a polygon, and the
    /// rest stay rigid over it. Render's own names, spelled again because
    /// physics does not depend on the renderer.
    pub(crate) const POLYGON: &str = "polygon";
    pub(crate) const RIGID_2D_DRAWERS: &[&str] = &["sprite", "shape2d", "text2d"];
}

/// The events physics announces from a node, each heard there as
/// `on_<name>(payload)`.
pub(crate) mod hook {
    pub(crate) use balaur_core::hooks::{COLLISION_ENTER, COLLISION_EXIT};
    pub(crate) const CONTACT_FORCE: &str = "contact_force";
    pub(crate) const JOINT_BREAK: &str = "joint_break";
    pub(crate) const TEAR: &str = "tear";
    pub(crate) const SLEEPING_CHANGED: &str = "sleeping_changed";

    /// What each component announces, for the Events view and the reference.
    pub(crate) const COLLIDER: &[(&str, &str)] = &[
        (COLLISION_ENTER, ENTER),
        (COLLISION_EXIT, EXIT),
        (CONTACT_FORCE, FORCE),
    ];
    /// A body hears what every collider under it hears, as well as its own sleep.
    pub(crate) const BODY: &[(&str, &str)] = &[
        (
            COLLISION_ENTER,
            "`#{ other, sensor, removed, points, normals }` for a collider under it, as that collider hears it",
        ),
        (
            COLLISION_EXIT,
            "`#{ other, sensor, removed }` for a collider under it, as that collider hears it",
        ),
        (
            CONTACT_FORCE,
            "`#{ other, force, direction, total_force, max_force, started }` for a collider under it, as that collider hears it",
        ),
        (SLEEPING_CHANGED, "whether it sleeps now"),
    ];
    const ENTER: &str = "`#{ other, sensor, removed, points, normals }`: the other collider's node, whether either is a sensor, and each contact point on this collider with its normal pointing away from it, in world space; a sensor's has no points";
    const EXIT: &str = "`#{ other, sensor, removed }`: the other collider's node, whether either is a sensor, and whether the touch ended because a collider went away";
    const FORCE: &str = "`#{ other, force, direction, total_force, max_force, started }`; `direction` and `total_force` point from this collider towards the other";
    pub(crate) const JOINT: &[(&str, &str)] = &[(
        JOINT_BREAK,
        "`#{ a, b, force, torque }`: the two ends, and the force and torque it broke at",
    )];
    pub(crate) const SOFT_BODY: &[(&str, &str)] = &[
        (COLLISION_ENTER, ENTER),
        (COLLISION_EXIT, EXIT),
        (CONTACT_FORCE, FORCE),
        (SLEEPING_CHANGED, "whether it sleeps now"),
        (
            TEAR,
            "`#{ pieces, edges, cells, removed_edges, split_particles, inserted_particles, piece_particles, clusters, moved_joints }`, the record `tear_softbody` answers",
        ),
    ];
}

/// Schema text from `(key, spec)` lines: the key comes from `keys`, the spec
/// is the `{ type = ..., description = ... }` table the inspector reads.
pub(crate) fn schema(lines: &[(&str, &str)]) -> String {
    lines
        .iter()
        .map(|(key, spec)| format!("{key} = {spec}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The words a schema property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
