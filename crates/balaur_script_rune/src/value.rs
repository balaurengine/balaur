//! Conversions between the neutral `balaur_script::Value` and Rune's.

pub(crate) mod component;
mod glam_api;
pub(crate) mod glam_types;
mod live;

pub use glam_types::{Vec2, Vec3};

use anyhow::{Result, anyhow};
use balaur_script::{CallbackId, Value as Neutral};
use rune::alloc::clone::TryClone as _;
use smol_str::SmolStr;

/// A node handle as scripts see it. Opaque on purpose: a script may store one
/// and hand it back, but the bits are the engine's business.
#[derive(rune::Any, Clone, Copy)]
#[rune(item = ::balaur)]
pub struct Node {
    pub(crate) id: u64,
}

impl Node {
    // The shape Rune's protocol registration takes: the receiver by reference
    // and the operand as a value it can convert.
    #[allow(
        clippy::trivially_copy_pass_by_ref,
        clippy::needless_pass_by_value,
        reason = "an associated function registered with Rune"
    )]
    fn same(&self, other: rune::Value) -> bool {
        other.borrow_ref::<Node>().is_ok_and(|n| n.id == self.id)
    }
}

#[derive(rune::Any, Clone, Copy)]
#[rune(item = ::balaur)]
pub struct Color {
    #[rune(get, set)]
    pub r: f64,
    #[rune(get, set)]
    pub g: f64,
    #[rune(get, set)]
    pub b: f64,
    #[rune(get, set)]
    pub a: f64,
}

/// A colour's four channels, so one set of operators serves them.
trait Lanes: rune::Any + Copy {
    fn lanes(&self) -> [f64; 4];
    fn from_lanes(lanes: [f64; 4]) -> Self;
}

impl Lanes for Color {
    fn lanes(&self) -> [f64; 4] {
        [self.r, self.g, self.b, self.a]
    }
    fn from_lanes(l: [f64; 4]) -> Self {
        Self {
            r: l[0],
            g: l[1],
            b: l[2],
            a: l[3],
        }
    }
}

/// The right side of an operator: the same type lane by lane, or one number
/// for every lane.
fn rhs<T: Lanes>(value: &rune::Value) -> anyhow::Result<[f64; 4]> {
    if let Ok(other) = value.borrow_ref::<T>() {
        return Ok(other.lanes());
    }
    if let Ok(f) = value.as_float() {
        return Ok([f; 4]);
    }
    if let Ok(i) = value.as_signed() {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a script integer used as a scale"
        )]
        return Ok([i as f64; 4]);
    }
    Err(anyhow!(
        "`{}` is not a number or the same kind of vector",
        value.type_info()
    ))
}

fn zip<T: Lanes>(a: &T, b: &rune::Value, f: fn(f64, f64) -> f64) -> anyhow::Result<T> {
    let (l, r) = (a.lanes(), rhs::<T>(b)?);
    Ok(T::from_lanes([
        f(l[0], r[0]),
        f(l[1], r[1]),
        f(l[2], r[2]),
        f(l[3], r[3]),
    ]))
}

#[allow(clippy::float_cmp, reason = "Godot's `==` on a vector is exact too")]
fn same<T: Lanes>(a: &T, b: &rune::Value) -> bool {
    b.borrow_ref::<T>().is_ok_and(|b| b.lanes() == a.lanes())
}

/// An operator's failure raised in the script, where a `Result` would be a
/// value the script never looks at.
fn vm<T>(result: anyhow::Result<T>) -> rune::runtime::VmResult<T> {
    match result {
        Ok(value) => rune::runtime::VmResult::Ok(value),
        Err(why) => rune::runtime::VmResult::Err(rune::runtime::VmError::panic(why.to_string())),
    }
}

