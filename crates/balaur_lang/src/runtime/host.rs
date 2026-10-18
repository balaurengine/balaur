// Every import a module makes, answered in one place whichever engine runs it:
// the script's `extern fn`s, the engine's functions, and the built-ins.

use super::debugger::{self, Debugger};
use super::engine::{ImportCtx, Raw, Shared};
use super::values::{Reader, Value, decode, encode_plain, slots};
use crate::{Meta, Ty, words};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) type SyncFn = Box<dyn FnMut(&[Value]) -> Value>;
pub(crate) type AsyncFn = Box<dyn FnMut(&[Value]) -> (f64, Value)>;
/// Answers an engine function by key, its arguments, and the type it must return.
pub(crate) type Resolver = Box<dyn FnMut(&str, &[Value], &Ty) -> Result<Value, String>>;

/// The host functions a script may call: its own `extern fn`s by name, and the
/// engine's through one resolver.
#[derive(Default)]
pub struct HostFns {
    pub(crate) sync: HashMap<String, SyncFn>,
    pub(crate) asynch: HashMap<String, AsyncFn>,
    pub(crate) resolver: Option<Resolver>,
}

impl HostFns {
    #[must_use]
    pub fn on(mut self, name: &str, f: impl FnMut(&[Value]) -> Value + 'static) -> Self {
        self.sync.insert(name.into(), Box::new(f));
        self
    }
    /// An `extern async fn`: `f` says how many seconds until it resolves, and to what.
    #[must_use]
    pub fn on_async(
        mut self,
        name: &str,
        f: impl FnMut(&[Value]) -> (f64, Value) + 'static,
    ) -> Self {
        self.asynch.insert(name.into(), Box::new(f));
        self
    }
    /// The engine's functions: `module::name`, `node.name`, `component.property.get`/`.set`.
    #[must_use]
    pub fn engine(
        mut self,
        f: impl FnMut(&str, &[Value], &Ty) -> Result<Value, String> + 'static,
    ) -> Self {
        self.resolver = Some(Box::new(f));
        self
    }
}

enum Kind {
    Sync(usize, Vec<Ty>, Ty),
    Async(usize, Vec<Ty>),
    Engine(String, Vec<Ty>, Ty),
    EngineAsync(String, Vec<Ty>, Ty),
    Str(&'static str, Vec<Ty>, Ty),
    FmtI64,
    FmtF64,
    AwaitI64,
    AwaitF64,
    AwaitI32,
    Unary(fn(f64) -> f64),
    Binary(fn(f64, f64) -> f64),
    Probe,
}

pub(crate) struct HostState {
    pub meta: Arc<Meta>,
    kinds: Vec<Kind>,
    sync: Vec<SyncFn>,
    asynch: Vec<AsyncFn>,
    resolver: Option<Resolver>,
    pub timers: HashMap<i64, (f64, Value)>,
    pub resolved: HashMap<i64, Value>,
    /// An engine token a script awaits: its handle, and what it resolves to.
    pub tokens: HashMap<u64, (i64, Ty)>,
    next_handle: i64,
    pub now: f64,
    pub debugger: Debugger,
}

fn math(name: &str) -> Option<Kind> {
    let one: Option<fn(f64) -> f64> = match name {
        "sin" => Some(libm::sin),
        "cos" => Some(libm::cos),
        "tan" => Some(libm::tan),
        "asin" => Some(libm::asin),
        "acos" => Some(libm::acos),
        "atan" => Some(libm::atan),
        "exp" => Some(libm::exp),
        "ln" => Some(libm::log),
        _ => None,
    };
    one.map(Kind::Unary).or(match name {
        "atan2" => Some(Kind::Binary(libm::atan2)),
        "pow" => Some(Kind::Binary(libm::pow)),
        _ => None,
    })
}

impl HostState {
    pub(crate) fn new(meta: Arc<Meta>, mut fns: HostFns) -> Result<Self, String> {
        let (mut sync, mut asynch, mut kinds) = (vec![], vec![], vec![]);
        for imp in &meta.imports {
            let name = imp.name.as_str();
            let missing = || format!("the script needs host function `{name}`");
            let kind = if let Some(x) = meta.externs.iter().find(|x| x.name == name) {
                if x.is_async {
                    asynch.push(fns.asynch.remove(name).ok_or_else(missing)?);
                    Kind::Async(asynch.len() - 1, x.params.clone())
                } else {
                    sync.push(fns.sync.remove(name).ok_or_else(missing)?);
                    Kind::Sync(sync.len() - 1, x.params.clone(), x.ret.clone())
                }
            } else if let Some(f) = meta.host_calls.iter().find(|f| f.key == name) {
                if fns.resolver.is_none() {
                    return Err(format!(
                        "the script calls the engine's `{name}`, and this host has no engine"
                    ));
                }
                if f.is_async {
                    Kind::EngineAsync(f.key.clone(), f.params.clone(), f.ret.clone())
                } else {
                    Kind::Engine(f.key.clone(), f.params.clone(), f.ret.clone())
                }
            } else if let Some(m) = name.strip_prefix(words::MATH_PREFIX) {
                math(m).ok_or_else(missing)?
            } else if let Some(m) = name
                .strip_prefix(words::STR_PREFIX)
                .and_then(crate::strings::method)
            {
                Kind::Str(m.name, crate::strings::params(m), crate::strings::ty(m.ret))
            } else {
                match name {
                    words::FMT_I64 => Kind::FmtI64,
                    words::FMT_F64 => Kind::FmtF64,
                    words::AWAIT_I64 => Kind::AwaitI64,
                    words::AWAIT_F64 => Kind::AwaitF64,
                    words::AWAIT_I32 => Kind::AwaitI32,
                    words::DBG_PROBE => Kind::Probe,
                    _ => return Err(missing()),
                }
            };
            kinds.push(kind);
        }
        let debugger = Debugger::new(meta.clone());
        let resolver = fns.resolver.take();
        Ok(HostState {
            meta,
            kinds,
            sync,
            asynch,
            resolver,
            timers: HashMap::new(),
            resolved: HashMap::new(),
            tokens: HashMap::new(),
            next_handle: 0,
            now: 0.0,
            debugger,
        })
    }
}

pub(crate) struct CtxReader<'a>(pub &'a dyn ImportCtx);

