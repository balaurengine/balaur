// A counted loop's array accesses lose their bounds checks only where one
// test up front proves every index in range; these are the cases around it.

use super::{call, run};
use balaur_lang::runtime::Value;

const SCRIPT: &str = "
pub fn sum_from(from: int, to: int) -> float {
    let xs = [0.0; 8];
    for i in 0..8 { xs[i] = i; }
    let t = 0.0;
    for i in from..to { t += xs[i]; }
    t
}

pub fn grow() -> int {
    let xs = [1, 2, 3];
    for i in 0..3 {
        xs.push(xs[i] * 10);
    }
    xs[5]
}

pub fn swap_out() -> int {
    let xs = [1, 2, 3];
    let total = 0;
    for i in 0..3 {
        total += xs[i];
        xs = [100, 200, 300];
    }
    total
}

pub fn shadowed() -> int {
    let xs = [1, 2, 3];
    let total = 0;
    for i in 0..3 {
        let xs = [10, 20];
        total += xs[i % 2];
    }
    total
}
";

#[test]
fn a_loop_inside_the_array_reads_what_a_checked_one_would() {
    let mut rt = run(SCRIPT);
    assert_eq!(
        call(&mut rt, "sum_from", &[Value::Int(2), Value::Int(5)]),
        Value::Float(9.0)
    );
    assert_eq!(
        call(&mut rt, "sum_from", &[Value::Int(0), Value::Int(8)]),
        Value::Float(28.0)
    );
    assert_eq!(
        call(&mut rt, "sum_from", &[Value::Int(5), Value::Int(2)]),
        Value::Float(0.0)
    );
}

#[test]
fn a_loop_past_either_end_still_traps() {
    let mut rt = run(SCRIPT);
    assert!(
        rt.call("sum_from", &[Value::Int(0), Value::Int(9)])
            .is_err()
    );
    assert!(
        rt.call("sum_from", &[Value::Int(-1), Value::Int(3)])
            .is_err()
    );
    assert_eq!(
        call(&mut rt, "sum_from", &[Value::Int(1), Value::Int(2)]),
        Value::Float(1.0)
    );
}

#[test]
fn a_body_that_resizes_or_replaces_an_array_keeps_its_checks() {
    let mut rt = run(SCRIPT);
    assert_eq!(call(&mut rt, "grow", &[]), Value::Int(30));
    assert_eq!(call(&mut rt, "swap_out", &[]), Value::Int(1 + 200 + 300));
    assert_eq!(call(&mut rt, "shadowed", &[]), Value::Int(10 + 20 + 10));
    assert_eq!(rt.live(), 0);
}
