//! Pointer, key, action, scroll and resize hooks, dispatched on the tick.
//!
//! Here rather than in either plugin: what the pointer is over is the
//! renderer's to answer and what the pointer did is input's, and this crate
//! is the one that has both. Every hook goes to the node's `bindings` first
//! and its script second, so a scene is interactive with or without code.
//!
//! Resolved from state the tick already carries, so a replayed session
//! dispatches the same hooks on every machine.

use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage, bindings, hooks, node_id_of};
use balaur_script::Value;

/// What the pointer was over and doing last tick, so this tick can say what
/// changed. Not simulation state: it is derived from input, which is.
#[derive(Default)]
pub(crate) struct Pointer {
    over: Option<Entity>,
    /// The node the press landed on, so a release over it is a click and a
    /// release elsewhere is a drop.
    pressed: Option<Entity>,
    dragging: bool,
    /// The window as it was, so a resize is dispatched once per change.
    size: (u32, u32),
}

/// Send one hook to a node: its bindings, then its script.
///
/// Bindings first, so a node carrying both sees the world the table left, and
/// a script that wants to run first simply does not use a binding.
fn dispatch(eng: &Engine, entity: Entity, event: &str, args: &[Value]) {
    bindings::fire(eng, entity, event, args);
    let Some(host) = eng.script_host() else {
        return;
    };
    let node = node_id_of(entity);
    let hook = hooks::hook_of(event);
    if host.has_method(node, &hook) {
        host.call_on(node, &hook, args);
    }
}

/// Send a hook to every node in the scene, for the events that belong to the
/// whole window rather than to one node.
fn broadcast(eng: &Engine, event: &str, args: &[Value]) {
    let everyone = {
        let world = eng.world();
        balaur_core::scene::collect_subtree(&world, eng.root())
    };
    // The hook's name is spelled once for the whole scene, not once a node.
    let hook = hooks::hook_of(event);
    let host = eng.script_host();
    for entity in everyone {
        bindings::fire(eng, entity, event, args);
        let Some(host) = host.as_ref() else {
            continue;
        };
        let node = node_id_of(entity);
        if host.has_method(node, &hook) {
            host.call_on(node, &hook, args);
        }
    }
}

/// The node under the pointer, or `None`.
///
/// 3D asks the renderer, which casts against the triangles a node actually
/// draws; 2D takes the smallest shape whose box contains the point, which is
/// the rule the editor's own picker uses.
fn under_pointer(eng: &Engine) -> Option<Entity> {
    balaur_render::pick_under_pointer(eng)
}

fn button_name(button: u8) -> Value {
    Value::Str(
        match button {
            1 => "right",
            2 => "middle",
            _ => "left",
        }
        .to_string(),
    )
}

/// Everything the pointer did this tick.
fn pointer_system(eng: &Engine, state: &mut Pointer) {
    let Some(input) = eng.try_resource::<balaur_input::InputSnapshot>() else {
        return;
    };
    let (pressed, released, delta, scroll) = {
        let input = input.borrow();
        (
            input.mouse_just_pressed(0),
            input.mouse_just_released(0),
            input.mouse_delta(),
            input.scroll_delta(),
        )
    };
    let over = under_pointer(eng);
    if over != state.over {
        if let Some(was) = state.over {
            dispatch(eng, was, "pointer_exit", &[]);
        }
        if let Some(now) = over {
            dispatch(eng, now, "pointer_enter", &[]);
        }
        state.over = over;
    }
    if pressed {
        state.pressed = over;
        state.dragging = false;
        if let Some(node) = over {
            dispatch(eng, node, "pointer_down", &[button_name(0)]);
        }
    }
    if state.pressed.is_some() && (delta.0 != 0.0 || delta.1 != 0.0) {
        state.dragging = true;
        if let Some(node) = state.pressed {
            dispatch(
                eng,
                node,
                "pointer_drag",
                &[
                    Value::Num(f64::from(delta.0)),
                    Value::Num(f64::from(delta.1)),
                ],
            );
        }
    }
    if released {
        if let Some(node) = state.pressed {
            dispatch(eng, node, "pointer_up", &[button_name(0)]);
            // A press and a release on one node is a click; a release over
            // another node is a drop on that one, which is what a drag ends as.
            if over == Some(node) && !state.dragging {
                dispatch(eng, node, "pointer_click", &[button_name(0)]);
            } else if let Some(landed) = over {
                dispatch(
                    eng,
                    landed,
                    "pointer_drop",
                    &[Value::Node(node_id_of(node).0)],
                );
            }
        }
        state.pressed = None;
        state.dragging = false;
    }
    if scroll.1 != 0.0 || scroll.0 != 0.0 {
        let args = [
            Value::Num(f64::from(scroll.0)),
            Value::Num(f64::from(scroll.1)),
        ];
        match over {
            Some(node) => dispatch(eng, node, "scroll", &args),
            None => broadcast(eng, "scroll", &args),
        }
    }
}

