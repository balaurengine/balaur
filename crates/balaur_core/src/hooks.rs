//! Every script method the engine calls on a node, by name.
//!
//! One list, so the Events view, the "any declared" gate and the dispatcher
//! cannot spell a hook three ways. A plugin's own hooks stay in its crate;
//! these are the ones core, input and rendering dispatch.

/// The two hooks the engine calls by name, and the [`BINDABLE`] events whose
/// rows run beside them.
pub const ON_VARIABLE_CHANGED: &str = "on_variable_changed";
pub const ON_STATE_CHANGED: &str = "on_state_changed";
pub const VARIABLE_CHANGED: &str = "variable_changed";
pub const STATE_CHANGED: &str = "state_changed";
/// Called on every script when the game pauses or resumes, the nodes the
/// pause just stopped included.
pub const ON_PAUSED_CHANGED: &str = "on_paused_changed";
/// Called on every script, a paused one too, when the window comes to the
/// front or leaves it, with whether it is in front now.
pub const ON_FOCUSED_CHANGED: &str = "on_focused_changed";
/// Called on every script, a paused one too, when the system turns dark mode
/// on or off.
pub const ON_DARK_MODE_CHANGED: &str = "on_dark_mode_changed";
/// Called on every script, a paused one too, when the window is asked to
/// close, before it does.
pub const ON_QUIT_REQUESTED: &str = "on_quit_requested";
/// Called on every script, a paused one too, when the game goes to the
/// background or comes back: a browser tab hidden, a window minimised or
/// covered, an Android activity sent back.
pub const ON_SUSPENDED_CHANGED: &str = "on_suspended_changed";
/// Called on every script, a paused one too, when the system is short of
/// memory: what can be rebuilt is worth freeing.
pub const ON_LOW_MEMORY: &str = "on_low_memory";
/// Called on every script when the screen's safe area moves.
pub const ON_SAFE_AREA_CHANGED: &str = "on_safe_area_changed";
/// Called on every script when the screen turns between portrait and landscape.
pub const ON_ORIENTATION_CHANGED: &str = "on_orientation_changed";
/// Called on every script when a gamepad is plugged in or goes away.
pub const ON_GAMEPAD_CONNECTED: &str = "on_gamepad_connected";
pub const ON_GAMEPAD_DISCONNECTED: &str = "on_gamepad_disconnected";
/// Called on every script the frame after a script set a setting.
pub const ON_SETTING_CHANGED: &str = "on_setting_changed";
/// Called on every script the frame after the language changed.
pub const ON_LOCALE_CHANGED: &str = "on_locale_changed";

/// What a script's instance is called with over its life, by name.
pub const INIT: &str = "init";
pub const UPDATE: &str = "update";
pub const FIXED_UPDATE: &str = "fixed_update";
pub const DRAW_UI: &str = "draw_ui";
pub const ON_FREE: &str = "on_free";
pub const ON_HOT_RELOAD: &str = "on_hot_reload";
pub const DEFAULTS: &str = "defaults";
pub const SAVE_STATE: &str = "save_state";
pub const LOAD_STATE: &str = "load_state";

/// The pointer, key and window events a node hears, each as `on_<name>`.
pub const POINTER_ENTER: &str = "pointer_enter";
pub const POINTER_EXIT: &str = "pointer_exit";
pub const POINTER_DOWN: &str = "pointer_down";
pub const POINTER_UP: &str = "pointer_up";
/// The event a click on a node, or on a widget, answers to.
pub const POINTER_CLICK: &str = "pointer_click";
pub const POINTER_DRAG: &str = "pointer_drag";
pub const POINTER_DROP: &str = "pointer_drop";
/// The button a pointer event names, by index: primary, secondary, middle.
pub const BUTTONS: [&str; 3] = ["left", "right", "middle"];
pub const KEY_DOWN: &str = "key_down";
pub const KEY_UP: &str = "key_up";
pub const ACTION: &str = "action";
pub const ACTION_RELEASED: &str = "action_released";
pub const SCROLL: &str = "scroll";
pub const RESIZE: &str = "resize";
/// A collider starting and stopping to touch another, which physics sends.
pub const COLLISION_ENTER: &str = "collision_enter";
pub const COLLISION_EXIT: &str = "collision_exit";

