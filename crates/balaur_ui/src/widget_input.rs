//! What the draw saw, applied inside the next tick.
//!
//! egui pumps its events during the render pass, which runs after the tick and
//! not at all headless. Writing `clicked` there moved the digest of a tick no
//! recording could reproduce, and `on_click` never ran on a replay or under
//! `--headless`. The draw now only reports what it saw; this takes that report
//! at the top of the next tick, from a resource a recording carries.

use balaur_core::hecs::{Entity, World};
use balaur_core::{Engine, Stage, replay};
use balaur_script::Value;
use serde::{Deserialize, Serialize};

use crate::vocabulary::words as w;
use crate::widget_layer::{Edit, Widget};

/// How a frame names a widget: its stable id, and its entity bits for a tree
/// built by hand. Bits alone would not survive the respawn a rollback does.
type WidgetKey = (String, u64);

/// One frame of widget input, in the form a recording replays.
///
/// Registered as a replay resource under `ui`, so a verified session feeds the
/// same clicks back rather than asking a window that is not there.
#[derive(Default, Serialize, Deserialize)]
pub struct WidgetInputSnapshot {
    clicked: Vec<WidgetKey>,
    /// The widget focus arrived at, recorded only on the frame it changed.
    focused: Option<WidgetKey>,
    edits: Vec<(WidgetKey, Edit)>,
}

/// What the last draw reported and no tick has taken yet.
#[derive(Default)]
pub struct WidgetInputBuffer(Option<WidgetInputSnapshot>);

pub(crate) fn register(reg: &mut balaur_plugin::Registry<'_>) {
    reg.insert_resource(WidgetInputBuffer::default());
    reg.insert_resource(WidgetInputSnapshot::default());
    reg.add_replay_resource::<WidgetInputSnapshot>("ui");
    // After core's replay restore, which is the first system in the stage, and
    // before `update`, so a handler runs in the tick that owns the click.
    reg.add_system(Stage::First, apply_system);
}

fn key_of(world: &World, entity: Entity) -> WidgetKey {
    (
        balaur_core::ids::of(world, entity).unwrap_or_default(),
        entity.to_bits().get(),
    )
}

/// The widget a key names now, which is a different entity after a respawn.
fn resolve(eng: &Engine, key: &WidgetKey) -> Option<Entity> {
    let world = eng.world();
    if !key.0.is_empty()
        && let Some(entity) = balaur_core::ids::find(&world, eng.root(), &key.0)
    {
        return Some(entity);
    }
    let entity = Entity::from_bits(key.1)?;
    world.contains(entity).then_some(entity)
}

/// Hand one pass's clicks, edits and focus arrival to the next tick.
///
/// Nothing is written to the world here: the draw walks a snapshot of the
/// tree, and a handler may free or reparent the nodes it is walking.
pub(crate) fn record(
    eng: &Engine,
    clicked: &[Entity],
    edits: Vec<(Entity, Edit)>,
    focused: Option<Entity>,
) {
    let (hit, arrived, changes) = {
        let world = eng.world();
        (
            clicked
                .iter()
                .map(|&e| key_of(&world, e))
                .collect::<Vec<_>>(),
            focused.map(|e| key_of(&world, e)),
            edits
                .into_iter()
                .map(|(entity, edit)| (key_of(&world, entity), edit))
                .collect::<Vec<_>>(),
        )
    };
    let buffer = eng.resource::<WidgetInputBuffer>();
    let mut buffer = buffer.borrow_mut();
    // Added to, not replaced: a display faster than the tick draws twice
    // between two ticks, and neither pass's clicks may be dropped.
    let frame = buffer.0.get_or_insert_with(WidgetInputSnapshot::default);
    for key in hit {
        if !frame.clicked.contains(&key) {
            frame.clicked.push(key);
        }
    }
    if arrived.is_some() {
        frame.focused = arrived;
    }
    // In order, so the last width a drag reported is the one that lands.
    frame.edits.extend(changes);
}

/// What a widget emits from its own node when its value changes, and when a
/// field is submitted, with the new value: a `[[nodes.bindings]]` row answers
/// `emitted:change` on any node's script, as a Godot signal connected in a
/// scene does.
pub const CHANGE_EVENT: &str = "change";
pub const SUBMIT_EVENT: &str = "submit";

