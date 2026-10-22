// Every import a module makes, answered in one place whichever engine runs it:
// the script's `extern fn`s, the engine's functions, and the built-ins.

use super::debugger::{self, Debugger};
use super::engine::{ImportCtx, Raw, Raws, Shared};
use super::values::{Reader, Value, decode, encode_plain, width};
use crate::{Meta, Ty, words};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub(crate) type SyncFn = Box<dyn FnMut(&[Value]) -> Value>;
pub(crate) type AsyncFn = Box<dyn FnMut(&[Value]) -> (f64, Value)>;
/// One engine function, bound to what it calls when the module is made, so a
/// call parses no key and looks nothing up. Shared, since the engine may call
/// back into the same file while it answers.
pub type Bound = Rc<dyn Fn(&[Value]) -> Result<Value, String>>;
/// Binds an engine function by its key and the type it must return.
pub(crate) type Binder = Rc<dyn Fn(&str, &Ty) -> Result<Bound, String>>;

/// The host functions a script may call: its own `extern fn`s by name, and the
/// engine's through one binder.
#[derive(Default)]
pub struct HostFns {
    pub(crate) sync: HashMap<String, SyncFn>,
    pub(crate) asynch: HashMap<String, AsyncFn>,
    pub(crate) binder: Option<Binder>,
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
    /// The engine's functions: `module::name`, `node.name`,
    /// `component.property.get`/`.set`, answered by key on every call.
    #[must_use]
    pub fn engine(
        self,
        f: impl Fn(&str, &[Value], &Ty) -> Result<Value, String> + 'static,
    ) -> Self {
        let f = Rc::new(f);
        self.bind_engine(move |key, ret| {
            let (f, key, ret) = (f.clone(), key.to_string(), ret.clone());
            Ok(Rc::new(move |args: &[Value]| f(&key, args, &ret)) as Bound)
        })
    }
    /// The engine's functions, each bound once by key when a module is made:
    /// `bind(key, ret)` answers with what a call to it runs.
    #[must_use]
    pub fn bind_engine(
        mut self,
        bind: impl Fn(&str, &Ty) -> Result<Bound, String> + 'static,
    ) -> Self {
        self.binder = Some(Rc::new(bind));
        self
    }
}

/// An import's key, parameters and result, shared so a call clones nothing.
struct Sig {
    key: String,
    params: Vec<Ty>,
    ret: Ty,
}

fn sig(key: &str, params: &[Ty], ret: &Ty) -> Rc<Sig> {
    Rc::new(Sig {
        key: key.to_string(),
        params: params.to_vec(),
        ret: ret.clone(),
    })
}

