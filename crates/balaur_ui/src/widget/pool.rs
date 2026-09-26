//! A pool of widget nodes under one host, filled from a list of specs every
//! pass. A form whose fields change with the selection cannot be authored,
//! so a script says what it wants and the pool makes, reuses and hides nodes.
//!
//! A spec is a `widget` property table plus `on` and `on_submit`, the
//! callbacks that hear the reader's edit. A spec carrying `controls` is a
//! group: one node holding its controls. A node is written only when what is
//! asked of it changed, and spare nodes are hidden rather than freed.
//!
//! In Rust because the shell asks for a hundred controls a pass, and merging
//! and comparing their tables in Rune was two fifths of the editor's script.
//! Every write goes through the node operations a script calls, so what a
//! node ends up holding is what the Rune pool left on it.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::engine_api::ENGINE_OPS;
use balaur_core::node_api::NODE_OPS;
use balaur_script::{Bindings, BindingsExt, CallbackHost as _, Value};
use rustc_hash::FxHashMap;

use crate::vocabulary::{keys as k, pool as p, words as w};

type Spec = BTreeMap<String, Value>;
type Op = fn(&Engine, &[Value]) -> Result<Value>;

/// A label's line where a row is stacked: the label role's text and its air.
const LABEL_H: f64 = 16.0;

/// What each host was last asked for, by host node.
///
/// `written` is the table last written to each control, so an unchanged pass
/// writes nothing. `held` is the value last seen on a control that carries
/// one: a report that differs from it is the reader's edit.
#[derive(Default)]
pub(crate) struct PoolState {
    hosts: FxHashMap<u64, HostMemo>,
}

#[derive(Default)]
struct HostMemo {
    written: FxHashMap<String, Value>,
    held: FxHashMap<String, Value>,
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
}

impl Ops {
    fn resolve() -> Option<Self> {
        let node_op = |name: &str| NODE_OPS.iter().find(|o| o.name == name).map(|o| o.call);
        Some(Self {
            scene_node: ENGINE_OPS
                .iter()
                .find(|o| o.module == "scene" && o.name == "get_node")?
                .call,
            node: node_op("get_node")?,
            children: node_op("children")?,
            add_child: node_op("add_child")?,
            get: node_op("get_component")?,
            set: node_op("set_component")?,
            patch: node_op("patch_component")?,
        })
    }
}

/// What carries a control's value, per kind. A kind this does not name
/// reports nothing, which is what a label and a separator do.
fn carrier(kind: &str) -> &'static str {
    match kind {
        w::NUMBER_FIELD | w::SLIDER | w::PROGRESS_BAR => k::VALUE,
        w::CHECKBOX | w::SWITCH => k::CHECKED,
        w::TEXT_FIELD | w::DROPDOWN | w::TEXT_AREA | w::LIST => k::TEXT,
        w::COLOR_PICKER => k::PICKED_COLOR,
        w::BUTTON => k::CLICKED,
        _ => "",
    }
}

