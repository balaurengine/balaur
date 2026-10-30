//! The `inspector` kind: one of a node's components as a form, a row a
//! property, read off the registry's schema and the node itself.
//!
//! The rows are the pool's own, filled here rather than by a script: the
//! widget names a node and a component, and the rows are written from what
//! the node holds. Nothing is read back each pass: an inspector fills again
//! when its widget is written, when one of its rows was edited, when its width
//! moved, or when a script says the node changed with `ui.reread`. A script
//! showing a component of twenty properties states one control, not twenty.
//!
//! An edit on a row is heard here and reported from the inspector's node as
//! one `change`: the property, the steps into its value, and the value now
//! there, in the units the component stores. Where the edit lands is the
//! script's to decide, and the next pass reads the result back off the node.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;

use crate::vocabulary::{inspect as i, words as w};
use crate::widget::node::Widget;

mod form;

use form::{Form, Span, channels, empty_table, label_of, texts};

/// The share of its width a label takes where the widget states none.
const LABEL_SHARE: f32 = 0.34;

/// What each inspector was last filled from, and what an edit on each of
/// their controls means.
#[derive(Default)]
pub(crate) struct InspectorState {
    /// Every node a `widget` was written to as an inspector, still one.
    known: Vec<Entity>,
    filled: FxHashMap<u64, Filled>,
    heard: FxHashMap<u64, Heard>,
}

struct Filled {
    asked: Asked,
    /// An edit was heard since: the next pass writes what the node holds,
    /// even where that is what it held before.
    stale: bool,
    /// The rows each property took, for a pass that fills some of them.
    spans: Vec<Span>,
    /// Properties said to have moved since, whose rows alone the next pass
    /// writes again where nothing else moved.
    moved: Vec<String>,
}

/// Everything one inspector's rows are built from.
#[derive(PartialEq)]
struct Asked {
    target: Option<Entity>,
    component: SmolStr,
    options: Vec<SmolStr>,
    label_width: f32,
    stacked: bool,
    row_height: f32,
    relative_to: SmolStr,
    data: Option<toml::Value>,
}

impl Asked {
    /// Everything but the data: what the widget and the layout say.
    fn same_shape(&self, other: &Self) -> bool {
        self.target == other.target
            && self.component == other.component
            && self.options == other.options
            && self.label_width.to_bits() == other.label_width.to_bits()
            && self.stacked == other.stacked
            && self.row_height.to_bits() == other.row_height.to_bits()
            && self.relative_to == other.relative_to
    }
}

/// One step into a property's value: an entry of a list, or a key of a map
/// or a record.
#[derive(Clone, PartialEq, Debug)]
enum Step {
    Index(usize),
    Key(String),
}

impl Step {
    fn value(&self) -> Value {
        match self {
            Self::Index(n) => Value::Int(i64::try_from(*n).unwrap_or(i64::MAX)),
            Self::Key(key) => Value::Str(key.clone()),
        }
    }
}

/// Where a row writes: the property, the steps into it, and the label its
/// row wears.
#[derive(Clone)]
struct Cursor {
    property: String,
    path: Vec<Step>,
    label: String,
}

impl Cursor {
    fn top(property: &str) -> Self {
        Self {
            property: property.to_owned(),
            path: Vec::new(),
            label: label_of(property),
        }
    }

    fn step(&self, step: Step, label: String) -> Self {
        let mut path = self.path.clone();
        path.push(step);
        Self {
            property: self.property.clone(),
            path,
            label,
        }
    }
}

/// What an edit on one control means, and where it lands.
struct Heard {
    host: u64,
    at: Cursor,
    means: Means,
}

