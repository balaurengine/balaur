//! The words and keys only soft bodies spell, apart so the rest of the
//! vocabulary has room; `words` and `keys` re-export both halves.

pub(crate) mod words {
    use crate::vocabulary::words::{
        AUTO, BOTH, BOX, CHAIN, CIRCLE, MESH, NONE, OFF, OUTLINE, POLYLINE, SOLID, SPHERE,
        TRIANGLE_MESH,
    };

    pub(crate) const ROPE_SOFT: &str = "rope";
    pub(crate) const CLOTH: &str = "cloth";
    pub(crate) const CLOTH_TUBE: &str = "cloth_tube";
    pub(crate) const VOLUMETRIC: &str = "volumetric";
    pub(crate) const GRID: &str = "grid";
    pub(crate) const SOFT_POLYGON: &str = "polygon";
    /// A soft body whose particles and elements the author lists.
    pub(crate) const CUSTOM: &str = "custom";
    /// How a 3D soft body's particles and elements are laid out, in the order
    /// the inspector offers them.
    pub(crate) const SOFT_KINDS: &[&str] = &[
        BOX,
        SPHERE,
        CLOTH,
        CLOTH_TUBE,
        ROPE_SOFT,
        VOLUMETRIC,
        TRIANGLE_MESH,
        CUSTOM,
    ];
    /// The 2D layouts. A tetrahedrized volume is a triangulated area here, so
    /// `volumetric` spells the same word in both dimensions.
    pub(crate) const SOFT_KINDS_2D: &[&str] = &[
        GRID,
        CIRCLE,
        SOFT_POLYGON,
        OUTLINE,
        ROPE_SOFT,
        VOLUMETRIC,
        TRIANGLE_MESH,
        POLYLINE,
        CUSTOM,
    ];
    /// Which segments a 2D soft polyline joins: its points in order, or the
    /// edges of its mesh's triangles.
    pub(crate) const POLYLINE_EDGES: &[&str] = &[CHAIN, MESH];

    pub(crate) const ALL: &str = "all";
    pub(crate) const LISTED: &str = "listed";
    /// Which of a soft body's edges resist stretching only.
    pub(crate) const TENSION_MODES: &[&str] = &[NONE, ALL, LISTED];
    /// What a soft-body edge holds: two neighbours, or two second neighbours.
    pub(crate) const STRUCTURAL: &str = "structural";
    pub(crate) const BENDING: &str = "bending";

    pub(crate) const VOLUME: &str = "volume";
    pub(crate) const COROTATIONAL: &str = "corotational";
    pub(crate) const NEO_HOOKEAN: &str = "neo_hookean";
    /// What a cell resists with: a volume constraint, or one of the two
    /// elastic models a Young modulus parameterises.
    pub(crate) const CELL_MODELS: &[&str] = &[VOLUME, COROTATIONAL, NEO_HOOKEAN];

    pub(crate) const CONSTRAINTS: &str = "constraints";
    pub(crate) const FEM: &str = "fem";
    /// Which of rapier's two solvers simulates the body's elasticity.
    pub(crate) const SOFT_SOLVERS: &[&str] = &[CONSTRAINTS, FEM];

    pub(crate) const COMPRESSION_FLOW: &str = "compression";
    pub(crate) const TENSION: &str = "tension";
    /// Whether an edge takes a permanent set under a squeeze, a stretch, or
    /// both.
    pub(crate) const PLASTIC_FLOWS: &[&str] = &[BOTH, COMPRESSION_FLOW, TENSION];

    pub(crate) const SHELL: &str = "shell";
    /// How a soft body's closed surface meets what is inside it: `solid`
    /// encloses matter, `shell` holds bodies in, `auto` is solid when closed.
    pub(crate) const ORIENTATIONS: &[&str] = &[AUTO, SOLID, SHELL];

    pub(crate) const ON: &str = "on";
    /// How a `collision_mesh` follows the body: vertex by particle, by the
    /// nearest particle, or riding the cells that hold it.
    pub(crate) const BIND_PARTICLES: &str = "particles";
    pub(crate) const NEAREST: &str = "nearest";
    pub(crate) const BIND_CELLS: &str = "cells";
    pub(crate) const COLLISION_BINDINGS: &[&str] = &[NEAREST, BIND_PARTICLES, BIND_CELLS];
    /// Whether a soft body is pulled back to its built shape; `auto` keeps the
    /// layout's own choice.
    pub(crate) const SHAPE_MATCHING_MODES: &[&str] = &[AUTO, ON, OFF];

    pub(crate) const KEEP: &str = "keep";
    pub(crate) const STAND_DOWN: &str = "stand_down";
    pub(crate) const ALONG_NORMAL: &str = "along_normal";
    /// What the point contacts inside a soft body's volume contact do.
    pub(crate) const PATCH_CONSTRAINTS: &[&str] = &[KEEP, STAND_DOWN, ALONG_NORMAL];
}

