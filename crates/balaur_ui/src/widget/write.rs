//! `ui.write` and `ui.patch`: a control table written onto one widget node,
//! each key to the component that holds it, and only what moved since the
//! last write. A form builds its rows from item scenes and writes each
//! control through here, so a row whose one value moved takes one write.
//!
//! A control's `on_click`, `on_change` or `on_submit` names a method its
//! `target` node hears the edit with: kept here, never on the widget, and
//! called from the frame's input with the control's `data`, the edit and the
//! control's node.
//!
//! In Rust because a form writes a hundred controls a pass, and merging and
//! comparing their tables in a script was two fifths of the editor's work.
//! Every write goes through the node operations a script calls, so what a
//! node ends up holding is what a script's own writes would have left on it.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_core::node_api::NODE_OPS;
use balaur_script::{Module, NodeId, Value};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::vocabulary::{control as c, keys as k, words as w};

const WRITE: &str = "write";
const PATCH: &str = "patch";

type Spec = BTreeMap<String, Value>;
type Op = fn(&Engine, &[Value]) -> Result<Value>;

/// What was last written to each node, and who hears each.
#[derive(Default)]
pub(crate) struct Written {
    memos: FxHashMap<Entity, Memo>,
    heard: FxHashMap<Entity, Hearer>,
}

/// The method each of a control's events calls, on which node, and what it
/// is handed first.
struct Hearer {
    data: Value,
    target: u64,
    /// The method each of [`HANDLERS`] names, in its order; empty for none.
    calls: [String; HANDLERS.len()],
}

/// The control keys that name a handler, and the event each hears.
const HANDLERS: [(&str, &str); 3] = [
    (c::ON_CLICK, balaur_core::hooks::POINTER_CLICK),
    (c::ON_CHANGE, crate::widget::input::VALUE_CHANGED_EVENT),
    (c::ON_SUBMIT, crate::widget::input::SUBMITTED_EVENT),
];

impl Hearer {
    fn method(&self, event: &str) -> Option<&String> {
        let at = HANDLERS.iter().position(|(_, heard)| *heard == event)?;
        Some(&self.calls[at]).filter(|method| !method.is_empty())
    }
}

/// One handler a control's event calls, found before any runs.
pub(crate) struct Call {
    on: Entity,
    method: String,
    args: Vec<Value>,
}

impl Call {
    pub(crate) fn run(self, eng: &Engine) {
        if let Some(host) = eng.script_host() {
            host.call_on(balaur_core::node_id_of(self.on), &self.method, &self.args);
        }
    }
}

#[derive(Default)]
struct Memo {
    /// The table last written, as it was asked for.
    asked: Option<Value>,
    /// The keys written since the node was last set whole: what a patch
    /// would leave on it.
    worn: FxHashSet<String>,
}

/// The keys a control carries for whoever hears it, and its children: none
/// is the widget's to hold.
const NOT_WRITTEN: [&str; 6] = [
    c::ON_CLICK,
    c::ON_CHANGE,
    c::ON_SUBMIT,
    c::TARGET,
    c::DATA,
    c::CONTROLS,
];

/// The node operations a write goes through, resolved once.
#[derive(Clone, Copy)]
struct Ops {
    get: Op,
    set: Op,
    patch: Op,
    has: Op,
    remove: Op,
    set_visible: Op,
}

impl Ops {
    fn resolve() -> Option<Self> {
        let node_op = |name: &str| NODE_OPS.iter().find(|o| o.name == name).map(|o| o.call);
        Some(Self {
            get: node_op("component_property")?,
            set: node_op("set_component")?,
            patch: node_op("patch_component")?,
            has: node_op("has_component")?,
            remove: node_op("remove_component")?,
            set_visible: node_op("set_visible")?,
        })
    }
}

