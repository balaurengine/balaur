//! Every key and word an animation component, asset, tween or option table
//! spells, and the script constants beside them. One list for the crate, so a
//! schema, its parser and the read-back cannot disagree about a spelling.

/// The component key, as the registry and every `describe` entry spell it.
pub(crate) const COMPONENT: &str = "animation";

/// Every key a component, an asset, a tween or a `play` option table spells,
/// for the schemas, the parsers, the readers and the importer alike.
pub mod keys {
    pub const ADVANCE_MODE: &str = "advance_mode";
    pub const ANGLE_LIMIT: &str = "angle_limit";
    pub const AUTOPLAY: &str = "autoplay";
    pub const BLEND_CURVE: &str = "blend_curve";
    pub const BLEND_TIME: &str = "blend_time";
    pub const BONE: &str = "bone";
    pub const BONES: &str = "bones";
    pub const BREAK_LOOP_AT_END: &str = "break_loop_at_end";
    pub const BY: &str = "by";
    pub const CALL: &str = "call";
    /// What a method key hands the method it calls.
    pub const ARGS: &str = "args";
    pub const CHAIN_COUNT: &str = "chain_count";
    pub const CHECK: &str = "check";
    pub const CHECK_NODE: &str = "check_node";
    pub const CONDITION: &str = "condition";
    pub const DAMPING: &str = "damping";
    pub const DELAY: &str = "delay";
    pub const DURATION: &str = "duration";
    pub const EASE: &str = "ease";
    pub const ENABLED: &str = "enabled";
    pub const FLIP_BEND_DIRECTION: &str = "flip_bend_direction";
    pub const FROM: &str = "from";
    pub const FROM_START: &str = "from_start";
    pub const GRAVITY: &str = "gravity";
    pub const INTERPOLATION: &str = "interpolation";
    pub const INTERVAL: &str = "interval";
    pub const ITERATIONS: &str = "iterations";
    pub const KEYS: &str = "keys";
    pub const KIND: &str = "kind";
    pub const LAG: &str = "lag";
    pub const LENGTH: &str = "length";
    pub const LIBRARY: &str = "library";
    pub const LOOPS: &str = "loops";
    pub const LOOP_MODE: &str = "loop_mode";
    pub const MACHINE: &str = "machine";
    pub const MASS: &str = "mass";
    pub const NAME: &str = "name";
    pub const OFFSET: &str = "offset";
    pub const PARALLEL: &str = "parallel";
    pub const PLAYER: &str = "player";
    pub const PRIORITY: &str = "priority";
    pub const PROFILE: &str = "profile";
    pub const PROPERTY: &str = "property";
    pub const RESET: &str = "reset";
    pub const REST_POSITION: &str = "rest_position";
    pub const REST_ROTATION: &str = "rest_rotation";
    pub const RETARGET: &str = "retarget";
    pub const ROOT_NODE: &str = "root_node";
    pub const SPEED_SCALE: &str = "speed_scale";
    pub const START: &str = "start";
    pub const STATES: &str = "states";
    pub const STEPS: &str = "steps";
    /// A tween step's index, and a tween's handle, in a tween event's payload.
    pub const STEP: &str = "step";
    pub const TWEEN: &str = "tween";
    /// How many times a tween has run through, in a `tween_looped` payload.
    pub const PLAYED: &str = "played";
    pub const STIFFNESS: &str = "stiffness";
    pub const SWITCH_MODE: &str = "switch_mode";
    pub const TARGET: &str = "target";
    pub const THEN: &str = "then";
    pub const TIME: &str = "time";
    pub const TO: &str = "to";
    pub const TOLERANCE: &str = "tolerance";
    pub const TRACKS: &str = "tracks";
    pub const TRANSITIONS: &str = "transitions";
    pub const USE_GRAVITY: &str = "use_gravity";
    pub const VALUE: &str = "value";
}

/// The closed sets of words a clip, a machine, a modifier and a script spell,
/// written once so a parser, a schema and a read-back cannot disagree.
pub mod words {
    /// A clip's `loop_mode`, beside `LINEAR` below.
    pub const NONE: &str = "none";
    pub const PINGPONG: &str = "pingpong";

    /// A track's `interpolation`; `LINEAR` is also a clip's repeating `loop_mode`.
    pub const STEP: &str = "step";
    pub const LINEAR: &str = "linear";
    pub const CUBIC: &str = "cubic";

    /// The transform and appearance properties a track drives by name.
    pub const POSITION: &str = "position";
    pub const ROTATION_EULER: &str = "rotation_euler";
    pub const ROTATION: &str = "rotation";
    pub const SCALE: &str = "scale";
    pub const VISIBLE: &str = "visible";
    pub const TINT: &str = "tint";

    /// A transition's `advance_mode`: never on its own, only by travel, or on its
    /// own too.
    pub const DISABLED: &str = "disabled";
    pub const ENABLED: &str = "enabled";
    pub const AUTO: &str = "auto";

