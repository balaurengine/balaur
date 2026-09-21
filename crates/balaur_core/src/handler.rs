//! How a plugin sends a result back to a script, and how it reads the options
//! table that names one.
//!
//! Every subsystem doing work off the frame faces the same two problems: what
//! to call when the work lands, and how to read the table the script passed
//! in. Both answers are small and both are identical everywhere, so they live
//! here rather than once per protocol crate.
//!
//! A handler is a method *name* on a node, never a function value.
//! `Value::Callback` is valid only during the binding call that received it,
//! so anything outliving that call is named and looked up later.

use anyhow::{Result, anyhow};
use balaur_script::{NodeId, Value};

use crate::Engine;

/// Where one piece of finished work reports: a method on one node's script,
/// dispatched through `ScriptHost::call_on`.
#[derive(Clone, Debug)]
pub struct Handler {
    pub node: NodeId,
    pub method: String,
}

/// The handler a binding's node-and-options arguments name, or `None` for a
/// nil node — fire and forget.
///
/// `key` is the option a script uses to override the method, `default_method`
/// what it is called when the script says nothing.
///
/// # Errors
/// When the node argument is neither a node nor nil, or the named override is
/// not a string.
pub fn handler_of(
    node: &Value,
    opts: Option<&Value>,
    key: &str,
    default_method: &str,
) -> Result<Option<Handler>> {
    let node = match node {
        Value::Node(id) => NodeId(*id),
        Value::Nil => return Ok(None),
        other => return Err(anyhow!("argument 0 should be a node or nil, got {other:?}")),
    };
    let method = match opt(opts, key) {
        Some(Value::Str(name)) => name.clone(),
        Some(other) => return Err(anyhow!("`{key}` should be a method name, got {other:?}")),
        None => default_method.to_string(),
    };
    Ok(Some(Handler { node, method }))
}

/// One key out of a script options table, or `None` if the table, the key or
/// its type is missing.
#[must_use]
pub fn opt<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    match opts? {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// The `headers` table of an options value, as string pairs.
///
/// # Errors
/// When `headers` is not a table, or a value in it is not a string.
pub fn headers_of(opts: Option<&Value>) -> Result<Vec<(String, String)>> {
    match opt(opts, "headers") {
        Some(Value::Map(pairs)) => pairs
            .iter()
            .map(|(k, v)| match v {
                Value::Str(s) => Ok((k.clone(), s.clone())),
                other => Err(anyhow!("header `{k}` should be a string, got {other:?}")),
            })
            .collect(),
        Some(other) => Err(anyhow!("headers should be a table, got {other:?}")),
        None => Ok(Vec::new()),
    }
}

/// Deliver finished work: each value to the handlers named for it, then to
/// whatever awaits the request's token. Called with the world unborrowed,
/// since a handler is script code and may do anything to it.
pub fn dispatch(eng: &Engine, dispatches: Vec<(Vec<Handler>, u64, Value)>) {
    let Some(host) = eng.script_host() else {
        return;
    };
    for (targets, token, value) in dispatches {
        for handler in targets {
            host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
        }
        host.wake(token, &value);
    }
}

/// An engine id as the script sees it. Ids are `u64` and scripts count in
/// `i64`, so this is the one place the narrowing is decided.
#[must_use]
pub fn id_value(id: u64) -> Value {
    Value::Int(i64::try_from(id).unwrap_or(i64::MAX))
}
