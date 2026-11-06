//! How a plugin sends a result back to a script, and how it reads the options
//! table that names one.
//!
//! Every subsystem doing work off the frame faces the same two problems: what
//! to call when the work lands, and how to read the table the script passed
//! in. Both answers are small and both are identical everywhere, so they live
//! here rather than once per protocol crate.
//!
//! A handler is a method *name* on a node: a script hands the engine no
//! function, so anything that reports later is named and looked up then.

use anyhow::{Result, anyhow};
use balaur_script::{Module, NodeId, Value};

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

/// The `headers` list of an options value, each row a `{ name, value }`, as
/// string pairs. A name may repeat.
///
/// # Errors
/// When `headers` is not a list, or a row is not two strings.
pub fn headers_of(opts: Option<&Value>) -> Result<Vec<(String, String)>> {
    match opt(opts, "headers") {
        Some(Value::List(rows)) => rows
            .iter()
            .map(|row| {
                let text = |key| match opt(Some(row), key) {
                    Some(Value::Str(s)) => Ok(s.clone()),
                    other => Err(anyhow!(
                        "a header's `{key}` should be a string, got {other:?}"
                    )),
                };
                Ok((text("name")?, text("value")?))
            })
            .collect(),
        Some(Value::Nil) | None => Ok(Vec::new()),
        Some(other) => Err(anyhow!("headers should be a list, got {other:?}")),
    }
}

/// The bytes a script's `[int]` holds, each one 0 to 255.
///
/// # Errors
/// When `value` is not a list of ints, or one is past a byte.
pub fn bytes_of(value: &Value) -> Result<Vec<u8>> {
    let items = match value {
        Value::Bytes(bytes) => return Ok(bytes.clone()),
        Value::List(items) => items,
        other => {
            return Err(anyhow!(
                "bytes are a list of ints, got {}",
                other.type_name()
            ));
        }
    };
    items
        .iter()
        .map(|item| match item {
            Value::Int(byte) => {
                u8::try_from(*byte).map_err(|_| anyhow!("{byte} is not a byte: 0 to 255"))
            }
            other => Err(anyhow!("bytes hold ints, not a {}", other.type_name())),
        })
        .collect()
}

/// Wake whatever awaits each finished call's token, with its value. Called
/// with the world unborrowed, since what wakes is script code and may do
/// anything to it.
pub fn wake_all(eng: &Engine, finished: Vec<(u64, Value)>) {
    let Some(host) = eng.script_host() else {
        return;
    };
    for (token, value) in finished {
        host.wake(token, &value);
    }
}

/// An engine id as the script sees it. Ids are `u64` and scripts count in
/// `i64`, so this is the one place the narrowing is decided.
#[must_use]
pub fn id_value(id: u64) -> Value {
    Value::Int(i64::try_from(id).unwrap_or(i64::MAX))
}

/// The `kind` an event map carries when the work failed, the same in every
/// protocol: what a script compares against `EVENT_ERROR`.
pub const ERROR: &str = "error";

/// A module's event kinds as `EVENT_<KIND>` constants, so a script compares
/// `e["kind"] == websocket::EVENT_OPEN` rather than spelling the word.
pub fn install_event_kinds<E>(m: &mut Module<E>, kinds: &[&str]) {
    for kind in kinds {
        m.constant(
            &format!("EVENT_{}", kind.to_uppercase()),
            Value::Str((*kind).to_string()),
        );
    }
}