    /// A transition's `switch_mode`: cut now, cut keeping the playhead, or wait
    /// for the clip's end.
    pub const IMMEDIATE: &str = "immediate";
    pub const SYNC: &str = "sync";
    pub const AT_END: &str = "at_end";

    /// The state a transition reaches to stop the machine: Godot's `End`.
    pub const END: &str = "end";

    /// A modifier's `kind`.
    pub const LOOK_AT: &str = "look_at";
    pub const TWO_BONE_IK: &str = "two_bone_ik";
    pub const FABRIK: &str = "fabrik";
    pub const CCDIK: &str = "ccdik";
    pub const JIGGLE: &str = "jiggle";
    pub const FOLLOW: &str = "follow";
    pub const MODIFIER_KINDS: &[&str] = &[LOOK_AT, TWO_BONE_IK, FABRIK, CCDIK, JIGGLE, FOLLOW];
}

use words as w;

/// A clip's `loop_mode`s, as `animation::LOOP_PINGPONG` and the rest.
pub const LOOP_MODES: &[(&str, &str)] = &[
    ("LOOP_NONE", w::NONE),
    ("LOOP_LINEAR", w::LINEAR),
    ("LOOP_PINGPONG", w::PINGPONG),
];

/// A track's `interpolation` modes.
pub const INTERPOLATIONS: &[(&str, &str)] = &[
    ("INTERPOLATION_STEP", w::STEP),
    ("INTERPOLATION_LINEAR", w::LINEAR),
    ("INTERPOLATION_CUBIC", w::CUBIC),
];

/// The properties a track drives by name; a component's is `component/property`.
pub const PROPERTIES: &[(&str, &str)] = &[
    ("PROPERTY_POSITION", w::POSITION),
    ("PROPERTY_ROTATION_EULER", w::ROTATION_EULER),
    ("PROPERTY_ROTATION", w::ROTATION),
    ("PROPERTY_SCALE", w::SCALE),
    ("PROPERTY_VISIBLE", w::VISIBLE),
    ("PROPERTY_TINT", w::TINT),
    ("PROPERTY_DEFORM", crate::clip::DEFORM),
];

/// A state machine transition's `advance_mode`s.
pub const ADVANCE_MODES: &[(&str, &str)] = &[
    ("ADVANCE_MODE_DISABLED", w::DISABLED),
    ("ADVANCE_MODE_ENABLED", w::ENABLED),
    ("ADVANCE_MODE_AUTO", w::AUTO),
];

/// A state machine transition's `switch_mode`s.
pub const SWITCH_MODES: &[(&str, &str)] = &[
    ("SWITCH_MODE_IMMEDIATE", w::IMMEDIATE),
    ("SWITCH_MODE_SYNC", w::SYNC),
    ("SWITCH_MODE_AT_END", w::AT_END),
];

/// The state a transition names to stop its machine.
pub const MACHINE_STATES: &[(&str, &str)] = &[("STATE_END", w::END)];

/// `modifier2d` and `modifier3d` kinds.
pub const MODIFIER_KINDS: &[(&str, &str)] = &[
    ("MODIFIER_LOOK_AT", w::LOOK_AT),
    ("MODIFIER_TWO_BONE_IK", w::TWO_BONE_IK),
    ("MODIFIER_FABRIK", w::FABRIK),
    ("MODIFIER_CCDIK", w::CCDIK),
    ("MODIFIER_JIGGLE", w::JIGGLE),
    ("MODIFIER_FOLLOW", w::FOLLOW),
];

/// The events a player and a machine emit from their node.
pub const EVENTS: &[(&str, &str)] = &[
    ("EVENT_ANIMATION_STARTED", crate::system::STARTED_EVENT),
    ("EVENT_ANIMATION_CHANGED", crate::system::CHANGED_EVENT),
    ("EVENT_ANIMATION_LOOPED", crate::system::LOOPED_EVENT),
    ("EVENT_ANIMATION_FINISHED", crate::system::FINISHED_EVENT),
    ("EVENT_STATE_STARTED", crate::machine::STATE_STARTED_EVENT),
    ("EVENT_STATE_FINISHED", crate::machine::STATE_FINISHED_EVENT),
    ("EVENT_TWEEN_FINISHED", crate::system::TWEEN_FINISHED_EVENT),
    ("EVENT_TWEEN_LOOPED", crate::system::TWEEN_LOOPED_EVENT),
    ("EVENT_TWEEN_STEP", crate::system::TWEEN_STEP_EVENT),
];

/// Every table above, installed on the `animation` module.
pub const CONSTANTS: &[&[(&str, &str)]] = &[
    LOOP_MODES,
    INTERPOLATIONS,
    PROPERTIES,
    ADVANCE_MODES,
    SWITCH_MODES,
    MACHINE_STATES,
    MODIFIER_KINDS,
    EVENTS,
];

/// Every curve name as its constant: `in_out_sine` is `EASE_IN_OUT_SINE`.
#[must_use]
pub fn ease_constants() -> Vec<(String, &'static str)> {
    crate::ease::names()
        .into_iter()
        .map(|name| (format!("EASE_{}", name.to_ascii_uppercase()), name))
        .collect()
}
