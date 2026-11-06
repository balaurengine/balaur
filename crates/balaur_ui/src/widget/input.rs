//! What the draw saw, applied inside the next tick.
//!
//! egui pumps its events during the render pass, which runs after the tick and
//! not at all headless. Acting on a click there moved the digest of a tick no
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
use crate::widget::properties::{Numeral, WidgetProperties};
use crate::widget::schema::follow;

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
/// on the widget and it says `submit`. What a headless harness types with, and
/// what proves a pooled row hears it.
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

/// Change a widget's value as the reader would, settled at the next tick: the
/// value lands and the widget announces `change`. A number sets a slider or a
/// number field, a colour a swatch, a boolean a fold, and text a field, a
/// dropdown's pick, a list's row or a tab. False for a disabled widget or a
/// value its kind does not take.
pub fn edit(eng: &Engine, entity: Entity, value: &Value) -> bool {
    let edit = {
        let world = eng.world();
        let Ok(widget) = world.get::<&Widget>(entity) else {
            return false;
        };
        if widget.disabled {
            return false;
        }
        let kind = widget.kind.as_str();
        match value {
            Value::Num(n) => Some(Edit::Value(*n as f32)),
            Value::Int(n) => Some(Edit::Value(*n as f32)),
            Value::Bool(open) => Some(Edit::Open(*open)),
            Value::Color(rgba) => Some(Edit::Color(*rgba)),
            Value::Str(text) if kind == w::DROPDOWN || kind == w::MENU => {
                Some(Edit::Choice(text.clone()))
            }
            Value::Str(text) if kind == w::LIST || kind == w::TREE || kind == w::TABLE => {
                Some(Edit::Picked(text.clone(), Vec::new()))
            }
            Value::Str(text) if kind == w::TABS => Some(Edit::Active(text.clone())),
            Value::Str(text) => Some(Edit::Text(text.clone())),
            _ => None,
        }
    };
    let Some(edit) = edit else {
        return false;
    };
    record(eng, &[], vec![(entity, edit)], None);
    true
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
/// What a `code` widget emits when its caret moved, with `{ x, y, index }`:
/// where a completion list opens, and what it completes.
pub const CARET_EVENT: &str = "caret";

/// What a row view emits when a dragged row is dropped, with the row moved,
/// the row it landed on, and where it went.
pub const MOVE_EVENT: &str = "move";

/// What a `list` emits when a card dragged out of it is let go, with the card.
pub const DROP_EVENT: &str = "drop";

/// What a `list` or `tree` emits when a mark on a row is clicked, with the row
/// and the mark's name.
pub const MARK_EVENT: &str = "mark";

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
/// What a widget emits when the box it was laid out in moved or changed size,
/// with the box: only from a node someone listens to it on, and once for a
/// node that stopped drawing, with an empty box.
pub const RESIZED_EVENT: &str = "resized";
/// What the UI says from no node when text was pasted and no field took it,
/// with the text: the platform clipboard is not readable on demand.
pub const PASTED_EVENT: &str = "pasted";
/// What the UI says from no node when an image was pasted and no field took
/// it, with its `{ width, height }`: `ui::save_pasted_image` writes it out.
pub const PASTED_IMAGE_EVENT: &str = "pasted_image";

/// Which widgets the pointer was over at the end of the last pass.
#[derive(Default)]
pub(crate) struct Hovered(Vec<Entity>);

/// Which widgets had a popup, a dialog or a window up at the end of the last
/// pass.
#[derive(Default)]
pub(crate) struct Shown(Vec<Entity>);

/// What the pass saw over its whole length: the pointer's edits, and each
/// popup, dialog and window that came up or went away.
/// Say `pasted_image` for an image pasted where no field took it, and keep
/// the image for `ui::save_pasted_image`. Every pass, as the pasted text is
/// read: a pass that draws nothing still saw the paste.
pub(crate) fn hear_pasted_image(eng: &Engine, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() || !balaur_core::events::heard(eng, PASTED_IMAGE_EVENT) {
        return;
    }
    let image = ctx.input(|i| {
        i.events.iter().rev().find_map(|event| match event {
            egui::Event::PasteImage(image) => Some(image.clone()),
            _ => None,
        })
    });
    let Some(image) = image else {
        return;
    };
    let [width, height] = image.size;
    eng.resource::<crate::UiState>().borrow_mut().pasted_image = Some(image);
    balaur_core::events::emit(
        eng,
        PASTED_IMAGE_EVENT,
        Value::Map(vec![
            (
                "width".into(),
                Value::Int(i64::try_from(width).unwrap_or(i64::MAX)),
            ),
            (
                "height".into(),
                Value::Int(i64::try_from(height).unwrap_or(i64::MAX)),
            ),
        ]),
    );
}

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
    (CARET_EVENT, "`{ x, y, index }`"),
    (MOVE_EVENT, "`[moved, target, side]`"),
    (DROP_EVENT, "the card"),
    (MARK_EVENT, "`{ row, mark }`"),
    (DOUBLE_CLICK_EVENT, "nil"),
    (FOCUS_EVENT, "nil"),
    (BLUR_EVENT, "nil"),
    (
        COMMIT_EVENT,
        "the value, once the drag or the typing is over",
    ),
    (ACTIVATE_EVENT, "the row double-clicked"),
    (FOLD_EVENT, "`{ row, open }`"),
    (OPENED_EVENT, "nil"),
    (CLOSED_EVENT, "nil"),
    (SCROLLED_EVENT, "the offset, `[x, y]`"),
    (CLOSE_REQUEST_EVENT, "nil"),
    (
        RESIZED_EVENT,
        "`{ x, y, width, height }`, while someone listens on the node",
    ),
];

