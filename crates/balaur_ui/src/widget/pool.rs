//! A pool of widget nodes under one host, filled from a list of specs every
//! pass. A form whose fields change with the selection cannot be authored,
//! so a script says what it wants and the pool makes, reuses and hides nodes.
//!
//! A spec is a `widget` property table plus `on_click`, `on_change` and
//! `on_submit`, each the name of a method that hears the control's
//! `pointer_click`, `value_changed` or `submitted`: called on the nearest node above
//! the control whose script has it, with the spec's `data`, then the edit,
//! then the control's node. The pool keeps the names; no widget holds one.
//! A spec carrying `controls` is a group: one node holding its controls. A
//! node is written only when what is asked of it changed, and spare nodes
//! are hidden rather than freed.
//!
//! Nothing is polled: the widget input system tells the pool what each
//! control said, so a strip whose spec did not change need not be filled again.
//!
//! In Rust because the shell asks for a hundred controls a pass, and merging
//! and comparing their tables in a script was two fifths of the editor's work.
//! Every write goes through the node operations a script calls, so what a
//! node ends up holding is what a script's own writes would have left on it.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::engine_api::KERNEL_OPS;
use balaur_core::node_api::NODE_OPS;
use balaur_script::{Module, Value};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::vocabulary::{keys as k, pool as p, words as w};

const ALL_SIDES: &str = "all_sides";
const FILL_SLOT: &str = "fill_slot";
const FILL_STRIP: &str = "fill_strip";
const PATCH_CONTROL: &str = "patch_control";
const SIDES_WORD: &str = "sides";

type Spec = BTreeMap<String, Value>;
type Op = fn(&Engine, &[Value]) -> Result<Value>;

/// What each host was last asked for, by host node, and who hears each
/// control, by control node.
///
/// `written` is the table last written to each control, so an unchanged pass
/// writes nothing.
#[derive(Default)]
pub(crate) struct PoolState {
    hosts: FxHashMap<u64, HostMemo>,
    pooled: FxHashMap<u64, Pooled>,
}

#[derive(Default)]
struct HostMemo {
    written: FxHashMap<String, Value>,
    /// The keys each slot was written with since it was last set whole: what
    /// a patch would leave on it.
    worn: FxHashMap<String, FxHashSet<String>>,
}

/// A control the pool filled: where its last write is remembered, the
/// `data` its handlers are handed first, and the method each event calls.
struct Pooled {
    host: u64,
    key: String,
    data: Value,
    /// The method each of [`HANDLERS`] names, in its order; empty for none.
    calls: [String; HANDLERS.len()],
}

/// The spec keys that name a handler, and the event each hears.
const HANDLERS: [(&str, &str); 3] = [
    (p::ON_CLICK, balaur_core::hooks::POINTER_CLICK),
    (p::ON_CHANGE, crate::widget::input::VALUE_CHANGED_EVENT),
    (p::ON_SUBMIT, crate::widget::input::SUBMITTED_EVENT),
];

/// One handler a pooled control's event calls, found before any runs.
pub(crate) struct PoolCall {
    on: balaur_core::hecs::Entity,
    method: String,
    args: Vec<Value>,
}

impl PoolCall {
    pub(crate) fn run(self, eng: &Engine) {
        if let Some(host) = eng.script_host() {
            host.call_on(balaur_core::node_id_of(self.on), &self.method, &self.args);
        }
    }
}

/// The node operations the pool writes through, resolved once.
#[derive(Clone, Copy)]
struct Ops {
    scene_node: Op,
    node: Op,
    children: Op,
    add_child: Op,
    get: Op,
    set: Op,
    patch: Op,
    has: Op,
    remove: Op,
    visible: Op,
    set_visible: Op,
}

