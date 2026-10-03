//! The words and keys the touch components and the pad verbs spell, written
//! once so a schema, its reader and the editor's inspector cannot disagree
//! about a word.

/// The closed word sets.
pub(crate) mod words {
    pub(crate) const TOUCH_BUTTON: &str = "touch_button";
    pub(crate) const TOUCH_STICK: &str = "touch_stick";

    pub(crate) const RECTANGLE: &str = "rectangle";
    pub(crate) const CIRCLE: &str = "circle";
    /// What a button's touch area is.
    pub(crate) const SHAPES: &[&str] = &[RECTANGLE, CIRCLE];

    pub(crate) const TOP_LEFT: &str = "top_left";
    pub(crate) const TOP_RIGHT: &str = "top_right";
    pub(crate) const BOTTOM_LEFT: &str = "bottom_left";
    pub(crate) const BOTTOM_RIGHT: &str = "bottom_right";
    pub(crate) const CENTER: &str = "center";
    pub(crate) const CENTER_LEFT: &str = "center_left";
    pub(crate) const CENTER_RIGHT: &str = "center_right";
    pub(crate) const CENTER_TOP: &str = "center_top";
    pub(crate) const CENTER_BOTTOM: &str = "center_bottom";
    /// The same nine the `widget` component anchors to, less `fill`: a
    /// control has a size of its own, so there is nothing to fill.
    pub(crate) const ANCHORS: &[&str] = &[
        TOP_LEFT,
        CENTER_TOP,
        TOP_RIGHT,
        CENTER_LEFT,
        CENTER,
        CENTER_RIGHT,
        BOTTOM_LEFT,
        CENTER_BOTTOM,
        BOTTOM_RIGHT,
    ];

    pub(crate) const ALWAYS: &str = "always";
    pub(crate) const TOUCHSCREEN: &str = "touchscreen";
    /// When a control is on screen and taking fingers.
    pub(crate) const VISIBILITIES: &[&str] = &[ALWAYS, TOUCHSCREEN];

    /// A pad's power, as SDL3 names its states.
    pub(crate) const POWER_UNKNOWN: &str = "unknown";
    pub(crate) const POWER_ON_BATTERY: &str = "on_battery";
    pub(crate) const POWER_NO_BATTERY: &str = "no_battery";
    pub(crate) const POWER_CHARGING: &str = "charging";
    pub(crate) const POWER_CHARGED: &str = "charged";
    pub(crate) const POWER_STATES: &[&str] = &[
        POWER_UNKNOWN,
        POWER_ON_BATTERY,
        POWER_NO_BATTERY,
        POWER_CHARGING,
        POWER_CHARGED,
    ];

    /// Where a pad's layout came from: a mapping in SDL's format, or the OS.
    pub(crate) const MAPPING_SDL: &str = "sdl";
    pub(crate) const MAPPING_DRIVER: &str = "driver";

    /// How a positional rumble weakens with distance.
    pub(crate) const FALLOFF_INVERSE: &str = "inverse";
    pub(crate) const FALLOFF_LINEAR: &str = "linear";
    pub(crate) const FALLOFF_EXPONENTIAL: &str = "exponential";
    pub(crate) const FALLOFFS: &[&str] = &[FALLOFF_INVERSE, FALLOFF_LINEAR, FALLOFF_EXPONENTIAL];
}

/// Every property key the touch components read.
pub(crate) mod keys {
    pub(crate) const ACTION: &str = "action";
    pub(crate) const ACTION_X: &str = "action_x";
    pub(crate) const ACTION_Y: &str = "action_y";
    pub(crate) const ANCHOR: &str = "anchor";
    pub(crate) const COLOR: &str = "color";
    pub(crate) const DEADZONE: &str = "deadzone";
    pub(crate) const HEIGHT: &str = "height";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const KNOB_COLOR: &str = "knob_color";
    pub(crate) const KNOB_RADIUS: &str = "knob_radius";
    pub(crate) const OFFSET: &str = "offset";
    pub(crate) const PRESSED_COLOR: &str = "pressed_color";
    pub(crate) const RADIUS: &str = "radius";
    pub(crate) const RECENTER: &str = "recenter";
    pub(crate) const VISIBILITY: &str = "visibility";
    pub(crate) const WIDTH: &str = "width";

    /// `gamepad_rumble`'s options.
    pub(crate) const STRONG: &str = "strong";
    pub(crate) const WEAK: &str = "weak";
    pub(crate) const DURATION: &str = "duration";
    pub(crate) const DELAY: &str = "delay";
    pub(crate) const PULSE: &str = "pulse";
    pub(crate) const GAP: &str = "gap";
    pub(crate) const ATTACK: &str = "attack";
    pub(crate) const ATTACK_LEVEL: &str = "attack_level";
    pub(crate) const FADE: &str = "fade";
    pub(crate) const FADE_LEVEL: &str = "fade_level";
    pub(crate) const POSITION: &str = "position";
    pub(crate) const MIN_DISTANCE: &str = "min_distance";
    pub(crate) const MAX_DISTANCE: &str = "max_distance";
    pub(crate) const FALLOFF: &str = "falloff";
    pub(crate) const ROLLOFF: &str = "rolloff";

    /// `gamepad_info`, `gamepad_power` and `feed_gamepad`.
    pub(crate) const CONNECTED: &str = "connected";
    pub(crate) const NAME: &str = "name";
    pub(crate) const OS_NAME: &str = "os_name";
    pub(crate) const GUID: &str = "guid";
    pub(crate) const VENDOR: &str = "vendor";
    pub(crate) const PRODUCT: &str = "product";
    pub(crate) const MAPPING: &str = "mapping";
    pub(crate) const RUMBLE: &str = "rumble";
    pub(crate) const BUTTONS: &str = "buttons";
    pub(crate) const AXES: &str = "axes";
    pub(crate) const POWER: &str = "power";
    pub(crate) const STATE: &str = "state";
    pub(crate) const LEVEL: &str = "level";
    pub(crate) const GYRO: &str = "gyro";
    pub(crate) const ACCELERATION: &str = "acceleration";
    pub(crate) const TOUCHES: &str = "touches";
    pub(crate) const ID: &str = "id";
    pub(crate) const X: &str = "x";
    pub(crate) const Y: &str = "y";
}
