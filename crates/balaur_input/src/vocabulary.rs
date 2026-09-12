//! The words and keys the touch components spell, written once so a schema,
//! its reader and the editor's inspector cannot disagree about a word.

/// The closed word sets.
pub(crate) mod words {
    pub(crate) const TOUCH_BUTTON: &str = "touch_button";
    pub(crate) const TOUCH_STICK: &str = "touch_stick";

    pub(crate) const RECT: &str = "rect";
    pub(crate) const CIRCLE: &str = "circle";
    /// What a button's touch area is.
    pub(crate) const SHAPES: &[&str] = &[RECT, CIRCLE];

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
    pub(crate) const KNOB_COLOR: &str = "knob_color";
    pub(crate) const KNOB_RADIUS: &str = "knob_radius";
    pub(crate) const OFFSET: &str = "offset";
    pub(crate) const PRESSED_COLOR: &str = "pressed_color";
    pub(crate) const RADIUS: &str = "radius";
    pub(crate) const RECENTER: &str = "recenter";
    pub(crate) const SHAPE: &str = "shape";
    pub(crate) const VISIBILITY: &str = "visibility";
    pub(crate) const WIDTH: &str = "width";
}