enum Means {
    /// A number in the units shown, clamped there and stored converted back.
    Number {
        degrees: bool,
        int: bool,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// One of a vector's numbers; the others are what the node holds now.
    Channel {
        slot: usize,
        count: usize,
        degrees: bool,
    },
    Color,
    Bool,
    Choice,
    /// A field, written as it is typed or, under `submit`, on Enter.
    Text {
        submit: bool,
    },
    /// One of a set's options, turned on or off.
    Flag(String),
    /// A new entry for the list or map at the cursor, under `key` in a map.
    Add {
        fresh: toml::Value,
        key: Option<String>,
    },
    /// One entry taken out of the list or map at the cursor.
    Remove(Step),
    /// An asset embedded or saved, with the reference as the row shows it.
    Verb {
        verb: &'static str,
        reference: String,
        asset: String,
    },
}

/// What an inspector reads off its widget.
struct Asking {
    entity: Entity,
    target: SmolStr,
    component: SmolStr,
    options: Vec<SmolStr>,
    label_width: f32,
    stacked: bool,
    row_height: f32,
    relative_to: SmolStr,
}

impl Asking {
    fn of(entity: Entity, widget: &Widget) -> Self {
        let inspect = &widget.inspect;
        Self {
            entity,
            target: inspect.target.clone(),
            component: inspect.component.clone(),
            options: widget.options.clone(),
            label_width: inspect.label_width,
            stacked: widget.wrap,
            row_height: widget.row_height,
            relative_to: inspect.relative_to.clone(),
        }
    }
}

/// Fill every inspector on screen from the node it shows, where what it is
/// built from moved since its last fill.
pub(crate) fn refresh(eng: &Engine) {
    let Some(state) = eng.try_resource::<InspectorState>() else {
        return;
    };
    // A frame with no inspector in it pays this and no more.
    if state.borrow().known.is_empty() {
        return;
    }
    let known = std::mem::take(&mut state.borrow_mut().known);
    let (asking, turned, kept) = sort_known(eng, known);
    state.borrow_mut().known = kept;
    forget_the_gone(eng, &turned);
    for one in asking {
        if let Err(err) = refresh_one(eng, &one) {
            tracing::error!("an inspector's rows: {err:#}");
        }
    }
}

/// Note an inspector's widget written, so the next pass fills it: one just
/// made, or one whose node, component or options moved.
pub(crate) fn noticed(eng: &Engine, entity: Entity) {
    if let Some(state) = eng.try_resource::<InspectorState>() {
        let mut state = state.borrow_mut();
        if !state.known.contains(&entity) {
            state.known.push(entity);
        }
        if let Some(filled) = state.filled.get_mut(&balaur_core::node_id_of(entity).0) {
            filled.stale = true;
        }
    }
}

/// Fill every inspector at or under `entity` again at the next pass, which
/// they owe: the nodes they show changed in a way no widget says. A panel
/// names itself to have its whole form read again, `component` narrows that
/// to the inspectors showing it, and `properties` to those properties' rows.
/// False where there is none.
pub fn reread(
    eng: &Engine,
    entity: Entity,
    component: Option<&str>,
    properties: Option<&[String]>,
) -> bool {
    let Some(state) = eng.try_resource::<InspectorState>() else {
        return false;
    };
    let inspectors: Vec<Entity> = {
        let world = eng.world();
        balaur_core::scene::collect_subtree(&world, entity)
            .into_iter()
            .filter(|&one| {
                world.get::<&Widget>(one).is_ok_and(|widget| {
                    widget.kind == w::INSPECTOR
                        && component.is_none_or(|name| widget.inspect.component == name)
                })
            })
            .collect()
    };
    let mut state = state.borrow_mut();
    for &one in &inspectors {
        if let Some(filled) = state.filled.get_mut(&balaur_core::node_id_of(one).0) {
            match properties {
                Some(names) if !filled.stale => {
                    for name in names {
                        if !filled.moved.contains(name) {
                            filled.moved.push(name.clone());
                        }
                    }
                }
                _ => filled.stale = true,
            }
        }
        crate::widget::arena::widget_changed(one);
    }
    !inspectors.is_empty()
}

/// The known inspectors split three ways: the ones to fill this pass, the
/// ones that are no inspector any more, and every one still an inspector.
fn sort_known(eng: &Engine, known: Vec<Entity>) -> (Vec<Asking>, Vec<Entity>, Vec<Entity>) {
    let world = eng.world();
    let mut asking = Vec::new();
    let mut turned = Vec::new();
    let mut kept = Vec::with_capacity(known.len());
    for entity in known {
        let Ok(widget) = world.get::<&Widget>(entity) else {
            turned.push(entity);
            continue;
        };
        if widget.kind != w::INSPECTOR {
            turned.push(entity);
            continue;
        }
        kept.push(entity);
        let shown = world
            .get::<&balaur_core::GlobalAppearance>(entity)
            .map_or(widget.visible, |seen| seen.visible);
        if shown {
            asking.push(Asking::of(entity, &widget));
        }
    }
    (asking, turned, kept)
}

/// Forget what the inspector at `node` was filled from, for a slot whose rows
/// were just taken away: the next fill builds them again.
pub(crate) fn forget(eng: &Engine, node: u64) {
    if let Some(state) = eng.try_resource::<InspectorState>() {
        state.borrow_mut().filled.remove(&node);
    }
}

/// Drop what was kept for the inspectors in `turned` and for controls that
/// were freed, and take away the rows of a node that is no inspector any
/// more: under a kind that lays nothing out each would be anchored alone.
fn forget_the_gone(eng: &Engine, turned: &[Entity]) {
    let alive = |id: &u64| {
        balaur_core::entity_of(balaur_script::NodeId(*id))
            .is_ok_and(|entity| eng.world().contains(entity))
    };
    {
        let state = eng.resource::<InspectorState>();
        let mut state = state.borrow_mut();
        for entity in turned {
            state.filled.remove(&balaur_core::node_id_of(*entity).0);
        }
        if !turned.is_empty() {
            state
                .heard
                .retain(|id, heard| alive(id) && alive(&heard.host));
        }
    }
    for &entity in turned {
        let rows = eng
            .world()
            .get::<&balaur_core::scene::Children>(entity)
            .map(|kids| kids.0.clone())
            .unwrap_or_default();
        balaur_core::scene::free_nodes(eng, &rows);
    }
}

fn refresh_one(eng: &Engine, one: &Asking) -> anyhow::Result<()> {
    let target = target_of(eng, &one.target);
    let label_width = if one.label_width > 0.0 {
        one.label_width
    } else {
        crate::widget::arrange::drawn_at(one.entity).map_or(0.0, |r| r.width() * LABEL_SHARE)
    };
    let mut asked = Asked {
        target,
        component: one.component.clone(),
        options: one.options.clone(),
        label_width,
        stacked: one.stacked,
        row_height: one.row_height,
        relative_to: one.relative_to.clone(),
        data: None,
    };
    let id = balaur_core::node_id_of(one.entity).0;
    // The component is read only when something said it may have moved: the
    // read builds the whole table, and doing it every pass was the polling.
    let (same, moved, spans) = {
        let state = eng.resource::<InspectorState>();
        let mut state = state.borrow_mut();
        match state.filled.get_mut(&id) {
            Some(filled) if !filled.stale && filled.asked.same_shape(&asked) => (
                true,
                std::mem::take(&mut filled.moved),
                filled.spans.clone(),
            ),
            _ => (false, Vec::new(), Vec::new()),
        }
    };
    if same {
        // Some properties moved: their rows alone, where they still take
        // the rows they took.
        if moved.is_empty() || refill_moved(eng, one, &asked, id, &moved, &spans)? {
            return Ok(());
        }
    }
    asked.data = target.and_then(|node| balaur_core::components::get(eng, node, &one.component));
    let schema = {
        let registry = eng.resource::<balaur_core::components::ComponentRegistry>();
        let registry = registry.borrow();
        registry
            .def(&one.component)
            .map(|def| std::rc::Rc::clone(&def.schema))
    };
    let mut form = Form::new(eng, &asked, id);
    let spans = match (schema.as_deref(), asked.data.as_ref()) {
        (Some(schema), Some(data)) => form.component(schema, data, &one.options),
        _ => Vec::new(),
    };
    let Form { rows, .. } = form;
    let specs: Vec<Value> = rows.iter().map(|row| row.spec.clone()).collect();
    let nodes = crate::widget::pool::fill_rows_on(
        eng,
        one.entity,
        &specs,
        f64::from(asked.label_width),
        asked.stacked,
    )?;
    let state = eng.resource::<InspectorState>();
    let mut state = state.borrow_mut();
    state.heard.retain(|_, heard| heard.host != id);
    for (row, controls) in rows.into_iter().zip(nodes) {
        for (heard, node) in row.heard.into_iter().zip(controls) {
            if let Some(heard) = heard {
                state.heard.insert(balaur_core::node_id_of(node).0, heard);
            }
        }
    }
    state.filled.insert(
        id,
        Filled {
            asked,
            stale: false,
            spans,
            moved: Vec::new(),
        },
    );
    Ok(())
}

// The rows of the properties in `moved` written again in place, each from
// what the node holds now. False when one now takes another number of rows,
// a list that grew, which a whole fill answers.
fn refill_moved(
    eng: &Engine,
    one: &Asking,
    asked: &Asked,
    id: u64,
    moved: &[String],
    spans: &[Span],
) -> anyhow::Result<bool> {
    let Some(node) = asked.target else {
        return Ok(false);
    };
    let Some(schema) = ({
        let registry = eng.resource::<balaur_core::components::ComponentRegistry>();
        let registry = registry.borrow();
        registry
            .def(&one.component)
            .map(|def| std::rc::Rc::clone(&def.schema))
    }) else {
        return Ok(false);
    };
    let mut written = Vec::new();
    let mut heard = Vec::new();
    for name in moved {
        // A property this inspector does not show has no rows to write.
        let Some(span) = spans.iter().find(|span| span.property == *name) else {
            continue;
        };
        let Some(spec) = schema.get(name) else {
            return Ok(false);
        };
        let value = balaur_core::components::property(eng, node, &one.component, name)
            .or_else(|| spec.get(i::DEFAULT).cloned())
            .unwrap_or_else(|| toml::Value::String(String::new()));
        let mut form = Form::new(eng, asked, id);
        form.property(name, spec, &value);
        if form.rows.len() != span.count {
            return Ok(false);
        }
        for (offset, row) in form.rows.into_iter().enumerate() {
            written.push((span.first + offset, row.spec));
            heard.push(row.heard);
        }
    }
    let nodes = crate::widget::pool::fill_rows_at(
        eng,
        one.entity,
        &written,
        f64::from(asked.label_width),
        asked.stacked,
    )?;
    let state = eng.resource::<InspectorState>();
    let mut state = state.borrow_mut();
    for (row, controls) in heard.into_iter().zip(nodes) {
        for (heard, node) in row.into_iter().zip(controls) {
            if let Some(heard) = heard {
                state.heard.insert(balaur_core::node_id_of(node).0, heard);
            }
        }
    }
    Ok(true)
}

/// The node a `target` names: a path as `scene.get_node` takes it, or as
/// `node.path()` spells it, which starts with the root's own name.
fn target_of(eng: &Engine, path: &str) -> Option<Entity> {
    let world = eng.world();
    let root = eng.root();
    balaur_core::scene::find_node(&world, root, path).or_else(|| {
        let top = world.get::<&balaur_core::scene::Name>(root).ok()?.0.clone();
        let rest = path.strip_prefix(top.as_str())?.strip_prefix('/')?;
        balaur_core::scene::find_node(&world, root, rest)
    })
}
/// The inspectors' controls a tick heard, each as one `change` from the
/// inspector that drew it: the property, the steps into it, and the value
/// now there or the verb asked for.
pub(crate) fn heard(
    eng: &Engine,
    emitted: &[(Entity, &'static str, Value)],
    clicked: &[Entity],
) -> Vec<(Entity, Value)> {
    let Some(state) = eng.try_resource::<InspectorState>() else {
        return Vec::new();
    };
    let events = emitted
        .iter()
        .map(|(entity, event, value)| (*entity, *event, Some(value)))
        .chain(clicked.iter().map(|entity| (*entity, CLICK, None)));
    let mut out = Vec::new();
    for (entity, event, value) in events {
        let id = balaur_core::node_id_of(entity).0;
        let said = {
            let state = state.borrow();
            let Some(heard) = state.heard.get(&id) else {
                continue;
            };
            let Some(said) = meant(eng, &state, heard, event, value) else {
                continue;
            };
            (heard.host, said, keeps_typing(heard, event))
        };
        let (host, said, typing) = said;
        if !typing && let Some(filled) = state.borrow_mut().filled.get_mut(&host) {
            filled.stale = true;
        }
        if let Ok(inspector) = balaur_core::entity_of(balaur_script::NodeId(host)) {
            out.push((inspector, said));
        }
    }
    out
}

/// A click, which the pass reports beside the edits rather than as one.
const CLICK: &str = "";

/// Whether this event leaves a field mid-edit: one that writes on Enter
/// hears every key, and a fill under it would put back what it held.
fn keeps_typing(heard: &Heard, event: &str) -> bool {
    matches!(heard.means, Means::Text { submit: true })
        && event == crate::widget::input::CHANGE_EVENT
}

/// What one control's event means at its cursor, or `None` where it means
/// nothing there.
fn meant(
    eng: &Engine,
    state: &InspectorState,
    heard: &Heard,
    event: &str,
    value: Option<&Value>,
) -> Option<Value> {
    use crate::widget::input::{CHANGE_EVENT, SUBMIT_EVENT};
    let filled = state.filled.get(&heard.host)?;
    let held = || held_at(eng, &filled.asked, &heard.at);
    let written = match (&heard.means, event, value) {
        (
            Means::Number {
                degrees,
                int,
                min,
                max,
            },
            CHANGE_EVENT,
            Some(v),
        ) => {
            let mut n = number(v)?;
            if let Some(min) = min {
                n = n.max(*min);
            }
            if let Some(max) = max {
                n = n.min(*max);
            }
            if *int {
                Value::Int(n.round() as i64)
            } else if *degrees {
                Value::Num(n.to_radians())
            } else {
                Value::Num(n)
            }
        }
        (
            Means::Channel {
                slot,
                count,
                degrees,
            },
            CHANGE_EVENT,
            Some(v),
        ) => {
            let n = number(v)?;
            let mut out = channels(&held().unwrap_or_else(empty_table), *count, 0.0);
            out[*slot] = if *degrees { n.to_radians() } else { n };
            Value::List(out.into_iter().map(Value::Num).collect())
        }
        (Means::Color, CHANGE_EVENT, Some(Value::Color(rgba))) => Value::List(
            rgba.iter()
                .map(|c| Value::Num(f64::from(c.clamp(0.0, 1.0))))
                .collect(),
        ),
        (Means::Bool, CHANGE_EVENT, Some(Value::Bool(on))) => Value::Bool(*on),
        (Means::Choice | Means::Text { submit: false }, CHANGE_EVENT, Some(Value::Str(text)))
        | (Means::Text { submit: true }, SUBMIT_EVENT, Some(Value::Str(text))) => {
            Value::Str(text.clone())
        }
        (Means::Flag(_) | Means::Add { .. } | Means::Remove(_), CLICK, None) => {
            reshaped(&heard.means, held())?
        }
        (
            Means::Verb {
                verb,
                reference,
                asset,
            },
            CLICK,
            None,
        ) => {
            return Some(edit_of(
                &heard.at,
                &[
                    (i::VERB, Value::text(*verb)),
                    (i::VALUE, Value::text(reference)),
                    (i::ASSET, Value::text(asset)),
                ],
            ));
        }
        _ => return None,
    };
    Some(edit_of(&heard.at, &[(i::VALUE, written)]))
}

/// A set, a list or a map with one click's change made to it: an option
/// turned round, an entry added, or one taken out.
fn reshaped(means: &Means, held: Option<toml::Value>) -> Option<Value> {
    if let Means::Flag(name) = means {
        let mut set = texts(&held.unwrap_or_else(|| toml::Value::Array(Vec::new())));
        let was = set.contains(name);
        set.retain(|other| other != name);
        if !was {
            set.push(name.clone());
        }
        return Some(Value::List(set.into_iter().map(Value::Str).collect()));
    }
    let mut holder = held?;
    match (means, &mut holder) {
        (Means::Add { fresh, .. }, toml::Value::Array(items)) => items.push(fresh.clone()),
        (
            Means::Add {
                fresh,
                key: Some(key),
            },
            toml::Value::Table(table),
        ) => {
            table.insert(key.clone(), fresh.clone());
        }
        (Means::Remove(Step::Index(n)), toml::Value::Array(items)) if *n < items.len() => {
            items.remove(*n);
        }
        (Means::Remove(Step::Key(key)), toml::Value::Table(table)) => {
            table.remove(key);
        }
        _ => return None,
    }
    balaur_core::node_api::from_toml(&holder).ok()
}

/// A `change` event's table: where it lands, then what it carries.
fn edit_of(at: &Cursor, carried: &[(&str, Value)]) -> Value {
    let mut entries = vec![
        (i::PROPERTY.to_owned(), Value::text(&at.property)),
        (
            i::PATH.to_owned(),
            Value::List(at.path.iter().map(Step::value).collect()),
        ),
    ];
    entries.extend(
        carried
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone())),
    );
    Value::Map(entries)
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Num(n) => Some(*n),
        Value::Int(n) => Some(*n as f64),
        _ => None,
    }
}

/// What the node holds at a cursor now, read again rather than kept from
/// the fill: a row is built before the edit that reads it.
fn held_at(eng: &Engine, asked: &Asked, at: &Cursor) -> Option<toml::Value> {
    let data = balaur_core::components::get(eng, asked.target?, &asked.component)?;
    let mut held = data.get(&at.property)?.clone();
    for step in &at.path {
        held = match (held, step) {
            (toml::Value::Array(items), Step::Index(n)) => items.into_iter().nth(*n)?,
            (toml::Value::Table(mut table), Step::Key(key)) => table.remove(key)?,
            _ => return None,
        };
    }
    Some(held)
}
