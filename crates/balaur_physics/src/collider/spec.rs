//! The spec of each row more than one collider takes, for a component to
//! place among its own rows in the order its inspector shows them. A whole
//! group every collider takes the same is a `*_schema` function beside the
//! component instead.

use crate::vocabulary::{self as v, words as w};

pub(crate) fn collision_layer() -> String {
    let layers = balaur_core::layers::options();
    format!(
        r#"{{ type = "flags", default = ["1"], options = [{layers}], description = "The layers this collider is on", group = "filtering" }}"#
    )
}

pub(crate) fn collision_mask() -> String {
    let layers = balaur_core::layers::options();
    format!(
        r#"{{ type = "flags", default = [], options = [{layers}], description = "The layers it collides with; empty means every layer", group = "filtering" }}"#
    )
}

pub(crate) fn collision_test() -> String {
    let both = w::BOTH;
    let tests = v::options(w::TEST_MODES);
    format!(
        r#"{{ type = "enum", default = "{both}", options = [{tests}], description = "Whether a pair is tested when both colliders' layers accept the other, or when either does; two colliders that differ use both", group = "filtering" }}"#
    )
}

pub(crate) fn solver_layer() -> String {
    let layers = balaur_core::layers::options();
    format!(
        r#"{{ type = "flags", default = ["1"], options = [{layers}], description = "Layers for the solver alone: a pair can be detected but not resolved", group = "filtering" }}"#
    )
}

pub(crate) fn solver_mask() -> String {
    let layers = balaur_core::layers::options();
    format!(
        r#"{{ type = "flags", default = [], options = [{layers}], description = "Which solver layers this one pushes against; empty means all of them", group = "filtering" }}"#
    )
}

pub(crate) fn solver_test() -> String {
    let both = w::BOTH;
    let tests = v::options(w::TEST_MODES);
    format!(
        r#"{{ type = "enum", default = "{both}", options = [{tests}], description = "The same choice for the solver layers", group = "filtering" }}"#
    )
}

pub(crate) fn contact_pairs() -> String {
    let watched = v::options(w::DEFAULT_COLLISIONS);
    let collisions = v::options(&v::flags::collision_types().map(|(name, _)| name));
    format!(
        r#"{{ type = "flags", default = [{watched}], options = [{collisions}], description = "Which pairs of body kinds this collider is tested against; a sensor watching kinematic platforms needs more than the default", group = "filtering" }}"#
    )
}

pub(crate) fn sensor() -> String {
    r#"{ type = "bool", default = false, description = "Detects overlaps without colliding: bodies pass through and are reported" }"#.into()
}

pub(crate) fn enabled() -> String {
    r#"{ type = "bool", default = true, description = "Collide at all; a disabled collider keeps its shape and costs nothing" }"#.into()
}

pub(crate) fn one_way() -> String {
    r#"{ type = "bool", default = false, description = "A platform bodies land on from the side one_way_axis names and pass through from the other", group = "contacts" }"#.into()
}

pub(crate) fn one_way_angle() -> String {
    r#"{ type = "float", default = 0.1, min = 0.0, max = 180.0, unit = "degrees", description = "How far a contact's normal may lean from one_way_axis and still hold the body; radians in the file", group = "contacts" }"#.into()
}

pub(crate) fn collision_margin() -> String {
    r#"{ type = "float", default = 0.0, min = 0.0, description = "A margin the solver treats as already touching; stops thin shapes tunnelling and jittering", group = "contacts" }"#.into()
}

pub(crate) fn two_sided_edges() -> String {
    r#"{ type = "bool", default = false, description = "fix_internal_edges for a mesh hit from both sides: a contact from behind a triangle is kept and smoothed, not dropped", group = "contacts" }"#.into()
}

pub(crate) fn density() -> String {
    r#"{ type = "float", default = 1.0, min = 0.0, description = "Mass per volume, so the shape's size sets its mass; 0 makes a collider that adds no mass to its body", group = "mass" }"#.into()
}

pub(crate) fn mass() -> String {
    r#"{ type = "float", default = 0.0, min = 0.0, description = "Mass in kilograms, overriding what density works out to; 0 keeps the density", group = "mass" }"#.into()
}