/// What a control that does not name its own shape gets. A node is patched
/// rather than set, so a key this write leaves out would keep the last
/// one's: a spacer that becomes a label would draw six pixels wide. Only rows
/// every kind reads: one beside the widget would give each control its
/// component, and a node that wore one is set whole when a control drops it.
fn shapeless() -> Spec {
    Spec::from([
        (k::WIDTH.into(), Value::Int(0)),
        (k::HEIGHT.into(), Value::Int(0)),
        (k::GROW.into(), Value::Int(0)),
        (k::CORNER_RADIUS.into(), Value::Int(-1)),
        (k::FILL.into(), Value::text("")),
        (k::TOOLTIP.into(), Value::text("")),
        (k::TOOLTIP_KEY.into(), Value::text("")),
        (k::TEXT.into(), Value::text("")),
        (k::OFFSET.into(), both(0)),
        (k::SCALE.into(), Value::Int(1)),
    ])
}

/// A `gap` the same across and down.
fn both(px: i64) -> Value {
    Value::List(vec![Value::Int(px), Value::Int(px)])
}

/// `base` with every entry of `over` written on top.
fn merge(mut base: Spec, over: &Value) -> Result<Spec> {
    let Value::Map(entries) = over else {
        return Err(anyhow!("a control is a table, got {over:?}"));
    };
    for (key, value) in entries {
        base.insert(key.clone(), value.clone());
    }
    Ok(base)
}

