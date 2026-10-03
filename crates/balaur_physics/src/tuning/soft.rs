//! The soft-body half of a world's parameters: `[physics] soft_*` and
//! `[physics.soft_recovery]`, which rapier keeps on
//! `IntegrationParameters::soft_bodies`.

use crate::vocabulary::{self as v, keys as k, words as w};

/// The `[physics]` rows every soft body of a world shares.
pub(super) fn schema() -> String {
    v::schema(&[
        (
            k::SOFT_RESWEEP_STRAIN,
            r#"{ type = "float", default = 0.75, min = 0.0, help = "The strain past which a soft body's constraint is solved again after the contacts in every substep, so a light body under heavier ones is not torn." }"#,
        ),
        (
            k::SOFT_MAX_EXTRA_SUBSTEPS,
            r#"{ type = "int", default = 4, min = 0, max = 64, help = "The most extra substeps a soft body asks for while it is hit fast; 0 asks for none." }"#,
        ),
        (
            k::SOFT_CONTACT_STIFFENING,
            r#"{ type = "float", default = 4.0, min = 0.0, help = "How many times stiffer than contact_frequency_hz a soft body's contacts are solved, as a contact holds a few particles' mass rather than the body's." }"#,
        ),
        (
            k::SOFT_LINEAR_TOLERANCE,
            r#"{ type = "float", default = 0.00001, min = 0.0, help = "Where the fem solver's linear solve stops, as a relative residual." }"#,
        ),
        (
            k::SOFT_MAX_LINEAR_ITERATIONS,
            r#"{ type = "int", default = 20, min = 0, max = 10000, help = "The most iterations the fem solver's linear solve takes." }"#,
        ),
        (
            k::SOFT_MAX_DENSE_DOFS,
            r#"{ type = "int", default = 600, min = 0, max = 1000000, help = "The largest fem body, in particles times dimensions, solved directly rather than iteratively." }"#,
        ),
    ])
}

