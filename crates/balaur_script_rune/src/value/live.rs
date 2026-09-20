//! `node.call` and `node.script_field` between two Rune scripts. The values
//! cross as themselves, so a table one script hands another is the same
//! table, and a map keeps its int keys. A call the debugger has to step, or
//! a node with no Rune script, takes the engine's path instead.

use hecs::Entity;
use rune::TypeHash as _;
use rune::alloc::clone::TryClone as _;
use rune::runtime::{InstAddress, Memory, Output, VmResult};

use crate::RuneHost;

impl RuneHost {
    /// What `node.call(method, ..)` answers, or `None` for a call the
    /// engine's path has to make.
    pub(crate) fn call_live(
        &self,
        entity: Entity,
        method: &str,
        args: &[rune::Value],
    ) -> Option<rune::Value> {
        let (key, this, takes) = {
            let state = self.state.borrow();
            let instance = state.instances.get(&entity)?;
            let key = instance.key.clone();
            let stepped = state.break_on_error
                || state.break_next
                || state
                    .breakpoints
                    .get(&*key)
                    .is_some_and(|b| !b.ips.is_empty());
            if stepped || self.is_held(entity, &state) {
                return None;
            }
            let takes = state
                .scripts
                .get(&*key)
                .and_then(|script| script.functions.iter().find(|f| f.name == method))
                .map_or(usize::MAX, |declared| declared.arity);
            (key, instance.state.try_clone().ok()?, takes)
        };
        let Some(found) = self.resolve(&key, method) else {
            return Some(unit());
        };
        // The instance first, then as many arguments as the method takes.
        let mut call = vec![this];
        call.extend(args.iter().take(takes.saturating_sub(1)).cloned());
        match found.function.call::<rune::Value>(call).into_result() {
            Ok(value) if value.type_hash() == rune::runtime::Future::HASH => {
                self.settle_call(entity, &key, method, value, None);
                Some(unit())
            }
            Ok(value) => Some(value),
            Err(err) => {
                self.report(&key, method, &err);
                Some(unit())
            }
        }
    }

    /// What `node.script_field(name)` answers: the member itself.
    pub(crate) fn field_live(&self, entity: Entity, name: &str) -> Option<rune::Value> {
        let state = self.state.borrow();
        let instance = state.instances.get(&entity)?;
        let object = instance.state.borrow_ref::<rune::runtime::Object>().ok()?;
        Some(object.get(name).cloned().unwrap_or_else(unit))
    }
}

fn unit() -> rune::Value {
    rune::to_value(()).expect("unit always converts")
}

/// A node method answered by `live` when the node runs a Rune script, and by
/// `fallback`, the engine's own binding, when it does not.
pub(crate) fn live_or<F, L>(
    handle: usize,
    fallback: F,
    live: L,
) -> impl 'static + Fn(&mut dyn Memory, InstAddress, usize, Output) -> VmResult<()> + Send + Sync
where
    F: 'static + Fn(&mut dyn Memory, InstAddress, usize, Output) -> VmResult<()> + Send + Sync,
    L: 'static + Fn(&RuneHost, Entity, &[rune::Value]) -> Option<rune::Value> + Send + Sync,
{
    move |stack: &mut dyn Memory, addr: InstAddress, args: usize, out: Output| {
        let answered = {
            // Copied off: the callee runs on a VM of its own while this
            // stack waits.
            let values: Vec<rune::Value> = rune::vm_try!(stack.slice_at(addr, args)).to_vec();
            let entity = values
                .first()
                .and_then(|v| v.borrow_ref::<super::Node>().ok().map(|n| n.id))
                .and_then(|id| balaur_core::entity_of(balaur_script::NodeId(id)).ok());
            let host = crate::bindings::engine_of(handle).and_then(|e| e.script_host());
            let rune_host = host
                .as_ref()
                .and_then(|h| h.as_any().downcast_ref::<RuneHost>());
            match (entity, rune_host) {
                (Some(entity), Some(rune_host)) => live(rune_host, entity, &values[1..]),
                _ => None,
            }
        };
        match answered {
            Some(value) => {
                rune::vm_try!(out.store(stack, value));
                VmResult::Ok(())
            }
            None => fallback(stack, addr, args, out),
        }
    }
}
