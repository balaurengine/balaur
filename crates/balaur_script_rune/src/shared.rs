//! Calling one unit's function from inside another unit's VM.
//!
//! `script::require`'s exports are native trampolines holding only a slot in
//! the table here, so a reload that refills the slot reaches every caller,
//! including one that kept a function out of the module.

use std::cell::RefCell;

use rune::alloc::clone::TryClone as _;
use rune::runtime::{Function, VmError, VmResult};

thread_local! {
    /// The functions behind `script::require`'s exports, by the slot a
    /// trampoline carries.
    pub(crate) static SHARED_FNS: RefCell<Vec<Function>> = const { RefCell::new(Vec::new()) };
}

/// The most parameters a trampoline forwards: Rune's native functions stop there.
pub(crate) const MOST_ARGS: usize = 5;

/// A native function forwarding to `SHARED_FNS[slot]` with `arity` args.
/// Arity is fixed per wrapper because Rune native functions are typed;
/// script-model functions keep their whole signature on one line, which is
/// where the arity was read from.
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
    type V = rune::Value;
    let label = label.to_string();
    Some(match arity {
        0 => Function::new(move || relay(slot, &label, Vec::new())),
        1 => Function::new(move |a: V| relay(slot, &label, vec![a])),
        2 => Function::new(move |a: V, b: V| relay(slot, &label, vec![a, b])),
        3 => Function::new(move |a: V, b: V, c: V| relay(slot, &label, vec![a, b, c])),
        4 => Function::new(move |a: V, b: V, c: V, d: V| relay(slot, &label, vec![a, b, c, d])),
        5 => Function::new(move |a: V, b: V, c: V, d: V, e: V| {
            relay(slot, &label, vec![a, b, c, d, e])
        }),
        _ => return None,
    })
}
