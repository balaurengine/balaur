//! Godot's `@GlobalScope` functions that are one engine call each.

/// Godot's `@GlobalScope` maths and random numbers.
pub(super) fn arithmetic(name: &str, all: &str, one: &str) -> Option<String> {
    Some(match name {
        "sqrt" => format!("math::sqrt({one})"),
        "pow" => format!("math::pow({all})"),
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "exp" | "log" => {
            format!("math::{name}({one})")
        }
        "atan2" => format!("math::atan2({all})"),
        "deg_to_rad" => format!("math::rad({one})"),
        "rad_to_deg" => format!("math::deg({one})"),
        "lerp" | "lerpf" => format!("(gd.lerp)({all})"),
        "sign" | "signf" | "signi" => format!("(gd.sign)({one})"),
        "snapped" | "snappedf" | "snappedi" => format!("(gd.snapped)({all})"),
        "fmod" | "fposmod" => format!("(gd.fmod)({all})"),
        "posmod" => format!("(gd.posmod)({all})"),
        "move_toward" => format!("(gd.move_toward)({all})"),
        "randf" => "rng::random()".into(),
        "randi" => "rng::int(0, 2147483647)".into(),
        "randi_range" => format!("rng::int({all})"),
        "randf_range" => format!("rng::range({all})"),
        "randomize" => "()".into(),
        _ => return None,
    })
}

/// A property written on one of Godot's singletons: `Engine.max_fps = 0`.
/// The frame cap is the `window/max_fps` setting; low-processor mode has no
/// switch here, since the loop already paces itself against the tick.
pub(crate) fn singleton_write(class: &str, field: &str, value: &str) -> Option<String> {
    Some(match (class, field) {
        ("Engine", "max_fps") => format!("settings::set(\"window/max_fps\", {value})"),
        ("OS", "low_processor_usage_mode" | "low_processor_usage_mode_sleep_usec") => "()".into(),
        _ => return None,
    })
}