/// Say `pasted` for a paste no field took, where someone listens for it.
pub(crate) fn hear_paste(eng: &Engine, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() || !balaur_core::events::heard(eng, PASTED_EVENT) {
        return;
    }
    let pasted = ctx.input(|i| {
        i.events.iter().rev().find_map(|event| match event {
            egui::Event::Paste(text) => Some(text.clone()),
            _ => None,
        })
    });
    if let Some(text) = pasted {
        balaur_core::events::emit(eng, PASTED_EVENT, Value::Str(text));
    }
}

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
    // The pool's controls hear their edits and clicks here, before any named
    // handler, so a strip nobody refilled still answers.
    crate::widget::pool::heard(eng, &emitted);
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
        let told = |entity: Entity| Value::Node(balaur_core::node_id_of(entity).0);
        let signals: Vec<_> = signals
            .into_iter()
            .map(|(entity, method)| {
                (
                    recipient(eng, host.as_ref(), entity, &method),
                    method,
                    crate::widget::pool::handler_args(eng, entity, Value::Nil, told(entity)),
                )
            })
            .collect();
        let typed: Vec<_> = typed
            .into_iter()
            .map(|(entity, method, value)| {
                (
                    recipient(eng, host.as_ref(), entity, &method),
                    method,
                    crate::widget::pool::handler_args(eng, entity, value, told(entity)),
                )
            })
            .collect();
        for (entity, method, args) in signals.into_iter().chain(typed) {
            host.call_on(balaur_core::node_id_of(entity), &method, &args);
        }
    }
    announce_focus(eng, focused.as_ref());
}

