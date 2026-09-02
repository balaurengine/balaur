//! A script host that records what the engine called, and nothing else.
//!
//! Method tracks and the finished signal go out through
//! `ScriptHost::call_on`, so a test that wants to see one either runs a
//! language or stands in for it. Standing in for it is what makes the
//! assertions read as "the engine called `on_landed` on this node once",
//! with no Rune anywhere in the way — the real backend is exercised
//! separately in `tests/script.rs`.

#![allow(
    dead_code,
    reason = "each test binary compiles this module and uses part of it"
)]

use balaur_core::hecs::Entity;
use balaur_script::{NodeId, ScriptHost, Value};

/// Every `(node, method, args)` the engine has called, in order.
#[derive(Default)]
pub(crate) struct Calls(std::cell::RefCell<Vec<(u64, String, Vec<Value>)>>);

impl Calls {
    /// How many times `method` was called on `node`.
    pub(crate) fn count(&self, node: Entity, method: &str) -> usize {
        let id = balaur_core::node_id_of(node).0;
        self.0
            .borrow()
            .iter()
            .filter(|(n, m, _)| *n == id && m == method)
            .count()
    }

    /// The arguments the engine passed with the first call of `method` on
    /// `node`. `None` when it was never called.
    pub(crate) fn args(&self, node: Entity, method: &str) -> Option<Vec<Value>> {
        let id = balaur_core::node_id_of(node).0;
        self.0
            .borrow()
            .iter()
            .find(|(n, m, _)| *n == id && m == method)
            .map(|(_, _, args)| args.clone())
    }

    /// The methods called on `node`, in the order they went out.
    pub(crate) fn order(&self, node: Entity) -> Vec<String> {
        let id = balaur_core::node_id_of(node).0;
        self.0
            .borrow()
            .iter()
            .filter(|(n, _, _)| *n == id)
            .map(|(_, m, _)| m.clone())
            .collect()
    }
}

impl ScriptHost<balaur_core::Engine> for Calls {
    fn module(
        &self,
        _: &str,
    ) -> anyhow::Result<Box<dyn balaur_script::Bindings<balaur_core::Engine>>> {
        Ok(Box::new(balaur_script::NoBindings))
    }
    fn attach_with_props(&self, _: NodeId, _: &str, _: &[(String, Value)]) -> anyhow::Result<()> {
        Ok(())
    }
    fn detach(&self, _: NodeId) {}
    fn update(&self, _: f32) {}
    fn fixed_update(&self, _: f32) {}
    fn save_state(&self) -> Vec<(NodeId, balaur_script::Value)> {
        Vec::new()
    }
    fn load_state(&self, _: &[(NodeId, balaur_script::Value)]) {}
    fn pump_reloads(&self) {}
    fn reload(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn call_on(&self, node: NodeId, method: &str, args: &[Value]) -> Option<Value> {
        self.0
            .borrow_mut()
            .push((node.0, method.to_string(), args.to_vec()));
        None
    }
    fn call_all(&self, _: &str) {}
    fn wake(&self, _: u64, _: &Value) {}
    fn scene_source(&self, _: &str) -> Option<String> {
        None
    }
    fn instance_count(&self) -> usize {
        0
    }
    fn invoke(
        &self,
        _: balaur_script::CallbackId,
        _: &[balaur_script::Value],
    ) -> anyhow::Result<balaur_script::Value> {
        Ok(balaur_script::Value::Nil)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