impl Ops {
    fn resolve() -> Option<Self> {
        let node_op = |name: &str| NODE_OPS.iter().find(|o| o.name == name).map(|o| o.call);
        Some(Self {
            scene_node: KERNEL_OPS
                .iter()
                .flat_map(|ops| ops.iter())
                .find(|o| o.module == w::SCENE && o.name == w::GET_NODE)?
                .call,
            node: node_op(w::GET_NODE)?,
            children: node_op("children")?,
            add_child: node_op("add_child")?,
            get: node_op("component_property")?,
            set: node_op("set_component")?,
            patch: node_op("patch_component")?,
            has: node_op("has_component")?,
            remove: node_op("remove_component")?,
            visible: node_op(w::overflow::VISIBLE)?,
            set_visible: node_op("set_visible")?,
        })
    }
}

/// What a control that does not name its own shape gets. A pooled node is
/// patched rather than set, so a key this pass leaves out would keep the last
/// one's: a spacer that becomes a label would draw six pixels wide. Only rows
/// every kind reads: one beside the widget would give each control its
/// component, and a slot that wore one is set whole when a control drops it.
fn shapeless() -> Spec {
    Spec::from([
        (k::WIDTH.into(), Value::Int(0)),
        (k::HEIGHT.into(), Value::Int(0)),
        (k::GROW.into(), Value::Int(0)),
        (k::CORNER_RADIUS.into(), Value::Int(-1)),
        (k::FILL.into(), Value::text("")),
        (k::TOOLTIP.into(), Value::text("")),
        (k::TEXT.into(), Value::text("")),
    ])
}

/// A `gap` the same across and down.
fn both(px: i64) -> Value {
    Value::List(vec![Value::Int(px), Value::Int(px)])
}

