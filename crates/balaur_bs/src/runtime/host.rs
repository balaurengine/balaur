// Every import a module makes, answered in one place whichever engine runs it:
// the script's `extern fn`s, the engine's functions, and the built-ins.

use super::args::{CallArgs, Slots};
use super::debugger::{self, Debugger};
use super::engine::{ImportCtx, Raw, Raws, Shared};
use super::values::{Reader, Value, encode_plain, width};
use crate::{Meta, Ty, words};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub(crate) type SyncFn = Box<dyn FnMut(&[Value]) -> Value>;
pub(crate) type AsyncFn = Box<dyn FnMut(&[Value]) -> (f64, Value)>;
/// One engine function, bound to what it calls when the module is made, so a
/// call parses no key and looks nothing up. Shared, since the engine may call
/// back into the same file while it answers.
pub type Bound = Rc<dyn Fn(&dyn CallArgs) -> Result<Value, String>>;
/// Binds an engine function by its key and the type it must return.
pub(crate) type Binder = Rc<dyn Fn(&str, &Ty) -> Result<Bound, String>>;
/// Where a node's script object lives, when it runs class `class` of file
/// `file` in this instance: its pointer, which the host counts once more for
/// the script that asked.
pub(crate) type ScriptOf = Rc<FindScript>;
pub(crate) type FindScript = dyn Fn(i64, &str, &str) -> Option<i32>;

/// The host functions a script may call: its own `extern fn`s by name, and the
/// engine's through one binder.
#[derive(Default)]
pub struct HostFns {
    pub(crate) sync: HashMap<String, SyncFn>,
    pub(crate) asynch: HashMap<String, AsyncFn>,
    pub(crate) binder: Option<Binder>,
    pub(crate) script_of: Option<ScriptOf>,
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
            Ok(Rc::new(move |args: &dyn CallArgs| {
                let values: Vec<Value> = (0..args.len()).map(|index| args.value(index)).collect();
                f(&key, &values, &ret)
            }) as Bound)
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
    /// What `node.script` finds: a node's script object, by the node, the
    /// file and its class; nil without this.
    #[must_use]
    pub fn scripts(mut self, find: impl Fn(i64, &str, &str) -> Option<i32> + 'static) -> Self {
        self.script_of = Some(Rc::new(find));
        self
    }
}

/// An import's key, parameters and result, shared so a call clones nothing,
/// and where each parameter's slots start.
struct Sig {
    key: String,
    params: Vec<Ty>,
    ret: Ty,
    at: Vec<usize>,
}

fn sig(meta: &Meta, key: &str, params: &[Ty], ret: &Ty) -> Rc<Sig> {
    Rc::new(Sig {
        key: key.to_string(),
        params: params.to_vec(),
        ret: ret.clone(),
        at: super::args::offsets(meta, params),
    })
}

enum Kind {
    Sync(usize, Rc<Sig>),
    Async(usize, Rc<Sig>),
    Engine(Rc<Sig>, Bound),
    EngineAsync(Rc<Sig>, Bound),
    FmtF64,
    ParseF64,
    ScriptOf,
    AwaitI64,
    AwaitF64,
    AwaitI32,
    AwaitHeap,
    Unary(fn(f64) -> f64),
    Binary(fn(f64, f64) -> f64),
    Probe,
}

pub(crate) struct HostState {
    pub meta: Arc<Meta>,
    kinds: Vec<Kind>,
    /// A linked script's other units, numbered from 1 in the order they were
    /// made: each one's own types, and what its imports run.
    units: Vec<(Arc<crate::Compiled>, Vec<Kind>)>,
    /// What a unit made later binds its imports from.
    fns: HostFns,
    /// Each `extern fn` taken out of `fns` so far, by name, and where it went
    /// in `sync` or `asynch`: two units calling one share it.
    bound: HashMap<String, usize>,
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
    /// Calls out of the module so far, for a budget to count.
    pub calls: u64,
    /// Which slots of each script struct hold a class, worked out once, or
    /// nil for one that holds a string, list or `any` outside a class and so
    /// cannot be shared ([`super::Runtime::share`]).
    /// By unit, since two scripts may each name a type `Spinner`.
    pub(crate) shares: Vec<rustc_hash::FxHashMap<String, Option<Rc<[usize]>>>>,
    /// What letting go of each script struct releases ([`Release`]), by unit.
    pub(crate) releases: Vec<rustc_hash::FxHashMap<String, Rc<[Release]>>>,
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
    pub(crate) fn new(meta: Arc<Meta>, fns: HostFns) -> Result<Self, String> {
        let debugger = Debugger::new(meta.clone());
        let mut state = HostState {
            meta,
            kinds: vec![],
            units: vec![],
            fns,
            bound: HashMap::new(),
            sync: vec![],
            asynch: vec![],
            timers: HashMap::new(),
            resolved: HashMap::new(),
            tokens: HashMap::new(),
            heap_awaits: HashMap::new(),
            next_handle: 0,
            inside: None,
            extra: Raws::new(),
            calls: 0,
            shares: vec![],
            releases: vec![],
            now: 0.0,
            debugger,
        };
        let own = state.meta.clone();
        state.kinds = state.kinds_of(&own)?;
        Ok(state)
    }