pub(crate) mod keys {
    pub(crate) const AUTHORED_VELOCITY_MARGIN: &str = "authored_velocity_margin";
    pub(crate) const BEND_DAMPING: &str = "bend_damping";
    pub(crate) const BEND_EDGE_INDICES: &str = "bend_edge_indices";
    pub(crate) const BEND_HZ: &str = "bend_hz";
    pub(crate) const BOUNDARY_SUBDIVISIONS: &str = "boundary_subdivisions";
    pub(crate) const CELL_INDICES: &str = "cell_indices";
    pub(crate) const CELL_MODEL: &str = "cell_model";
    pub(crate) const CELL_SIZE: &str = "cell_size";
    pub(crate) const CLUSTER: &str = "cluster";
    pub(crate) const CLUSTERS: &str = "clusters";
    pub(crate) const COLLIDES: &str = "collides";
    pub(crate) const COLLISION_MESH: &str = "collision_mesh";
    pub(crate) const COLLISION_BINDING: &str = "collision_binding";
    pub(crate) const COLLISION_BINDING_DISTANCE: &str = "collision_binding_distance";
    pub(crate) const COLLISION_SELF_CONTACTS: &str = "collision_self_contacts";
    pub(crate) const COLOR: &str = "color";
    pub(crate) const CROSSING_REPULSION: &str = "crossing_repulsion";
    pub(crate) const CROSSING_REPULSION_GUIDE: &str = "crossing_repulsion_guide";
    pub(crate) const CROSSING_REPULSION_SELF_GUIDE: &str = "crossing_repulsion_self_guide";
    pub(crate) const CROSS_BODY_DETECTION: &str = "cross_body_detection";
    pub(crate) const CROSS_BODY_EXPEL_GATE: &str = "cross_body_expel_gate";
    pub(crate) const DAMAGED: &str = "damaged";
    pub(crate) const DEFORMATION_DAMPING: &str = "deformation_damping";
    pub(crate) const DETECTION_MOTION_GATING: &str = "detection_motion_gating";
    pub(crate) const DIHEDRAL_INDICES: &str = "dihedral_indices";
    pub(crate) const EDGE_DAMPING: &str = "edge_damping";
    pub(crate) const EDGE_HZ: &str = "edge_hz";
    pub(crate) const EDGE_INDICES: &str = "edge_indices";
    pub(crate) const EDGE_PLASTIC_CREEP: &str = "edge_plastic_creep";
    pub(crate) const EDGE_PLASTIC_FLOW: &str = "edge_plastic_flow";
    pub(crate) const EDGE_PLASTIC_MAX: &str = "edge_plastic_max";
    pub(crate) const EDGE_PLASTIC_YIELD: &str = "edge_plastic_yield";
    pub(crate) const EDGE_SPECULATION: &str = "edge_speculation";
    pub(crate) const EDGE_SPRINGS: &str = "edge_springs";
    pub(crate) const EDGE_STAND_DOWN: &str = "edge_stand_down";
    pub(crate) const ELASTIC_DAMPING: &str = "elastic_damping";
    pub(crate) const END_RADIUS: &str = "end_radius";
    pub(crate) const HZ: &str = "hz";
    pub(crate) const INITIAL_REST_ANGLE: &str = "initial_rest_angle";
    pub(crate) const INITIAL_REST_LENGTH: &str = "initial_rest_length";
    pub(crate) const INITIAL_REST_POSITION: &str = "initial_rest_position";
    pub(crate) const INSERTED_PARTICLES: &str = "inserted_particles";
    pub(crate) const INTERIOR_STRENGTH: &str = "interior_strength";
    pub(crate) const INVERSE_MASS: &str = "inverse_mass";
    pub(crate) const INVERTED_CELL_DETECTION: &str = "inverted_cell_detection";
    pub(crate) const KEEPS_PROXY: &str = "keeps_proxy";
    pub(crate) const MASSES: &str = "masses";
    pub(crate) const MAX_TEARS_PER_STEP: &str = "max_tears_per_step";
    pub(crate) const MIN_ANGLE: &str = "min_angle";
    pub(crate) const MIN_PIECE: &str = "min_piece";
    pub(crate) const MOVED_JOINTS: &str = "moved_joints";
    pub(crate) const ON_SURFACE: &str = "on_surface";
    pub(crate) const ORIENTATION: &str = "orientation";
    pub(crate) const OVERLAP_CONSTRAINTS: &str = "overlap_constraints";
    pub(crate) const OVERLAP_CONSTRAINT_PACE: &str = "overlap_constraint_pace";
    pub(crate) const OVERLAP_EDGE_STAND_DOWN: &str = "overlap_edge_stand_down";
    pub(crate) const OVERLAP_KEPT_DEPTH: &str = "overlap_kept_depth";
    pub(crate) const OVERLAP_MULTI_VOLUME: &str = "overlap_multi_volume";
    pub(crate) const OVERLAP_NORMAL_PUSH: &str = "overlap_normal_push";
    pub(crate) const OVERLAP_PATCH_CONSTRAINTS: &str = "overlap_patch_constraints";
    pub(crate) const OVERLAP_PATIENCE_TICKS: &str = "overlap_patience_ticks";
    pub(crate) const OVERLAP_PROGRESS_MARGIN: &str = "overlap_progress_margin";
    pub(crate) const OVERLAP_RIGID_BODIES: &str = "overlap_rigid_bodies";
    pub(crate) const OVERLAP_SELF_REGIONS: &str = "overlap_self_regions";
    pub(crate) const OVERLAP_SKIN_VOLUME: &str = "overlap_skin_volume";
    pub(crate) const OVERLAP_SKIP_SELF_TANGLED: &str = "overlap_skip_self_tangled";
    pub(crate) const OVERLAP_SPLIT: &str = "overlap_split";
    pub(crate) const PARTICLE: &str = "particle";
    pub(crate) const PARTICLES: &str = "particles";
    pub(crate) const PARTICLE_COUNT: &str = "particle_count";
    pub(crate) const PARTICLE_RADIUS: &str = "particle_radius";
    pub(crate) const PIECE_PARTICLES: &str = "piece_particles";
    pub(crate) const PINNED: &str = "pinned";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const TARGET: &str = "target";
    pub(crate) const PINNED_PARTICLES: &str = "pinned_particles";
    pub(crate) const PLASTIC_CREEP: &str = "plastic_creep";
    pub(crate) const PLASTIC_MAX: &str = "plastic_max";
    pub(crate) const PLASTIC_SET: &str = "plastic_set";
    pub(crate) const PLASTIC_STRAIN: &str = "plastic_strain";
    pub(crate) const PLASTIC_STRETCH: &str = "plastic_stretch";
    pub(crate) const PLASTIC_YIELD: &str = "plastic_yield";
    pub(crate) const POISSON_RATIO: &str = "poisson_ratio";
    pub(crate) const RECOVERY_PACE: &str = "recovery_pace";
    pub(crate) const REMOVED_EDGES: &str = "removed_edges";
    pub(crate) const RESISTANCE: &str = "resistance";
    pub(crate) const REST_ANGLE: &str = "rest_angle";
    pub(crate) const REST_POSITION: &str = "rest_position";
    pub(crate) const REST_VOLUME: &str = "rest_volume";
    /// Parts of a soft body with a material of their own, each a rapier cluster.
    pub(crate) const REGIONS: &str = "regions";
    pub(crate) const SEAMS: &str = "seams";
    pub(crate) const SELF_CROSSING_DETECTION: &str = "self_crossing_detection";
    pub(crate) const SELF_STAND_DOWN: &str = "self_stand_down";
    pub(crate) const SHAPE_MATCHING: &str = "shape_matching";
    pub(crate) const SHAPE_MATCHING_DAMPING: &str = "shape_matching_damping";
    pub(crate) const SHAPE_MATCHING_HZ: &str = "shape_matching_hz";
    pub(crate) const SHEAR_DAMPING: &str = "shear_damping";
    pub(crate) const SHEAR_HZ: &str = "shear_hz";
    pub(crate) const SKIN: &str = "skin";
    pub(crate) const SKIN_COLLISION: &str = "skin_collision";
    pub(crate) const SMOOTHING: &str = "smoothing";
    pub(crate) const SMOOTHING_GUARD: &str = "smoothing_guard";
    pub(crate) const SOLVER: &str = "solver";
    pub(crate) const SOLVER_SUBSTEPS: &str = "solver_substeps";
    pub(crate) const SPLIT_PARTICLES: &str = "split_particles";
    pub(crate) const STIFFNESS_SCALE: &str = "stiffness_scale";
    pub(crate) const STRESS: &str = "stress";
    pub(crate) const SURFACE_INDICES: &str = "surface_indices";
    pub(crate) const TEAR_FORCE: &str = "tear_force";
    pub(crate) const TEAR_RESISTANCE: &str = "tear_resistance";
    pub(crate) const TEAR_SMOOTHING: &str = "tear_smoothing";
    pub(crate) const TEAR_STRAIN: &str = "tear_strain";
    pub(crate) const TENSION_ONLY: &str = "tension_only";
    pub(crate) const TENSION_ONLY_EDGES: &str = "tension_only_edges";
    pub(crate) const VERTICES: &str = "vertices";
    pub(crate) const VOLUMES: &str = "volumes";
    pub(crate) const VOLUME_DAMPING: &str = "volume_damping";
    pub(crate) const VOLUME_FACTOR: &str = "volume_factor";
    pub(crate) const VOLUME_HZ: &str = "volume_hz";
    pub(crate) const VOLUME_PRESERVATION: &str = "volume_preservation";
    pub(crate) const WARP_DAMPING: &str = "warp_damping";
    pub(crate) const WARP_HZ: &str = "warp_hz";
    pub(crate) const WEFT_DAMPING: &str = "weft_damping";
    pub(crate) const WEFT_HZ: &str = "weft_hz";
    pub(crate) const WIRE_INDICES: &str = "wire_indices";
    pub(crate) const YOUNG_MODULUS: &str = "young_modulus";
}
