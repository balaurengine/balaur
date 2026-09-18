//! Conversions between the neutral `balaur_script::Value` and Rune's.

pub(crate) mod component;

use anyhow::{Result, anyhow};
use balaur_script::{CallbackId, Value as Neutral};
use rune::alloc::clone::TryClone as _;

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

/// A vector as scripts see it. Rune has no tuple-struct literals across the
/// FFI, so bindings take and return this.
#[derive(rune::Any, Clone, Copy)]
#[rune(item = ::balaur)]
pub struct Vec2 {
    #[rune(get, set)]
    pub x: f64,
    #[rune(get, set)]
    pub y: f64,
}

#[derive(rune::Any, Clone, Copy)]
#[rune(item = ::balaur)]
pub struct Vec3 {
    #[rune(get, set)]
    pub x: f64,
    #[rune(get, set)]
    pub y: f64,
    #[rune(get, set)]
    pub z: f64,
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

/// The numbers of a vector or a colour, so one set of operators serves all three.
trait Lanes: rune::Any + Copy {
    fn lanes(&self) -> [f64; 4];
    fn from_lanes(lanes: [f64; 4]) -> Self;
}

impl Lanes for Vec2 {
    fn lanes(&self) -> [f64; 4] {
        [self.x, self.y, 0.0, 0.0]
    }
    fn from_lanes(l: [f64; 4]) -> Self {
        Self { x: l[0], y: l[1] }
    }
}

impl Lanes for Vec3 {
    fn lanes(&self) -> [f64; 4] {
        [self.x, self.y, self.z, 0.0]
    }
    fn from_lanes(l: [f64; 4]) -> Self {
        Self { x: l[0], y: l[1], z: l[2] }
    }
}

impl Lanes for Color {
    fn lanes(&self) -> [f64; 4] {
        [self.r, self.g, self.b, self.a]
    }
    fn from_lanes(l: [f64; 4]) -> Self {
        Self { r: l[0], g: l[1], b: l[2], a: l[3] }
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
        #[allow(clippy::cast_precision_loss, reason = "a script integer used as a scale")]
        return Ok([i as f64; 4]);
    }
    Err(anyhow!("`{}` is not a number or the same kind of vector", value.type_info()))
}

fn zip<T: Lanes>(a: &T, b: &rune::Value, f: fn(f64, f64) -> f64) -> anyhow::Result<T> {
    let (l, r) = (a.lanes(), rhs::<T>(b)?);
    Ok(T::from_lanes([f(l[0], r[0]), f(l[1], r[1]), f(l[2], r[2]), f(l[3], r[3])]))
}

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

/// `+ - * /` and their assigning forms, and `==`, on one lane type.
macro_rules! arithmetic {
    ($m:expr, $t:ty) => {{
        use rune::runtime::Protocol as P;
        $m.associated_function(&P::ADD, |a: &$t, b: rune::Value| vm(zip::<$t>(a, &b, |x, y| x + y)))?;
        $m.associated_function(&P::SUB, |a: &$t, b: rune::Value| vm(zip::<$t>(a, &b, |x, y| x - y)))?;
        $m.associated_function(&P::MUL, |a: &$t, b: rune::Value| vm(zip::<$t>(a, &b, |x, y| x * y)))?;
        $m.associated_function(&P::DIV, |a: &$t, b: rune::Value| vm(zip::<$t>(a, &b, |x, y| x / y)))?;
        $m.associated_function(&P::ADD_ASSIGN, |a: &mut $t, b: rune::Value| {
            let out = zip::<$t>(a, &b, |x, y| x + y);
            vm(out.map(|v| *a = v))
        })?;
        $m.associated_function(&P::SUB_ASSIGN, |a: &mut $t, b: rune::Value| {
            let out = zip::<$t>(a, &b, |x, y| x - y);
            vm(out.map(|v| *a = v))
        })?;
        $m.associated_function(&P::MUL_ASSIGN, |a: &mut $t, b: rune::Value| {
            let out = zip::<$t>(a, &b, |x, y| x * y);
            vm(out.map(|v| *a = v))
        })?;
        $m.associated_function(&P::DIV_ASSIGN, |a: &mut $t, b: rune::Value| {
            let out = zip::<$t>(a, &b, |x, y| x / y);
            vm(out.map(|v| *a = v))
        })?;
        $m.associated_function(&P::PARTIAL_EQ, |a: &$t, b: rune::Value| same::<$t>(a, &b))?;
        $m.associated_function(&P::EQ, |a: &$t, b: rune::Value| same::<$t>(a, &b))?;
    }};
}

/// The geometry a vector's own methods answer: length, direction, and the
/// products and blends Godot's `Vector2` and `Vector3` carry.
macro_rules! geometry {
    ($m:expr, $t:ty) => {{
        $m.associated_function("length", |v: &$t| dot(v.lanes(), v.lanes()).sqrt())?;
        $m.associated_function("dot", |v: &$t, o: rune::Value| {
            vm(rhs::<$t>(&o).map(|r| dot(v.lanes(), r)))
        })?;
        $m.associated_function("normalized", |v: &$t| {
            let l = v.lanes();
            let len = dot(l, l).sqrt();
            if len == 0.0 {
                return *v;
            }
            <$t>::from_lanes([l[0] / len, l[1] / len, l[2] / len, 0.0])
        })?;
        $m.associated_function("distance_to", |v: &$t, o: rune::Value| {
            vm(zip::<$t>(v, &o, |x, y| x - y).map(|d| {
                let d = d.lanes();
                dot(d, d).sqrt()
            }))
        })?;
        $m.associated_function("lerp", |v: &$t, o: rune::Value, t: f64| {
            let l = v.lanes();
            vm(rhs::<$t>(&o).map(|to| {
                <$t>::from_lanes([
                    l[0] + (to[0] - l[0]) * t,
                    l[1] + (to[1] - l[1]) * t,
                    l[2] + (to[2] - l[2]) * t,
                    l[3] + (to[3] - l[3]) * t,
                ])
            }))
        })?;
    }};
}

fn dot(a: [f64; 4], b: [f64; 4]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Register the value types every binding may see, and give `Node` the whole
/// engine node API as methods.
///
/// The operations come from `balaur_core::node_api::NODE_OPS`, so this is
/// only the `node.position()` sugar — the behaviour is shared with every other
/// language.
pub(crate) fn install(
    m: &mut rune::Module,
    engine: &balaur_core::Engine,
) -> Result<(), rune::ContextError> {
    m.ty::<Node>()?;
    // `a == b` on two handles: the same node. Anything else is not equal.
    m.associated_function(&rune::runtime::Protocol::PARTIAL_EQ, Node::same)?;
    m.associated_function(&rune::runtime::Protocol::EQ, Node::same)?;
    m.ty::<Vec2>()?;
    m.ty::<Vec3>()?;
    m.ty::<Color>()?;
    m.function("new", |x: f64, y: f64| Vec2 { x, y })
        .build_associated::<Vec2>()?;
    m.function("new", |x: f64, y: f64, z: f64| Vec3 { x, y, z })
        .build_associated::<Vec3>()?;
    m.function("new", |r: f64, g: f64, b: f64, a: f64| Color { r, g, b, a })
        .build_associated::<Color>()?;
    arithmetic!(m, Vec2);
    arithmetic!(m, Vec3);
    arithmetic!(m, Color);
    geometry!(m, Vec2);
    geometry!(m, Vec3);

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
        m.raw_function(
            declared.name,
            crate::bindings::bound_handler(handle, "node method was registered on another thread"),
        )
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
        return Ok(Neutral::Str(s.to_string()));
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
            out.push((k.to_string(), to_neutral(val)?));
        }
        // Rune objects do not preserve insertion order; sort so a binding sees
        // the same map every run.
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
        return Some(Neutral::Str(s.to_string()));
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
                out.push((k.to_string(), plain));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Some(Neutral::Map(out));
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
        Neutral::Str(s) => rune::to_value(s.clone())?,
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
        Neutral::Callback(CallbackId(id)) => {
            return Err(anyhow!("cannot hand callback {id} back to a script"));
        }
    };
    Ok(out)
}