/// Keys and declared actions, which belong to the scene rather than to a node.
fn input_system(eng: &Engine, state: &mut Pointer) {
    let Some(input) = eng.try_resource::<balaur_input::InputSnapshot>() else {
        return;
    };
    // Only the keys something asked about: broadcasting every key on the
    // board would be a hook per node per keystroke, and a scene that reads
    // none of them would pay for all of them.
    let (down, up) = {
        let input = input.borrow();
        let taken = |test: &dyn Fn(&str) -> bool| -> Vec<String> {
            balaur_input::known_keys()
                .iter()
                .filter(|key| test(key))
                .map(|key| (*key).to_string())
                .collect()
        };
        (
            taken(&|key| input.just_pressed(key)),
            taken(&|key| input.just_released(key)),
        )
    };
    for key in down {
        broadcast(eng, "key_down", &[Value::Str(key)]);
    }
    for key in up {
        broadcast(eng, "key_up", &[Value::Str(key)]);
    }
    if let Some(actions) = eng.try_resource::<balaur_input::InputActions>() {
        let fired: Vec<String> = {
            let actions = actions.borrow();
            actions
                .names()
                .into_iter()
                .filter(|name| actions.just_pressed(name))
                .collect()
        };
        for name in fired {
            broadcast(eng, "action", &[Value::Str(name)]);
        }
    }
    let size = balaur_render::viewport_size(eng);
    if size != state.size && state.size != (0, 0) {
        broadcast(
            eng,
            "resize",
            &[Value::Num(f64::from(size.0)), Value::Num(f64::from(size.1))],
        );
    }
    state.size = size;
}

/// Say what runs each binding action core cannot run itself.
///
/// Here rather than in each plugin: this crate is the one that has them all,
/// and a build without one is a build whose bindings say so rather than
/// silently doing nothing.
fn fill_action_runners(app: &balaur_core::App) {
    use balaur_core::bindings::{Action, set_runner};
    use std::rc::Rc;

    let eng = &app.engine;
    set_runner(
        eng,
        Action::Play,
        Rc::new(|eng: &Engine, entity, value: &Value| {
            balaur_anim::play(eng, entity, &text_of(value))
        }),
    );
    // Only in a build with audio. A binding naming `sound` in one without it
    // is an error saying so, which is the point of the registry.
    #[cfg(feature = "audio")]
    set_runner(
        eng,
        Action::Sound,
        Rc::new(|eng: &Engine, entity, _value: &Value| {
            balaur_audio::play_on(eng, entity).map(|_| ())
        }),
    );
    set_runner(
        eng,
        Action::Spawn,
        Rc::new(|eng: &Engine, entity, value: &Value| {
            let source = balaur_core::project::scene_text(eng, &text_of(value))?;
            balaur_core::project::instantiate_scene(eng, &source, entity, true)
        }),
    );
    set_runner(
        eng,
        Action::Switch,
        Rc::new(|eng: &Engine, _entity, value: &Value| {
            balaur_core::scene_switch::request(eng, &text_of(value));
            Ok(())
        }),
    );
    set_runner(
        eng,
        Action::OpenUrl,
        Rc::new(|_eng: &Engine, _entity, value: &Value| {
            balaur_core::desktop::open_url(&text_of(value))
        }),
    );
    set_runner(
        eng,
        Action::Emit,
        Rc::new(|eng: &Engine, entity, value: &Value| {
            balaur_core::events::emit_from(eng, entity, &text_of(value), Value::Nil);
            Ok(())
        }),
    );
}

fn text_of(value: &Value) -> String {
    match value {
        Value::Str(s) => s.clone(),
        Value::Num(n) => format!("{n}"),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// Register the dispatchers on the app.
///
/// At `First`, after input has been pumped and before scripts update, so a
/// hook and the tick's own `update` see the same world.
pub(crate) fn install(app: &mut balaur_core::App) {
    fill_action_runners(app);
    let mut state = Pointer::default();
    app.add_system(Stage::First, move |eng: &Engine, _| {
        pointer_system(eng, &mut state);
        input_system(eng, &mut state);
    });
    app.add_system(Stage::Last, balaur_core::variables::dispatch_changes_system);
    app.add_system(Stage::Last, balaur_core::scene_switch::apply_system);
}
