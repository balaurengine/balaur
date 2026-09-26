//! What the draw saw, applied inside the next tick.
//!
//! egui pumps its events during the render pass, which runs after the tick and
//! not at all headless. Writing `clicked` there moved the digest of a tick no
//! recording could reproduce, and `on_click` never ran on a replay or under
//! `--headless`. The draw now only reports what it saw; this takes that report
//! at the top of the next tick, from a resource a recording carries.

use balaur_core::hecs::{Entity, World};
use balaur_core::{Engine, Stage, hooks, replay};
use balaur_script::Value;
use serde::{Deserialize, Serialize};

use crate::vocabulary::keys as k;
use crate::vocabulary::words as w;
use crate::widget::layer::Edit;
use crate::widget::node::Widget;

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
    reg.insert_resource(Hovered::default());
    reg.insert_resource(Shown::default());
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

/// Click a widget as the pointer would, settled at the next tick: no window
/// or draw pass needed, so a headless harness drives the game with it. False,
/// and nothing clicked, for a node a pointer could not click: hidden,
/// disabled, or no widget at all. `hidden` clicks a hidden one anyway, as a
/// test emitting a button's signal does; a disabled one never.
pub fn click(eng: &Engine, entity: Entity, hidden: bool) -> bool {
    let clickable = {
        let world = eng.world();
        let shown = hidden
            || world
                .get::<&balaur_core::GlobalAppearance>(entity)
                .is_ok_and(|a| a.visible);
        let enabled = world.get::<&Widget>(entity).is_ok_and(|w| !w.disabled);
        shown && enabled
    };
    if clickable {
        record(eng, &[entity], Vec::new(), None);
    }
    clickable
}

/// Submit a `text_field` as Enter would, settled at the next tick: the text lands
/// on the widget and `submitted` is true for one frame. What a headless
/// harness types with, and what proves a pooled row hears it.
pub fn submit(eng: &Engine, entity: Entity, text: &str) -> bool {
    let writable = eng
        .world()
        .get::<&Widget>(entity)
        .is_ok_and(|widget| !widget.disabled);
    if writable {
        record(
            eng,
            &[],
            vec![(entity, Edit::Submit(text.to_owned()))],
            None,
        );
    }
    writable
}

/// What a widget emits from its own node when its value changes, and when a
/// field is submitted, with the new value: a `[[nodes.bindings.rows]]` row answers
/// `emitted:change` on any node's script, as a Godot signal connected in a
/// scene does.
pub const CHANGE_EVENT: &str = "change";
pub const SUBMIT_EVENT: &str = "submit";
/// What a clicked widget emits from its node, the `on_click` key's name
/// without its `on_`, as `change` and `submit` are: what a script awaits as
/// `task::wait(events::next("click", button))`.
pub const CLICK_EVENT: &str = "click";

/// What a `[url]` span emits when it is clicked, with its target.
pub const LINK_EVENT: &str = "link";

/// What a `code` widget emits when its gutter is clicked, with the line.
pub const GUTTER_EVENT: &str = "gutter";

/// What a row view emits when a dragged row is dropped, with the row moved,
/// the row it landed on, and where it went.
pub const MOVE_EVENT: &str = "move";

/// What a `list` emits when a card dragged out of it is let go, with the card.
pub const DROP_EVENT: &str = "drop";

/// What a widget emits when the primary button clicks twice over it, when
/// focus arrives at it, and when focus leaves it.
pub const DOUBLE_CLICK_EVENT: &str = "double_click";
pub const FOCUS_EVENT: &str = "focus";
pub const BLUR_EVENT: &str = "blur";