    // The types unit `unit` reads and answers calls by: 0 is the script's.
    fn types(&self, unit: usize) -> Types {
        match unit {
            0 => Types::Own(self.meta.clone()),
            u => Types::Unit(self.units[u - 1].0.clone()),
        }
    }

    fn kinds(&self, unit: usize) -> &[Kind] {
        match unit {
            0 => &self.kinds,
            u => &self.units[u - 1].1,
        }
    }

    /// Another unit of a linked script, its imports bound: the number its
    /// calls are dispatched by.
    pub(crate) fn bind_unit(&mut self, unit: Arc<crate::Compiled>) -> Result<usize, String> {
        let kinds = self.kinds_of(&unit.meta)?;
        self.units.push((unit, kinds));
        Ok(self.units.len())
    }

    // What each of `meta`'s imports runs.
    fn kinds_of(&mut self, meta: &Meta) -> Result<Vec<Kind>, String> {
        let mut kinds = vec![];
        for imp in &meta.imports {
            let name = imp.name.as_str();
            let missing = || format!("the script needs host function `{name}`");
            let kind = if let Some(x) = meta.externs.iter().find(|x| x.name == name) {
                let at = match self.bound.get(name) {
                    Some(at) => *at,
                    None if x.is_async => {
                        let f = self.fns.asynch.remove(name).ok_or_else(missing)?;
                        self.asynch.push(f);
                        self.asynch.len() - 1
                    }
                    None => {
                        let f = self.fns.sync.remove(name).ok_or_else(missing)?;
                        self.sync.push(f);
                        self.sync.len() - 1
                    }
                };
                self.bound.insert(name.to_string(), at);
                if x.is_async {
                    Kind::Async(at, sig(meta, name, &x.params, &x.ret))
                } else {
                    Kind::Sync(at, sig(meta, name, &x.params, &x.ret))
                }
            } else if let Some(f) = meta.host_calls.iter().find(|f| f.key == name) {
                let Some(bind) = &self.fns.binder else {
                    return Err(format!(
                        "the script calls the engine's `{name}`, and this host has no engine"
                    ));
                };
                // An async one answers with the token it is woken by.
                let signature = sig(meta, &f.key, &f.params, &f.ret);
                if f.is_async {
                    Kind::EngineAsync(signature, bind(&f.key, &Ty::Int)?)
                } else {
                    Kind::Engine(signature, bind(&f.key, &f.ret)?)
                }
            } else if let Some(m) = name.strip_prefix(words::MATH_PREFIX) {
                math(m).ok_or_else(missing)?
            } else {
                match name {
                    words::FMT_F64 => Kind::FmtF64,
                    words::PARSE_F64 => Kind::ParseF64,
                    words::SCRIPT_OF => Kind::ScriptOf,
                    words::AWAIT_I64 => Kind::AwaitI64,
                    words::AWAIT_F64 => Kind::AwaitF64,
                    words::AWAIT_I32 => Kind::AwaitI32,
                    words::AWAIT_HEAP => Kind::AwaitHeap,
                    words::DBG_PROBE => Kind::Probe,
                    _ => return Err(missing()),
                }
            };
            kinds.push(kind);
        }
        Ok(kinds)
    }
}

pub(crate) struct CtxReader<'a>(pub &'a dyn ImportCtx);

impl Reader for CtxReader<'_> {
    fn bytes(&self, at: usize, len: usize) -> Vec<u8> {
        self.0.read(at, len)
    }
}

/// A host call's arguments, kept inline: a call allocates nothing for them.
// An `extern fn`'s arguments, each read whole.
fn values_of(meta: &Meta, ctx: &dyn ImportCtx, sig: &Sig, raw: &[Raw]) -> Vec<Value> {
    Slots::read(meta, ctx, &sig.params, &sig.at, raw).into_values()
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
            let p = ctx.alloc(8 + s.len())?;
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
                ctx.alloc(items.len() * esz)?
            };
            for (i, it) in items.iter().enumerate() {
                let b: Vec<u8> = encode_owned(ctx, meta, et, it)?
                    .into_iter()
                    .flat_map(Raw::bytes)
                    .collect();
                ctx.write(data + i * esz, &b);
            }
            let h = ctx.alloc(16)?;
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
            let p = ctx.alloc(crate::anys::CELL as usize)?;
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
                let p = ctx.alloc(super::values::object_size(meta, s))?;
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

