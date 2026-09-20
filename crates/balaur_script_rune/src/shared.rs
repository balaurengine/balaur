//! Calling one unit's function from inside another unit's VM.
//!
//! `script::require`'s exports are native trampolines holding only a slot in
//! the table here, so a reload that refills the slot reaches every caller,
//! including one that kept a function out of the module.

use std::cell::RefCell;

use rune::alloc::clone::TryClone as _;
use rune::runtime::{Function, InstAddress, Memory, Output, VmError, VmResult};

thread_local! {
    /// The functions behind `script::require`'s exports, by the slot a
    /// trampoline carries.
    pub(crate) static SHARED_FNS: RefCell<Vec<Function>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn trampoline(slot: usize, arity: usize, label: &str) -> Option<Function> {
    // The callee's error goes back to the caller, prefixed with the function
    // that failed: a logged error and a nil answer hid which call it was.
    fn relay(slot: usize, label: &str, args: Vec<rune::Value>) -> VmResult<rune::Value> {
        // Cloned out before the call: the callee may itself require.
        let function = SHARED_FNS.with(|f| f.borrow()[slot].try_clone().ok());
        match function.map(|f| f.call::<rune::Value>(args).into_result()) {
            Some(Ok(value)) => VmResult::Ok(value),
            Some(Err(err)) => VmResult::Err(VmError::panic(format!("{label}: {err}"))),
            None => VmResult::Ok(rune::to_value(()).expect("unit always converts")),
        }
    }
    let label = label.to_string();
    let handler = move |stack: &mut dyn Memory, addr: InstAddress, args: usize, out: Output| {
        if args != arity {
            return VmResult::Err(VmError::panic(format!(
                "{label} takes {arity} arguments, called with {args}"
            )));
        }
        let taken = rune::vm_try!(stack.slice_at(addr, args)).to_vec();
        let value = rune::vm_try!(relay(slot, &label, taken));
        rune::vm_try!(out.store(stack, value));
        VmResult::Ok(())
    };
    Some(Function::from_handler(
        std::sync::Arc::new(handler),
        rune::Hash::EMPTY,
    ))
}