/// What a widget emits once a drag or a typed number that changed its value
/// is over, when a row is double-clicked, and when a tree row folds.
pub const COMMIT_EVENT: &str = "commit";
pub const ACTIVATE_EVENT: &str = "activate";
pub const FOLD_EVENT: &str = "fold";
/// What a widget emits when its popup, dialog or window comes up and goes
/// away, and when its `scroll` moves.
pub const OPENED_EVENT: &str = "opened";
pub const CLOSED_EVENT: &str = "closed";
pub const SCROLLED_EVENT: &str = "scrolled";
/// What a `window` emits when its close button is pressed, before it shuts
/// or, with `hide_on_close` off, instead of shutting.
pub const CLOSE_REQUEST_EVENT: &str = "close_request";

/// Which widgets the pointer was over at the end of the last pass.
#[derive(Default)]
pub(crate) struct Hovered(Vec<Entity>);

/// Which widgets had a popup, a dialog or a window up at the end of the last
/// pass.
#[derive(Default)]
pub(crate) struct Shown(Vec<Entity>);

/// What the pass saw over its whole length: the pointer's edits, and each
/// popup, dialog and window that came up or went away.
pub(crate) fn pass_edits(
    eng: &Engine,
    ctx: &egui::Context,
    painting: &crate::widget::layer::Painting<'_>,
) -> Vec<(Entity, Edit)> {
    let mut out = pointer_edits(eng, ctx, &painting.under);
    let shown = &painting.shown;
    let was = std::mem::replace(&mut eng.resource::<Shown>().borrow_mut().0, shown.clone());
    out.extend(
        shown
            .iter()
            .filter(|e| !was.contains(e))
            .map(|e| (*e, Edit::Opened)),
    );
    out.extend(
        was.iter()
            .filter(|e| !shown.contains(e))
            .map(|e| (*e, Edit::Closed)),
    );
    out
}

/// The pointer's edits for this pass: enter and exit for every widget it
/// came over or left, and the button presses over the innermost one.
fn pointer_edits(eng: &Engine, ctx: &egui::Context, under: &[Entity]) -> Vec<(Entity, Edit)> {
    let mut out = Vec::new();
    let was = std::mem::replace(
        &mut eng.resource::<Hovered>().borrow_mut().0,
        under.to_vec(),
    );
    out.extend(
        under
            .iter()
            .filter(|e| !was.contains(e))
            .map(|e| (*e, Edit::Entered)),
    );
    out.extend(
        was.iter()
            .filter(|e| !under.contains(e))
            .map(|e| (*e, Edit::Left)),
    );
    let Some(innermost) = under.last().copied() else {
        return out;
    };
    let buttons = [
        egui::PointerButton::Primary,
        egui::PointerButton::Secondary,
        egui::PointerButton::Middle,
    ];
    ctx.input(|i| {
        for (button, name) in buttons.into_iter().zip(balaur_core::hooks::BUTTONS) {
            if i.pointer.button_pressed(button) {
                out.push((innermost, Edit::Pressed(name.to_string())));
            }
            if i.pointer.button_released(button) {
                out.push((innermost, Edit::Released(name.to_string())));
            }
        }
        if i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
        {
            out.push((innermost, Edit::DoubleClicked));
        }
    });
    out
}