fn table(entries: &[(&str, Value)]) -> Spec {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

/// `base` with every entry of `over` written on top.
fn merge(mut base: Spec, over: &Value) -> Result<Spec> {
    let Value::Map(entries) = over else {
        return Err(anyhow!("a pooled control is a table, got {over:?}"));
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
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn list(value: Option<&Value>) -> &[Value] {
    match value {
        Some(Value::List(items)) => items,
        _ => &[],
    }
}

/// One host's pass: the engine, the operations and whose memo it keeps.
struct Pool<'a> {
    eng: &'a Engine,
    ops: Ops,
    host: u64,
    /// Whether a control's spec differed from the one last written to it,
    /// which is what the fill answers.
    changed: std::cell::Cell<bool>,
}

impl Pool<'_> {
    fn widget(&self, node: &Value, key: &str) -> Value {
        (self.ops.get)(
            self.eng,
            &[node.clone(), Value::text(p::WIDGET), Value::text(key)],
        )
        .unwrap_or(Value::Nil)
    }

    /// Write a control's spec: the widget's rows to `widget`, and the rows
    /// its kind keeps beside it (`widget_table`, `widget_menu` and the rest)
    /// to that component. A whole write takes off a beside component the
    /// spec no longer names, as it drops the widget's own rows.
    fn write(&self, op: Op, node: &Value, spec: Value) -> Result<()> {
        let Value::Map(entries) = spec else {
            return op(self.eng, &[node.clone(), Value::text(p::WIDGET), spec]).map(|_| ());
        };
        let named = entries
            .iter()
            .find(|(key, _)| key == k::KIND)
            .map(|(_, kind)| kind.clone());
        // Asked for once, by a row two components hold for kinds apart.
        let kind = std::cell::OnceCell::new();
        let kind = || {
            kind.get_or_init(
                || match named.clone().unwrap_or_else(|| self.widget(node, k::KIND)) {
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
            // A spec's `visible` is the node's, not the widget's.
            if key == p::VISIBLE {
                (self.ops.set_visible)(self.eng, &[node.clone(), value])?;
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
                &[node.clone(), Value::text(p::WIDGET), Value::Map(own)],
            )?;
        }
        for name in crate::widget::beside::names() {
            match beside.iter().position(|(held, _)| held == name) {
                Some(at) => {
                    let rows = beside.swap_remove(at).1;
                    op(
                        self.eng,
                        &[node.clone(), Value::text(*name), Value::Map(rows)],
                    )?;
                }
                None if whole => {
                    let named = [node.clone(), Value::text(*name)];
                    if matches!((self.ops.has)(self.eng, &named), Ok(Value::Bool(true))) {
                        (self.ops.remove)(self.eng, &named)?;
                    }
                }
                None => {}
            }
        }
        Ok(())
    }

    fn children(&self, node: &Value) -> Vec<Value> {
        match (self.ops.children)(self.eng, std::slice::from_ref(node)) {
            Ok(Value::List(kids)) => kids,
            _ => Vec::new(),
        }
    }

    fn child(&self, node: &Value, name: &str) -> Value {
        (self.ops.node)(self.eng, &[node.clone(), Value::text(name)]).unwrap_or(Value::Nil)
    }

    /// A child named `name` under `node`, made with `spec` as its widget.
    fn make(&self, node: &Value, name: &str, spec: &[(&str, Value)]) -> Result<Value> {
        let made = (self.ops.add_child)(self.eng, &[node.clone(), Value::text(name)])?;
        self.write(self.ops.set, &made, as_value(&table(spec)))?;
        Ok(made)
    }

    fn with_memo<T>(&self, f: impl FnOnce(&mut HostMemo) -> T) -> T {
        let state = self.eng.resource::<PoolState>();
        let mut state = state.borrow_mut();
        f(state.hosts.entry(self.host).or_default())
    }

    /// Forget what this host was asked for: a node just made holds none of it.
    fn forget(&self) {
        self.with_memo(|memo| memo.written.clear());
    }

    /// Keep what `node`'s handlers are handed first and the method each of
    /// its events calls, taken off `want`: neither is the widget's to hold.
    fn listen(&self, key: &str, node: &Value, want: &mut Spec) {
        let data = want.remove(p::DATA).unwrap_or(Value::Nil);
        let calls = HANDLERS.map(|(named, _)| match want.remove(named) {
            Some(Value::Str(method)) => method,
            _ => String::new(),
        });
        let Value::Node(id) = node else {
            return;
        };
        let fresh = Pooled {
            host: self.host,
            key: key.to_string(),
            data,
            calls,
        };
        let clicks = fresh.hears(balaur_core::hooks::POINTER_CLICK);
        let state = self.eng.resource::<PoolState>();
        let was = state.borrow_mut().pooled.insert(*id, fresh);
        if was.is_some_and(|was| was.hears(balaur_core::hooks::POINTER_CLICK)) != clicks {
            self.click_hearer_changed(*id);
        }
    }

    /// A spare control hears nothing.
    fn deaf(&self, node: &Value) {
        let Value::Node(id) = node else {
            return;
        };
        let state = self.eng.resource::<PoolState>();
        let was = state.borrow_mut().pooled.remove(id);
        if was.is_some_and(|was| was.hears(balaur_core::hooks::POINTER_CLICK)) {
            self.click_hearer_changed(*id);
        }
    }

    /// A label or a panel senses a click only where one is heard.
    fn click_hearer_changed(&self, id: u64) {
        if let Ok(entity) = balaur_core::entity_of(balaur_script::NodeId(id)) {
            balaur_core::events::hearers_may_have_changed(self.eng, entity);
        }
    }

    /// Write `spec` to `node` unless it is what was written last pass; a
    /// patch carries only what differs from that, so a control whose one
    /// property moved takes the one property's write.
    fn patched(&self, key: &str, node: &Value, spec: &Spec) -> Result<()> {
        let mark = format!("{key}#");
        let value = as_value(spec);
        let before = self.with_memo(|memo| memo.written.get(&mark).cloned());
        if !self.noted(&mark, &value) {
            return Ok(());
        }
        // A patch keeps every key the new table leaves out, so a slot would
        // wear whatever its last control stated: a kind, a role, a range that
        // clamps the next number. Setting drops them.
        let swapped = match spec.get(k::KIND) {
            Some(kind) => {
                *kind != self.widget(node, k::KIND)
                    || spec
                        .get(k::ROLE)
                        .cloned()
                        .unwrap_or_else(|| Value::text(""))
                        != self.widget(node, k::ROLE)
            }
            None => false,
        };
        let whole = self.with_memo(|memo| {
            let worn = memo.worn.entry(mark).or_default();
            let whole = swapped || worn.iter().any(|key| !spec.contains_key(key));
            if whole {
                worn.clear();
            }
            worn.extend(spec.keys().cloned());
            whole
        });
        if whole {
            return self.write(self.ops.set, node, value);
        }
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
        self.write(self.ops.patch, node, moved)
    }

    /// Write what `patch` names on a control and note it, so the next fill
    /// compares against what the control holds. What it holds already is not
    /// written; true when something was.
    fn patch_held(&self, key: &str, node: &Value, patch: &Value) -> Result<bool> {
        let Value::Map(entries) = patch else {
            return Err(anyhow!("a control's patch is a table, got {patch:?}"));
        };
        let mark = format!("{key}#");
        let moved = self.with_memo(|memo| {
            // An edit forgot it: the next fill writes it whole.
            let Some(Value::Map(held)) = memo.written.get_mut(&mark) else {
                return entries
                    .iter()
                    .filter(|(_, value)| *value != Value::Nil)
                    .cloned()
                    .collect();
            };
            let mut moved = Vec::with_capacity(entries.len());
            for (name, value) in entries {
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
            return Ok(false);
        }
        self.write(self.ops.patch, node, Value::Map(moved))?;
        Ok(true)
    }

    /// Whether `value` differs from what `mark` was last written with, noted
    /// as written when it does.
    fn noted(&self, mark: &str, value: &Value) -> bool {
        let before = self.with_memo(|memo| memo.written.get(mark).map(|held| held == value));
        match before {
            Some(true) => return false,
            // An edit's forgetting is not a new spec: only a write over one.
            Some(false) => self.changed.set(true),
            None => (),
        }
        self.with_memo(|memo| memo.written.insert(mark.to_string(), value.clone()));
        true
    }

    /// A spare slot hears nothing and is hidden. It keeps what it is: the
    /// fill that shows it again decides what its new control drops.
    fn hidden(&self, key: &str, node: &Value) -> Result<()> {
        self.deaf(node);
        let value = as_value(&table(&[(p::VISIBLE, Value::Bool(false))]));
        if !self.noted(&format!("{key}#"), &value) {
            return Ok(());
        }
        self.write(self.ops.patch, node, value)
    }

    /// One control: who hears it, then what the spec wants it to hold.
    fn control(&self, key: &str, node: &Value, want: Spec) -> Result<()> {
        let mut set = want;
        self.listen(key, node, &mut set);
        set.entry(p::VISIBLE.into()).or_insert(Value::Bool(true));
        self.patched(key, node, &set)
    }

    /// A control, or a group when its spec carries `controls`.
    fn one(&self, key: &str, node: &Value, spec: &Value) -> Result<()> {
        let want = merge(shapeless(), spec)?;
        match want.get(p::CONTROLS).cloned() {
            Some(inner) => self.group(key, node, want, &inner),
            None => self.control(key, node, want),
        }
    }

    /// A box with no air in it holding its own controls: how a tab and the
    /// mark that closes it read as one tile.
    fn group(&self, key: &str, node: &Value, want: Spec, inner: &Value) -> Result<()> {
        let mut set = table(&[(k::KIND, Value::text(w::ROW)), (k::GAP, both(0))]);
        set.extend(want);
        self.listen(key, node, &mut set);
        set.entry(p::VISIBLE.into()).or_insert(Value::Bool(true));
        set.remove(p::CONTROLS);
        self.patched(key, node, &set)?;
        let inner = list(Some(inner));
        self.fill(key, node, inner)
    }

    /// Put `controls` on `node`'s children, making what is missing and
    /// hiding what is spare. `prefix` keys them apart from another group's.
    fn fill(&self, prefix: &str, node: &Value, controls: &[Value]) -> Result<()> {
        let mut made = self.children(node).len();
        if made < controls.len() {
            self.forget();
        }
        while made < controls.len() {
            self.make(
                node,
                &format!("C{made}"),
                &[(k::KIND, Value::text(w::LABEL))],
            )?;
            made += 1;
        }
        for (i, kid) in self.children(node).iter().enumerate() {
            let key = if prefix.is_empty() {
                format!("{i}")
            } else {
                format!("{prefix}:{i}")
            };
            match controls.get(i) {
                Some(spec) => self.one(&key, kid, spec)?,
                None => self.hidden(&key, kid)?,
            }
        }
        Ok(())
    }

    /// The child at `slot`, made with any before it that are missing.
    fn slot_at(&self, host: &Value, slot: usize) -> Result<Value> {
        let name = format!("C{slot}");
        let held = self.child(host, &name);
        if held != Value::Nil {
            return Ok(held);
        }
        self.forget();
        for index in self.children(host).len()..=slot {
            self.make(
                host,
                &format!("C{index}"),
                &[(k::KIND, Value::text(w::LABEL))],
            )?;
        }
        Ok(self.child(host, &name))
    }
}

/// The host at `path`, shown, and the pass that fills it; nothing when no
/// node is there.
fn open<'a>(eng: &'a Engine, ops: Ops, path: &str) -> Result<Option<(Pool<'a>, Value)>> {
    let host = (ops.scene_node)(eng, &[Value::text(path)])?;
    open_node(eng, ops, host)
}

/// `open` for a host already in hand.
fn open_node(eng: &Engine, ops: Ops, host: Value) -> Result<Option<(Pool<'_>, Value)>> {
    let Value::Node(id) = host else {
        return Ok(None);
    };
    let fresh = !eng.resource::<PoolState>().borrow().hosts.contains_key(&id);
    if fresh {
        sweep(eng);
    }
    let pool = Pool {
        eng,
        ops,
        host: id,
        changed: std::cell::Cell::new(false),
    };
    // Read off the host rather than the memo: a script hides a whole pool
    // with a plain write, which the memo never hears.
    if (ops.visible)(eng, std::slice::from_ref(&host))? == Value::Bool(false) {
        (ops.set_visible)(eng, &[host.clone(), Value::Bool(true)])?;
    }
    Ok(Some((pool, host)))
}

/// Drop the hosts and controls that were freed. Run when a
/// host is first filled, which is when one made for a while may have gone.
fn sweep(eng: &Engine) {
    let alive = |id: &u64| {
        balaur_core::entity_of(balaur_script::NodeId(*id))
            .is_ok_and(|entity| eng.world().contains(entity))
    };
    let state = eng.resource::<PoolState>();
    let mut state = state.borrow_mut();
    state.hosts.retain(|id, _| alive(id));
    state.pooled.retain(|id, _| alive(id));
}

impl Pooled {
    /// The method this control's spec names for `event`.
    fn method(&self, event: &str) -> Option<&String> {
        let at = HANDLERS.iter().position(|(_, heard)| *heard == event)?;
        Some(&self.calls[at]).filter(|method| !method.is_empty())
    }

    fn hears(&self, event: &str) -> bool {
        self.method(event).is_some()
    }
}

/// Whether a pooled control's spec names a handler for `event`.
pub(crate) fn hears(eng: &Engine, entity: balaur_core::hecs::Entity, event: &str) -> bool {
    let id = balaur_core::node_id_of(entity).0;
    eng.try_resource::<PoolState>().is_some_and(|state| {
        state
            .borrow()
            .pooled
            .get(&id)
            .is_some_and(|one| one.hears(event))
    })
}

/// What the pool's controls said this tick: mark what an edit changed behind
/// the memo's back, so the next fill writes it again, and answer, for each
/// event in turn, the handler its control's spec names for it.
pub(crate) fn heard(
    eng: &Engine,
    said: &[(balaur_core::hecs::Entity, &'static str, Value)],
) -> Vec<Option<PoolCall>> {
    let Some(state) = eng.try_resource::<PoolState>() else {
        return Vec::new();
    };
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let mut calls = Vec::with_capacity(said.len());
    for (entity, event, value) in said {
        let id = balaur_core::node_id_of(*entity).0;
        let Some(pooled) = state.pooled.get(&id) else {
            calls.push(None);
            continue;
        };
        // A click alone writes nothing onto the control.
        if *event != balaur_core::hooks::POINTER_CLICK
            && let Some(memo) = state.hosts.get_mut(&pooled.host)
        {
            memo.written.remove(&format!("{}#", pooled.key));
        }
        calls.push(pooled.method(event).and_then(|method| {
            // A pooled button's click is a `true`.
            let edit = if *event == balaur_core::hooks::POINTER_CLICK {
                Value::Bool(true)
            } else {
                as_reported(value)
            };
            Some(PoolCall {
                on: balaur_core::bindings::nearest_with_method(eng, *entity, method)?,
                method: method.clone(),
                args: vec![pooled.data.clone(), edit, Value::Node(id)],
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

fn fill_strip(eng: &Engine, ops: Ops, path: &str, controls: &Value) -> Result<bool> {
    let Some((pool, host)) = open(eng, ops, path)? else {
        return Ok(false);
    };
    pool.fill("", &host, list(Some(controls)))?;
    Ok(pool.changed.get())
}

/// One child of `host`, the one at `slot`, filled as `fill_strip` fills each,
/// made with any before it that are missing, or hidden for no spec. A host of
/// hundreds of children writes the few on screen.
fn fill_slot(
    eng: &Engine,
    ops: Ops,
    host: Value,
    slot: usize,
    spec: Option<&Value>,
) -> Result<bool> {
    let Some((pool, host)) = open_node(eng, ops, host)? else {
        return Ok(false);
    };
    let kid = pool.slot_at(&host, slot)?;
    let key = format!("{slot}");
    match spec {
        Some(spec) => pool.one(&key, &kid, spec)?,
        None => pool.hidden(&key, &kid)?,
    }
    Ok(pool.changed.get())
}

/// What `patch` names written to the control at `at`, child indices down from
/// `host`, and noted, so the next fill compares against it. False where no
/// control is there or nothing moved.
fn patch_control(eng: &Engine, ops: Ops, host: Value, at: &[usize], patch: &Value) -> Result<bool> {
    let Some((pool, mut node)) = open_node(eng, ops, host)? else {
        return Ok(false);
    };
    for index in at {
        node = pool.child(&node, &format!("C{index}"));
        if node == Value::Nil {
            return Ok(false);
        }
    }
    let key = at
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(":");
    pool.patch_held(&key, &node, patch)
}

/// A pooled control's fields: the pool's own, then the widget rows a script
/// sets on one, typed as the `widget` schema and those beside it read them.
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

fn sides_of(sizes: [f64; 4]) -> Value {
    let row = |(name, size): (&str, f64)| (name.to_string(), Value::Num(size));
    Value::Map(SIDE_NAMES.into_iter().zip(sizes).map(row).collect())
}

const CONTROL: &str = "{ kind: string?, controls: [Control]?, on_click: string?, on_change: string?, on_submit: string?, data: any, \
    role: string?, text: string?, tooltip: string?, icon: string?, icon_color: string?, image: string?, \
    font: string?, prefix: string?, trailing: string?, placeholder: string?, fill: string?, stroke: string?, \
    align_items: string?, justify: string?, text_align: string?, anchor: string?, fit: string?, \
    grow: float?, width: float?, height: float?, min_width: float?, x: float?, y: float?, \
    font_size: float?, icon_size: float?, corner_radius: float?, row_height: float?, \
    min: float?, max: float?, step: float?, drag_speed: float?, font_weight: int?, decimals: int?, \
    visible: bool?, enabled: bool?, interactive: bool?, focusable: bool?, checked: bool?, wrap: bool?, truncate: bool?, \
    absolute: bool?, keep_open: bool?, secret: bool?, arrows: bool?, \
    options: [string]?, items: [Item]?, value: any, text_color: string?, picked_color: Color?, padding: Sides?, inset: Sides?, margin: Sides?, gap: Vec2? }";

/// `ui.fill_strip` and `ui.fill_slot`.
pub(crate) fn install_pool(m: &mut Module<Engine>) {
    m.describe(&[
        (
            SIDES_WORD,
            &[],
            "(left: float, top: float, right: float, bottom: float) -> Sides",
            "A `Sides` for a control's `padding`, `margin` or `inset`.",
        ),
        (
            ALL_SIDES,
            &[],
            "(size: float) -> Sides",
            "A `Sides` the same on every side.",
        ),
        (
            FILL_STRIP,
            &[],
            "(host: string, controls: [Control]) -> bool",
            "Put one widget node per control table under the node at `host`, made, reused and hidden as the list changes; a node is written only when its table changed. A control's `on_click` or `on_change` hears the reader's edit and `on_submit` hears Enter as they happen: the name of a method, called on the nearest node above the control whose script has it with the control's `data`, then the edit, then the control's node. So a strip that did not change need not be filled again, and a control carrying `controls` is a row holding them. True when a control's table differed from the last one written to it.",
        ),
        (
            FILL_SLOT,
            &[],
            "(host: node, slot: int, control: Control?) -> bool",
            "One child of `host`, the one at `slot`, filled as `fill_strip` fills each, made with any before it that are missing, or hidden for nil. The others are left as they are, so a list of hundreds of rows writes the few on screen and, as they scroll, the ones entering. True when the control's table differed from the last one written to it.",
        ),
        (
            PATCH_CONTROL,
            &[],
            "(host: node, at: [int], patch: Control) -> bool",
            "What `patch` names written to the pooled control at `at`, child indices down from `host`, and remembered, so the next fill compares against it. For a value that follows the node: what the patch leaves out is not asked about, and a handler's name is a fill's to give. False where no control is there or nothing moved.",
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
            "One pooled control: its `kind`, the `widget` rows it sets and those of the component beside it that holds them, what hears its edit (`on_click`, `on_change`, `on_submit`, a method on the nearest node above it) and the `data` that handler is called with; a control with `controls` is a row holding them.",
        ),
        (
            "Item",
            "{ label: string?, icon: string?, note: string?, tint: string?, key: string?, depth: int?, marks: [Mark]?, cells: [string]? }",
            "One row of a `list`, `tree` or `table`: what it says and shows, its `key`, which `text`, `selection` and every row event know it by and which is its label or first cell where it names none, how deep in a tree it sits, its marks, and a table's `cells`.",
        ),
        (
            "Mark",
            "{ name: string, glyph: string }",
            "A mark at the right of a row: the glyph it draws from the icon face, and the name its `marked` event reports.",
        ),
    ]);
    let Some(ops) = Ops::resolve() else {
        tracing::error!("the widget pool cannot find the node operations it writes through");
        return;
    };
    m.function(
        SIDES_WORD,
        |_: &Engine, (left, top, right, bottom): (f64, f64, f64, f64)| {
            Ok(sides_of([left, top, right, bottom]))
        },
    );
    m.function(ALL_SIDES, |_: &Engine, size: f64| Ok(sides_of([size; 4])));
    m.function(
        FILL_STRIP,
        move |eng: &Engine, (host, controls): (String, Value)| {
            fill_strip(eng, ops, &host, &controls)
        },
    );
    install_one_row(m, ops);
}

/// `ui.fill_slot` and `ui.patch_control`: a long list that keeps what is on screen.
fn install_one_row(m: &mut Module<Engine>, ops: Ops) {
    m.function(
        FILL_SLOT,
        move |eng: &Engine, (host, slot, control): (balaur_script::NodeId, i64, Option<Value>)| {
            fill_slot(
                eng,
                ops,
                Value::Node(host.0),
                usize::try_from(slot)?,
                control.as_ref(),
            )
        },
    );
    m.function(
        PATCH_CONTROL,
        move |eng: &Engine, (host, at, patch): (balaur_script::NodeId, Value, Value)| {
            let at = list(Some(&at))
                .iter()
                .map(|index| match index {
                    Value::Int(n) => usize::try_from(*n).map_err(anyhow::Error::from),
                    other => Err(anyhow!(
                        "a control's place is a list of ints, got {other:?}"
                    )),
                })
                .collect::<Result<Vec<_>>>()?;
            patch_control(eng, ops, Value::Node(host.0), &at, &patch)
        },
    );
}