/// A spec as a node operation takes it: keys in order, as a script's table
/// arrives.
fn as_value(spec: &Spec) -> Value {
    Value::Map(
        spec.iter()
            // copy: the spec stays the memo's to compare against
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

// The rows a control gives as a `Sides`, and the widget holds as a `vec4`.
const SIDE_NAMES: [&str; 4] = ["left", "top", "right", "bottom"];
const SIDES: [&str; 3] = [k::PADDING, k::MARGIN, k::INSET];

fn four_sides(value: Value) -> Value {
    let Value::Map(sides) = &value else {
        return value;
    };
    let side = |name: &str| {
        let found = sides.iter().find(|(key, _)| key == name);
        found.map_or(Value::Num(0.0), |(_, side)| side.clone())
    };
    Value::List(SIDE_NAMES.map(side).to_vec())
}

/// One node's write: the engine, the operations, and whose memo it keeps.
struct Writer<'a> {
    eng: &'a Engine,
    ops: Ops,
    entity: Entity,
    node: Value,
}

impl Writer<'_> {
    fn widget(&self, key: &str) -> Value {
        (self.ops.get)(
            self.eng,
            &[self.node.clone(), Value::text(c::WIDGET), Value::text(key)],
        )
        .unwrap_or(Value::Nil)
    }

    fn with_memo<T>(&self, f: impl FnOnce(&mut Memo) -> T) -> T {
        let written = self.eng.resource::<Written>();
        let mut written = written.borrow_mut();
        f(written.memos.entry(self.entity).or_default())
    }

    /// Write `spec`: the widget's rows to `widget`, and the rows its kind
    /// keeps beside it (`widget_table`, `widget_menu` and the rest) to that
    /// component. A whole write takes off a beside component the spec no
    /// longer names, as it drops the widget's own rows.
    fn write(&self, op: Op, spec: Value) -> Result<()> {
        let Value::Map(entries) = spec else {
            return op(self.eng, &[self.node.clone(), Value::text(c::WIDGET), spec]).map(|_| ());
        };
        let named = entries
            .iter()
            .find(|(key, _)| key == k::KIND)
            // copy: the entries move into their components below
            .map(|(_, kind)| kind.clone());
        // Asked for once, by a row two components hold for kinds apart.
        let kind = std::cell::OnceCell::new();
        let kind = || {
            kind.get_or_init(
                || match named.clone().unwrap_or_else(|| self.widget(k::KIND)) {
                    Value::Str(word) => word,
                    _ => String::new(),
                },
            )
            .clone()
        };
        let mut own = Vec::new();
        let mut beside: Vec<(&'static str, Vec<(String, Value)>)> = Vec::new();
        for (key, value) in entries {
            let value = if SIDES.contains(&key.as_str()) {
                four_sides(value)
            } else {
                value
            };
            // A control's `visible` is the node's, not the widget's.
            if key == c::VISIBLE {
                (self.ops.set_visible)(self.eng, &[self.node.clone(), value])?;
                continue;
            }
            let owner = match crate::widget::beside::owners_of(&key) {
                [] => None,
                [one] => Some(one.name),
                several => crate::widget::beside::owner_for(several, &kind()).map(|one| one.name),
            };
            match owner {
                Some(name) => match beside.iter_mut().find(|(held, _)| *held == name) {
                    Some((_, rows)) => rows.push((key, value)),
                    None => beside.push((name, vec![(key, value)])),
                },
                None => own.push((key, value)),
            }
        }
        let whole = std::ptr::fn_addr_eq(op, self.ops.set);
        if whole || !own.is_empty() {
            op(
                self.eng,
                &[self.node.clone(), Value::text(c::WIDGET), Value::Map(own)],
            )?;
        }
        for name in crate::widget::beside::names() {
            match beside.iter().position(|(held, _)| held == name) {
                Some(at) => {
                    let rows = beside.swap_remove(at).1;
                    op(
                        self.eng,
                        &[self.node.clone(), Value::text(*name), Value::Map(rows)],
                    )?;
                }
                None if whole => {
                    let named = [self.node.clone(), Value::text(*name)];
                    if matches!((self.ops.has)(self.eng, &named), Ok(Value::Bool(true))) {
                        (self.ops.remove)(self.eng, &named)?;
                    }
                }
                None => {}
            }
        }
        Ok(())
    }

    /// Write `spec` unless it is what was written last; a patch carries only
    /// what differs from that, so a control whose one property moved takes
    /// the one property's write. True when an earlier write differed.
    fn written(&self, spec: &Spec) -> Result<bool> {
        let value = as_value(spec);
        let before = self.with_memo(|memo| memo.asked.clone());
        if before.as_ref() == Some(&value) {
            return Ok(false);
        }
        // A patch keeps every key the new table leaves out, so a node would
        // wear whatever its last control stated: a kind, a role, a range that
        // clamps the next number. Setting drops them.
        let swapped = match spec.get(k::KIND) {
            Some(kind) => {
                *kind != self.widget(k::KIND)
                    || spec
                        .get(k::ROLE)
                        .cloned()
                        .unwrap_or_else(|| Value::text(""))
                        != self.widget(k::ROLE)
            }
            None => false,
        };
        let whole = self.with_memo(|memo| {
            let whole = swapped || memo.worn.iter().any(|key| !spec.contains_key(key));
            if whole {
                memo.worn.clear();
            }
            memo.worn.extend(spec.keys().cloned());
            memo.asked = Some(value.clone());
            whole
        });
        if whole {
            self.write(self.ops.set, value)?;
        } else {
            let moved = match (&before, &value) {
                (Some(Value::Map(before)), Value::Map(now)) => Value::Map(
                    now.iter()
                        .filter(|(name, held)| {
                            !before
                                .iter()
                                .any(|(was, written)| was == name && written == held)
                        })
                        .cloned()
                        .collect(),
                ),
                _ => value,
            };
            self.write(self.ops.patch, moved)?;
        }
        Ok(before.is_some())
    }

    /// Show or hide the node, whatever the memo says: something else may have
    /// shown it, as `scene.repeat` shows a node it reuses. A node hidden hears
    /// nothing.
    fn shown(&self, visible: Value) -> Result<()> {
        if visible == Value::Bool(false) {
            let written = self.eng.resource::<Written>();
            let gone = written.borrow_mut().heard.remove(&self.entity);
            if gone.is_some() {
                balaur_core::events::hearers_may_have_changed(self.eng, self.entity);
            }
        }
        (self.ops.set_visible)(self.eng, &[self.node.clone(), visible]).map(|_| ())
    }

    /// Write what `patch` names and note it, so the next write compares
    /// against what the node holds. What it holds already is not written;
    /// true when something was. Its `visible` is written whatever the memo says.
    fn patched(&self, patch: &Value) -> Result<bool> {
        let Value::Map(entries) = patch else {
            return Err(anyhow!("a control's patch is a table, got {patch:?}"));
        };
        let mut shown = false;
        if let Some((_, visible)) = entries.iter().find(|(key, _)| key == c::VISIBLE) {
            self.shown(visible.clone())?;
            shown = true;
        }
        let entries: Vec<&(String, Value)> = entries
            .iter()
            .filter(|(key, _)| key != c::VISIBLE)
            .collect();
        let moved = self.with_memo(|memo| {
            // An edit forgot it: the next write sets it whole.
            let Some(Value::Map(held)) = memo.asked.as_mut() else {
                return entries
                    .iter()
                    .filter(|(_, value)| *value != Value::Nil)
                    // copy: what is written is the node operation's to own
                    .map(|&entry| entry.clone())
                    .collect();
            };
            let mut moved = Vec::with_capacity(entries.len());
            for (name, value) in entries.iter().copied() {
                if *value == Value::Nil {
                    continue;
                }
                match held.iter_mut().find(|(was, _)| was == name) {
                    Some((_, written)) if written == value => {}
                    Some((_, written)) => {
                        written.clone_from(value);
                        moved.push((name.clone(), value.clone()));
                    }
                    None => moved.push((name.clone(), value.clone())),
                }
            }
            moved
        });
        if moved.is_empty() {
            return Ok(shown);
        }
        self.write(self.ops.patch, Value::Map(moved))?;
        Ok(true)
    }
}

fn writer(eng: &Engine, ops: Ops, node: NodeId) -> Result<Writer<'_>> {
    let entity = balaur_core::entity_of(node)?;
    if eng.try_resource::<Written>().is_none() {
        eng.insert_resource(Written::default());
        balaur_core::scene::on_freed(eng, forget_freed);
    }
    Ok(Writer {
        eng,
        ops,
        entity,
        node: Value::Node(node.0),
    })
}