/// `[physics.soft_recovery]`: how soft bodies find and undo tangles, one row
/// per switch and number rapier has.
pub(super) fn recovery_schema() -> String {
    let flag = |key: &str, default: bool, help: &str| {
        format!(r#"{key} = {{ type = "bool", default = {default}, help = "{help}" }}"#)
    };
    let patch = v::options(w::PATCH_CONSTRAINTS);
    let along = w::ALONG_NORMAL;
    [
        flag(k::AUTHORED_VELOCITY_MARGIN, true, "Widen the contact margin for velocities set between steps, so a fast new body does not tunnel."),
        flag(k::EDGE_SPECULATION, false, "Let edge contacts reach ahead, so bodies crossing corner first collide; on, pressed 3D piles stay crossed."),
        flag(k::INVERTED_CELL_DETECTION, true, "Look for cells turned inside out each step."),
        flag(k::SELF_CROSSING_DETECTION, true, "Look for a surface crossing itself each step."),
        flag(k::DETECTION_MOTION_GATING, true, "Skip that look while the surface has not moved far enough to cross itself."),
        flag(k::CROSS_BODY_DETECTION, true, "Look for two soft surfaces crossing each other."),
        flag(k::SELF_STAND_DOWN, true, "Let go of a tangled part's own contacts, so its elasticity undoes the tangle."),
        flag(k::CROSS_BODY_EXPEL_GATE, true, "Let a contact where two surfaces cross push apart only, never hold."),
        flag(k::EDGE_STAND_DOWN, true, "Let go of edge contacts where two surfaces cross."),
        flag(k::CROSSING_REPULSION, false, "Push a crossed part back through instead of letting go of it."),
        flag(k::CROSSING_REPULSION_GUIDE, false, "Push it along the pair's overlap direction rather than the crossed face's normal; closed bodies only."),
        flag(k::CROSSING_REPULSION_SELF_GUIDE, false, "Pull a part folded into its own body out along the fold's direction; closed bodies only."),
        format!(r#"{} = {{ type = "float", default = 0.5, min = 0.0, help = "How fast a tangle is undone, in length units per second scaled by length_unit." }}"#, k::RECOVERY_PACE),
        flag(k::OVERLAP_CONSTRAINTS, true, "Push two closed surfaces apart by how much they overlap, one constraint a pair; switches the overlap rows below."),
        flag(k::OVERLAP_RIGID_BODIES, true, "Do the same against rigid colliders."),
        flag(k::OVERLAP_SKIP_SELF_TANGLED, true, "Leave out a body crossing itself, whose overlap points the wrong way."),
        flag(k::OVERLAP_EDGE_STAND_DOWN, true, "Let go of the 3D edge contacts of a pair an overlap constraint holds."),
        format!(r#"{} = {{ type = "float", default = 1.0, min = 0.0, help = "The most an overlap constraint may change a side's speed a step, in multiples of recovery_pace." }}"#, k::OVERLAP_CONSTRAINT_PACE),
        format!(r#"{} = {{ type = "enum", default = "{along}", options = [{patch}], help = "What the point contacts inside an overlap do: stay, let go, or push along the overlap's normal." }}"#, k::OVERLAP_PATCH_CONSTRAINTS),
        flag(k::OVERLAP_SKIN_VOLUME, false, "Measure the overlap between the surfaces grown by their contact skins, so resting bodies hold apart before the skins meet."),
        format!(r#"{} = {{ type = "float", default = 0.0, min = 0.0, help = "The overlap kept at rest, as a fraction of the two skins." }}"#, k::OVERLAP_KEPT_DEPTH),
        flag(k::OVERLAP_SELF_REGIONS, false, "Push apart two parts of one closed body that overlap each other."),
        flag(k::OVERLAP_NORMAL_PUSH, true, "Push a whole overlap along one direction rather than each point along its own."),
        flag(k::OVERLAP_MULTI_VOLUME, false, "Split each overlap into a grid of constraints, so pressure varies across it."),
        format!(r#"{} = {{ type = "int", default = 3, min = 1, max = 16, help = "Cells along each side of that grid." }}"#, k::OVERLAP_SPLIT),
        format!(r#"{} = {{ type = "int", default = 240, min = 0, help = "Steps an overlap may go without shrinking before it stops being pushed." }}"#, k::OVERLAP_PATIENCE_TICKS),
        format!(r#"{} = {{ type = "float", default = 0.02, min = 0.0, help = "How much an overlap must shrink, as a fraction, to count as shrinking." }}"#, k::OVERLAP_PROGRESS_MARGIN),
    ]
    .join("\n")
}

/// The soft-body rows onto one world's parameters. `f` and `boolean` read a
/// `[physics]` key; `rf`, `rb` and `rw` read a `soft_recovery` one, a word
/// for `rw`. An absent key keeps what the world has.
macro_rules! write_soft {
    ($rapier:ident, $p:expr_2021, $f:expr_2021, $rf:expr_2021, $rb:expr_2021, $rw:expr_2021) => {{
        use crate::vocabulary::{keys as k, words as w};
        let s = &mut $p.soft_bodies;
        let real = |read: &dyn Fn(&str, f32) -> f32, key: &str, now: crate::scalar::Real| {
            crate::scalar::real(read(key, crate::scalar::f32_of(now)).max(0.0))
        };
        let count = |read: &dyn Fn(&str, f32) -> f32, key: &str, now: usize| {
            read(key, now as f32).max(0.0) as usize
        };
        s.resweep_strain = real(&$f, k::SOFT_RESWEEP_STRAIN, s.resweep_strain);
        s.max_extra_substeps = count(&$f, k::SOFT_MAX_EXTRA_SUBSTEPS, s.max_extra_substeps);
        s.contact_stiffening = real(&$f, k::SOFT_CONTACT_STIFFENING, s.contact_stiffening);
        s.fem.linear_tolerance = real(&$f, k::SOFT_LINEAR_TOLERANCE, s.fem.linear_tolerance);
        s.fem.max_linear_iterations = count(
            &$f,
            k::SOFT_MAX_LINEAR_ITERATIONS,
            s.fem.max_linear_iterations,
        );
        s.fem.max_dense_dofs = count(&$f, k::SOFT_MAX_DENSE_DOFS, s.fem.max_dense_dofs);
        let r = &mut s.recovery;
        let b = $rb;
        r.authored_velocity_margin = b(k::AUTHORED_VELOCITY_MARGIN, r.authored_velocity_margin);
        r.edge_speculation = b(k::EDGE_SPECULATION, r.edge_speculation);
        r.inverted_cell_detection = b(k::INVERTED_CELL_DETECTION, r.inverted_cell_detection);
        r.self_crossing_detection = b(k::SELF_CROSSING_DETECTION, r.self_crossing_detection);
        r.detection_motion_gating = b(k::DETECTION_MOTION_GATING, r.detection_motion_gating);
        r.cross_body_detection = b(k::CROSS_BODY_DETECTION, r.cross_body_detection);
        r.self_stand_down = b(k::SELF_STAND_DOWN, r.self_stand_down);
        r.cross_body_expel_gate = b(k::CROSS_BODY_EXPEL_GATE, r.cross_body_expel_gate);
        r.edge_stand_down = b(k::EDGE_STAND_DOWN, r.edge_stand_down);
        r.crossing_repulsion = b(k::CROSSING_REPULSION, r.crossing_repulsion);
        r.crossing_repulsion_guide = b(k::CROSSING_REPULSION_GUIDE, r.crossing_repulsion_guide);
        r.crossing_repulsion_self_guide = b(
            k::CROSSING_REPULSION_SELF_GUIDE,
            r.crossing_repulsion_self_guide,
        );
        r.recovery_pace = real(&$rf, k::RECOVERY_PACE, r.recovery_pace);
        r.overlap_constraints = b(k::OVERLAP_CONSTRAINTS, r.overlap_constraints);
        r.overlap_rigid = b(k::OVERLAP_RIGID_BODIES, r.overlap_rigid);
        r.overlap_skip_self_tangled = b(k::OVERLAP_SKIP_SELF_TANGLED, r.overlap_skip_self_tangled);
        r.overlap_edge_stand_down = b(k::OVERLAP_EDGE_STAND_DOWN, r.overlap_edge_stand_down);
        r.overlap_constraint_pace =
            real(&$rf, k::OVERLAP_CONSTRAINT_PACE, r.overlap_constraint_pace);
        {
            use crate::$rapier::dynamics::SoftPatchConstraints as Patch;
            r.overlap_patch_constraints = match ($rw)(k::OVERLAP_PATCH_CONSTRAINTS).as_deref() {
                Some(w::KEEP) => Patch::Keep,
                Some(w::STAND_DOWN) => Patch::StandDown,
                Some(w::ALONG_NORMAL) => Patch::AlongNormal,
                _ => r.overlap_patch_constraints,
            };
        }
        r.overlap_skin_volume = b(k::OVERLAP_SKIN_VOLUME, r.overlap_skin_volume);
        r.overlap_kept_depth = real(&$rf, k::OVERLAP_KEPT_DEPTH, r.overlap_kept_depth);
        r.overlap_self_regions = b(k::OVERLAP_SELF_REGIONS, r.overlap_self_regions);
        r.overlap_normal_push = b(k::OVERLAP_NORMAL_PUSH, r.overlap_normal_push);
        r.overlap_multi_volume = b(k::OVERLAP_MULTI_VOLUME, r.overlap_multi_volume);
        r.overlap_split = count(&$rf, k::OVERLAP_SPLIT, r.overlap_split as usize).max(1) as u32;
        r.overlap_patience =
            count(&$rf, k::OVERLAP_PATIENCE_TICKS, r.overlap_patience as usize) as u32;
        r.overlap_progress_margin =
            real(&$rf, k::OVERLAP_PROGRESS_MARGIN, r.overlap_progress_margin);
    }};
}

pub(crate) use write_soft;

/// One world's soft-body rows as the table `physics.tuning` reads back, the
/// recovery rows under `soft_recovery`.
macro_rules! soft_value {
    ($rapier:ident, $p:expr_2021) => {{
        use crate::vocabulary::{keys as k, words as w};
        use balaur_script::Value;
        let s = &$p.soft_bodies;
        let r = &s.recovery;
        let number = |x: crate::scalar::Real| Value::Num(f64::from(crate::scalar::f32_of(x)));
        let count = |n: usize| Value::Int(i64::try_from(n).unwrap_or(i64::MAX));
        let patch = {
            use crate::$rapier::dynamics::SoftPatchConstraints as Patch;
            match r.overlap_patch_constraints {
                Patch::Keep => w::KEEP,
                Patch::StandDown => w::STAND_DOWN,
                Patch::AlongNormal => w::ALONG_NORMAL,
            }
        };
        let recovery: Vec<(&str, Value)> = vec![
            (
                k::AUTHORED_VELOCITY_MARGIN,
                Value::Bool(r.authored_velocity_margin),
            ),
            (k::EDGE_SPECULATION, Value::Bool(r.edge_speculation)),
            (
                k::INVERTED_CELL_DETECTION,
                Value::Bool(r.inverted_cell_detection),
            ),
            (
                k::SELF_CROSSING_DETECTION,
                Value::Bool(r.self_crossing_detection),
            ),
            (
                k::DETECTION_MOTION_GATING,
                Value::Bool(r.detection_motion_gating),
            ),
            (k::CROSS_BODY_DETECTION, Value::Bool(r.cross_body_detection)),
            (k::SELF_STAND_DOWN, Value::Bool(r.self_stand_down)),
            (
                k::CROSS_BODY_EXPEL_GATE,
                Value::Bool(r.cross_body_expel_gate),
            ),
            (k::EDGE_STAND_DOWN, Value::Bool(r.edge_stand_down)),
            (k::CROSSING_REPULSION, Value::Bool(r.crossing_repulsion)),
            (
                k::CROSSING_REPULSION_GUIDE,
                Value::Bool(r.crossing_repulsion_guide),
            ),
            (
                k::CROSSING_REPULSION_SELF_GUIDE,
                Value::Bool(r.crossing_repulsion_self_guide),
            ),
            (k::RECOVERY_PACE, number(r.recovery_pace)),
            (k::OVERLAP_CONSTRAINTS, Value::Bool(r.overlap_constraints)),
            (k::OVERLAP_RIGID_BODIES, Value::Bool(r.overlap_rigid)),
            (
                k::OVERLAP_SKIP_SELF_TANGLED,
                Value::Bool(r.overlap_skip_self_tangled),
            ),
            (
                k::OVERLAP_EDGE_STAND_DOWN,
                Value::Bool(r.overlap_edge_stand_down),
            ),
            (
                k::OVERLAP_CONSTRAINT_PACE,
                number(r.overlap_constraint_pace),
            ),
            (k::OVERLAP_PATCH_CONSTRAINTS, Value::Str(patch.into())),
            (k::OVERLAP_SKIN_VOLUME, Value::Bool(r.overlap_skin_volume)),
            (k::OVERLAP_KEPT_DEPTH, number(r.overlap_kept_depth)),
            (k::OVERLAP_SELF_REGIONS, Value::Bool(r.overlap_self_regions)),
            (k::OVERLAP_NORMAL_PUSH, Value::Bool(r.overlap_normal_push)),
            (k::OVERLAP_MULTI_VOLUME, Value::Bool(r.overlap_multi_volume)),
            (k::OVERLAP_SPLIT, count(r.overlap_split as usize)),
            (
                k::OVERLAP_PATIENCE_TICKS,
                count(r.overlap_patience as usize),
            ),
            (
                k::OVERLAP_PROGRESS_MARGIN,
                number(r.overlap_progress_margin),
            ),
        ];
        vec![
            (k::SOFT_RESWEEP_STRAIN, number(s.resweep_strain)),
            (k::SOFT_MAX_EXTRA_SUBSTEPS, count(s.max_extra_substeps)),
            (k::SOFT_CONTACT_STIFFENING, number(s.contact_stiffening)),
            (k::SOFT_LINEAR_TOLERANCE, number(s.fem.linear_tolerance)),
            (
                k::SOFT_MAX_LINEAR_ITERATIONS,
                count(s.fem.max_linear_iterations),
            ),
            (k::SOFT_MAX_DENSE_DOFS, count(s.fem.max_dense_dofs)),
            (
                k::SOFT_RECOVERY,
                Value::Map(
                    recovery
                        .into_iter()
                        .map(|(key, value)| (key.to_string(), value))
                        .collect(),
                ),
            ),
        ]
    }};
}

pub(crate) use soft_value;
