//! A profiled module counts each statement it runs, and each function's
//! calls and own statements, the same on every engine and machine.

use crate::{call, run_with};
use balaur_bs::Options;
use balaur_bs::runtime::{HostFns, Value};

const SRC: &str = r"
fn twice(x: int) -> int {
    let y = x * 2;
    y
}
pub fn outer(n: int) -> int {
    let total = 0;
    for i in 0..n {
        total += twice(i);
    }
    total
}
";

#[test]
fn a_profiled_module_counts_statements_and_each_functions_own() {
    let options = Options {
        profile: true,
        ..Options::default()
    };
    let mut rt = run_with(SRC, HostFns::default(), &options);
    let before = rt.steps().expect("a profiled module counts");
    assert_eq!(call(&mut rt, "outer", &[Value::Int(3)]), Value::Int(6));
    let spent = rt.steps().unwrap().wrapping_sub(before);
    // `outer`: its `let`, the `for`, three passes of its body and its tail;
    // `twice`: a `let` and a tail, three times.
    assert_eq!(spent, 6 + 3 * 2);
    let mut costs = rt.function_costs();
    costs.sort();
    assert_eq!(
        costs,
        [("outer".to_string(), 1, 6), ("twice".to_string(), 3, 6)]
    );
    rt.clear_profile();
    assert!(rt.function_costs().is_empty());
}

#[test]
fn a_module_built_without_counters_reports_none() {
    let mut rt = run_with(SRC, HostFns::default(), &Options::default());
    call(&mut rt, "outer", &[Value::Int(3)]);
    assert_eq!(rt.steps(), None);
    assert!(rt.function_costs().is_empty());
}
