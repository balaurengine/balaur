//! A soft body's schema, a function a group in the order the inspector
//! shows them; each dimension adds the layout rows only it has.

use crate::vocabulary::{self as v, keys as k, words as w};

/// The rows first: how the body is laid out, from `kinds` with `default`
/// first, then whether it simulates, what it weighs and what it is drawn in.
pub(crate) fn primary_schema(kinds: &[&str], default: &str) -> String {
    let kinds = v::options(kinds);
    v::schema(&[
        (
            k::KIND,
            &format!(
                r#"{{ type = "enum", default = "{default}", options = [{kinds}], description = "How the body's particles and elements are laid out" }}"#
            ),
        ),
        (
            k::ENABLED,
            r#"{ type = "bool", default = true, description = "Take part in the simulation; off leaves the body where it is with no contacts or constraints until it is on again, without a rebuild" }"#,
        ),
        (
            k::MASS,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "What the whole body weighs, spread over its particles" }"#,
        ),
        (
            k::GRAVITY_SCALE,
            r#"{ type = "float", default = 1.0, description = "How much gravity pulls on the particles" }"#,
        ),
        (
            k::COLOR,
            &format!(
                r#"{{ type = "color", default = {:?}, description = "What the body is drawn in when its node has nothing of its own to deform, as a cloth or a rope has not" }}"#,
                super::DEFAULT_COLOR
            ),
        ),
        (k::SOFT_MATERIAL, &crate::soft_material::row_spec()),
        (k::PHYSICS_MATERIAL, &crate::material::row_spec()),
    ])
}

/// Where the body may tear first, along a run of edges.
pub(crate) fn seams_schema() -> String {
    let a = k::A;
    let b = k::B;
    v::schema(&[(
        k::SEAMS,
        &format!(
            r#"{{ type = "list", of = {{ type = "record", fields = {{ {a} = {{ type = "int", default = 0 }}, {b} = {{ type = "int", default = 1 }} }} }}, default = [], description = "Structural edges added between particles the layout left apart, each as long as its two particles stand at the start", group = "shape" }}"#
        ),
    )])
}

/// What single particles weigh, and how they slow and rest.
pub(crate) fn particles_schema() -> String {
    v::schema(&[
        (
            k::PINNED_PARTICLES,
            r#"{ type = "list", of = { type = "int" }, default = [], description = "The particles held where they are, by index: a cloth hangs from these, and `softbody_particles` says how many there are to choose from", group = "particles" }"#,
        ),
        (
            k::MASSES,
            r#"{ type = "list", of = { type = "float" }, default = [], description = "Each particle's own mass, by index; empty spreads `mass` over them evenly", group = "particles" }"#,
        ),
        (
            k::LINEAR_DAMPING,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Air friction on the particles", group = "particles" }"#,
        ),
        (
            k::CAN_SLEEP,
            r#"{ type = "bool", default = true, description = "Let the body stop being simulated once it settles", group = "particles" }"#,
        ),
    ])
}

/// How the body meets the world and itself.
pub(crate) fn collision_schema() -> String {
    let auto = w::AUTO;
    let orientations = v::options(w::ORIENTATIONS);
    v::schema(&[
        (
            k::COLLIDES,
            r#"{ type = "bool", default = true, description = "Meet the world at all; off, the body passes through everything and only its pins and ties hold it", group = "collision" }"#,
        ),
        (
            k::SENSOR,
            r#"{ type = "bool", default = false, description = "Report contacts and push nothing: the body passes through what it touches", group = "collision" }"#,
        ),
        (
            k::SELF_COLLISION,
            r#"{ type = "bool", default = false, description = "Let the body's own surface collide with itself, which stops a cloth passing through its own fold", group = "collision" }"#,
        ),
        (
            k::PARTICLE_RADIUS,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "How thick the particles are; 0 takes what the layout works out", group = "collision" }"#,
        ),
        (
            k::ORIENTATION,
            &format!(
                r#"{{ type = "enum", default = "{auto}", options = [{orientations}], description = "How the surface meets what is inside it: solid encloses matter and pushes bodies out, shell holds bodies in, auto is solid when the surface is closed", group = "collision" }}"#
            ),
        ),
        (
            k::SKIN_COLLISION,
            r#"{ type = "bool", default = false, description = "Meet the world through the skin rather than the cells' boundary", group = "collision" }"#,
        ),
        (
            k::COLLISION_MESH,
            r#"{ type = "asset", asset = "mesh", default = "", description = "A mesh, in the node's space, the body meets the world through beside its own surface, deformed with the body: its triangles in 3D, the outline of its triangles in 2D", group = "collision" }"#,
        ),
        (
            k::COLLISION_BINDING,
            &format!(
                r#"{{ type = "enum", default = "{nearest}", options = [{bindings}], description = "How the collision mesh follows the body: each vertex on the nearest particle within `collision_binding_distance`, vertex i on particle i, or riding the cell that holds it, so a coarse cage of cells can carry a fine mesh", group = "collision" }}"#,
                nearest = crate::vocabulary::words::NEAREST,
                bindings = crate::vocabulary::options(crate::vocabulary::words::COLLISION_BINDINGS),
            ),
        ),
        (
            k::COLLISION_BINDING_DISTANCE,
            r#"{ type = "float", default = 0.01, min = 0.0, description = "How far a collision-mesh vertex may sit from the particle `nearest` binds it to", group = "collision" }"#,
        ),
        (
            k::COLLISION_SELF_CONTACTS,
            r#"{ type = "bool", default = false, description = "Let the collision mesh collide with itself", group = "collision" }"#,
        ),
    ])
}