enum Kind {
    Sync(usize, Rc<Sig>),
    Async(usize, Rc<Sig>),
    Engine(Rc<Sig>, Bound),
    EngineAsync(Rc<Sig>, Bound),
    Str(&'static str, Rc<Sig>),
    Any(&'static str),
    FmtF64,
    AwaitI64,
    AwaitF64,
    AwaitI32,
    AwaitHeap,
    Box,
    Unbox,
    Unary(fn(f64) -> f64),
    Binary(fn(f64, f64) -> f64),
    Probe,
}

pub(crate) struct HostState {
    pub meta: Arc<Meta>,
    kinds: Vec<Kind>,
    sync: Vec<SyncFn>,
    asynch: Vec<AsyncFn>,

    pub timers: HashMap<i64, (f64, Value)>,
    pub resolved: HashMap<i64, Value>,
    /// An engine token a script awaits: its handle, and what it resolves to.
    pub tokens: HashMap<u64, (i64, Ty)>,
    /// What a handle resolves to when that is a string, an array or an
    /// `any`: the value is rebuilt in the module when the await takes it.
    pub heap_awaits: HashMap<i64, Ty>,
    next_handle: i64,
    pub now: f64,
    pub debugger: Debugger,
    /// The module's context while an engine function it called is running:
    /// where a call back into the same file goes ([`super::Reentry`]).
    pub(crate) inside: Option<*mut (dyn ImportCtx + 'static)>,
    /// A direct module's call results past the first, until it reads them.
    extra: Raws,
    /// Which slots of each script struct hold a class, worked out once, or
    /// nil for one that holds a string, list or `any` outside a class and so
    /// cannot be shared ([`super::Runtime::share`]).
    pub(crate) shares: rustc_hash::FxHashMap<String, Option<Rc<[usize]>>>,
    /// What letting go of each script struct releases ([`Release`]).
    pub(crate) releases: rustc_hash::FxHashMap<String, Rc<[Release]>>,
}

/// One heap slot of a struct being let go of: where it sits, its type, and
/// the module's release function for it when it has one.
pub(crate) struct Release {
    pub slot: usize,
    pub ty: Ty,
    pub dec: Option<String>,
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
                    Kind::Async(asynch.len() - 1, sig(name, &x.params, &x.ret))
                } else {
                    sync.push(fns.sync.remove(name).ok_or_else(missing)?);
                    Kind::Sync(sync.len() - 1, sig(name, &x.params, &x.ret))
                }
            } else if let Some(f) = meta.host_calls.iter().find(|f| f.key == name) {
                let Some(bind) = &fns.binder else {
                    return Err(format!(
                        "the script calls the engine's `{name}`, and this host has no engine"
                    ));
                };
                // An async one answers with the token it is woken by.
                if f.is_async {
                    Kind::EngineAsync(sig(&f.key, &f.params, &f.ret), bind(&f.key, &Ty::Int)?)
                } else {
                    Kind::Engine(sig(&f.key, &f.params, &f.ret), bind(&f.key, &f.ret)?)
                }
            } else if let Some(m) = name.strip_prefix(words::MATH_PREFIX) {
                math(m).ok_or_else(missing)?
            } else if let Some(m) = name
                .strip_prefix(words::STR_PREFIX)
                .and_then(crate::strings::method)
            {
                Kind::Str(
                    m.name,
                    sig(name, &crate::strings::params(m), &crate::strings::ty(m.ret)),
                )
            } else if let Some(m) = name
                .strip_prefix(words::ANY_PREFIX)
                .and_then(crate::anys::method)
            {
                Kind::Any(m.name)
            } else {
                match name {
                    words::FMT_F64 => Kind::FmtF64,
                    words::AWAIT_I64 => Kind::AwaitI64,
                    words::AWAIT_F64 => Kind::AwaitF64,
                    words::AWAIT_I32 => Kind::AwaitI32,
                    words::AWAIT_HEAP => Kind::AwaitHeap,
                    words::DBG_PROBE => Kind::Probe,
                    words::BOX => Kind::Box,
                    words::UNBOX => Kind::Unbox,
                    _ => return Err(missing()),
                }
            };
            kinds.push(kind);
        }
        let debugger = Debugger::new(meta.clone());
        Ok(HostState {
            meta,
            kinds,
            sync,
            asynch,
            timers: HashMap::new(),
            resolved: HashMap::new(),
            tokens: HashMap::new(),
            heap_awaits: HashMap::new(),
            next_handle: 0,
            inside: None,
            extra: Raws::new(),
            shares: rustc_hash::FxHashMap::default(),
            releases: rustc_hash::FxHashMap::default(),
            now: 0.0,
            debugger,
        })
    }
}

// Into an `any`, or out of one.
fn convert(ctx: &mut dyn ImportCtx, meta: &Meta, args: &[Raw], into: bool) -> Result<Raws, String> {
    if into {
        boxed(ctx, meta, args)
    } else {
        unboxed(ctx, meta, args)
    }
}

/// A value as the module laid it out at `args[1]`, read by its type's text
/// at `args[0]`, handed back in an `any`.
fn boxed(ctx: &mut dyn ImportCtx, meta: &Meta, args: &[Raw]) -> Result<Raws, String> {
    let reader = CtxReader(ctx);
    let Value::Str(text) = super::values::read_str(&reader, args[0].i32() as u32 as usize) else {
        return Err("a boxed value without its type".into());
    };
    let ty =
        crate::parse::parse_type(&text).ok_or_else(|| format!("`{text}` is no type to box"))?;
    let v = super::values::read_value(meta, &reader, &ty, args[1].i32() as u32 as usize);
    encode_owned(ctx, meta, &Ty::Any, &v)
}

/// The `any` at `args[1]` as a value of the type whose text is at `args[0]`,
/// its slots written to `args[2]`: a map fills a struct by field name.
fn unboxed(ctx: &mut dyn ImportCtx, meta: &Meta, args: &[Raw]) -> Result<Raws, String> {
    let (ty, v) = {
        let reader = CtxReader(ctx);
        let Value::Str(text) = super::values::read_str(&reader, args[0].i32() as u32 as usize)
        else {
            return Err("an unboxed value without its type".into());
        };
        let ty = crate::parse::parse_type(&text)
            .ok_or_else(|| format!("`{text}` is no type to unbox"))?;
        let v = super::values::read_any(meta, &reader, args[1].i32() as u32 as usize);
        (ty, v)
    };
    let v = shaped(meta, &ty, v);
    let b: Vec<u8> = encode_owned(ctx, meta, &ty, &v)?
        .into_iter()
        .flat_map(Raw::bytes)
        .collect();
    ctx.write(args[2].i32() as u32 as usize, &b);
    Ok(Raws::new())
}

