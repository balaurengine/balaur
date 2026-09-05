//! Native functions, from the neutral `Bindings` trait into a Rune module.
//!
//! Rune requires handlers to be `Send + Sync`, but a binding closure captures
//! the engine, which is `Rc`-based. So the handler carries only an index and
//! looks the closure up in a thread-local: safe, no unsafe, and a call from
//! the wrong thread fails with a script error instead of being undefined.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use balaur_core::Engine;
use balaur_script::{BoundFn, CallbackId, Value};
use rune::alloc::clone::TryClone as _;
use rune::runtime::{InstAddress, Memory, Output, VmError, VmResult};

thread_local! {
    /// Registered bindings, indexed by the handle a Rune handler carries.
    static BOUND: RefCell<Vec<(Engine, BoundFn<Engine>)>> = const { RefCell::new(Vec::new()) };
    /// Script functions passed into a binding, live only for that call.
    static CALLBACKS: RefCell<Vec<(u64, rune::runtime::Function)>> =
        const { RefCell::new(Vec::new()) };
    static NEXT_CALLBACK: Cell<u64> = const { Cell::new(1) };
    /// Every function and constant declared through the seam, for the API
    /// dump: Rune's context cannot be walked from outside.
    static API: RefCell<Vec<ApiEntry>> = const { RefCell::new(Vec::new()) };
    /// What each described function does: `(module, name, acts_on, doc)`.
    static DOCS: RefCell<Vec<FnEntry>> = const { RefCell::new(Vec::new()) };
    /// `(module, doc)` for each module that described itself.
    static MODULE_DOCS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// `(module, function)` -> its `BOUND` handle, for component handles.
    static NAMED: RefCell<HashMap<(String, String), usize>> = RefCell::new(HashMap::new());
}

/// One function's reference entry, as its module declared it.
#[derive(Clone, Debug)]
pub(crate) struct FnEntry {
    pub(crate) module: String,
    pub(crate) name: String,
    /// Components the function reads or writes; empty when it acts on none.
    pub(crate) acts_on: Vec<String>,
    /// The signature its module spelled out, for a function registered raw
    /// and so having no Rust types to read. Empty when the typed seam
    /// recorded one instead.
    pub(crate) signature: String,
    pub(crate) doc: String,
}

/// One declared script-facing name.
#[derive(Clone, Debug)]
pub struct ApiEntry {
    pub module: String,
    pub name: String,
    /// The constant's value as text; `None` for a function.
    pub constant: Option<String>,
    /// `args -> returns` in Rust spelling, when the function was declared
    /// through the typed seam.
    pub signature: Option<String>,
}

pub(crate) fn api_entries() -> Vec<ApiEntry> {
    API.with_borrow(Clone::clone)
}

pub(crate) fn api_docs() -> Vec<FnEntry> {
    DOCS.with_borrow(Clone::clone)
}

pub(crate) fn api_module_docs() -> Vec<(String, String)> {
    MODULE_DOCS.with_borrow(Clone::clone)
}

pub(crate) fn bound_handle(module: &str, name: &str) -> Option<usize> {
    NAMED.with_borrow(|n| n.get(&(module.to_string(), name.to_string())).copied())
}

/// Call a bound function by handle; `None` when it lives on another thread.
pub(crate) fn call_bound(handle: usize, args: &[Value]) -> Option<anyhow::Result<Value>> {
    BOUND.with_borrow(|b| b.get(handle).map(|(engine, f)| f(engine, args)))
}

fn record(module: &str, name: &str, constant: Option<String>) {
    API.with_borrow_mut(|api| {
        api.push(ApiEntry {
            module: module.to_string(),
            name: name.to_string(),
            constant,
            signature: None,
        });
    });
}

/// Register a script function for the duration of one binding call.
pub(crate) fn hold_callback(f: rune::runtime::Function) -> CallbackId {
    let id = NEXT_CALLBACK.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    CALLBACKS.with_borrow_mut(|c| c.push((id, f)));
    CallbackId(id)
}

pub(crate) fn lookup_callback(id: CallbackId) -> Option<rune::runtime::Function> {
    CALLBACKS.with_borrow(|c| {
        c.iter()
            .find(|(held, _)| *held == id.0)
            .and_then(|(_, f)| f.try_clone().ok())
    })
}

/// Drops every callback registered since it was created.
///
/// A callback is valid only while the binding that received it is running, so
/// the registry is truncated on the way out. A script that stashes one and
/// calls it later gets an error rather than a dangling function.
pub(crate) struct CallbackScope {
    base: usize,
}

impl CallbackScope {
    pub(crate) fn enter() -> Self {
        Self {
            base: CALLBACKS.with_borrow(Vec::len),
        }
    }
}

impl Drop for CallbackScope {
    fn drop(&mut self) {
        CALLBACKS.with_borrow_mut(|c| c.truncate(self.base));
    }
}

/// A named group of bindings, becoming one Rune module.
///
/// Rune installs a module into its context whole, so unlike Lua there is
/// nowhere to write a function as it is registered. The module therefore joins
/// the host's pending list when it is dropped — which is the end of the
/// plugin's `build`, before any script runs.
pub struct RuneModule {
    module: Option<rune::Module>,
    name: String,
    engine: Engine,
    pending: Rc<RefCell<Vec<rune::Module>>>,
}