/// What a control that does not name its own shape gets. A pooled node is
/// patched rather than set, so a key this pass leaves out would keep the last
/// one's: a spacer that becomes a label would draw six pixels wide.
fn shapeless() -> Spec {
    Spec::from([
        (k::WIDTH.into(), Value::Int(0)),
        (k::HEIGHT.into(), Value::Int(0)),
        (k::GROW.into(), Value::Int(0)),
        (k::CORNER_RADIUS.into(), Value::Int(-1)),
        (k::FILL.into(), Value::text("")),
        (k::TOOLTIP.into(), Value::text("")),
        (k::CHECKED.into(), Value::Bool(false)),
        // A pooled node is made a label, whose text defaults to "label".
        (k::TEXT.into(), Value::text("")),
    ])
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

/// A spec as a node operation takes it: callbacks dropped, since a component
/// cannot hold one, and keys in order, as a script's table arrives.
fn as_value(spec: &Spec) -> Value {
    Value::Map(
        spec.iter()
            .filter(|(_, value)| !matches!(value, Value::Callback(_)))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn number(value: Option<&Value>, fallback: f64) -> f64 {
    match value {
        Some(Value::Num(n)) => *n,
        Some(Value::Int(i)) => *i as f64,
        _ => fallback,
    }
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
}

impl Pool<'_> {
    fn widget(&self, node: &Value, key: &str) -> Value {
        (self.ops.get)(
            self.eng,
            &[node.clone(), Value::text(p::WIDGET), Value::text(key)],
        )
        .unwrap_or(Value::Nil)
    }

    fn write(&self, op: Op, node: &Value, spec: Value) -> Result<()> {
        op(self.eng, &[node.clone(), Value::text(p::WIDGET), spec]).map(|_| ())
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

    /// Forget what this host was asked for: a node just made holds none of
    /// it, and a reader's edit is a value the next pass has to write over.
    fn forget(&self) {
        self.eng
            .resource::<PoolState>()
            .borrow_mut()
            .hosts
            .remove(&self.host);
    }

    /// Write `spec` to `node` unless it is what was written last pass.
    fn patched(&self, key: &str, node: &Value, spec: &Spec) -> Result<()> {
        let mark = format!("{key}#");
        let value = as_value(spec);
        let same = self.with_memo(|memo| memo.written.get(&mark) == Some(&value));
        if same {
            return Ok(());
        }
        self.with_memo(|memo| memo.written.insert(mark, value.clone()));
        // A patch keeps every key the new table leaves out, so a slot that
        // changes kind or role would wear the last one's. Setting drops them.
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
        let op = if swapped {
            self.ops.set
        } else {
            self.ops.patch
        };
        self.write(op, node, value)
    }

    fn hidden(&self, key: &str, node: &Value) -> Result<()> {
        self.patched(key, node, &table(&[(k::VISIBLE, Value::Bool(false))]))
    }

    /// One control: what it reports, then what the spec wants it to hold.
    ///
    /// Read before write, because a node reports the pass before: the value
    /// on it now is either what the pool wrote or the reader's edit over it.
    fn control(&self, key: &str, node: &Value, want: Spec) -> Result<()> {
        let kind = match want.get(k::KIND) {
            Some(Value::Str(kind)) => kind.clone(),
            _ => w::LABEL.to_string(),
        };
        let carries = carrier(&kind);
        let held = format!("{key}:{carries}");
        let callback = |name: &str| match want.get(name) {
            Some(Value::Callback(id)) => Some(*id),
            _ => None,
        };
        let mut fired = false;
        // A node that was another kind last pass holds that control's value.
        let same = self.widget(node, k::KIND) == Value::text(kind.as_str());
        if same
            && self.widget(node, k::SUBMITTED) == Value::Bool(true)
            && let Some(act) = callback(k::ON_SUBMIT)
        {
            self.eng.invoke(act, &[self.widget(node, k::TEXT)])?;
        }
        if same && carries == k::CLICKED {
            if self.widget(node, k::CLICKED) == Value::Bool(true)
                && let Some(act) = callback(p::ON)
            {
                self.eng.invoke(act, &[Value::Bool(true)])?;
            }
        } else if same && !carries.is_empty() {
            let seen = self.widget(node, carries);
            let known = self
                .with_memo(|memo| memo.held.get(&held).cloned())
                .unwrap_or_else(|| seen.clone());
            if seen != known {
                if let Some(act) = callback(p::ON) {
                    self.eng.invoke(act, std::slice::from_ref(&seen))?;
                }
                fired = true;
                // Held at what the reader left for one pass: the model may
                // clamp, and snapping back before it answers is a jump.
                self.with_memo(|memo| memo.held.insert(held.clone(), seen));
            }
        }
        let mut set = want;
        set.insert(k::VISIBLE.into(), Value::Bool(true));
        for gone in [p::ON, k::ON_SUBMIT, k::CLICKED, k::SUBMITTED] {
            set.remove(gone);
        }
        if fired {
            set.remove(carries);
            self.forget();
        }
        self.patched(key, node, &set)?;
        // Remembered as the node holds it: 0.2 goes through an f32.
        if !carries.is_empty() && carries != k::CLICKED && !fired {
            let now = self.widget(node, carries);
            self.with_memo(|memo| memo.held.insert(held, now));
        }
        Ok(())
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
        let mut set = table(&[(k::KIND, Value::text(w::ROW)), (k::GAP, Value::Int(0))]);
        set.extend(want);
        set.insert(k::VISIBLE.into(), Value::Bool(true));
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

    /// A row: a label of a fixed width, then the control column taking what
    /// is left, with a `draw` hatch beside the controls for a bespoke row.
    fn make_row(&self, host: &Value, index: usize) -> Result<()> {
        let row = self.make(
            host,
            &format!("R{index}"),
            &[
                (k::KIND, Value::text(w::ROW)),
                (k::GAP, Value::Int(6)),
                (k::HEIGHT, Value::Int(24)),
            ],
        )?;
        self.make(
            &row,
            "L",
            &[
                (k::KIND, Value::text(w::LABEL)),
                (k::ROLE, Value::text(p::TEXT_LABEL)),
            ],
        )?;
        let column = self.make(
            &row,
            "H",
            &[
                (k::KIND, Value::text(w::ROW)),
                (k::GAP, Value::Int(0)),
                (k::GROW, Value::Int(1)),
            ],
        )?;
        // No `gap`: the row's `slot_role` carries it, and a number here
        // would win over the theme.
        self.make(
            &column,
            "C",
            &[(k::KIND, Value::text(w::ROW)), (k::GROW, Value::Int(1))],
        )?;
        self.make(
            &column,
            "D",
            &[
                (k::KIND, Value::text(w::DRAW)),
                (k::VISIBLE, Value::Bool(false)),
                (k::GROW, Value::Int(1)),
            ],
        )?;
        Ok(())
    }

    /// Controls are made as a row needs them: a vec3 takes three, and a row
    /// that never held a vector never grows the other two.
    fn control_at(&self, slot: &Value, index: usize) -> Result<Value> {
        let name = format!("C{index}");
        let held = self.child(slot, &name);
        if held != Value::Nil {
            return Ok(held);
        }
        self.make(slot, &name, &[(k::KIND, Value::text(w::LABEL))])
    }

    /// One row of a form: its box, its label, its hatch and its controls.
    fn row(
        &self,
        i: usize,
        row: &Value,
        spec: &Value,
        label_w: &Value,
        stacked: bool,
    ) -> Result<()> {
        let Value::Map(entries) = spec else {
            return Err(anyhow!("a pooled row is a table, got {spec:?}"));
        };
        let spec: Spec = entries.iter().cloned().collect();
        let full = spec.get(p::FULL) == Some(&Value::Bool(true));
        let text = spec
            .get(p::LABEL)
            .cloned()
            .unwrap_or_else(|| Value::text(""));
        let labelled = text != Value::text("");
        let stack = stacked && !full && labelled;
        // A spec naming no height hugs what is in it, which is what a line of
        // help needs: how tall it is follows from the width it got.
        let h = number(spec.get(k::HEIGHT), 24.0);
        let tall = if h <= 0.0 {
            0.0
        } else if stack {
            h + LABEL_H + 2.0
        } else {
            h
        };
        let prefix = format!("r{i}");
        self.patched(
            &prefix,
            row,
            &table(&[
                (k::VISIBLE, Value::Bool(true)),
                (k::KIND, Value::text(if stack { w::COLUMN } else { w::ROW })),
                (k::GAP, Value::Int(if stack { 2 } else { 6 })),
                (k::HEIGHT, Value::Num(tall)),
            ]),
        )?;
        let label = self.child(row, "L");
        if label != Value::Nil {
            let width = if stack {
                Value::Num(0.0)
            } else {
                label_w.clone()
            };
            let or = |key: &str, fallback: &str| {
                spec.get(key)
                    .cloned()
                    .unwrap_or_else(|| Value::text(fallback))
            };
            self.patched(
                &format!("{prefix}:L"),
                &label,
                &table(&[
                    (k::VISIBLE, Value::Bool(!full && (labelled || !stacked))),
                    (k::TEXT, text.clone()),
                    (k::WIDTH, width),
                    (k::HEIGHT, Value::Num(if stack { LABEL_H } else { 0.0 })),
                    (k::ROLE, or(p::LABEL_ROLE, p::TEXT_LABEL)),
                    (k::TOOLTIP, or(k::TOOLTIP, "")),
                ]),
            )?;
        }
        let column = self.child(row, "H");
        if column == Value::Nil {
            return Ok(());
        }
        // A row the caller draws itself: a node still, in the column.
        let drawn = spec.get(k::DRAW).cloned();
        let hatch = self.child(&column, "D");
        if hatch != Value::Nil {
            let want = match &drawn {
                Some(name) => table(&[(k::VISIBLE, Value::Bool(true)), (k::DRAW, name.clone())]),
                None => table(&[(k::VISIBLE, Value::Bool(false))]),
            };
            self.patched(&format!("{prefix}:D"), &hatch, &want)?;
        }
        let slot = self.child(&column, "C");
        if slot == Value::Nil {
            return Ok(());
        }
        let role = spec
            .get(p::SLOT_ROLE)
            .cloned()
            .unwrap_or_else(|| Value::text(p::LAYOUT_CELLS));
        self.patched(
            &format!("{prefix}:C"),
            &slot,
            &table(&[(k::VISIBLE, Value::Bool(drawn.is_none())), (k::ROLE, role)]),
        )?;
        if drawn.is_some() {
            return Ok(());
        }
        let controls = list(spec.get(p::CONTROLS));
        if self.children(&slot).len() < controls.len() {
            self.forget();
        }
        for (n, control) in controls.iter().enumerate() {
            let node = self.control_at(&slot, n)?;
            self.one(&format!("{i}:{n}"), &node, control)?;
        }
        // Whatever a longer row left behind. Hidden, not freed: the next
        // selection may be a vector again.
        for (c, node) in self.children(&slot).iter().enumerate().skip(controls.len()) {
            self.hidden(&format!("{i}:{c}"), node)?;
        }
        Ok(())
    }
}

/// The host at `path`, shown, and the pass that fills it; nothing when no
/// node is there.
fn open<'a>(eng: &'a Engine, ops: Ops, path: &str) -> Result<Option<(Pool<'a>, Value)>> {
    let host = (ops.scene_node)(eng, &[Value::text(path)])?;
    let Value::Node(id) = host else {
        return Ok(None);
    };
    let pool = Pool { eng, ops, host: id };
    pool.patched("host", &host, &table(&[(k::VISIBLE, Value::Bool(true))]))?;
    Ok(Some((pool, host)))
}

fn fill_strip(eng: &Engine, ops: Ops, path: &str, controls: &Value) -> Result<()> {
    let Some((pool, host)) = open(eng, ops, path)? else {
        return Ok(());
    };
    pool.fill("", &host, list(Some(controls)))
}

fn fill_rows(
    eng: &Engine,
    ops: Ops,
    path: &str,
    specs: &Value,
    label_w: &Value,
    stacked: bool,
) -> Result<()> {
    let Some((pool, host)) = open(eng, ops, path)? else {
        return Ok(());
    };
    let specs = list(Some(specs));
    let mut made = pool.children(&host).len();
    if made < specs.len() {
        pool.forget();
    }
    while made < specs.len() {
        pool.make_row(&host, made)?;
        made += 1;
    }
    for (i, row) in pool.children(&host).iter().enumerate() {
        match specs.get(i) {
            Some(spec) => pool.row(i, row, spec, label_w, stacked)?,
            None => pool.hidden(&format!("r{i}"), row)?,
        }
    }
    Ok(())
}

/// `ui.fill_strip` and `ui.fill_rows`.
pub(crate) fn install_pool(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "fill_strip",
            &[],
            "(host: string, controls: list)",
            "Put one widget node per control table under the node at `host`, made, reused and hidden as the list changes; a node is written only when its table changed. A control's `on` hears the reader's edit and `on_submit` hears Enter, and a control carrying `controls` is a row holding them.",
        ),
        (
            "fill_rows",
            &[],
            "(host: string, rows: list, label_width: number, stacked: bool)",
            "`fill_strip` for a form: each row table is a `label`, its `controls` and an optional `draw` hatch, laid beside `label_width`, or above the controls when `stacked`. `full` spans both columns; `height`, `tooltip`, `label_role` and `slot_role` dress it.",
        ),
    ]);
    let Some(ops) = Ops::resolve() else {
        tracing::error!("the widget pool cannot find the node operations it writes through");
        return;
    };
    m.function(
        "fill_strip",
        move |eng: &Engine, (host, controls): (String, Value)| {
            fill_strip(eng, ops, &host, &controls)
        },
    );
    m.function(
        "fill_rows",
        move |eng: &Engine, (host, rows, label_w, stacked): (String, Value, Value, bool)| {
            fill_rows(eng, ops, &host, &rows, &label_w, stacked)
        },
    );
}
