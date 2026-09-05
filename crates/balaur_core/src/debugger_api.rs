//! The `debugger` script module: breakpoints, the pause, and stepping.
//!
//! Declared once through the seam like `engine_api`, so an editor written in
//! either language drives the debugger of either.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use std::rc::Rc;

use anyhow::{Result, anyhow};
use balaur_script::{Bindings, ScriptHost, StepMode, Value};

use crate::engine::Engine;
use crate::engine_api::{EngineOp, optional_node, text};

pub const DEBUGGER_OPS: &[EngineOp] = &[
    EngineOp {
        module: "debugger",
        name: "set_breakpoints",
        call: set_breakpoints,
    },
    EngineOp {
        module: "debugger",
        name: "breakpoints",
        call: breakpoints,
    },
    EngineOp {
        module: "debugger",
        name: "paused",
        call: paused,
    },
    EngineOp {
        module: "debugger",
        name: "resume",
        call: resume,
    },
    EngineOp {
        module: "debugger",
        name: "set_break_on_error",
        call: set_break_on_error,
    },
    EngineOp {
        module: "debugger",
        name: "break_on_error",
        call: break_on_error,
    },
    EngineOp {
        module: "debugger",
        name: "request_break",
        call: request_break,
    },
    EngineOp {
        module: "debugger",
        name: "set_scope",
        call: set_scope,
    },
    EngineOp {
        module: "debugger",
        name: "scope",
        call: scope,
    },
];

/// Declare the module's functions and the `STEP_*` constants `resume` takes.
pub fn install_debugger_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Breakpoints, the pause a stopped script sits in, and the ways out of \
         it. The same machinery the editor's Debugger dock and the Debug \
         Adapter Protocol server drive, so an outside editor and the built-in \
         one see one debugger.",
    );
    m.describe(&[
        ("set_breakpoints", &[], "(path: string, lines: [int])", "Replace one file's breakpoints with the given lines, returning the lines they landed on."),
        ("breakpoints", &[], "(path: string)", "The lines one file's breakpoints landed on."),
        ("paused", &[], "()", "Where a script is stopped — node, path, line, reason and frames, innermost first — or nil while none is."),
        ("resume", &[], "(mode: string?)", "Let the stopped script go on, in the given step mode (`CONTINUE`, `STEP_OVER`, `STEP_INTO`, `STEP_OUT`)."),
        ("set_break_on_error", &[], "(on: bool)", "Stop where a script throws instead of logging it and moving on; off by default."),
        ("break_on_error", &[], "()", "Whether a script that throws stops rather than being logged."),
        ("request_break", &[], "()", "Ask to stop at the next line any script runs; nothing is stopped yet when it returns."),
        ("set_scope", &[], "(node: node?)", "Limit the pause to one node's subtree, so an editor keeps running while the game stops; nil means the whole tree."),
        ("scope", &[], "()", "The node whose subtree a pause holds still, or nil when a pause stops the whole tree."),
    ]);
    for d in DEBUGGER_OPS {
        m.function_raw(d.name, Box::new(d.call));
    }
    m.constant("CONTINUE", Value::Str(StepMode::Continue.name().into()));
    m.constant("STEP_OVER", Value::Str(StepMode::Over.name().into()));
    m.constant("STEP_INTO", Value::Str(StepMode::Into.name().into()));
    m.constant("STEP_OUT", Value::Str(StepMode::Out.name().into()));
}

fn host(eng: &Engine) -> Result<Rc<dyn ScriptHost<Engine>>> {
    eng.script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))
}

fn lines(v: &Value) -> Vec<usize> {
    let mut out: Vec<usize> = match v {
        Value::List(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::Int(i) => usize::try_from(*i).ok(),
                Value::Num(n) if *n >= 1.0 => Some(*n as usize),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    out.sort_unstable();
    out.dedup();
    out
}

/// `debugger.set_breakpoints(path, { 12, 30 })`: replace a file's
/// breakpoints; returns the lines they landed on.
fn set_breakpoints(eng: &Engine, args: &[Value]) -> Result<Value> {
    let landed =
        host(eng)?.set_breakpoints(text(args, 0)?, &args.get(1).map(lines).unwrap_or_default())?;
    Ok(Value::List(
        landed
            .into_iter()
            .map(|l| Value::Int(i64::try_from(l).unwrap_or(i64::MAX)))
            .collect(),
    ))
}

fn breakpoints(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::List(
        host(eng)?
            .breakpoints(text(args, 0)?)
            .into_iter()
            .map(|l| Value::Int(i64::try_from(l).unwrap_or(i64::MAX)))
            .collect(),
    ))
}

/// Where a script is stopped, or nil: `{ node, path, line, reason, frames }`,
/// each frame `{ function, path, line, locals }`, innermost first.
fn paused(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(host(eng)?.paused().map_or(Value::Nil, |p| p.to_value()))
}

/// `debugger.resume(debugger.STEP_OVER)`; no argument continues.
fn resume(eng: &Engine, args: &[Value]) -> Result<Value> {
    let mode = match args.first() {
        None | Some(Value::Nil) => StepMode::Continue,
        Some(Value::Str(name)) => StepMode::parse(name)
            .ok_or_else(|| anyhow!("{name} is not a step mode; use debugger.STEP_*"))?,
        Some(other) => return Err(anyhow!("resume takes a step mode, got {other:?}")),
    };
    host(eng)?.resume(mode);
    Ok(Value::Nil)
}

/// `debugger.set_break_on_error(true)`: stop where a script throws, instead
/// of logging it and moving on. Off by default, because it puts every
/// synchronous call through the stepping executor.
fn set_break_on_error(eng: &Engine, args: &[Value]) -> Result<Value> {
    host(eng)?.set_break_on_error(matches!(args.first(), Some(Value::Bool(true))));
    Ok(Value::Nil)
}

fn break_on_error(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Bool(host(eng)?.break_on_error()))
}

/// `debugger.request_break()`: stop at the next line a script runs. Nothing
/// is paused when this returns — `paused` answers once the stop arrives.
fn request_break(eng: &Engine, _: &[Value]) -> Result<Value> {
    host(eng)?.request_break();
    Ok(Value::Nil)
}

/// The subtree a pause holds still, so an editor can keep its own scripts
/// running while the game it hosts is stopped. Nil means the whole tree.
fn set_scope(eng: &Engine, args: &[Value]) -> Result<Value> {
    eng.set_debug_scope(optional_node(args, 0)?);
    Ok(Value::Nil)
}

fn scope(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(eng
        .debug_scope()
        .map_or(Value::Nil, |e| Value::Node(crate::node_id_of(e).0)))
}
