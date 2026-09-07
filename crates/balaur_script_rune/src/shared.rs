//! Calling one unit's function from inside another unit's VM.
//!
//! A unit-bound `Function` value dispatches into the wrong unit when it is
//! called from inside another unit's execution. A call routed through a native
//! Rust function lands correctly, so `script::require`'s exports are native
//! trampolines holding only a slot in the table here.

use std::cell::RefCell;

use rune::alloc::clone::TryClone as _;
use rune::runtime::Function;

thread_local! {
    /// The functions behind `script::require`'s exports, by the slot a
    /// trampoline carries.
    pub(crate) static SHARED_FNS: RefCell<Vec<Function>> = const { RefCell::new(Vec::new()) };
}

/// A native function forwarding to `SHARED_FNS[slot]` with `arity` args.
/// Arity is fixed per wrapper because Rune native functions are typed;
/// script-model functions keep their whole signature on one line, which is
/// where the arity was read from.
pub(crate) fn trampoline(slot: usize, arity: usize) -> Option<Function> {
    fn relay(slot: usize, args: Vec<rune::Value>) -> rune::Value {
        // Cloned out before the call: the callee may itself require.
        let function = SHARED_FNS.with(|f| f.borrow()[slot].try_clone().ok());
        let outcome = function.map(|f| f.call::<rune::Value>(args).into_result());
        match outcome {
            Some(Ok(value)) => value,
            Some(Err(err)) => {
                tracing::error!("a required function failed: {err}");
                rune::to_value(()).expect("unit always converts")
            }
            None => rune::to_value(()).expect("unit always converts"),
        }
    }
    Some(match arity {
        0 => Function::new(move || relay(slot, Vec::new())),
        1 => Function::new(move |a: rune::Value| relay(slot, vec![a])),
        2 => Function::new(move |a: rune::Value, b: rune::Value| relay(slot, vec![a, b])),
        3 => Function::new(move |a: rune::Value, b: rune::Value, c: rune::Value| {
            relay(slot, vec![a, b, c])
        }),
        4 => Function::new(
            move |a: rune::Value, b: rune::Value, c: rune::Value, d: rune::Value| {
                relay(slot, vec![a, b, c, d])
            },
        ),
        5 => Function::new(
            move |a: rune::Value,
                  b: rune::Value,
                  c: rune::Value,
                  d: rune::Value,
                  e: rune::Value| { relay(slot, vec![a, b, c, d, e]) },
        ),
        _ => return None,
    })
}