impl Reader for CtxReader<'_> {
    fn bytes(&self, at: usize, len: usize) -> Vec<u8> {
        self.0.read(at, len)
    }
}

fn args_of(meta: &Meta, ctx: &dyn ImportCtx, tys: &[Ty], raw: &[Raw]) -> Vec<Value> {
    let reader = CtxReader(ctx);
    let mut at = 0;
    tys.iter()
        .map(|t| {
            let k = slots(meta, t).len();
            let v = decode(meta, &reader, t, &raw[at..at + k]);
            at += k;
            v
        })
        .collect()
}

fn alloc(ctx: &mut dyn ImportCtx, n: usize) -> Result<usize, String> {
    let p = ctx.call(words::ALLOC, &[Raw::I32(n as i32)], 1)?;
    let p = p.first().map_or(0, |r| r.i32() as u32 as usize);
    ctx.write(p, &vec![0; n]);
    Ok(p)
}

/// `v` as a value of `ty` the script owns: a string or array is allocated in
/// the module, with the one reference the caller now holds.
fn encode_owned(
    ctx: &mut dyn ImportCtx,
    meta: &Meta,
    ty: &Ty,
    v: &Value,
) -> Result<Vec<Raw>, String> {
    Ok(match (ty, v) {
        (Ty::Str, Value::Str(_) | Value::Void) => {
            let s = if let Value::Str(s) = v {
                s.as_str()
            } else {
                ""
            };
            let p = alloc(ctx, 8 + s.len())?;
            let mut b = 1i32.to_le_bytes().to_vec();
            b.extend((s.len() as u32).to_le_bytes());
            b.extend(s.as_bytes());
            ctx.write(p, &b);
            vec![Raw::I32(p as i32)]
        }
        (Ty::Array(et), Value::Array(_) | Value::Void) => {
            let items: &[Value] = if let Value::Array(items) = v {
                items
            } else {
                &[]
            };
            let esz = 8 * slots(meta, et).len();
            let data = if items.is_empty() {
                0
            } else {
                alloc(ctx, items.len() * esz)?
            };
            for (i, it) in items.iter().enumerate() {
                let b: Vec<u8> = encode_owned(ctx, meta, et, it)?
                    .into_iter()
                    .flat_map(Raw::bytes)
                    .collect();
                ctx.write(data + i * esz, &b);
            }
            let h = alloc(ctx, 16)?;
            let mut b = 1i32.to_le_bytes().to_vec();
            for x in [items.len(), items.len(), data] {
                b.extend((x as u32).to_le_bytes());
            }
            ctx.write(h, &b);
            vec![Raw::I32(h as i32)]
        }
        (Ty::Struct(s), Value::Struct(_, fields)) => {
            let mut out = vec![];
            for (n, t) in meta.fields(s).unwrap_or(&[]) {
                let fv = fields
                    .iter()
                    .find(|(k, _)| k == n)
                    .map_or(&Value::Void, |(_, v)| v);
                out.extend(encode_owned(ctx, meta, t, fv)?);
            }
            out
        }
        _ => encode_plain(meta, ty, v)?,
    })
}