// A dynamic value in the shape of `ty`: a map as the struct it is wanted as,
// a number as the kind wanted, nil as an empty list.
fn shaped(meta: &Meta, ty: &Ty, v: Value) -> Value {
    match (ty, v) {
        (Ty::Struct(s), Value::Map(entries) | Value::Struct(_, entries)) => {
            let fields = meta.fields(s).unwrap_or(&[]);
            let values = fields
                .iter()
                .map(|(n, t)| {
                    let got = entries
                        .iter()
                        .find(|(k, _)| k == n)
                        .map_or(Value::Void, |(_, v)| v.clone());
                    (n.clone(), shaped(meta, t, got))
                })
                .collect();
            Value::Struct(s.clone(), values)
        }
        (Ty::Array(et), Value::Array(items)) => {
            Value::Array(items.into_iter().map(|i| shaped(meta, et, i)).collect())
        }
        (Ty::Array(_), Value::Void) => Value::Array(vec![]),
        (Ty::Optional(_), Value::Void) => Value::Void,
        (Ty::Optional(t), v) => shaped(meta, t, v),
        (Ty::Float, Value::Int(x)) => Value::Float(x as f64),
        (Ty::Int, Value::Float(x)) => Value::Int(x as i64),
        (_, v) => v,
    }
}

pub(crate) struct CtxReader<'a>(pub &'a dyn ImportCtx);

impl Reader for CtxReader<'_> {
    fn bytes(&self, at: usize, len: usize) -> Vec<u8> {
        self.0.read(at, len)
    }
}

/// A host call's arguments, kept inline: a call allocates nothing for them.
type Values = SmallVec<[Value; 6]>;

fn args_of(meta: &Meta, ctx: &dyn ImportCtx, tys: &[Ty], raw: &[Raw]) -> Values {
    let reader = CtxReader(ctx);
    let mut at = 0;
    tys.iter()
        .map(|t| {
            let k = width(meta, t);
            let v = decode(meta, &reader, t, &raw[at..at + k]);
            at += k;
            v
        })
        .collect()
}

pub(crate) fn alloc(ctx: &mut dyn ImportCtx, n: usize) -> Result<usize, String> {
    let p = ctx.call(words::ALLOC, &[Raw::I32(n as i32)], 1)?;
    let p = p.first().map_or(0, |r| r.i32() as u32 as usize);
    ctx.write(p, &vec![0; n]);
    Ok(p)
}

/// `v` as a value of `ty` the script owns: a string or array is allocated in
/// the module, with the one reference the caller now holds.
pub(crate) fn encode_owned(
    ctx: &mut dyn ImportCtx,
    meta: &Meta,
    ty: &Ty,
    v: &Value,
) -> Result<Raws, String> {
    // What nearly every call returns, with nothing to allocate.
    match (ty, v) {
        (Ty::Void, _) => return Ok(Raws::new()),
        (Ty::Int, Value::Int(x)) | (Ty::Node, Value::Node(x) | Value::Int(x)) => {
            return Ok(smallvec::smallvec![Raw::I64(*x)]);
        }
        (Ty::Float, Value::Float(x)) => return Ok(smallvec::smallvec![Raw::F64(*x)]),
        (Ty::Bool, Value::Bool(b)) => return Ok(smallvec::smallvec![Raw::I32(i32::from(*b))]),
        _ => {}
    }
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
            smallvec::smallvec![Raw::I32(p as i32)]
        }
        (Ty::Array(et), Value::Array(_) | Value::Void) => {
            let items: &[Value] = if let Value::Array(items) = v {
                items
            } else {
                &[]
            };
            let esz = 8 * width(meta, et);
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
            smallvec::smallvec![Raw::I32(h as i32)]
        }
        (Ty::Any, v) => {
            let cell = super::values::any_cell(v, |t, v| {
                Ok::<u32, String>(encode_owned(ctx, meta, t, v)?[0].i32() as u32)
            })?;
            let Some((tag, payload)) = cell else {
                return Ok(smallvec::smallvec![Raw::I32(0)]);
            };
            let p = alloc(ctx, crate::anys::CELL as usize)?;
            ctx.write(p, &super::values::cell_bytes(tag, payload));
            smallvec::smallvec![Raw::I32(p as i32)]
        }
        (Ty::Optional(_), Value::Void) => Raws::from_vec(super::values::zero(meta, ty)),
        (Ty::Optional(t), v) => {
            let mut out = encode_owned(ctx, meta, t, v)?;
            out.push(Raw::I32(1));
            out
        }
        (Ty::Struct(s), Value::Struct(_, fields)) => {
            let mut out = Raws::new();
            for (n, t) in meta.fields(s).unwrap_or(&[]) {
                let fv = fields
                    .iter()
                    .find(|(k, _)| k == n)
                    .map_or(&Value::Void, |(_, v)| v);
                out.extend(encode_owned(ctx, meta, t, fv)?);
            }
            // A class: those slots in a new object, counted once.
            if meta.is_class(s) {
                let p = alloc(ctx, super::values::object_size(meta, s))?;
                let mut b = 1i32.to_le_bytes().to_vec();
                b.extend([0; 4]);
                b.extend(out.iter().flat_map(|r| r.bytes()));
                ctx.write(p, &b);
                return Ok(smallvec::smallvec![Raw::I32(p as i32)]);
            }
            out
        }
        _ => Raws::from_vec(encode_plain(meta, ty, v)?),
    })
}