/// `+ - * /` and `==` on a colour; `c += d` is `c = c + d`.
macro_rules! arithmetic {
    ($m:expr, $t:ty) => {{
        use rune::runtime::Protocol as P;
        $m.associated_function(&P::ADD, |a: &$t, b: rune::Value| {
            vm(zip::<$t>(a, &b, |x, y| x + y))
        })?;
        $m.associated_function(&P::SUB, |a: &$t, b: rune::Value| {
            vm(zip::<$t>(a, &b, |x, y| x - y))
        })?;
        $m.associated_function(&P::MUL, |a: &$t, b: rune::Value| {
            vm(zip::<$t>(a, &b, |x, y| x * y))
        })?;
        $m.associated_function(&P::DIV, |a: &$t, b: rune::Value| {
            vm(zip::<$t>(a, &b, |x, y| x / y))
        })?;
        $m.associated_function(&P::PARTIAL_EQ, |a: &$t, b: rune::Value| same::<$t>(a, &b))?;
        $m.associated_function(&P::EQ, |a: &$t, b: rune::Value| same::<$t>(a, &b))?;
    }};
}

pub(crate) fn install(
    m: &mut rune::Module,
    engine: &balaur_core::Engine,
) -> Result<(), rune::ContextError> {
    m.ty::<Node>()?;
    // `a == b` on two handles: the same node. Anything else is not equal.
    m.associated_function(&rune::runtime::Protocol::PARTIAL_EQ, Node::same)?;
    m.associated_function(&rune::runtime::Protocol::EQ, Node::same)?;
    // A node keys a map by its id, as Godot's dictionaries key by object.
    m.associated_function(
        &rune::runtime::Protocol::HASH,
        |n: &Node, hasher: &mut rune::runtime::Hasher| {
            std::hash::Hasher::write_u64(hasher, n.id);
        },
    )?;
    m.ty::<Color>()?;
    m.function("new", |r: f64, g: f64, b: f64, a: f64| Color { r, g, b, a })
        .build_associated::<Color>()?;
    arithmetic!(m, Color);
    glam_types::copy!(m, Color);
    m.associated_function("with_r", |c: &Color, r: f64| Color { r, ..*c })?;
    m.associated_function("with_g", |c: &Color, g: f64| Color { g, ..*c })?;
    m.associated_function("with_b", |c: &Color, b: f64| Color { b, ..*c })?;
    m.associated_function("with_a", |c: &Color, a: f64| Color { a, ..*c })?;
    glam_types::install(m)?;
    glam_api::install(m)?;

    // A component-driven operation lives on that component's handle
    // (`node.transform.translate`); the node keeps only what no component owns.
    let component_driven: std::collections::HashSet<String> = crate::bindings::api_docs()
        .into_iter()
        .filter(|d| d.module == "node" && !d.acts_on.is_empty())
        .map(|d| d.name)
        .collect();
    for declared in balaur_core::node_api::NODE_OPS {
        if component_driven.contains(declared.name) {
            continue;
        }
        let call = declared.call;
        let engine = engine.clone();
        let handle = crate::bindings::hold_node_fn(engine, call);
        let bound =
            crate::bindings::bound_handler(handle, "node method was registered on another thread");
        // Between two Rune scripts these hand over the values themselves.
        match declared.name {
            "call" => m.raw_function(
                declared.name,
                live::live_or(handle, bound, |host, node, args| {
                    let method = args.first()?.borrow_string_ref().ok()?.to_owned();
                    host.call_live(node, &method, &args[1..])
                }),
            ),
            "script_field" => m.raw_function(
                declared.name,
                live::live_or(handle, bound, |host, node, args| {
                    let name = args.first()?.borrow_string_ref().ok()?.to_owned();
                    host.field_live(node, &name)
                }),
            ),
            _ => m.raw_function(declared.name, bound),
        }
        .build_associated::<Node>()?;
    }
    component::install(m, engine)?;
    Ok(())
}