fn write_text(ctx: &mut dyn ImportCtx, out: i32, text: &str) -> Vec<Raw> {
    ctx.write(out as u32 as usize, text.as_bytes());
    vec![Raw::I32(text.len() as i32)]
}

/// Runs import `index` of the module with `args`, returning its results.
pub(crate) fn dispatch(
    state: &Shared,
    ctx: &mut dyn ImportCtx,
    index: usize,
    args: &[Raw],
) -> Result<Vec<Raw>, String> {
    let f64_at = |i: usize| match args[i] {
        Raw::F64(x) => x,
        r => f64::from(r.i32()),
    };
    let i64_at = |i: usize| match args[i] {
        Raw::I64(x) => x,
        r => i64::from(r.i32()),
    };
    let mut h = state.borrow_mut();
    let meta = h.meta.clone();
    let resolved = |h: &HostState| h.resolved.get(&i64_at(0)).cloned().unwrap_or(Value::Void);
    match &h.kinds[index] {
        Kind::FmtI64 => Ok(write_text(ctx, args[1].i32(), &i64_at(0).to_string())),
        // `{:?}` keeps the `.0` on whole floats, and is the shortest form that reads back exactly.
        Kind::FmtF64 => Ok(write_text(ctx, args[1].i32(), &format!("{:?}", f64_at(0)))),
        Kind::AwaitI64 => Ok(vec![Raw::I64(match resolved(&h) {
            Value::Int(x) | Value::Node(x) => x,
            _ => 0,
        })]),
        Kind::AwaitF64 => Ok(vec![Raw::F64(match resolved(&h) {
            Value::Float(x) => x,
            Value::Int(x) => x as f64,
            _ => 0.0,
        })]),
        Kind::AwaitI32 => Ok(vec![Raw::I32(i32::from(resolved(&h) == Value::Bool(true)))]),
        Kind::Unary(f) => Ok(vec![Raw::F64(f(f64_at(0)))]),
        Kind::Binary(f) => Ok(vec![Raw::F64(f(f64_at(0), f64_at(1)))]),
        Kind::Probe => {
            drop(h);
            debugger::on_probe(state, ctx, args[0].i32() as u32);
            Ok(vec![])
        }
        Kind::Sync(at, tys, ret) => {
            let (at, ret) = (*at, ret.clone());
            let values = args_of(&meta, ctx, tys, args);
            let v = (h.sync[at])(&values);
            drop(h);
            encode_owned(ctx, &meta, &ret, &v)
        }
        Kind::Async(at, tys) => {
            let at = *at;
            let values = args_of(&meta, ctx, tys, args);
            let (delay, v) = (h.asynch[at])(&values);
            h.next_handle += 1;
            let (handle, due) = (h.next_handle, h.now + delay);
            h.timers.insert(handle, (due, v));
            Ok(vec![Raw::I64(handle)])
        }
        // The engine may run other scripts before it answers, so the state is
        // not held across the call.
        Kind::Engine(key, tys, ret) => {
            let (key, ret) = (key.clone(), ret.clone());
            let values = args_of(&meta, ctx, tys, args);
            let mut resolve = h.resolver.take().ok_or("this host has no engine")?;
            drop(h);
            let v = resolve(&key, &values, &ret);
            state.borrow_mut().resolver = Some(resolve);
            encode_owned(ctx, &meta, &ret, &v?)
        }
        Kind::Str(name, tys, ret) => {
            let (name, tys, ret) = (*name, tys.clone(), ret.clone());
            drop(h);
            let values = args_of(&meta, ctx, &tys, args);
            let recv = match values.first() {
                Some(Value::Str(s)) => s.as_str(),
                _ => "",
            };
            let v = crate::strings::run(name, recv, &values[1..]);
            encode_owned(ctx, &meta, &ret, &v)
        }
        // The engine answers with a token it wakes later; the script awaits a
        // handle of its own, so a token never collides with an extern's.
        Kind::EngineAsync(key, tys, ret) => {
            let (key, ret) = (key.clone(), ret.clone());
            let values = args_of(&meta, ctx, tys, args);
            let mut resolve = h.resolver.take().ok_or("this host has no engine")?;
            drop(h);
            let token = resolve(&key, &values, &Ty::Int);
            let mut h = state.borrow_mut();
            h.resolver = Some(resolve);
            let Value::Int(token) = token? else {
                return Err(format!("`{key}` answered without a token"));
            };
            h.next_handle += 1;
            let handle = h.next_handle;
            h.tokens.insert(token.cast_unsigned(), (handle, ret));
            Ok(vec![Raw::I64(handle)])
        }
    }
}
