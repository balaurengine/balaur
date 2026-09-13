//! Every script method the engine calls on a node, by name.
//!
//! One list, so the Events view, the "any declared" gate and the dispatcher
//! cannot spell a hook three ways. A plugin's own hooks stay in its crate;
//! these are the ones core, input and rendering dispatch.

pub const ON_POINTER_ENTER: &str = "on_pointer_enter";
pub const ON_POINTER_EXIT: &str = "on_pointer_exit";
pub const ON_POINTER_DOWN: &str = "on_pointer_down";
pub const ON_POINTER_UP: &str = "on_pointer_up";
pub const ON_POINTER_CLICK: &str = "on_pointer_click";
pub const ON_POINTER_DRAG: &str = "on_pointer_drag";
pub const ON_POINTER_DROP: &str = "on_pointer_drop";
pub const ON_KEY_DOWN: &str = "on_key_down";
pub const ON_KEY_UP: &str = "on_key_up";
pub const ON_ACTION: &str = "on_action";
pub const ON_SCROLL: &str = "on_scroll";
pub const ON_RESIZE: &str = "on_resize";
pub const ON_VARIABLE_CHANGED: &str = "on_variable_changed";
pub const ON_STATE_CHANGED: &str = "on_state_changed";

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