/// `control` written onto `node`: shown, its shape where it names none, and
/// a row of `gap` air where it carries `controls`, whose nodes are the
/// caller's to write.
fn write(eng: &Engine, ops: Ops, node: NodeId, control: &Value) -> Result<bool> {
    let want = merge(shapeless(), control)?;
    let mut set = if want.contains_key(c::CONTROLS) {
        // A box holding its own controls: the air its role states, and none
        // where it has no role, which is how a tab and its close mark read
        // as one tile.
        let air = if want.contains_key(k::ROLE) { -1 } else { 0 };
        let mut group = Spec::from([
            (k::KIND.into(), Value::text(w::ROW)),
            (k::GAP.into(), both(air)),
        ]);
        group.extend(want);
        group
    } else {
        want
    };
    let writer = writer(eng, ops, node)?;
    listen(eng, writer.entity, &set)?;
    for key in NOT_WRITTEN {
        set.remove(key);
    }
    let visible = set.remove(c::VISIBLE).unwrap_or(Value::Bool(true));
    let changed = writer.written(&set)?;
    writer.shown(visible)?;
    Ok(changed)
}

/// Keep who hears `entity`'s events, from its control's `on_*`, `target` and
/// `data`: none where it names no handler.
fn listen(eng: &Engine, entity: Entity, control: &Spec) -> Result<()> {
    let calls = HANDLERS.map(|(named, _)| match control.get(named) {
        // copy: the table stays the writer's to compare
        Some(Value::Str(method)) => method.clone(),
        _ => String::new(),
    });
    let named = calls.iter().find(|method| !method.is_empty());
    let target = match control.get(c::TARGET) {
        Some(Value::Node(id)) => Some(*id),
        _ => None,
    };
    let fresh = match (named, target) {
        (None, _) => None,
        (Some(method), None) => {
            let at = balaur_core::scene::node_path(&eng.world(), entity);
            return Err(anyhow!(
                "the control at `{at}` calls `{method}` but names no `target` node to call it on"
            ));
        }
        (Some(_), Some(target)) => Some(Hearer {
            data: control.get(c::DATA).cloned().unwrap_or(Value::Nil),
            target,
            calls,
        }),
    };
    let clicks = |hearer: Option<&Hearer>| {
        hearer.is_some_and(|h| h.method(balaur_core::hooks::POINTER_CLICK).is_some())
    };
    let now = clicks(fresh.as_ref());
    let was = {
        let written = eng.resource::<Written>();
        let mut written = written.borrow_mut();
        let was = match fresh {
            Some(hearer) => written.heard.insert(entity, hearer),
            None => written.heard.remove(&entity),
        };
        clicks(was.as_ref())
    };
    // A label or a panel senses a click only where one is heard.
    if was != now {
        balaur_core::events::hearers_may_have_changed(eng, entity);
    }
    Ok(())
}

