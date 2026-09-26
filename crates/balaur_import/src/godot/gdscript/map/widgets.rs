//! Godot's control signals and the widget keys that hear them.

/// The widget key a built-in Godot signal is spelled by here. A clicked
/// widget calls `on_click` on the first ancestor whose script has the method,
/// which is what `button.pressed.connect(self._on_pressed)` meant.
pub(crate) fn widget_signal(signal: &str) -> Option<&'static str> {
    WIDGET_SIGNALS
        .iter()
        .find(|(godot, _)| *godot == signal)
        .map(|(_, key)| *key)
}

/// The widget keys a control signal lands on, which `balaur_ui` spells.
pub(crate) const ON_CLICK: &str = "on_click";
pub(crate) const ON_CHANGE: &str = "on_change";
pub(crate) const ON_SUBMIT: &str = "on_submit";
pub(crate) const ON_FOCUS: &str = "on_focus";

/// Each Godot control signal and the widget key that hears it. The scene
/// importer and the translator read it here; the shim keeps a copy a test
/// holds to it.
pub(crate) const WIDGET_SIGNALS: &[(&str, &str)] = &[
    ("pressed", ON_CLICK),
    ("button_up", ON_CLICK),
    ("toggled", ON_CHANGE),
    ("value_changed", ON_CHANGE),
    ("text_changed", ON_CHANGE),
    ("item_selected", ON_CHANGE),
    ("color_changed", ON_CHANGE),
    ("tab_changed", ON_CHANGE),
    ("tab_selected", ON_CHANGE),
    ("folding_changed", ON_CHANGE),
    ("close_requested", ON_CHANGE),
    ("text_submitted", ON_SUBMIT),
    ("focus_entered", ON_FOCUS),
];
