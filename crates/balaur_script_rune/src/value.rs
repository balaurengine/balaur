//! Conversions between the neutral `balaur_script::Value` and Rune's.

use anyhow::{anyhow, Result};
use balaur_script::{CallbackId, Value as Neutral};

/// A node handle as scripts see it. Opaque on purpose: a script may store one
/// and hand it back, but the bits are the engine's business.
#[derive(rune::Any, Clone, Copy)]
#[rune(item = ::balaur)]
pub struct Node {
    pub(crate) id: u64,
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

/// Register the value types every binding may see.
pub(crate) fn install(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.ty::<Node>()?;
    m.ty::<Vec2>()?;
    m.ty::<Vec3>()?;
    m.ty::<Color>()?;
    Ok(())
}

/// Rune value -> neutral. A function becomes a call-scoped callback.
pub(crate) fn to_neutral(v: &rune::Value) -> Result<Neutral> {
    use rune::runtime::{Object, OwnedTuple};
    if let Ok(b) = rune::from_value::<bool>(v.clone()) {
        return Ok(Neutral::Bool(b));
    }
    if let Ok(i) = rune::from_value::<i64>(v.clone()) {
        return Ok(Neutral::Int(i));
    }
    if let Ok(f) = rune::from_value::<f64>(v.clone()) {
        return Ok(Neutral::Num(f));
    }
    if let Ok(s) = rune::from_value::<String>(v.clone()) {
        return Ok(Neutral::Str(s));
    }
    if let Ok(n) = rune::from_value::<Node>(v.clone()) {
        return Ok(Neutral::Node(n.id));
    }
    if let Ok(p) = rune::from_value::<Vec2>(v.clone()) {
        return Ok(Neutral::Vec2([p.x as f32, p.y as f32]));
    }
    if let Ok(p) = rune::from_value::<Vec3>(v.clone()) {
        return Ok(Neutral::Vec3([p.x as f32, p.y as f32, p.z as f32]));
    }
    if let Ok(c) = rune::from_value::<Color>(v.clone()) {
        return Ok(Neutral::Color([
            c.r as f32, c.g as f32, c.b as f32, c.a as f32,
        ]));
    }
    if let Ok(f) = rune::from_value::<rune::runtime::Function>(v.clone()) {
        return Ok(Neutral::Callback(crate::bindings::hold_callback(f)));
    }
    if let Ok(items) = rune::from_value::<Vec<rune::Value>>(v.clone()) {
        return Ok(Neutral::List(
            items.iter().map(to_neutral).collect::<Result<_>>()?,
        ));
    }
    if let Ok(obj) = rune::from_value::<Object>(v.clone()) {
        let mut out = Vec::with_capacity(obj.len());
        for (k, val) in &obj {
            out.push((k.to_string(), to_neutral(val)?));
        }
        // Rune objects do not preserve insertion order; sort so a binding sees
        // the same map every run.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(Neutral::Map(out));
    }
    if rune::from_value::<OwnedTuple>(v.clone()).is_ok() {
        return Err(anyhow!("tuples are not a script value; use a list"));
    }
    Ok(Neutral::Nil)
}

/// Neutral -> Rune value.
pub(crate) fn from_neutral(v: &Neutral) -> Result<rune::Value> {
    let out = match v {
        Neutral::Nil => rune::to_value(())?,
        Neutral::Bool(b) => rune::to_value(*b)?,
        Neutral::Int(i) => rune::to_value(*i)?,
        Neutral::Num(n) => rune::to_value(*n)?,
        Neutral::Str(s) => rune::to_value(s.clone())?,
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
            return Err(anyhow!("cannot hand callback {id} back to a script"))
        }
    };
    Ok(out)
}