fn write_text(ctx: &mut dyn ImportCtx, out: i32, text: &str) -> Raws {
    ctx.write(out as u32 as usize, text.as_bytes());
    smallvec::smallvec![Raw::I32(text.len() as i32)]
}

/// Runs import `index` of the module with `args`, returning its results.
pub(crate) fn dispatch(
    state: &Shared,
    ctx: &mut dyn ImportCtx,
    index: usize,
    args: &[Raw],
) -> Result<Raws, String> {
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
        // `{:?}` keeps the `.0` on whole floats, and is the shortest form that reads back exactly.
        Kind::FmtF64 => Ok(write_text(ctx, args[1].i32(), &format!("{:?}", f64_at(0)))),
        Kind::AwaitI64 => Ok(smallvec::smallvec![Raw::I64(match resolved(&h) {
            Value::Int(x) | Value::Node(x) => x,
            _ => 0,
        })]),
        Kind::AwaitF64 => Ok(smallvec::smallvec![Raw::F64(match resolved(&h) {
            Value::Float(x) => x,
            Value::Int(x) => x as f64,
            _ => 0.0,
        })]),
        Kind::AwaitI32 => Ok(smallvec::smallvec![Raw::I32(i32::from(
            resolved(&h) == Value::Bool(true)
        ))]),
        Kind::Box | Kind::Unbox => {
            let into = matches!(h.kinds[index], Kind::Box);
            drop(h);
            convert(ctx, &meta, args, into)
        }
        Kind::Unary(f) => Ok(smallvec::smallvec![Raw::F64(f(f64_at(0)))]),
        Kind::Binary(f) => Ok(smallvec::smallvec![Raw::F64(f(f64_at(0), f64_at(1)))]),
        Kind::Probe => {
            drop(h);
            debugger::on_probe(state, ctx, args[0].i32() as u32);
            Ok(Raws::new())
        }
        Kind::Sync(at, sig) => {
            let (at, sig) = (*at, sig.clone());
            let values = args_of(&meta, ctx, &sig.params, args);
            let v = (h.sync[at])(&values);
            drop(h);
            encode_owned(ctx, &meta, &sig.ret, &v)
        }
        Kind::AwaitHeap => {
            let handle = i64_at(0);
            let v = resolved(&h);
            let ty = h.heap_awaits.remove(&handle).unwrap_or(Ty::Void);
            drop(h);
            encode_owned(ctx, &meta, &ty, &v)
        }
        Kind::Async(at, sig) => {
            let (at, sig) = (*at, sig.clone());
            let values = args_of(&meta, ctx, &sig.params, args);
            let (delay, v) = (h.asynch[at])(&values);
            h.next_handle += 1;
            let (handle, due) = (h.next_handle, h.now + delay);
            h.timers.insert(handle, (due, v));
            if sig.ret.is_heap() {
                h.heap_awaits.insert(handle, sig.ret.clone());
            }
            Ok(smallvec::smallvec![Raw::I64(handle)])
        }
        // The engine may run other scripts before it answers, so the state is
        // not held across the call.
        Kind::Engine(sig, bound) => {
            let (sig, bound) = (sig.clone(), bound.clone());
            let values = args_of(&meta, ctx, &sig.params, args);
            let outer = h.inside.replace(erase(ctx));
            drop(h);
            let v = bound(&values);
            state.borrow_mut().inside = outer;
            encode_owned(ctx, &meta, &sig.ret, &v?)
        }
        Kind::Any(name) => {
            let name = *name;
            drop(h);
            super::anys::run(ctx, &meta, name, args).map(Raws::from_vec)
        }
        Kind::Str(name, sig) => {
            let (name, sig) = (*name, sig.clone());
            drop(h);
            let values = args_of(&meta, ctx, &sig.params, args);
            let recv = match values.first() {
                Some(Value::Str(s)) => s.as_str(),
                _ => "",
            };
            let v = crate::strings::run(name, recv, &values[1..]);
            encode_owned(ctx, &meta, &sig.ret, &v)
        }
        // The engine answers with a token it wakes later; the script awaits a
        // handle of its own, so a token never collides with an extern's.
        Kind::EngineAsync(sig, bound) => {
            let (sig, bound) = (sig.clone(), bound.clone());
            let values = args_of(&meta, ctx, &sig.params, args);
            let outer = h.inside.replace(erase(ctx));
            drop(h);
            let out = engine_async(state, &bound, &sig.key, &values, sig.ret.clone());
            state.borrow_mut().inside = outer;
            out
        }
    }
}