/// Every event a widget emits from its node, with what it carries.
pub(crate) const EVENTS: &[(&str, &str)] = &[
    (CLICK_EVENT, "nil"),
    (CHANGE_EVENT, "the new value"),
    (SUBMIT_EVENT, "the text"),
    (LINK_EVENT, "the link's target"),
    (GUTTER_EVENT, "the line"),
    (MOVE_EVENT, "`[moved, target, side]`"),
    (DROP_EVENT, "the card"),
    (DOUBLE_CLICK_EVENT, "nil"),
    (FOCUS_EVENT, "nil"),
    (BLUR_EVENT, "nil"),
    (
        COMMIT_EVENT,
        "the value, once the drag or the typing is over",
    ),
    (ACTIVATE_EVENT, "the row double-clicked"),
    (FOLD_EVENT, "`#{ row, open }`"),
    (OPENED_EVENT, "nil"),
    (CLOSED_EVENT, "nil"),
    (SCROLLED_EVENT, "the offset, `[x, y]`"),
    (CLOSE_REQUEST_EVENT, "nil"),
];

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
    let (mut typed, submitted) = settle_edits(eng, &edits, &mut emitted);
    let signals = settle_clicks(eng, &clicked, &submitted, &mut typed, &mut emitted);
    // A pointer event is a core hook: its rows, and the node's own
    // `on_pointer_*`, as a world node's. The widget's own events are emitted.
    for (entity, event, value) in emitted {
        if hooks::BINDABLE.contains(&event) {
            balaur_core::events::announce(eng, entity, event, value);
        } else {
            balaur_core::events::emit_from(eng, entity, event, value);
        }
    }
    // A clicked widget's `pointer_click` rows, so a button can call any
    // node's script from the scene alone, as a world object's click can.
    for entity in clicked.iter().filter_map(|key| resolve(eng, key)) {
        balaur_core::bindings::fire(eng, entity, balaur_core::hooks::POINTER_CLICK, &[]);
        balaur_core::events::emit_from(eng, entity, CLICK_EVENT, Value::Nil);
    }
    // Dispatch once the world borrow is gone: a handler may spawn, free or
    // reparent nodes, and it must not do that mid-iteration.
    if let Some(host) = eng.script_host() {
        // Every recipient found before any handler runs, which may reshape
        // the tree the search walks.
        let signals: Vec<_> = signals
            .into_iter()
            .map(|(entity, method)| {
                let args = passed(eng, entity, Vec::new());
                (recipient(eng, host.as_ref(), entity, &method), method, args)
            })
            .collect();
        let typed: Vec<_> = typed
            .into_iter()
            .map(|(entity, method, value)| {
                let args = passed(eng, entity, vec![value]);
                (recipient(eng, host.as_ref(), entity, &method), method, args)
            })
            .collect();
        for (entity, method, args) in signals.into_iter().chain(typed) {
            host.call_on(balaur_core::node_id_of(entity), &method, &args);
        }
    }
    announce_focus(eng, focused.as_ref());
}

/// A handler's arguments, with the widget's own node last where it asks to
/// pass it.
fn passed(eng: &Engine, entity: Entity, mut args: Vec<Value>) -> Vec<Value> {
    let pass = eng
        .world()
        .get::<&Widget>(entity)
        .is_ok_and(|widget| widget.pass_node);
    if pass {
        args.push(Value::Node(balaur_core::node_id_of(entity).0));
    }
    args
}

