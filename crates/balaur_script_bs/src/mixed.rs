// Two backends as one host: a script goes to the one its extension names, a
// node's calls to the one that runs it, and frame-wide calls to both.

use crate::host::BsHost;
use anyhow::Result;
use balaur_core::Engine;
use balaur_script::{
    Bindings, BoundFn, CallbackId, FnDoc, NodeId, OwnedFn, Pause, RecordDoc, ScriptHost, StepMode,
    Value,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

pub struct Mixed {
    primary: Rc<dyn ScriptHost<Engine>>,
    lang: BsHost,
    ours: RefCell<HashSet<u64>>,
}

impl Mixed {
    pub fn new(primary: Rc<dyn ScriptHost<Engine>>, lang: BsHost) -> Self {
        Mixed {
            primary,
            lang,
            ours: RefCell::default(),
        }
    }
    pub fn lang(&self) -> &BsHost {
        &self.lang
    }
    fn is_ours(&self, node: NodeId) -> bool {
        self.ours.borrow().contains(&node.0)
    }
    fn of(&self, node: NodeId) -> &dyn ScriptHost<Engine> {
        if self.is_ours(node) {
            &self.lang
        } else {
            &*self.primary
        }
    }
    fn for_path(&self, path: &str) -> &dyn ScriptHost<Engine> {
        if BsHost::claims(path) {
            &self.lang
        } else {
            &*self.primary
        }
    }
}

/// A binding registered once and heard by both backends.
struct Both {
    a: Box<dyn Bindings<Engine>>,
    b: Box<dyn Bindings<Engine>>,
}

impl Bindings<Engine> for Both {
    fn function_raw(&mut self, name: &str, f: BoundFn<Engine>) {
        let f = Rc::new(f);
        let g = f.clone();
        self.a.function_raw(name, Box::new(move |c, a| f(c, a)));
        self.b.function_raw(name, Box::new(move |c, a| g(c, a)));
    }
    fn function_owned(&mut self, name: &str, f: OwnedFn<Engine>) {
        let f = Rc::new(f);
        let g = f.clone();
        self.a.function_owned(name, Box::new(move |c, a| f(c, a)));
        self.b.function_owned(name, Box::new(move |c, a| g(c, a)));
    }
    fn constant(&mut self, name: &str, value: Value) {
        self.a.constant(name, value.clone());
        self.b.constant(name, value);
    }
    fn signature(&mut self, name: &str, args: &str, returns: &str) {
        self.a.signature(name, args, returns);
        self.b.signature(name, args, returns);
    }
    fn module_doc(&mut self, doc: &'static str) {
        self.a.module_doc(doc);
        self.b.module_doc(doc);
    }
    fn describe(&mut self, entries: &[FnDoc]) {
        self.a.describe(entries);
        self.b.describe(entries);
    }
    fn records(&mut self, entries: &[RecordDoc]) {
        self.a.records(entries);
        self.b.records(entries);
    }
}

impl ScriptHost<Engine> for Mixed {
    fn module(&self, name: &str) -> Result<Box<dyn Bindings<Engine>>> {
        Ok(Box::new(Both {
            a: self.primary.module(name)?,
            b: self.lang.module(name)?,
        }))
    }

    fn attach_with_props(&self, node: NodeId, path: &str, props: &[(String, Value)]) -> Result<()> {
        self.detach(node);
        if BsHost::claims(path) {
            self.lang.attach_with_props(node, path, props)?;
            self.ours.borrow_mut().insert(node.0);
            Ok(())
        } else {
            self.primary.attach_with_props(node, path, props)
        }
    }

    fn hold_inits(&self) {
        self.primary.hold_inits();
        self.lang.hold_inits();
    }

    fn release_inits(&self) {
        self.primary.release_inits();
        self.lang.release_inits();
    }

    fn exports(&self, path: &str) -> Result<Vec<(String, Value)>> {
        self.for_path(path).exports(path)
    }

    fn detach(&self, node: NodeId) {
        if self.ours.borrow_mut().remove(&node.0) {
            self.lang.detach(node);
        } else {
            self.primary.detach(node);
        }
    }

    fn detach_all(&self, nodes: &[NodeId]) {
        let (ours, theirs): (Vec<NodeId>, Vec<NodeId>) =
            nodes.iter().partition(|n| self.is_ours(**n));
        self.primary.detach_all(&theirs);
        for n in ours {
            self.detach(n);
        }
    }

    fn update(&self, dt: f32) {
        self.primary.update(dt);
        self.lang.update(dt);
    }

    fn fixed_update(&self, dt: f32) {
        self.primary.fixed_update(dt);
        self.lang.fixed_update(dt);
    }

    fn save_state(&self) -> Vec<(NodeId, Value)> {
        let mut out = self.primary.save_state();
        out.extend(self.lang.save_state());
        out
    }

    fn load_state(&self, states: &[(NodeId, Value)]) {
        let (ours, theirs): (Vec<_>, Vec<_>) =
            states.iter().cloned().partition(|(n, _)| self.is_ours(*n));
        self.primary.load_state(&theirs);
        self.lang.load_state(&ours);
    }

    fn pump_reloads(&self) {
        self.primary.pump_reloads();
        self.lang.pump_reloads();
    }

    fn reload(&self, key: &str) -> Result<()> {
        self.for_path(key).reload(key)
    }

    fn call_on(&self, node: NodeId, method: &str, args: &[Value]) -> Option<Value> {
        self.of(node).call_on(node, method, args)
    }

    fn call_on_async(
        &self,
        node: NodeId,
        method: &str,
        args: &[Value],
        done: u64,
    ) -> Option<Value> {
        self.of(node).call_on_async(node, method, args, done)
    }

    fn has_method(&self, node: NodeId, method: &str) -> bool {
        self.of(node).has_method(node, method)
    }

    fn script_field(&self, node: NodeId, name: &str) -> Option<Value> {
        self.of(node).script_field(node, name)
    }

    fn call_in(&self, path: &str, function: &str, args: &[Value]) -> Result<Option<Value>> {
        self.for_path(path).call_in(path, function, args)
    }

    fn call_all(&self, method: &str) {
        self.primary.call_all(method);
        self.lang.call_all(method);
    }

    fn call_all_with(&self, method: &str, args: &[Value]) {
        self.primary.call_all_with(method, args);
        self.lang.call_all_with(method, args);
    }

    fn announce(&self, method: &str, args: &[Value]) {
        self.primary.announce(method, args);
        self.lang.announce(method, args);
    }

    fn wake(&self, token: u64, payload: &Value) {
        self.primary.wake(token, payload);
        self.lang.wake(token, payload);
    }

    fn scene_source(&self, rel: &str) -> Option<String> {
        self.primary.scene_source(rel)
    }

    fn instance_count(&self) -> usize {
        self.primary.instance_count() + self.lang.instance_count()
    }

    fn invoke(&self, callback: CallbackId, args: &[Value]) -> Result<Value> {
        self.primary.invoke(callback, args)
    }

    fn keep(&self, callback: CallbackId) -> Result<()> {
        self.primary.keep(callback)
    }

    fn release(&self, callback: CallbackId) {
        self.primary.release(callback);
    }

    fn set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        self.for_path(path).set_breakpoints(path, lines)
    }

    fn breakpoints(&self, path: &str) -> Vec<usize> {
        self.for_path(path).breakpoints(path)
    }

    fn set_profiling(&self, on: bool) {
        self.primary.set_profiling(on);
        self.lang.set_profiling(on);
    }

    fn script_costs(&self) -> Vec<(String, u64, u64)> {
        dearest(self.primary.script_costs(), self.lang.script_costs())
    }

    fn function_costs(&self) -> Vec<(String, u64, u64)> {
        dearest(self.primary.function_costs(), self.lang.function_costs())
    }

    fn set_break_on_error(&self, on: bool) {
        self.primary.set_break_on_error(on);
        self.lang.set_break_on_error(on);
    }

    fn break_on_error(&self) -> bool {
        self.primary.break_on_error() || self.lang.break_on_error()
    }

    fn request_break(&self) {
        self.primary.request_break();
        self.lang.request_break();
    }

    // One pause at a time: the game is frozen while either holds one.
    fn paused(&self) -> Option<Pause> {
        self.lang.paused().or_else(|| self.primary.paused())
    }

    fn resume(&self, mode: StepMode) {
        if self.lang.paused().is_some() {
            self.lang.resume(mode);
        } else {
            self.primary.resume(mode);
        }
    }

    fn tools_for(&self, path: &str) -> Option<&dyn balaur_script::LanguageTools> {
        self.lang.tools_for(path)
    }

    // Code written against the primary backend on purpose downcasts through
    // here, and keeps finding it.
    fn as_any(&self) -> &dyn core::any::Any {
        self.primary.as_any()
    }
}

// Both backends' costs in one list, dearest first. Rune counts instructions
// and Balaur Script statements, so the two are read apart.
fn dearest(mut a: Vec<(String, u64, u64)>, b: Vec<(String, u64, u64)>) -> Vec<(String, u64, u64)> {
    a.extend(b);
    a.sort_by(|x, y| y.2.cmp(&x.2).then_with(|| x.0.cmp(&y.0)));
    a
}