fn apply_system(eng: &Engine, _dt: f32) {
    // A replay keeps what `restore` just put back, and a re-simulated tick
    // keeps what its first run had; only a live tick takes the draw's report.
    if !replay::suppressed(eng) {
        let taken = eng.resource::<WidgetInputBuffer>().borrow_mut().0.take();
        *eng.resource::<WidgetInputSnapshot>().borrow_mut() = taken.unwrap_or_default();
    }
    let (clicked, focused, edits) = {
        let frame = eng.resource::<WidgetInputSnapshot>();
        let frame = frame.borrow();
        (
            frame.clicked.clone(),
            frame.focused.clone(),
            frame.edits.clone(),
        )
    };
    let mut emitted = Vec::new();
    let mut typed = settle_edits(eng, &edits, &mut emitted);
    let signals = settle_clicks(eng, &clicked, &mut typed, &mut emitted);
    for (entity, event, value) in emitted {
        balaur_core::events::emit_from(eng, entity, event, value);
    }
    // A clicked widget's `pointer_click` rows, so a button can call any
    // node's script from the scene alone, as a world object's click can.
    for entity in clicked.iter().filter_map(|key| resolve(eng, key)) {
        balaur_core::bindings::fire(eng, entity, balaur_core::hooks::POINTER_CLICK, &[]);
    }
    // Dispatch once the world borrow is gone: a handler may spawn, free or
    // reparent nodes, and it must not do that mid-iteration.
    if let Some(host) = eng.script_host() {
        // Every recipient found before any handler runs, which may reshape
        // the tree the search walks.
        let signals: Vec<_> = signals
            .into_iter()
            .map(|(entity, method)| (recipient(eng, host.as_ref(), entity, &method), method))
            .collect();
        let typed: Vec<_> = typed
            .into_iter()
            .map(|(entity, method, value)| (recipient(eng, host.as_ref(), entity, &method), method, value))
            .collect();
        for (entity, method) in signals {
            host.call_on(balaur_core::node_id_of(entity), &method, &[]);
        }
        for (entity, method, value) in typed {
            host.call_on(
                balaur_core::node_id_of(entity),
                &method,
                std::slice::from_ref(&value),
            );
        }
    }
    announce_focus(eng, focused.as_ref());
}

