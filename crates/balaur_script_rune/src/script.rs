//! One compiled script file, and one node's instance of it.
//!
//! Split from `lib.rs` because that file is the host and this is what the
//! host keeps per script: the unit, the methods resolved out of it, and the
//! VMs it lends a tick.

use std::rc::Rc;
use std::sync::Arc;

use rune::runtime::{Function, Vm};
use rune::{Sources, Unit};
use rustc_hash::FxHashMap;

use crate::debugger;
use crate::inspect::{PublicSignature, public_functions};

/// A resolved script function.
#[derive(Clone)]
pub(crate) struct Method {
    /// Kept for the paths that hand a callable back to Rune. Behind an `Rc`
    /// because `Function` is not `Clone` and this is cached, not consumed.
    pub(crate) function: Rc<Function>,
    /// Precomputed, for the profiled path that calls a VM by hash.
    pub(crate) hash: rune::Hash,
    /// Whether the function runs to completion on the VM that called it. An
    /// async, generator or stream function's return value holds on to its VM,
    /// so it cannot use a pooled one.
    pub(crate) immediate: bool,
}

pub(crate) struct Script {
    /// The one `Rc` every instance of this file holds, so a tick can tell two
    /// instances of the same script apart by pointer rather than by string.
    pub(crate) key: Rc<str>,
    pub(crate) unit: Arc<Unit>,
    pub(crate) source: String,
    /// The sources the unit was compiled from, kept so a runtime error can be
    /// rendered against them. `VmError` carries instruction pointers; only
    /// the sources turn those into a file, a line and a caret. `None` for a
    /// packed script, which ships without them.
    pub(crate) sources: Option<Rc<Sources>>,
    /// Resolved lifecycle and signal handlers. A miss is cached too: most
    /// scripts define none of `on_free`, and asking every frame is not free.
    pub(crate) methods: FxHashMap<String, Option<Method>>,
    /// VMs to reuse: a tick borrows one per script and hands it back at the
    /// end, which keeps `Function::call`'s per-call `Vm::new` off every node.
    pub(crate) vms: Vec<Vm>,
    pub(crate) lines: Rc<debugger::Lines>,
    pub(crate) functions: Vec<PublicSignature>,
    /// Every file the unit was compiled from, as watcher keys, this one
    /// included: a `mod` submodule is folded in here and is a key nowhere
    /// else, so a save of one has to be mapped back to this root.
    pub(crate) deps: Vec<String>,
    /// `exports()` evaluated once, since it is the same table for every node
    /// running this file. The failure is cached too — a broken `exports` that
    /// re-ran per attach would fail once per node. A reload replaces the whole
    /// `Script`, so a changed default reaches the next attach without an
    /// invalidation step.
    pub(crate) exports: Option<Result<Vec<(String, balaur_script::Value)>, String>>,
}

impl Script {
    pub(crate) fn new(
        key: Rc<str>,
        unit: Arc<Unit>,
        source: String,
        sources: Sources,
        deps: Vec<String>,
    ) -> Self {
        let lines = Rc::new(debugger::Lines::of(&unit, &source));
        let functions = public_functions(&source);
        Self {
            key,
            unit,
            source,
            sources: Some(Rc::new(sources)),
            methods: FxHashMap::default(),
            vms: Vec::new(),
            lines,
            functions,
            deps,
            exports: None,
        }
    }

    /// A script read back from a pack: the unit is already built, and there is
    /// no source behind it to render a span against or to hot reload from.
    pub(crate) fn compiled(key: Rc<str>, unit: Arc<Unit>, functions: Vec<PublicSignature>) -> Self {
        let lines = Rc::new(debugger::Lines::of(&unit, ""));
        Self {
            key,
            unit,
            source: String::new(),
            sources: None,
            methods: FxHashMap::default(),
            vms: Vec::new(),
            lines,
            functions,
            deps: Vec::new(),
            exports: None,
        }
    }
}

pub(crate) struct Instance {
    /// Shared with every other record naming this script: a tick collects one
    /// key per instance per frame, and a `String` there is an allocation per
    /// node per frame for a name that never changes.
    pub(crate) key: Rc<str>,
    pub(crate) state: rune::Value,
}