/// Rune value -> neutral. A function becomes a call-scoped callback.
///
/// Tested in the order a binding call sees them — the node receiver, then
/// strings and numbers — and through the accessors that only inspect the
/// representation, so a mismatch costs a compare and never a clone.
pub(crate) fn to_neutral(v: &rune::Value) -> Result<Neutral> {
    use rune::runtime::Object;
    // Borrow rather than convert: `from_value` on a Rune `Any` moves the value
    // out of its shared cell, so reading a node would destroy it.
    if let Ok(n) = v.borrow_ref::<Node>() {
        return Ok(Neutral::Node(n.id));
    }
    if let Ok(s) = v.borrow_string_ref() {
        return Ok(Neutral::Str(SmolStr::new(&*s)));
    }
    if let Ok(i) = v.as_signed() {
        return Ok(Neutral::Int(i));
    }
    // A count — `len()`, an index — is unsigned in Rune and an `Int` here.
    if let Ok(u) = v.as_unsigned() {
        return Ok(Neutral::Int(
            i64::try_from(u).map_err(|_| anyhow!("{u} does not fit a script integer"))?,
        ));
    }
    if let Ok(f) = v.as_float() {
        return Ok(Neutral::Num(f));
    }
    if let Ok(b) = v.as_bool() {
        return Ok(Neutral::Bool(b));
    }
    if let Ok(p) = v.borrow_ref::<Vec2>() {
        return Ok(Neutral::Vec2([p.x as f32, p.y as f32]));
    }
    if let Ok(p) = v.borrow_ref::<Vec3>() {
        return Ok(Neutral::Vec3([p.x as f32, p.y as f32, p.z as f32]));
    }
    if let Ok(c) = v.borrow_ref::<Color>() {
        return Ok(Neutral::Color([
            c.r as f32, c.g as f32, c.b as f32, c.a as f32,
        ]));
    }
    if let Ok(items) = v.borrow_ref::<rune::runtime::Vec>() {
        return Ok(Neutral::List(
            items.iter().map(to_neutral).collect::<Result<_>>()?,
        ));
    }
    if let Ok(obj) = v.borrow_ref::<Object>() {
        let mut out = Vec::with_capacity(obj.len());
        for (k, val) in obj.iter() {
            out.push((SmolStr::new(k.as_str()), to_neutral(val)?));
        }
        // Rune objects do not preserve insertion order; sort so a binding sees
        // the same map every run.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(Neutral::Map(out));
    }
    // A map keyed by more than strings, as a Godot dictionary is: its keys
    // spelled as text, the way JSON spells them.
    if let Ok(map) = v.borrow_ref::<rune::modules::collections::HashMap>() {
        let mut out = Vec::new();
        for (k, val) in map.entries()? {
            let key = match to_neutral(&k)? {
                Neutral::Str(s) => s,
                Neutral::Int(i) => i.to_string().into(),
                Neutral::Num(n) => n.to_string().into(),
                Neutral::Bool(b) => b.to_string().into(),
                other => return Err(anyhow!("a map key cannot be {other:?}")),
            };
            out.push((key, to_neutral(&val)?));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(Neutral::Map(out));
    }
    if let Ok(f) = v.borrow_ref::<rune::runtime::Function>() {
        return Ok(Neutral::Callback(crate::bindings::hold_callback(
            f.try_clone()?,
        )));
    }
    if let Ok(b) = v.borrow_ref::<rune::runtime::Bytes>() {
        return Ok(Neutral::Bytes(b.as_slice().to_vec()));
    }
    if let Ok(t) = v.borrow_tuple_ref() {
        // Unit is the empty tuple in Rune, and it is how a void function
        // returns. Anything longer has no neutral counterpart.
        if !t.is_empty() {
            return Err(anyhow!("tuples are not a script value; use a list"));
        }
    }
    Ok(Neutral::Nil)
}

/// Rune -> neutral, for a snapshot: like `to_neutral`, but functions and
/// foreign `Any` values are skipped rather than held -- a snapshot has to be
/// plain data. Nodes stay: entity bits are stable within one process.
pub(crate) fn to_plain(v: &rune::Value) -> Option<Neutral> {
    use rune::runtime::Object;
    // Through the accessors that only read the representation, so a value
    // that is not a number is not copied to find that out.
    if let Ok(b) = v.as_bool() {
        return Some(Neutral::Bool(b));
    }
    if let Ok(i) = v.as_signed() {
        return Some(Neutral::Int(i));
    }
    if let Ok(u) = v.as_unsigned() {
        return Some(Neutral::Int(i64::try_from(u).ok()?));
    }
    if let Ok(f) = v.as_float() {
        return Some(Neutral::Num(f));
    }
    if let Ok(s) = v.borrow_string_ref() {
        return Some(Neutral::Str(s.to_string().into()));
    }
    if let Ok(b) = v.borrow_ref::<rune::runtime::Bytes>() {
        return Some(Neutral::Bytes(b.as_slice().to_vec()));
    }
    if let Ok(n) = v.borrow_ref::<Node>() {
        return Some(Neutral::Node(n.id));
    }
    if let Ok(p) = v.borrow_ref::<Vec2>() {
        return Some(Neutral::Vec2([p.x as f32, p.y as f32]));
    }
    if let Ok(p) = v.borrow_ref::<Vec3>() {
        return Some(Neutral::Vec3([p.x as f32, p.y as f32, p.z as f32]));
    }
    if let Ok(c) = v.borrow_ref::<Color>() {
        return Some(Neutral::Color([
            c.r as f32, c.g as f32, c.b as f32, c.a as f32,
        ]));
    }
    if v.borrow_ref::<rune::runtime::Function>().is_ok() {
        return None;
    }
    if let Ok(items) = v.borrow_ref::<rune::runtime::Vec>() {
        return Some(Neutral::List(items.iter().filter_map(to_plain).collect()));
    }
    if let Ok(obj) = v.borrow_ref::<Object>() {
        let mut out = Vec::with_capacity(obj.len());
        for (k, val) in obj.iter() {
            if let Some(plain) = to_plain(val) {
                out.push((SmolStr::new(k.as_str()), plain));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Some(Neutral::Map(out));
    }
    if v.borrow_ref::<rune::modules::collections::HashMap>()
        .is_ok()
    {
        return to_neutral(v).ok();
    }
    if let Ok(t) = v.borrow_tuple_ref()
        && t.is_empty()
    {
        return Some(Neutral::Nil);
    }
    None
}

/// Neutral -> Rune value.
pub(crate) fn from_neutral(v: &Neutral) -> Result<rune::Value> {
    let out = match v {
        Neutral::Nil => rune::to_value(())?,
        Neutral::Bool(b) => rune::to_value(*b)?,
        Neutral::Int(i) => rune::to_value(*i)?,
        Neutral::Num(n) => rune::to_value(*n)?,
        Neutral::Str(s) => rune::to_value(s.as_str())?,
        // Rune has its own allocator, so a std `Vec<u8>` crosses by slice.
        Neutral::Bytes(b) => rune::to_value(rune::runtime::Bytes::from_slice(b.as_slice())?)?,
        Neutral::Node(id) => rune::to_value(Node { id: *id })?,
        Neutral::Vec2([x, y]) => rune::to_value(Vec2 {
            x: f64::from(*x),
            y: f64::from(*y),
        })?,
        Neutral::Vec3([x, y, z]) => rune::to_value(Vec3 {
            x: f64::from(*x),
            y: f64::from(*y),
            z: f64::from(*z),
        })?,
        Neutral::Color([r, g, b, a]) => rune::to_value(Color {
            r: f64::from(*r),
            g: f64::from(*g),
            b: f64::from(*b),
            a: f64::from(*a),
        })?,
        Neutral::Many(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(from_neutral(it)?);
            }
            rune::to_value(rune::runtime::OwnedTuple::try_from(out)?)?
        }
        Neutral::List(items) => {
            let mut out = rune::runtime::Vec::new();
            for it in items {
                out.push(from_neutral(it)?)?;
            }
            rune::to_value(out)?
        }
        Neutral::Map(pairs) => {
            let mut obj = rune::runtime::Object::new();
            for (k, val) in pairs {
                obj.insert(
                    rune::alloc::String::try_from(k.as_str())?,
                    from_neutral(val)?,
                )?;
            }
            rune::to_value(obj)?
        }
        // A function one script returned to another: the same function, while
        // the call that handed it over still holds it.
        Neutral::Callback(CallbackId(id)) => {
            let function = crate::bindings::lookup_callback(CallbackId(*id))
                .ok_or_else(|| anyhow!("callback {id} was used after its call returned"))?;
            rune::to_value(function)?
        }
    };
    Ok(out)
}
