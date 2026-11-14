//! The words and keys the pad verbs spell, written once so a verb, its
//! reader and the reference cannot disagree about a word.

/// The closed word sets.
#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
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

    pub(crate) mod bindings {
        pub(crate) const ALT: &str = "alt";
        pub(crate) const AXIS: &str = "axis";
        pub(crate) const BACK: &str = "back";
        pub(crate) const CMD: &str = "cmd";
        pub(crate) const CTRL: &str = "ctrl";
        pub(crate) const FORWARD: &str = "forward";
        pub(crate) const GAMEPAD: &str = "gamepad";
        pub(crate) const KEYS: &str = "keys";
        pub(crate) const LEFT: &str = "left";
        pub(crate) const MIDDLE: &str = "middle";
        pub(crate) const MOUSE: &str = "mouse";
        pub(crate) const RIGHT: &str = "right";
        pub(crate) const SHIFT: &str = "shift";
    }

    pub(crate) mod phases {
        pub(crate) const CANCEL: &str = "cancel";
        pub(crate) const END: &str = "end";
        pub(crate) const MOVE: &str = "move";
        pub(crate) const START: &str = "start";
    }

    pub(crate) mod general {
        pub(crate) const INPUT: &str = "input";
    }
}

/// Every key the pad verbs read and answer with.
#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
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

    /// `gamepad_info`, `gamepad_power` and `set_gamepad`.
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