fn engine_async(
    state: &Shared,
    bound: &Bound,
    key: &str,
    values: &[Value],
    ret: Ty,
) -> Result<Raws, String> {
    let token = bound(values);
    let mut h = state.borrow_mut();
    let Value::Int(token) = token? else {
        return Err(format!("`{key}` answered without a token"));
    };
    h.next_handle += 1;
    let handle = h.next_handle;
    if ret.is_heap() {
        h.heap_awaits.insert(handle, ret.clone());
    }
    h.tokens.insert(token.cast_unsigned(), (handle, ret));
    Ok(smallvec::smallvec![Raw::I64(handle)])
}

/// How a slot crosses a direct module's entry points: as an i64's bits.
pub(crate) fn to_bits(r: Raw) -> u64 {
    match r {
        Raw::I32(x) => u64::from(x.cast_unsigned()),
        Raw::I64(x) => x.cast_unsigned(),
        Raw::F64(x) => x.to_bits(),
    }
}

pub(crate) fn from_bits(s: crate::Slot, b: u64) -> Raw {
    match s {
        crate::Slot::I32 => Raw::I32(b as u32 as i32),
        crate::Slot::I64 => Raw::I64(b.cast_signed()),
        crate::Slot::F64 => Raw::F64(f64::from_bits(b)),
    }
}

/// The argument counts a direct module's entry points take, each once.
pub(crate) fn direct_arities(meta: &Meta) -> Vec<usize> {
    let mut n: Vec<usize> = meta.imports.iter().map(|i| i.params.len()).collect();
    n.sort_unstable();
    n.dedup();
    n
}

/// A direct module's call to import `index`, its arguments as the bits they
/// crossed as: the first result back, the rest kept for [`direct_result`].
pub(crate) fn direct_call(
    state: &Shared,
    ctx: &mut dyn ImportCtx,
    index: usize,
    bits: &[u64],
) -> Result<u64, String> {
    let meta = state.borrow().meta.clone();
    let import = meta
        .imports
        .get(index)
        .ok_or("a call to an import the module does not have")?;
    let args: super::engine::Args = import
        .params
        .iter()
        .zip(bits)
        .map(|(s, b)| from_bits(*s, *b))
        .collect();
    let mut out = dispatch(state, ctx, index, &args)?;
    let first = out.first().copied().map_or(0, to_bits);
    if out.len() > 1 {
        out.remove(0);
        state.borrow_mut().extra = out;
    }
    Ok(first)
}

/// Result `k` (1 or more) of a direct module's last call.
pub(crate) fn direct_result(state: &Shared, k: usize) -> u64 {
    state
        .borrow()
        .extra
        .get(k.wrapping_sub(1))
        .copied()
        .map_or(0, to_bits)
}

/// `ctx` with its lifetime forgotten, for [`HostState::inside`].
#[allow(
    clippy::transmute_ptr_to_ptr,
    reason = "a cast cannot extend a trait object's lifetime"
)]
fn erase(ctx: &mut dyn ImportCtx) -> *mut (dyn ImportCtx + 'static) {
    let p: *mut (dyn ImportCtx + '_) = ctx;
    // SAFETY: only the lifetime changes. `dispatch` clears the pointer before
    // it touches `ctx` again, and `Reentry` dereferences it only while the
    // engine call it was stored for is running, so it never outlives `ctx`.
    unsafe { std::mem::transmute::<*mut (dyn ImportCtx + '_), *mut (dyn ImportCtx + 'static)>(p) }
}
