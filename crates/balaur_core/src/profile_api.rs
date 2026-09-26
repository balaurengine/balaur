//! `engine.set_script_profiling`, `engine.script_costs` and `engine.function_costs`:
//! what the script host counted, as the rows a profiler lists.

// Every declaration shares one signature so they can sit in `ENGINE_OPS`;
// none of these has anything to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::Result;
use balaur_script::Value;

use crate::engine::Engine;

/// `engine.set_script_profiling(on)`: start or stop counting what each script
/// costs. Turning it on clears the tally.
pub(crate) fn set_script_profiling(eng: &Engine, args: &[Value]) -> Result<Value> {
    let on = matches!(args.first(), Some(Value::Bool(true)));
    if let Some(host) = eng.script_host() {
        host.set_profiling(on);
    }
    Ok(Value::Nil)
}

/// `engine.script_costs()`: what each script has cost since profiling
/// started, dearest first.
///
/// Counted in instructions, not seconds: the same run executes the same
/// instructions on every machine, so a number that moved is a real change.
pub(crate) fn script_costs(eng: &Engine, _: &[Value]) -> Result<Value> {
    let rows = eng
        .script_host()
        .map(|h| h.script_costs())
        .unwrap_or_default();
    Ok(cost_rows("path", rows))
}

/// `engine.function_costs()`: what each function has cost since profiling
/// started, dearest first, its own instructions without its callees'.
pub(crate) fn function_costs(eng: &Engine, _: &[Value]) -> Result<Value> {
    let rows = eng
        .script_host()
        .map(|h| h.function_costs())
        .unwrap_or_default();
    Ok(cost_rows("function", rows))
}

/// `{ <named>, calls, instructions }` a row, for the two cost readers.
fn cost_rows(named: &str, rows: Vec<(String, u64, u64)>) -> Value {
    Value::List(
        rows.into_iter()
            .map(|(name, calls, instructions)| {
                Value::Map(vec![
                    ("calls".to_string(), Value::Int(calls.cast_signed())),
                    (
                        "instructions".to_string(),
                        Value::Int(instructions.cast_signed()),
                    ),
                    (named.to_string(), Value::Str(name)),
                ])
            })
            .collect(),
    )
}