/// One edit written onto the properties of the widget it names, the widget's
/// fields brought after them, and what the edit says.
///
/// The match answers with the event, the value and the handler rather than
/// reporting them, so the three lines that emit and call are written once
/// for every kind of edit. `None` is an edit that reports nothing.
fn settle_one(
    entity: Entity,
    asked: &mut WidgetProperties,
    widget: &mut Widget,
    edit: &Edit,
    emitted: &mut Vec<(Entity, &'static str, Value)>,
    signals: &mut Vec<(Entity, String, Value)>,
) {
    let keys = written(asked, edit);
    follow(widget, asked, keys);
    let said = match edit {
        Edit::Width(_) | Edit::Height(_) | Edit::Moved(_) | Edit::Widths(_) | Edit::Sorted(..) => {
            None
        }
        Edit::Active(name) => Some((CHANGE_EVENT, Value::Str(name.clone()), &widget.on_change)),
        Edit::Text(text) => Some((CHANGE_EVENT, Value::Str(text.clone()), &widget.on_change)),
        Edit::Submit(text) => Some((SUBMIT_EVENT, Value::Str(text.clone()), &widget.on_submit)),
        Edit::Value(value) => {
            let said = Value::Num(f64::from(*value));
            Some((CHANGE_EVENT, said, &widget.on_change))
        }
        Edit::Open(open) => Some((CHANGE_EVENT, Value::Bool(*open), &widget.on_change)),
        Edit::Color(rgba) => Some((CHANGE_EVENT, Value::Color(*rgba), &widget.on_change)),
        Edit::Choice(choice) => Some((CHANGE_EVENT, Value::Str(choice.clone()), &widget.on_change)),
        Edit::Picked(row, rows) => {
            let said = picked(widget.multi, row, rows);
            Some((CHANGE_EVENT, said, &widget.on_change))
        }
        Edit::Dropped(moved, target, side) => {
            let said = [moved, target, side]
                .into_iter()
                .map(|part| Value::Str(part.clone()))
                .collect();
            Some((MOVE_EVENT, Value::List(said), &widget.on_move))
        }
        Edit::Carried(card) => Some((DROP_EVENT, Value::Str(card.clone()), &widget.on_drop)),
        Edit::Marked(row, mark) => Some((MARK_EVENT, marked(row, mark), &widget.on_mark)),
        // Written nowhere: a link and a gutter mark are the script's to act on.
        Edit::Link(target) => Some((LINK_EVENT, Value::Str(target.clone()), &widget.on_link)),
        Edit::Gutter(line) => Some((GUTTER_EVENT, Value::Int(*line), &widget.on_gutter)),
        Edit::Entered | Edit::Left | Edit::Pressed(_) | Edit::Released(_) => {
            emitted.extend(heard(edit).map(|(event, value)| (entity, event, value)));
            None
        }
        Edit::DoubleClicked
        | Edit::Blurred
        | Edit::Committed(_)
        | Edit::ColorCommitted(_)
        | Edit::Activated(_)
        | Edit::Folded(..)
        | Edit::Opened
        | Edit::Closed
        | Edit::Caret(..)
        | Edit::CloseRequested
        | Edit::Scrolled(_) => heard(edit).map(|(event, value)| {
            let handler = match event {
                DOUBLE_CLICK_EVENT => &widget.on_double_click,
                BLUR_EVENT => &widget.on_blur,
                COMMIT_EVENT => &widget.on_commit,
                ACTIVATE_EVENT => &widget.on_activate,
                FOLD_EVENT => &widget.on_fold,
                OPENED_EVENT => &widget.on_opened,
                CLOSED_EVENT => &widget.on_closed,
                CARET_EVENT => &widget.on_caret,
                SCROLLED_EVENT => &widget.on_scrolled,
                _ => &widget.on_close_request,
            };
            (event, value, handler)
        }),
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

/// What an edit writes onto the properties, and the keys it wrote: where
/// the reader left a drag, a pick or a keystroke, for `get` to report and
/// the widget to follow.
fn written(asked: &mut WidgetProperties, edit: &Edit) -> &'static [&'static str] {
    match edit {
        Edit::Width(width) => {
            asked.width = *width;
            &[k::WIDTH]
        }
        Edit::Height(height) => {
            asked.height = *height;
            &[k::HEIGHT]
        }
        Edit::Moved([dx, dy]) => {
            let (sx, sy) = crate::widget::window::drag_signs(&asked.anchor.0);
            asked.x += dx * sx;
            asked.y += dy * sy;
            &[k::X, k::Y]
        }
        Edit::Text(text) | Edit::Submit(text) => {
            asked.text = text.as_str().into();
            &[k::TEXT]
        }
        // A dropdown shows what was picked; a menu keeps its caption.
        Edit::Choice(choice) if asked.kind.0 != w::MENU => {
            asked.text = choice.as_str().into();
            &[k::TEXT]
        }
        Edit::Picked(row, _) => {
            asked.text = row.as_str().into();
            &[k::TEXT]
        }
        _ => &[],
    }
}

/// What the pointer, focus or a finished gesture did, as the event it is.
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
        Edit::Caret(x, y, index) => (
            CARET_EVENT,
            Value::Map(vec![
                (k::X.into(), Value::Num(f64::from(*x))),
                (k::Y.into(), Value::Num(f64::from(*y))),
                (
                    "index".into(),
                    Value::Int(i64::try_from(*index).unwrap_or(i64::MAX)),
                ),
            ]),
        ),
        Edit::CloseRequested => (CLOSE_REQUEST_EVENT, Value::Nil),
        _ => return None,
    };
    Some(said)
}