/// Apply a dragged seam, a chosen tab or typed text to the widget that owns
/// it, and collect the field handlers to call with what was typed.
fn settle_edits(
    eng: &Engine,
    edits: &[(WidgetKey, Edit)],
    emitted: &mut Vec<(Entity, &'static str, Value)>,
) -> Vec<(Entity, String, Value)> {
    let mut signals = Vec::new();
    for (key, edit) in edits {
        let Some(entity) = resolve(eng, key) else {
            continue;
        };
        let world = eng.world();
        let Ok(mut widget) = world.get::<&mut Widget>(entity) else {
            continue;
        };
        // Written straight onto the component, so the arena's copy of this one
        // is stale until the next pass re-reads it.
        crate::widget_arena::widget_changed(entity);
        match edit {
            Edit::Width(w) => widget.width = *w,
            Edit::Height(h) => widget.height = *h,
            Edit::Active(name) => widget.active = name.as_str().into(),
            Edit::Text(text) => {
                widget.text = text.as_str().into();
                emitted.push((entity, CHANGE_EVENT, Value::Str(text.clone())));
                if !widget.on_change.is_empty() {
                    signals.push((
                        entity,
                        widget.on_change.to_string(),
                        Value::Str(text.clone()),
                    ));
                }
            }
            Edit::Submit(text) => {
                widget.text = text.as_str().into();
                emitted.push((entity, SUBMIT_EVENT, Value::Str(text.clone())));
                if !widget.on_submit.is_empty() {
                    signals.push((
                        entity,
                        widget.on_submit.to_string(),
                        Value::Str(text.clone()),
                    ));
                }
            }
            Edit::Value(value) => {
                widget.value = *value;
                emitted.push((entity, CHANGE_EVENT, Value::Num(f64::from(*value))));
                if !widget.on_change.is_empty() {
                    signals.push((
                        entity,
                        widget.on_change.to_string(),
                        Value::Num(f64::from(*value)),
                    ));
                }
            }
            Edit::Open(open) => {
                widget.open = *open;
                emitted.push((entity, CHANGE_EVENT, Value::Bool(*open)));
                if !widget.on_change.is_empty() {
                    signals.push((entity, widget.on_change.to_string(), Value::Bool(*open)));
                }
            }
            Edit::Choice(choice) => {
                widget.text = choice.as_str().into();
                emitted.push((entity, CHANGE_EVENT, Value::Str(choice.clone())));
                if !widget.on_change.is_empty() {
                    signals.push((
                        entity,
                        widget.on_change.to_string(),
                        Value::Str(choice.clone()),
                    ));
                }
            }
            Edit::Moved([dx, dy]) => {
                let (sx, sy) = crate::widget_window::drag_signs(&widget.anchor);
                widget.x += dx * sx;
                widget.y += dy * sy;
            }
            Edit::Color(rgba) => {
                widget.color = *rgba;
                emitted.push((entity, CHANGE_EVENT, Value::Color(*rgba)));
                if !widget.on_change.is_empty() {
                    signals.push((entity, widget.on_change.to_string(), Value::Color(*rgba)));
                }
            }
        }
    }
    signals
}

/// Write this frame's `clicked` onto every widget, and collect the handlers.
///
/// Every widget, not only the ones hit: `clicked` is true for one frame, and a
/// button nobody pressed this tick has to say so.
fn settle_clicks(
    eng: &Engine,
    clicked: &[WidgetKey],
    changes: &mut Vec<(Entity, String, Value)>,
    emitted: &mut Vec<(Entity, &'static str, Value)>,
) -> Vec<(Entity, String)> {
    let hit: Vec<Entity> = clicked.iter().filter_map(|key| resolve(eng, key)).collect();
    let mut signals = Vec::new();
    // The grouped checks this tick ticked, unticked in a second pass: the loop
    // below holds one widget at a time and a group's siblings are others.
    let mut ticked: Vec<(Entity, smol_str::SmolStr)> = Vec::new();
    let world = eng.world();
    for (entity, widget) in &mut world.query::<(Entity, &mut Widget)>() {
        let struck = hit.contains(&entity);
        // Only on the change: this runs over every widget every frame, and a
        // write that put the same `false` back would rebuild the whole arena.
        if widget.clicked != struck {
            widget.clicked = struck;
            crate::widget_arena::widget_changed(entity);
        }
        if !struck {
            continue;
        }
        // A click on a check is the tick itself, by mouse or by `accept`. One
        // in a group is a radio: it ticks and stays ticked, and the pass below
        // unticks the rest of its group.
        if widget.kind == w::CHECK {
            let grouped = !widget.group.is_empty();
            let was = widget.checked;
            // Grouped, a click picks: it ticks and a second click leaves it
            // ticked, because something in the group has to be. On its own, a
            // click flips.
            widget.checked = if grouped { true } else { !was };
            if widget.checked != was {
                crate::widget_arena::widget_changed(entity);
                emitted.push((entity, CHANGE_EVENT, Value::Bool(widget.checked)));
                if !widget.on_change.is_empty() {
                    changes.push((
                        entity,
                        widget.on_change.to_string(),
                        Value::Bool(widget.checked),
                    ));
                }
            }
            if grouped {
                ticked.push((entity, widget.group.clone()));
            }
        }
        if !widget.on_click.is_empty() {
            signals.push((entity, widget.on_click.to_string()));
        }
    }
    for (struck, group) in &ticked {
        for (entity, widget) in &mut world.query::<(Entity, &mut Widget)>() {
            if entity == *struck || widget.kind != w::CHECK || widget.group != *group {
                continue;
            }
            if !widget.checked {
                continue;
            }
            widget.checked = false;
            crate::widget_arena::widget_changed(entity);
            emitted.push((entity, CHANGE_EVENT, Value::Bool(false)));
            if !widget.on_change.is_empty() {
                changes.push((entity, widget.on_change.to_string(), Value::Bool(false)));
            }
        }
    }
    signals
}

/// Tell the newly focused widget's script that focus arrived.
fn announce_focus(eng: &Engine, focused: Option<&WidgetKey>) {
    let Some(entity) = focused.and_then(|key| resolve(eng, key)) else {
        return;
    };
    let method = {
        let world = eng.world();
        let Ok(widget) = world.get::<&Widget>(entity) else {
            return;
        };
        if widget.on_focus.is_empty() {
            return;
        }
        widget.on_focus.clone()
    };
    if let Some(host) = eng.script_host() {
        let target = recipient(eng, host.as_ref(), entity, &method);
        host.call_on(balaur_core::node_id_of(target), &method, &[]);
    }
}

/// The node a widget's handler runs on: the widget's own when its script
/// declares the method, else the nearest ancestor whose script does. So a
/// button deep in a panel names a method on the panel's script, the way a
/// Godot signal is connected to the node that owns the scene. With none
/// declaring it, the widget's own node, where the call is the no-op it was.
fn recipient(
    eng: &Engine,
    host: &dyn balaur_script::ScriptHost<Engine>,
    entity: Entity,
    method: &str,
) -> Entity {
    let world = eng.world();
    let mut current = entity;
    loop {
        if host.has_method(balaur_core::node_id_of(current), method) {
            return current;
        }
        match world.get::<&balaur_core::scene::Parent>(current) {
            Ok(parent) => current = parent.0,
            Err(_) => return entity,
        }
    }
}
