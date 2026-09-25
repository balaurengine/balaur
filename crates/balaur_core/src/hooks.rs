//! Every script method the engine calls on a node, by name.
//!
//! One list, so the Events view, the "any declared" gate and the dispatcher
//! cannot spell a hook three ways. A plugin's own hooks stay in its crate;
//! these are the ones core, input and rendering dispatch.

/// The two hooks the engine calls by name rather than through a binding, so
/// each is spelled here as well as in [`BINDABLE`] without its prefix.
pub const ON_VARIABLE_CHANGED: &str = "on_variable_changed";
pub const ON_STATE_CHANGED: &str = "on_state_changed";
/// Called on every script when the game pauses or resumes, the nodes the
/// pause just stopped included.
pub const ON_PAUSED: &str = "on_paused";
/// Called on every script when the window comes to the front or leaves it,
/// with whether it is in front now.
pub const ON_FOCUS_CHANGED: &str = "on_focus_changed";
/// Called on every script when the system turns dark mode on or off.
pub const ON_DARK_MODE: &str = "on_dark_mode";
/// Called on every script when the window is asked to close, before it does.
pub const ON_QUIT_REQUESTED: &str = "on_quit_requested";

/// The event a click on a node, or on a widget, answers to.
pub const POINTER_CLICK: &str = "pointer_click";

/// The events a `[[nodes.bindings.rows]]` row may name, which are these hooks with
/// the `on_` prefix dropped. In the order the Events view offers them.
pub const BINDABLE: &[&str] = &[
    "pointer_enter",
    "pointer_exit",
    "pointer_down",
    "pointer_up",
    POINTER_CLICK,
    "pointer_drag",
    "pointer_drop",
    "key_down",
    "key_up",
    "action",
    "scroll",
    "resize",
    "variable_changed",
    "state_changed",
    "collision_start",
    "collision_stop",
];

/// The prefix of a binding event that answers a name the node emitted:
/// `emitted:died` runs when the node's script, or the engine on its behalf,
/// calls `node.emit("died")`. What a Godot signal connected in a scene is.
pub const EMITTED: &str = "emitted:";

/// Whether a binding may name `event`: one of [`BINDABLE`], or a name the
/// node emits.
#[must_use]
pub fn is_bindable(event: &str) -> bool {
    BINDABLE.contains(&event)
        || event
            .strip_prefix(EMITTED)
            .is_some_and(|name| !name.is_empty())
}

/// The hook one bindable event name is dispatched as.
#[must_use]
pub fn hook_of(event: &str) -> String {
    format!("on_{event}")
}