/// The events a `[[nodes.bindings.rows]]` row may name, which are these hooks with
/// the `on_` prefix dropped. In the order the Events view offers them.
pub const BINDABLE: &[&str] = &[
    POINTER_ENTER,
    POINTER_EXIT,
    POINTER_DOWN,
    POINTER_UP,
    POINTER_CLICK,
    POINTER_DRAG,
    POINTER_DROP,
    KEY_DOWN,
    KEY_UP,
    ACTION,
    ACTION_RELEASED,
    SCROLL,
    RESIZE,
    VARIABLE_CHANGED,
    STATE_CHANGED,
    COLLISION_ENTER,
    COLLISION_EXIT,
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
pub fn hook_of(event: &str) -> smol_str::SmolStr {
    // Inline for any name under 21 bytes: an event costs no allocation to spell.
    smol_str::format_smolstr!("on_{event}")
}

/// Every method the engine calls on a script by name, its arguments after
/// `this`, and when: what the generated hooks page lists. A component's own
/// events are listed with the component instead.
#[rustfmt::skip]
pub const REFERENCE: &[(&str, &str, &str)] = &[
    (INIT, "()", "Once, when the script is attached and its scene has loaded."),
    (UPDATE, "(dt)", "Every frame, with the seconds since the last; not while paused."),
    (FIXED_UPDATE, "(dt)", "Every fixed step, with the step's length; not while paused."),
    (DRAW_UI, "()", "Every UI pass, for immediate `ui::*` calls."),
    (ON_FREE, "()", "Once, as the node is freed or its script detached."),
    (ON_HOT_RELOAD, "()", "After the script's file changed and it was rebuilt, with its state kept."),
    (DEFAULTS, "()", "When the instance is made, before the scene's properties land."),
    (SAVE_STATE, "()", "When a rollback or a save snapshots the world; answers the state to keep."),
    (LOAD_STATE, "(state)", "When a rollback or a load puts that state back."),
    ("on_pointer_enter", "()", "The pointer came over the node."),
    ("on_pointer_exit", "()", "The pointer left the node."),
    ("on_pointer_down", "(button)", "A button went down over the node, then over every other node unless one answers `true`."),
    ("on_pointer_up", "(button)", "A button came up, reaching nodes as `on_pointer_down` does."),
    ("on_pointer_click", "(button)", "A press and release over the same node."),
    ("on_pointer_drag", "(dx, dy)", "The pointer moved with a button held, having pressed on the node."),
    ("on_pointer_drop", "(node)", "A drag ended over this node, from the node given."),
    ("on_key_down", "(key)", "A key went down; every node hears it, the last child first, until one answers `true`."),
    ("on_key_up", "(key)", "A key came up, reaching nodes as `on_key_down` does."),
    ("on_action", "(name)", "A declared input action was pressed, reaching nodes as `on_key_down` does."),
    ("on_action_released", "(name)", "A declared input action was let go, reaching nodes as `on_key_down` does."),
    ("on_scroll", "(dx, dy)", "The wheel turned, over the node or, over nothing, to every node."),
    ("on_resize", "(width, height)", "The window changed size, told to every node."),
    (ON_VARIABLE_CHANGED, "(name, value)", "A scene variable changed, told to every node at the end of the frame."),
    (ON_STATE_CHANGED, "(was, now)", "The node's `states` moved to another state."),
    (ON_PAUSED_CHANGED, "(paused)", "The game paused or resumed, told to every script, a paused one too."),
    (ON_FOCUSED_CHANGED, "(focused)", "The window came to the front or left it, told to every script."),
    (ON_DARK_MODE_CHANGED, "(dark)", "The system switched dark mode, told to every script."),
    (ON_QUIT_REQUESTED, "()", "The window was asked to close; every script hears it, then the app goes."),
    (ON_SUSPENDED_CHANGED, "(suspended)", "The game went to the background or came back: a hidden tab, a minimised or covered window, an app sent back; told to every script."),
    (ON_LOW_MEMORY, "()", "The system is short of memory, on iOS and Android; told to every script."),
    (ON_SAFE_AREA_CHANGED, "(insets)", "The screen's safe area moved, as `[left, top, right, bottom]` in pixels; told to every script."),
    (ON_ORIENTATION_CHANGED, "(orientation)", "The screen turned, to `portrait` or `landscape`; told to every script."),
    (ON_GAMEPAD_CONNECTED, "(pad)", "A gamepad was plugged in, with the id `input` names it by; told to every script."),
    (ON_GAMEPAD_DISCONNECTED, "(pad)", "A gamepad went away, with its id; told to every script."),
    (ON_SETTING_CHANGED, "(change)", "A script set a setting, `#{ path, value }`; every script hears it the next frame."),
    (ON_LOCALE_CHANGED, "(locale)", "The language changed; every script hears it the next frame."),
];