/// What the body's collider is filtered by, as a collider's is.
pub(crate) fn filtering_schema() -> String {
    v::schema(&[
        (
            k::COLLISION_LAYER,
            &crate::collider::spec::collision_layer(),
        ),
        (k::COLLISION_MASK, &crate::collider::spec::collision_mask()),
        (k::SOLVER_LAYER, &crate::collider::spec::solver_layer()),
        (k::SOLVER_MASK, &crate::collider::spec::solver_mask()),
        (k::CONTACT_PAIRS, &crate::collider::spec::contact_pairs()),
    ])
}

/// Per-part and per-edge exceptions to what the body is made of.
pub(crate) fn overrides_schema() -> String {
    let hz = k::HZ;
    let damping = k::DAMPING;
    let a = k::A;
    let b = k::B;
    let none = w::NONE;
    let tensions = v::options(w::TENSION_MODES);
    v::schema(&[
        (
            k::REGIONS,
            &format!(
                r#"{{ type = "list", of = {{ type = "record", fields = {{ {particles} = {{ type = "list", of = {{ type = "int" }}, default = [] }}, {stiffness} = {{ type = "float", default = 1.0 }}, {tear} = {{ type = "float", default = 1.0 }}, {matching} = {{ type = "bool", default = false }}, {pinned} = {{ type = "bool", default = false }}, {hz} = {{ type = "float", default = 0.0 }}, {damping} = {{ type = "float", default = 1.0 }} }} }}, default = [], description = "Parts of the body with a material of their own, each the particles it covers: `stiffness_scale` on the cells wholly inside, `tear_resistance` on its edges and cells, `shape_matching` towards its own frame, `pinned` to hold it still, and an edge spring of `hz` and `damping`, 0 hz keeping the body's. Each is a `soft_frame` body joints may attach to", group = "overrides" }}"#,
                particles = k::PARTICLES,
                stiffness = k::STIFFNESS_SCALE,
                tear = k::TEAR_RESISTANCE,
                matching = k::SHAPE_MATCHING,
                pinned = k::PINNED,
            ),
        ),
        (
            k::EDGE_SPRINGS,
            &format!(
                r#"{{ type = "list", of = {{ type = "record", fields = {{ {a} = {{ type = "int", default = 0 }}, {b} = {{ type = "int", default = 1 }}, {hz} = {{ type = "float", default = 30.0 }}, {damping} = {{ type = "float", default = 1.0 }} }} }}, default = [], description = "Edges with a spring of their own, its frequency in hertz and its damping ratio, instead of the edge or bend rows', each named by the two particles it joins", group = "overrides" }}"#
            ),
        ),
        (
            k::TENSION_ONLY,
            &format!(
                r#"{{ type = "enum", default = "{none}", options = [{tensions}], description = "Which edges resist stretching only, so the body folds freely and never pushes itself open: none, all, or the tension_only_edges listed", group = "overrides" }}"#
            ),
        ),
        (
            k::TENSION_ONLY_EDGES,
            &format!(
                r#"{{ type = "list", of = {{ type = "record", fields = {{ {a} = {{ type = "int", default = 0 }}, {b} = {{ type = "int", default = 1 }} }} }}, default = [], description = "The edges that resist stretching only when tension_only is listed, each named by the two particles it joins", group = "overrides" }}"#
            ),
        ),
    ])
}

/// How a crack is paced on this body, and which edges resist it.
pub(crate) fn tearing_schema() -> String {
    let a = k::A;
    let b = k::B;
    let resistance = k::RESISTANCE;
    v::schema(&[
        (
            k::TEAR_RESISTANCE,
            &format!(
                r#"{{ type = "list", of = {{ type = "record", fields = {{ {a} = {{ type = "int", default = 0 }}, {b} = {{ type = "int", default = 1 }}, {resistance} = {{ type = "float", default = 1.0 }} }} }}, default = [], description = "Edges that tear sooner or later than the rest, each named by the two particles it joins: below 1 is a perforation, above 1 a seam", group = "tearing" }}"#
            ),
        ),
        (
            k::MAX_TEARS_PER_STEP,
            r#"{ type = "int", default = 0, min = 0, description = "The most edges that may tear in one step, which paces a crack; 0 is no limit", group = "tearing" }"#,
        ),
        (
            k::MIN_PIECE,
            r#"{ type = "int", default = 0, min = 0, description = "The smallest piece, in elements, a tear may split off; 0 lets rapier choose", group = "tearing" }"#,
        ),
    ])
}

/// How the body is solved.
pub(crate) fn solver_schema() -> String {
    let constraints = w::CONSTRAINTS;
    let solvers = v::options(w::SOFT_SOLVERS);
    v::schema(&[
        (
            k::SOLVER,
            &format!(
                r#"{{ type = "enum", default = "{constraints}", options = [{solvers}], description = "Which solver runs the elasticity: sequential constraints, or an implicit Euler step over the whole body", group = "solver" }}"#
            ),
        ),
        (
            k::SOLVER_ITERATIONS,
            r#"{ type = "int", default = 3, min = 0, max = 64, description = "Extra iterations inside each substep, for the same", group = "solver" }"#,
        ),
        (
            k::SOLVER_SUBSTEPS,
            r#"{ type = "int", default = 0, min = 0, max = 64, description = "Extra solver substeps for this body and everything it touches", group = "solver" }"#,
        ),
        (
            k::DOMINANCE,
            r#"{ type = "int", default = 0, min = -127, max = 127, description = "Which body wins a contact: a higher one is never pushed by a lower one", group = "solver" }"#,
        ),
    ])
}