/// One edit written onto the widget it names, and what that says.
///
/// The match answers with the event, the value and the handler rather than
/// reporting them, so the three lines that emit and call are written once
/// for every kind of edit. `None` is an edit that reports nothing.
fn settle_one(
    entity: Entity,
    widget: &mut Widget,
    edit: &Edit,
    emitted: &mut Vec<(Entity, &'static str, Value)>,
    signals: &mut Vec<(Entity, String, Value)>,
) {
    let said = match edit {
        Edit::Width(w) => {
            widget.width = *w;
            None
        }
        Edit::Height(h) => {
            widget.height = *h;
            None
        }
        Edit::Active(name) => {
            widget.active = name.as_str().into();
            Some((CHANGE_EVENT, Value::Str(name.clone()), &widget.on_change))
        }
        Edit::Moved([dx, dy]) => {
            let (sx, sy) = crate::widget::window::drag_signs(&widget.anchor);
            widget.x += dx * sx;
            widget.y += dy * sy;
            None
        }
        Edit::Text(text) => {
            widget.text = text.as_str().into();
            Some((CHANGE_EVENT, Value::Str(text.clone()), &widget.on_change))
        }
        Edit::Submit(text) => {
            widget.text = text.as_str().into();
            Some((SUBMIT_EVENT, Value::Str(text.clone()), &widget.on_submit))
        }
        Edit::Value(value) => {
            widget.value = *value;
            let said = Value::Num(f64::from(*value));
            Some((CHANGE_EVENT, said, &widget.on_change))
        }
        Edit::Open(open) => {
            widget.open = *open;
            Some((CHANGE_EVENT, Value::Bool(*open), &widget.on_change))
        }
        Edit::Color(rgba) => {
            widget.color = *rgba;
            Some((CHANGE_EVENT, Value::Color(*rgba), &widget.on_change))
        }
        // A dropdown shows what was picked; a menu keeps its caption.
        Edit::Choice(choice) => {
            if widget.kind != w::MENU {
                widget.text = choice.as_str().into();
            }
            Some((CHANGE_EVENT, Value::Str(choice.clone()), &widget.on_change))
        }
        Edit::Picked(row, rows) => {
            let said = picked(widget, row, rows);
            Some((CHANGE_EVENT, said, &widget.on_change))
        }
        // The chrome a reader dragged: written down so the scene keeps it,
        // and reported to nobody, as a seam between two containers is.
        Edit::Widths(shares) => {
            widget.widths.clone_from(shares);
            None
        }
        Edit::Sorted(column, reverse) => {
            widget.sort = column.as_str().into();
            widget.reverse = *reverse;
            None
        }
        Edit::Dropped(moved, target, side) => {
            let said = [moved, target, side]
                .into_iter()
                .map(|part| Value::Str(part.clone()))
                .collect();
            Some((MOVE_EVENT, Value::List(said), &widget.on_move))
        }
        Edit::Carried(card) => Some((DROP_EVENT, Value::Str(card.clone()), &widget.on_drop)),
        // Written nowhere: a link and a gutter mark are the script's to act on.
        Edit::Link(target) => Some((LINK_EVENT, Value::Str(target.clone()), &widget.on_link)),
        Edit::Gutter(line) => Some((GUTTER_EVENT, Value::Int(*line), &widget.on_gutter)),
        Edit::Entered
        | Edit::Left
        | Edit::Pressed(_)
        | Edit::Released(_)
        | Edit::DoubleClicked
        | Edit::Blurred
        | Edit::Committed(_)
        | Edit::ColorCommitted(_)
        | Edit::Activated(_)
        | Edit::Folded(..)
        | Edit::Opened
        | Edit::Closed
        | Edit::Scrolled(_)
        | Edit::CloseRequested => {
            emitted.extend(heard(edit).map(|(event, value)| (entity, event, value)));
            None
        }
    };
    // The node emits its event whatever the widget carries, and the handler
    // is called only where one is named.
    let Some((event, value, handler)) = said else {
        return;
    };
    emitted.push((entity, event, value.clone()));
    if !handler.is_empty() {
        signals.push((entity, handler.to_string(), value));
    }
}

/// What the pointer, focus or a finished gesture did: an event with no
/// handler key, emitted as it is.
fn heard(edit: &Edit) -> Option<(&'static str, Value)> {
    let said = match edit {
        Edit::Entered => (hooks::POINTER_ENTER, Value::Nil),
        Edit::Left => (hooks::POINTER_EXIT, Value::Nil),
        Edit::Pressed(button) => (hooks::POINTER_DOWN, Value::Str(button.clone())),
        Edit::Released(button) => (hooks::POINTER_UP, Value::Str(button.clone())),
        Edit::DoubleClicked => (DOUBLE_CLICK_EVENT, Value::Nil),
        Edit::Blurred => (BLUR_EVENT, Value::Nil),
        Edit::Committed(value) => (COMMIT_EVENT, Value::Num(f64::from(*value))),
        Edit::ColorCommitted(rgba) => (COMMIT_EVENT, Value::Color(*rgba)),
        Edit::Activated(row) => (ACTIVATE_EVENT, Value::Str(row.clone())),
        Edit::Folded(row, open) => {
            let said = vec![
                (k::ROW.into(), Value::Str(row.clone())),
                (k::OPEN.into(), Value::Bool(*open)),
            ];
            (FOLD_EVENT, Value::Map(said))
        }
        Edit::Opened => (OPENED_EVENT, Value::Nil),
        Edit::Closed => (CLOSED_EVENT, Value::Nil),
        Edit::Scrolled(offset) => (SCROLLED_EVENT, Value::Vec2(*offset)),
        Edit::CloseRequested => (CLOSE_REQUEST_EVENT, Value::Nil),
        _ => return None,
    };
    Some(said)
}

/// A row pick written onto the widget, and what it says: the whole set where
/// it holds many, and the row hit where it holds one.
fn picked(widget: &mut Widget, row: &str, rows: &[String]) -> Value {
    widget.text = row.into();
    widget.selection = rows.iter().map(|row| row.as_str().into()).collect();
    if widget.multi {
        return Value::List(rows.iter().map(|row| Value::Str(row.clone())).collect());
    }
    Value::Str(row.to_owned())
}

/// Apply a dragged seam, a chosen tab or typed text to the widget that owns
/// it, and collect the field handlers to call with what was typed.
fn settle_edits(
    eng: &Engine,
    edits: &[(WidgetKey, Edit)],
    emitted: &mut Vec<(Entity, &'static str, Value)>,
) -> (Vec<(Entity, String, Value)>, Vec<Entity>) {
    let mut signals = Vec::new();
    let mut submitted = Vec::new();
    for (key, edit) in edits {
        let Some(entity) = resolve(eng, key) else {
            continue;
        };
        if matches!(edit, Edit::Submit(_)) {
            submitted.push(entity);
        }
        let world = eng.world();
        let Ok(mut widget) = world.get::<&mut Widget>(entity) else {
            continue;
        };
        // Written straight onto the component, so the arena's copy of this one
        // is stale until the next pass re-reads it.
        crate::widget::arena::widget_changed(entity);
        settle_one(entity, &mut widget, edit, emitted, &mut signals);
    }
    (signals, submitted)
}

/// Write this frame's `clicked` and `submitted` onto every widget, and collect
/// the handlers.
///
/// Every widget, not only the ones hit: both are true for one frame, and a
/// button nobody pressed this tick has to say so.
fn settle_clicks(
    eng: &Engine,
    clicked: &[WidgetKey],
    submitted: &[Entity],
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
            crate::widget::arena::widget_changed(entity);
        }
        let said = submitted.contains(&entity);
        if widget.submitted != said {
            widget.submitted = said;
            crate::widget::arena::widget_changed(entity);
        }
        if !struck {
            continue;
        }
        // A click on a check or toggle is the tick itself, by mouse or by
        // `accept`. One in a group is a radio: it ticks and stays ticked, and
        // the pass below unticks the rest of its group.
        if flips(widget) {
            let grouped = !widget.group.is_empty();
            let was = widget.checked;
            // Grouped, a click picks: it ticks and a second click leaves it
            // ticked, because something in the group has to be. On its own, a
            // click flips.
            widget.checked = if grouped { true } else { !was };
            if widget.checked != was {
                crate::widget::arena::widget_changed(entity);
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
            if entity == *struck || !flips(widget) || widget.group != *group {
                continue;
            }
            if !widget.checked {
                continue;
            }
            widget.checked = false;
            crate::widget::arena::widget_changed(entity);
            emitted.push((entity, CHANGE_EVENT, Value::Bool(false)));
            if !widget.on_change.is_empty() {
                changes.push((entity, widget.on_change.to_string(), Value::Bool(false)));
            }
        }
    }
    signals
}

/// A widget a click ticks and unticks: a `checkbox`, or a `toggle` button.
fn flips(widget: &Widget) -> bool {
    widget.kind == w::CHECKBOX || widget.kind == w::SWITCH || widget.toggle
}

/// Tell the newly focused widget's script that focus arrived.
fn announce_focus(eng: &Engine, focused: Option<&WidgetKey>) {
    let Some(entity) = focused.and_then(|key| resolve(eng, key)) else {
        return;
    };
    balaur_core::events::emit_from(eng, entity, FOCUS_EVENT, Value::Nil);
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