impl RuneModule {
    pub(crate) fn new(
        name: &str,
        engine: Engine,
        pending: Rc<RefCell<Vec<rune::Module>>>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            module: Some(rune::Module::with_crate(name)?),
            name: name.to_string(),
            engine,
            pending,
        })
    }
}

impl Drop for RuneModule {
    fn drop(&mut self) {
        if let Some(m) = self.module.take() {
            self.pending.borrow_mut().push(m);
        }
    }
}

impl balaur_script::Bindings<Engine> for RuneModule {
    fn function_raw(&mut self, name: &str, f: BoundFn<Engine>) {
        record(&self.name, name, None);
        let handle = BOUND.with_borrow_mut(|b| {
            b.push((self.engine.clone(), f));
            b.len() - 1
        });
        NAMED.with_borrow_mut(|n| {
            n.insert((self.name.clone(), name.to_string()), handle);
        });
        let Some(module) = self.module.as_mut() else {
            return;
        };
        let registered = module.raw_function(
            name,
            bound_handler(handle, "binding was registered on another thread"),
        );
        if let Err(err) = registered.build() {
            tracing::error!("binding {}::{name}: {err}", self.name);
        }
    }

    fn signature(&mut self, name: &str, args: &str, returns: &str) {
        API.with_borrow_mut(|api| {
            let entry = api
                .iter_mut()
                .rev()
                .find(|e| e.module == self.name && e.name == name && e.constant.is_none());
            if let Some(entry) = entry {
                entry.signature = Some(format!("{args} -> {returns}"));
            }
        });
    }

    fn module_doc(&mut self, doc: &'static str) {
        MODULE_DOCS.with_borrow_mut(|d| d.push((self.name.clone(), doc.to_string())));
    }

    fn describe(&mut self, entries: &[balaur_script::FnDoc]) {
        DOCS.with_borrow_mut(|docs| {
            docs.extend(
                entries
                    .iter()
                    .map(|(name, acts_on, signature, doc)| FnEntry {
                        module: self.name.clone(),
                        name: (*name).to_string(),
                        acts_on: acts_on.iter().map(|c| (*c).to_string()).collect(),
                        signature: (*signature).to_string(),
                        doc: (*doc).to_string(),
                    }),
            );
        });
    }

    fn constant(&mut self, name: &str, value: Value) {
        let shown = match &value {
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Num(n) => n.to_string(),
            Value::Str(s) => s.clone(),
            other => format!("{other:?}"),
        };
        record(&self.name, name, Some(shown));
        let Some(module) = self.module.as_mut() else {
            return;
        };
        let built = match &value {
            Value::Bool(b) => module.constant(name, *b).build().map(|_| ()),
            Value::Int(i) => module.constant(name, *i).build().map(|_| ()),
            Value::Num(n) => module.constant(name, *n).build().map(|_| ()),
            Value::Str(s) => module.constant(name, s.as_str()).build().map(|_| ()),
            other => {
                tracing::error!(
                    "constant {}::{name}: {other:?} is not a constant Rune type",
                    self.name
                );
                return;
            }
        };
        if let Err(err) = built {
            tracing::error!("constant {}::{name}: {err}", self.name);
        }
    }
}

/// Register a node operation so a Rune handler can reach it by index.
///
/// Same reason as `function_raw`: Rune wants `Send + Sync`, the engine is
/// `Rc`-based, so only an index crosses into the handler.
pub(crate) fn hold_node_fn(
    engine: Engine,
    call: fn(&Engine, &[Value]) -> anyhow::Result<Value>,
) -> usize {
    BOUND.with_borrow_mut(|b| {
        b.push((engine, Box::new(call)));
        b.len() - 1
    })
}

/// The handler body shared by every binding and every node method: the
/// arguments cross into neutral values, the bound Rust runs, and its answer
/// crosses back. `orphaned` is the panic for a handle this thread never
/// registered.
pub(crate) fn bound_handler(
    handle: usize,
    orphaned: &'static str,
) -> impl 'static + Fn(&mut dyn Memory, InstAddress, usize, Output) -> VmResult<()> + Send + Sync {
    move |stack: &mut dyn Memory, addr: InstAddress, args: usize, out: Output| {
        let values = rune::vm_try!(stack.slice_at(addr, args)).to_vec();
        let _scope = CallbackScope::enter();
        let mut neutral = Vec::with_capacity(values.len());
        for v in &values {
            match crate::value::to_neutral(v) {
                Ok(n) => neutral.push(n),
                Err(err) => return VmResult::Err(VmError::panic(err.to_string())),
            }
        }
        let called = BOUND.with_borrow(|b| b.get(handle).map(|(engine, f)| f(engine, &neutral)));
        let result = match called {
            Some(Ok(v)) => v,
            Some(Err(err)) => return VmResult::Err(VmError::panic(err.to_string())),
            None => return VmResult::Err(VmError::panic(orphaned)),
        };
        match crate::value::from_neutral(&result) {
            Ok(v) => rune::vm_try!(out.store(stack, v)),
            Err(err) => return VmResult::Err(VmError::panic(err.to_string())),
        }
        VmResult::Ok(())
    }
}