/// The types a call is read and answered by: the script's own, or a unit's.
enum Types {
    Own(Arc<Meta>),
    Unit(Arc<crate::Compiled>),
}

impl std::ops::Deref for Types {
    type Target = Meta;
    fn deref(&self) -> &Meta {
        match self {
            Types::Own(m) => m,
            Types::Unit(c) => &c.meta,
        }
    }
}

// `node.script` as a class: the node's object, counted once more for the
// script that asked, when it runs class `args[2]` of file `args[1]` in this
// instance; else 0.
fn script_of(ctx: &mut dyn ImportCtx, find: Option<&FindScript>, node: i64, args: &[Raw]) -> Raws {
    let text = |ctx: &mut dyn ImportCtx, at: i32| match super::values::read_str(
        &CtxReader(ctx),
        at as u32 as usize,
    ) {
        Value::Str(text) => text,
        _ => String::new(),
    };
    let (file, class) = (text(ctx, args[1].i32()), text(ctx, args[2].i32()));
    let pointer = find.and_then(|find| find(node, &file, &class)).unwrap_or(0);
    if pointer != 0 {
        let count = super::values::rd32(&CtxReader(ctx), pointer as u32 as usize);
        ctx.write(pointer as u32 as usize, &(count + 1).to_le_bytes());
    }
    smallvec::smallvec![Raw::I32(pointer)]
}

/// Runs import `index` of the module with `args`, returning its results;
/// `unit` is a linked script's unit making the call, 0 for its own file.
pub(crate) fn dispatch(
    state: &Shared,
    ctx: &mut dyn ImportCtx,
    unit: usize,
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
    h.calls += 1;
    let meta = h.types(unit);
    let resolved = |h: &HostState| h.resolved.get(&i64_at(0)).cloned().unwrap_or(Value::Void);
    let kind = &h.kinds(unit)[index];
    match kind {
        // `{:?}` keeps the `.0` on whole floats, and is the shortest form that reads back exactly.
        Kind::FmtF64 => Ok(write_text(ctx, args[1].i32(), &format!("{:?}", f64_at(0)))),
        // A number that does not parse is 0.
        Kind::ParseF64 => {
            let text = match super::values::read_str(&CtxReader(ctx), args[0].i32() as u32 as usize)
            {
                Value::Str(text) => text,
                _ => String::new(),
            };
            Ok(smallvec::smallvec![Raw::F64(text.parse().unwrap_or(0.0))])
        }
        Kind::ScriptOf => {
            let find = h.fns.script_of.clone();
            drop(h);
            Ok(script_of(ctx, find.as_deref(), i64_at(0), args))
        }
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
        Kind::Unary(f) => Ok(smallvec::smallvec![Raw::F64(f(f64_at(0)))]),
        Kind::Binary(f) => Ok(smallvec::smallvec![Raw::F64(f(f64_at(0), f64_at(1)))]),
        Kind::Probe => {
            drop(h);
            debugger::on_probe(state, ctx, args[0].i32() as u32);
            Ok(Raws::new())
        }
        Kind::Sync(at, sig) => {
            let (at, sig) = (*at, sig.clone());
            let values = values_of(&meta, ctx, &sig, args);
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
            let values = values_of(&meta, ctx, &sig, args);
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
            let slots = Slots::read(&meta, ctx, &sig.params, &sig.at, args);
            let outer = h.inside.replace(erase(ctx));
            drop(h);
            let answer = bound(&slots);
            state.borrow_mut().inside = outer;
            encode_owned(ctx, &meta, &sig.ret, &answer?)
        }
        // The engine answers with a token it wakes later; the script awaits a
        // handle of its own, so a token never collides with an extern's.
        Kind::EngineAsync(sig, bound) => {
            let (sig, bound) = (sig.clone(), bound.clone());
            let slots = Slots::read(&meta, ctx, &sig.params, &sig.at, args);
            let outer = h.inside.replace(erase(ctx));
            drop(h);
            let out = engine_async(state, &bound, &sig.key, &slots, sig.ret.clone());
            state.borrow_mut().inside = outer;
            out
        }
    }
}

fn engine_async(
    state: &Shared,
    bound: &Bound,
    key: &str,
    args: &dyn CallArgs,
    ret: Ty,
) -> Result<Raws, String> {
    let token = bound(args);
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
        crate::Slot::F64 => Raw::F64(super::values::float(b)),
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
    let mut out = dispatch(state, ctx, 0, index, &args)?;
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