/// What a `mark` event carries: the row, and the name of the mark on it.
fn marked(row: &str, mark: &str) -> Value {
    Value::Map(vec![
        (k::ROW.into(), Value::Str(row.to_owned())),
        (k::MARK.into(), Value::Str(mark.to_owned())),
    ])
}

/// What a row pick says: the whole set where the widget holds many, and the
/// row hit where it holds one.
fn picked(multi: bool, row: &str, rows: &[String]) -> Value {
    if multi {
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
) -> Vec<(Entity, String, Value)> {
    let mut signals = Vec::new();
    for (key, edit) in edits {
        let Some(entity) = resolve(eng, key) else {
            continue;
        };
        {
            let world = eng.world();
            let (Ok(mut asked), Ok(mut widget)) = (
                world.get::<&mut WidgetProperties>(entity),
                world.get::<&mut Widget>(entity),
            ) else {
                continue;
            };
            // Written straight onto the component, so the arena's copy of
            // this one is stale until the next pass re-reads it.
            crate::widget::arena::widget_changed(entity);
            settle_one(entity, &mut asked, &mut widget, edit, emitted, &mut signals);
        }
        besides_written(eng, entity, edit);
    }
    signals
}

/// What an edit writes onto a component beside the widget, where the rows
/// it changes are a kind's own: a picked colour, a table's column shares
/// and the column it sorts by. Written down so the scene keeps them.
fn besides_written(eng: &Engine, entity: Entity, edit: &Edit) {
    use crate::widget::beside::{
        self, WidgetColorPicker, WidgetRange, WidgetScroll, WidgetTable, WidgetWindow,
    };
    use crate::widget::beside_box::{WidgetFold, WidgetTabs};
    use crate::widget::beside_controls::WidgetRows;
    match edit {
        Edit::Picked(_, rows) => {
            beside::edit::<WidgetRows>(eng, entity, &[k::SELECTION], |held| {
                held.selection = rows.iter().map(|row| row.as_str().into()).collect();
            });
        }
        Edit::Active(name) => {
            beside::edit::<WidgetTabs>(eng, entity, &[k::CURRENT_PAGE], |held| {
                held.current_page = name.as_str().into();
            });
        }
        // Whichever the kind reads: the other leaves the node as it is.
        Edit::Open(open) => {
            beside::edit::<WidgetFold>(eng, entity, &[k::OPEN], |held| held.open = *open);
            beside::edit::<WidgetWindow>(eng, entity, &[k::OPEN], |held| held.open = *open);
        }
        Edit::Value(value) => {
            beside::edit::<WidgetRange>(eng, entity, &[k::VALUE], |held| held.value = *value);
        }
        // Written down as well as heard, so `scroll_offset` reads back where
        // the reader left it.
        Edit::Scrolled(offset) => {
            beside::edit::<WidgetScroll>(eng, entity, &[k::SCROLL_OFFSET], |held| {
                held.scroll_offset = *offset;
            });
        }
        Edit::Color(rgba) => {
            beside::edit::<WidgetColorPicker>(eng, entity, &[k::PICKED_COLOR], |held| {
                held.picked_color = *rgba;
            });
        }
        // The chrome a reader dragged, reported to nobody, as a seam between
        // two containers is.
        Edit::Widths(shares) => beside::edit::<WidgetTable>(eng, entity, &[k::WIDTHS], |held| {
            held.widths = shares.iter().copied().map(Numeral::of).collect();
        }),
        Edit::Sorted(column, reverse) => {
            beside::edit::<WidgetTable>(eng, entity, &[k::SORT, k::REVERSE], |held| {
                held.sort = column.as_str().into();
                held.reverse = *reverse;
            });
        }
        _ => {}
    }
}

/// Tick what this frame's clicks tick, and collect the handlers.
fn settle_clicks(
    eng: &Engine,
    clicked: &[WidgetKey],
    changes: &mut Vec<(Entity, String, Value)>,
    emitted: &mut Vec<(Entity, &'static str, Value)>,
) -> Vec<(Entity, String)> {
    let hit: Vec<Entity> = clicked.iter().filter_map(|key| resolve(eng, key)).collect();
    let mut signals = Vec::new();
    // The grouped checks this tick ticked, unticked below: the rest of a
    // group are other widgets.
    let mut ticked: Vec<(Entity, smol_str::SmolStr)> = Vec::new();
    for entity in hit {
        let Some(said) = click_of(eng, entity) else {
            continue;
        };
        // A click on a check or toggle is the tick itself, by mouse or by
        // `accept`. One in a group is a radio: it ticks and stays ticked, and
        // the pass below unticks the rest of its group.
        if said.flips {
            let grouped = !said.radio_group.is_empty();
            // Grouped, a click picks: it ticks and a second click leaves it
            // ticked, because something in the group has to be. On its own, a
            // click flips.
            let now = grouped || !said.checked;
            set_checked(eng, entity, now);
            if now != said.checked {
                told_checked(entity, now, &said.on_change, changes, emitted);
            }
            if grouped {
                ticked.push((entity, said.radio_group));
            }
        }
        if !said.on_click.is_empty() {
            signals.push((entity, said.on_click.to_string()));
        }
    }
    for (struck, group) in &ticked {
        let others: Vec<(Entity, smol_str::SmolStr)> = eng
            .world()
            .query::<(Entity, &Widget)>()
            .iter()
            .filter(|(entity, widget)| {
                *entity != *struck
                    && flips(widget)
                    && widget.radio_group == *group
                    && widget.checked
            })
            .map(|(entity, widget)| (entity, widget.on_change.clone()))
            .collect();
        for (entity, on_change) in others {
            set_checked(eng, entity, false);
            told_checked(entity, false, &on_change, changes, emitted);
        }
    }
    signals
}

/// What a click on one widget has to know of it.
struct ClickOf {
    flips: bool,
    checked: bool,
    radio_group: smol_str::SmolStr,
    on_change: smol_str::SmolStr,
    on_click: smol_str::SmolStr,
}

fn click_of(eng: &Engine, entity: Entity) -> Option<ClickOf> {
    let world = eng.world();
    let widget = world.get::<&Widget>(entity).ok()?;
    Some(ClickOf {
        flips: flips(&widget),
        checked: widget.checked,
        radio_group: widget.radio_group.clone(),
        on_change: widget.on_change.clone(),
        on_click: widget.on_click.clone(),
    })
}

fn set_checked(eng: &Engine, entity: Entity, now: bool) {
    use crate::widget::beside::edit;
    use crate::widget::beside_controls::WidgetCheck;
    edit::<WidgetCheck>(eng, entity, &[k::CHECKED], |held| held.checked = now);
}

/// A check's new state said from its node, and to its handler where it names
/// one.
fn told_checked(
    entity: Entity,
    now: bool,
    on_change: &str,
    changes: &mut Vec<(Entity, String, Value)>,
    emitted: &mut Vec<(Entity, &'static str, Value)>,
) {
    emitted.push((entity, CHANGE_EVENT, Value::Bool(now)));
    if !on_change.is_empty() {
        changes.push((entity, on_change.to_owned(), Value::Bool(now)));
    }
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

/// Where a widget's handler `method` goes: the nearest node at or above
/// `entity` whose script has it, or `entity` when none does. So a button deep
/// in a panel names a method on the panel's script, the way a Godot signal is
/// connected to the node that owns the scene. The method is handed the event's
/// value, then the widget's node, as many of the two as it declares.
pub(crate) fn recipient(
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
