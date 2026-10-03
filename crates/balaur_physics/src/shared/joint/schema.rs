//! The schema rows both joint components share: the `axes` records, the
//! lengths and switches, and what an articulation's chain holds.

use super::DEFAULT_DAMPING;
use crate::scalar::Real;

/// The schema both dimensions share; each adds its own axis-shaped half and
/// names the words an `axes` record's `axis` takes.
pub(crate) fn schema(axis_words: &[&str], default_axis: &str) -> String {
    use crate::vocabulary::{self as v, keys as k, words as w};
    let axis_options = v::options(axis_words);
    let motors = v::options(w::MOTOR_MODES);
    let models = v::options(w::MOTOR_MODELS);
    let (off, auto) = (w::OFF, w::AUTO);
    let (axis, limits, motor, target) = (k::AXIS, k::LIMITS, k::MOTOR, k::MOTOR_TARGET);
    let (velocity, max_force) = (k::MOTOR_TARGET_VELOCITY, k::MOTOR_MAX_FORCE);
    let (model, stiffness, damping) = (k::MOTOR_MODEL, k::STIFFNESS, k::DAMPING);
    let (link_damping, armature, friction) = (k::LINK_DAMPING, k::ARMATURE, k::FRICTION);
    let fields = [
        format!(r#"{axis} = {{ type = "enum", default = "{default_axis}", options = [{axis_options}], description = "The free axis this record limits and drives, in the joint's frame" }}"#),
        format!(r#"{limits} = {{ type = "vec2", default = [0.0, 0.0], description = "How far the axis may travel, low then high, in radians about a rotation axis; equal values mean no limit" }}"#),
        format!(r#"{motor} = {{ type = "enum", default = "{off}", options = [{motors}], description = "Drive the axis towards a speed, towards a position, or not at all" }}"#),
        format!(r#"{target} = {{ type = "float", default = 0.0, description = "The speed or the position the motor drives towards" }}"#),
        format!(r#"{velocity} = {{ type = "float", default = 0.0, description = "The speed a position motor wants at its target" }}"#),
        format!(r#"{max_force} = {{ type = "float", default = 0.0, min = 0.0, description = "The most force or torque the motor may use; 0 means as much as it takes" }}"#),
        format!(r#"{model} = {{ type = "enum", default = "{auto}", options = [{models}], description = "Whether the motor's strength is an acceleration, ignoring mass, or a force; auto is force for a spring's x and acceleration elsewhere" }}"#),
        format!(r#"{stiffness} = {{ type = "float", default = 0.0, min = 0.0, description = "Spring stiffness, for a position motor or a spring's x" }}"#),
        format!(r#"{damping} = {{ type = "float", default = {DEFAULT_DAMPING:?}, min = 0.0, description = "How quickly the motion settles, for a motor or a spring's x" }}"#),
        format!(r#"{link_damping} = {{ type = "float", default = -1.0, min = -1.0, description = "On an articulation: damping on this axis; below zero takes rapier's, {DEFAULT_LINK_DAMPING:?} about a rotation axis and 0 along a linear one" }}"#),
        format!(r#"{armature} = {{ type = "float", default = 0.0, min = 0.0, description = "On an articulation: inertia added to this axis, as a motor's own rotor adds it" }}"#),
        format!(r#"{friction} = {{ type = "float", default = 0.0, min = 0.0, description = "On an articulation: the most force or torque dry friction resists this axis with" }}"#),
    ]
    .join(", ");
    v::schema(&[
        (
            k::AXES,
            &format!(
                r#"{{ type = "list", of = {{ type = "record", fields = {{ {fields} }} }}, default = [], description = "One record per free axis to limit or drive. A rope's x takes no limits (max_length is its limit); a spring's x is its spring, whose motor stays off" }}"#
            ),
        ),
        (
            k::MAX_LENGTH,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The rope's greatest length" }"#,
        ),
        (
            k::REST_LENGTH,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The length a spring pulls back to" }"#,
        ),
        (
            k::COLLIDE_CONNECTED,
            r#"{ type = "bool", default = false, description = "Let the two joined bodies collide with each other" }"#,
        ),
        (
            k::BREAK_FORCE,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The force, in newtons, that snaps the joint and calls on_joint_break; 0 never breaks. Does not apply to an articulation" }"#,
        ),
        (
            k::BREAK_TORQUE,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The torque about the anchor, in newton metres, that snaps the joint; 0 never breaks. Does not apply to an articulation" }"#,
        ),
        (
            k::ARTICULATION,
            r#"{ type = "bool", default = false, description = "Solve in reduced coordinates: the chain never drifts and can be solved for inverse kinematics, but cannot close a loop or break" }"#,
        ),
        (
            k::ENABLED,
            r#"{ type = "bool", default = true, description = "Hold the two bodies together at all. Off keeps the joint and stops solving it; an articulation leaves its chain instead, as rapier's chains ignore the switch" }"#,
        ),
    ]) + "\n"
        + &articulation_schema()
}

/// The rows a joint reads beyond its frames, axes and lengths: how soft it
/// is, and what an articulation's chain holds for it.
fn articulation_schema() -> String {
    use crate::vocabulary::{self as v, keys as k};
    v::schema(&[
        (
            k::SOFTNESS_HZ,
            &format!(
                r#"{{ type = "float", default = {DEFAULT_SOFTNESS_HZ:?}, min = 0.0, description = "How stiffly the joint's locked axes and limits are held, as a spring frequency in hertz; lower lets them stretch and spring back" }}"#
            ),
        ),
        (
            k::SOFTNESS_DAMPING_RATIO,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "The damping ratio of that spring; 1 settles without overshooting" }"#,
        ),
        (
            k::KINEMATIC_LINK,
            r#"{ type = "bool", default = false, description = "On an articulation: the solver never changes this joint's velocity, so the link holds its pose against its parent and moves only by solve_ik" }"#,
        ),
        (
            k::SELF_COLLISION,
            r#"{ type = "bool", default = true, description = "On an articulation: let the chain's links collide with each other. One setting for the whole chain, off when any of its joints says off" }"#,
        ),
        (
            k::PASSIVE_STIFFNESS,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "On an articulation: a spring on each free axis pulling it towards passive_rest; 0 is none" }"#,
        ),
        (
            k::PASSIVE_REST,
            r#"{ type = "float", default = 0.0, description = "On an articulation: where that spring rests, in radians about a rotation axis and units along a linear one" }"#,
        ),
        (
            k::GEAR_WITH,
            r#"{ type = "node", default = "", description = "On an articulation: another joint node of the same chain this joint follows, its first free axis tied to that joint's first free axis" }"#,
        ),
        (
            k::GEAR_RATIO,
            r#"{ type = "float", default = 1.0, description = "How far this joint turns or slides per unit of the gear_with joint's" }"#,
        ),
        (
            k::GEAR_OFFSET,
            r#"{ type = "float", default = 0.0, description = "Where this joint stands when the gear_with joint is at 0" }"#,
        ),
    ])
}

/// rapier's own joint softness (`SpringCoefficients::joint_defaults`).
pub(crate) const DEFAULT_SOFTNESS_HZ: f32 = 1.0e6;
/// The damping rapier gives a new link's turning axes (`default_damping`);
/// its linear axes take none.
pub(crate) const DEFAULT_LINK_DAMPING: f32 = 0.1;

/// `softness_hz` and `softness_damping_ratio`.
pub(crate) fn softness(params: &toml::Value) -> (Real, Real) {
    use crate::vocabulary::{self as v, keys as k};
    (
        crate::scalar::real(v::f(params, k::SOFTNESS_HZ, DEFAULT_SOFTNESS_HZ).max(0.0)),
        crate::scalar::real(v::f(params, k::SOFTNESS_DAMPING_RATIO, 1.0).max(0.0)),
    )
}