/// Whether a control's table names a handler for `event`.
pub(crate) fn hears(eng: &Engine, entity: Entity, event: &str) -> bool {
    eng.try_resource::<Written>().is_some_and(|written| {
        written
            .borrow()
            .heard
            .get(&entity)
            .is_some_and(|hearer| hearer.method(event).is_some())
    })
}

/// What the controls said this tick: what an edit wrote behind the last
/// write's back is forgotten, so the next write sets it whole, and for each
/// event in turn, the handler its control names.
pub(crate) fn heard(eng: &Engine, said: &[(Entity, &'static str, Value)]) -> Vec<Option<Call>> {
    let Some(written) = eng.try_resource::<Written>() else {
        return Vec::new();
    };
    let mut written = written.borrow_mut();
    let mut calls = Vec::with_capacity(said.len());
    for (entity, event, value) in said {
        // A click alone writes nothing onto the control. What it wore stays
        // known, so the next write still drops what it no longer names.
        if *event != balaur_core::hooks::POINTER_CLICK
            && let Some(memo) = written.memos.get_mut(entity)
        {
            memo.asked = None;
        }
        let Some(hearer) = written.heard.get(entity) else {
            calls.push(None);
            continue;
        };
        calls.push(hearer.method(event).and_then(|method| {
            // A button's click is a `true`.
            let edit = if *event == balaur_core::hooks::POINTER_CLICK {
                Value::Bool(true)
            } else {
                as_reported(value)
            };
            let Some(on) = balaur_core::entity_of(NodeId(hearer.target))
                .ok()
                .filter(|on| eng.world().contains(*on))
            else {
                tracing::warn!("a control calls `{method}` on a node that is gone");
                return None;
            };
            Some(Call {
                on,
                method: method.clone(),
                args: vec![
                    hearer.data.clone(),
                    edit,
                    Value::Node(balaur_core::node_id_of(*entity).0),
                ],
            })
        }));
    }
    calls
}

/// A value as the widget's own reader reports it, which is what a handler
/// was always handed: a colour as its four channels.
fn as_reported(value: &Value) -> Value {
    match value {
        Value::Color(rgba) => Value::List(rgba.iter().map(|c| Value::Num(f64::from(*c))).collect()),
        other => other.clone(),
    }
}

fn forget_freed(eng: &Engine, freed: &[Entity]) {
    if let Some(written) = eng.try_resource::<Written>() {
        let mut written = written.borrow_mut();
        for entity in freed {
            written.memos.remove(entity);
            written.heard.remove(entity);
        }
    }
}

const CONTROL: &str = "{ kind: string?, controls: [Control]?, on_click: string?, on_change: string?, on_submit: string?, target: node?, data: any, \
    role: string?, text: string?, tooltip: string?, tooltip_key: string?, icon: string?, icon_color: string?, \
    trailing_icon: string?, trailing_role: string?, image: string?, placeholder_key: string?, \
    font: string?, prefix: string?, trailing: string?, placeholder: string?, fill: string?, stroke: string?, \
    align_items: string?, justify: string?, text_align: string?, anchor: string?, fit: string?, state: string?, \
    grow: float?, width: float?, height: float?, min_width: float?, x: float?, y: float?, \
    font_size: float?, icon_size: float?, corner_radius: float?, row_height: float?, \
    min: float?, max: float?, step: float?, drag_speed: float?, font_weight: int?, decimals: int?, \
    visible: bool?, enabled: bool?, interactive: bool?, focusable: bool?, checked: bool?, wrap: bool?, truncate: bool?, \
    absolute: bool?, keep_open: bool?, secret: bool?, arrows: bool?, loading: bool?, \
    options: [string]?, items: [Item]?, value: any, text_color: string?, picked_color: Color?, padding: Sides?, inset: Sides?, margin: Sides?, gap: Vec2?, \
    offset: Vec2?, scale: float? }";

/// `ui.write` and `ui.patch`, and the records a control table is.
pub(crate) fn install(m: &mut Module<Engine>) {
    m.describe(&[
        (
            WRITE,
            &[],
            "(nodes: [node], controls: [Control]) -> bool",
            "Write each control table onto the widget node in its place, the lists side by side and in one call however long: each key to the component that holds it, `visible` to the node, and only what moved since the last write, a key an earlier write named and this one leaves out put back to its shape's default. A control carrying `controls` is a row; its children are the caller's to write. What names who hears it, `on_click`, `on_change`, `on_submit`, `target` and `data`, is never written. True when an earlier write differed.",
        ),
        (
            PATCH,
            &[],
            "(node: node, patch: Control) -> bool",
            "What `patch` names written onto a widget node and remembered, so the next `write` compares against it: for a value that follows the node, what it leaves out is not asked about. False where nothing moved.",
        ),
    ]);
    m.records(&[
        (
            "Sides",
            "{ left: float, top: float, right: float, bottom: float }",
            "A control's `padding`, `margin` or `inset`, side by side; -1 is what the theme gives.",
        ),
        (
            "Control",
            CONTROL,
            "One control: its `kind`, the `widget` rows it sets and those of the component beside it that holds them, what hears its edit (`on_click`, `on_change`, `on_submit`, a method on its `target` node) and the `data` that handler is called with; a control with `controls` is a row holding them.",
        ),
        (
            "Item",
            "{ label: string?, icon: string?, region: [float]?, note: string?, tint: string?, key: string?, depth: int?, marks: [Mark]?, cells: [string]? }",
            "One row of a `list`, `tree` or `table`: what it says, the name of its icon in the theme's icon font, on a card the `region` of the list's sheet it shows instead, its `key`, which `text`, `selection` and every row event know it by and which is its label or first cell where it names none, how deep in a tree it sits, its marks, and a table's `cells`.",
        ),
        (
            "Mark",
            "{ name: string, icon: string }",
            "A mark at the right of a row: the name of the glyph it draws in the theme's icon font, and the name its `marked` event reports.",
        ),
    ]);
    let Some(ops) = Ops::resolve() else {
        tracing::error!("ui.write cannot find the node operations it writes through");
        return;
    };
    m.function(
        WRITE,
        move |eng: &Engine, (nodes, controls): (Value, Value)| {
            let (Value::List(nodes), Value::List(controls)) = (nodes, controls) else {
                return Err(anyhow!(
                    "write takes a list of nodes and a list of controls"
                ));
            };
            if nodes.len() != controls.len() {
                return Err(anyhow!(
                    "write takes a control a node, got {} nodes and {} controls",
                    nodes.len(),
                    controls.len()
                ));
            }
            let mut changed = false;
            for (node, control) in nodes.iter().zip(&controls) {
                let Value::Node(id) = node else {
                    return Err(anyhow!("write takes nodes, got {node:?}"));
                };
                changed |= write(eng, ops, NodeId(*id), control)?;
            }
            Ok(changed)
        },
    );
    m.function(
        PATCH,
        move |eng: &Engine, (node, patch): (NodeId, Value)| writer(eng, ops, node)?.patched(&patch),
    );
}
