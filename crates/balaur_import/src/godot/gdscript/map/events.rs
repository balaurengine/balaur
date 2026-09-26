//! The Godot signals the engine sends under names of its own.

/// The Godot signals the engine sends under a name of its own, and that name:
/// what a connect listens for, a scene row answers and an await waits on. A
/// signal whose values the engine's do not match (an index where the engine
/// hands a row) is left out. The shim keeps a copy a test holds to this.
pub(crate) const ENGINE_EVENTS: &[(&str, &str)] = &[
    ("body_entered", balaur::hooks::COLLISION_ENTER),
    ("area_entered", balaur::hooks::COLLISION_ENTER),
    ("body_exited", balaur::hooks::COLLISION_EXIT),
    ("area_exited", balaur::hooks::COLLISION_EXIT),
    ("mouse_entered", balaur::hooks::POINTER_ENTER),
    ("mouse_exited", balaur::hooks::POINTER_EXIT),
    ("focus_exited", balaur::ui::BLUR_EVENT),
    ("child_entered_tree", balaur::node_api::CHILD_ADDED_EVENT),
    ("child_exiting_tree", balaur::node_api::CHILD_REMOVED_EVENT),
    ("sleeping_state_changed", "sleeping_changed"),
    ("screen_entered", "screen_enter"),
    ("screen_exited", "screen_exit"),
    ("about_to_popup", balaur::ui::OPENED_EVENT),
    ("popup_hide", balaur::ui::CLOSED_EVENT),
];

/// The event the engine sends for a Godot signal: its own name for it, or
/// the signal's where the two agree.
pub(crate) fn engine_event(signal: &str) -> &str {
    ENGINE_EVENTS
        .iter()
        .find(|(godot, _)| *godot == signal)
        .map_or(signal, |(_, event)| event)
}
