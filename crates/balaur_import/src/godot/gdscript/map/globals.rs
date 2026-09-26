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
        "deg_to_rad" => format!("math::to_radians({one})"),
        "rad_to_deg" => format!("math::to_degrees({one})"),
        "lerp" | "lerpf" => format!("(gd.lerp)({all})"),
        "sign" | "signf" | "signi" => format!("(gd.sign)({one})"),
        "snapped" | "snappedf" | "snappedi" => format!("(gd.snapped)({all})"),
        "fmod" | "fposmod" => format!("(gd.fmod)({all})"),
        "posmod" => format!("(gd.posmod)({all})"),
        "move_toward" => format!("(gd.move_toward)({all})"),
        "randf" => "random::float()".into(),
        "randi" => "random::int(0, 2147483647)".into(),
        "randi_range" => format!("random::int({all})"),
        "randf_range" => format!("random::range({all})"),
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

// Several rows share a value without sharing a meaning: `CONNECT_ONE_SHOT` is
// not a mouse button, and merging them would hide what each row is for.
#[allow(clippy::match_same_arms)]
pub(crate) fn global_constant(name: &str) -> Option<&'static str> {
    Some(match name {
        "MOUSE_BUTTON_LEFT" => "1",
        "MOUSE_BUTTON_RIGHT" => "2",
        "MOUSE_BUTTON_MIDDLE" => "3",
        "MOUSE_BUTTON_WHEEL_UP" => "4",
        "MOUSE_BUTTON_WHEEL_DOWN" => "5",
        "OK" => "0",
        "FAILED" => "1",
        // No JavaScript bridge: `web::visible` and `on_dark_mode_changed` answer what
        // its probes read, so a script finds it absent.
        "JavaScriptBridge" => "()",
        // `typeof` answers the shim's type names, so its constants are those.
        "TYPE_NIL" => "\"nil\"",
        "TYPE_BOOL" => "\"bool\"",
        "TYPE_INT" => "\"int\"",
        "TYPE_FLOAT" => "\"float\"",
        "TYPE_STRING" | "TYPE_STRING_NAME" => "\"String\"",
        "TYPE_ARRAY" => "\"Array\"",
        "TYPE_DICTIONARY" => "\"Dictionary\"",
        "TYPE_OBJECT" => "\"Object\"",
        "CONNECT_ONE_SHOT" => "4",
        "CONNECT_DEFERRED" => "1",
        "HORIZONTAL" => "0",
        "VERTICAL" => "1",
        "HORIZONTAL_ALIGNMENT_LEFT" | "VERTICAL_ALIGNMENT_TOP" => "0",
        "HORIZONTAL_ALIGNMENT_CENTER" | "VERTICAL_ALIGNMENT_CENTER" => "1",
        "HORIZONTAL_ALIGNMENT_RIGHT" | "VERTICAL_ALIGNMENT_BOTTOM" => "2",
        "HORIZONTAL_ALIGNMENT_FILL" | "VERTICAL_ALIGNMENT_FILL" => "3",
        "SIZE_SHRINK_BEGIN" => "0",
        "SIZE_FILL" => "1",
        "SIZE_EXPAND" => "2",
        "SIZE_EXPAND_FILL" => "3",
        "SIZE_SHRINK_CENTER" => "4",
        "SIZE_SHRINK_END" => "8",
        // Godot's own numbers: `_notification` compares `what` against them,
        // and the hooks that call it hand the same ones over.
        "NOTIFICATION_ENTER_TREE" => "10",
        "NOTIFICATION_EXIT_TREE" => "11",
        "NOTIFICATION_READY" => "13",
        "NOTIFICATION_RESIZED" => "40",
        "NOTIFICATION_THEME_CHANGED" => "45",
        "NOTIFICATION_WM_CLOSE_REQUEST" => "1006",
        "NOTIFICATION_WM_GO_BACK_REQUEST" => "1007",
        "NOTIFICATION_TRANSLATION_CHANGED" => "2010",
        "NOTIFICATION_APPLICATION_RESUMED" => "2014",
        "NOTIFICATION_APPLICATION_PAUSED" => "2015",
        "NOTIFICATION_APPLICATION_FOCUS_IN" => "2016",
        "NOTIFICATION_APPLICATION_FOCUS_OUT" => "2017",
        _ => return None,
    })
}
