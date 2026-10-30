//! `script::store(key, value)` and `script::stored(key)`: state no one node
//! owns, what Rune's `script::store` and a Godot `static var` are. What one
//! scene leaves for the next: a round's results for the summary that
//! follows it. Held as neutral values, so a map stored by one file is read
//! back by another; a `[T]` or a struct crosses as its neutral shape.
use crate::convert::{from_neutral, to_neutral};
use balaur_bs::runtime::{CallArgs, Value as L};
use balaur_bs::{HostSignature, Ty};
use balaur_script::Value;
use std::cell::RefCell;
use std::collections::HashMap;

const STORE_FN: &str = "script::store";
const STORED_FN: &str = "script::stored";

thread_local! {
    static STORE: RefCell<HashMap<String, Value>> = RefCell::new(HashMap::new());
}

pub(crate) fn signatures() -> Vec<HostSignature> {
    vec![
        HostSignature {
            key: STORE_FN.to_string(),
            params: vec![Ty::Str, Ty::Any],
            ret: Ty::Void,
            is_async: false,
        },
        HostSignature {
            key: STORED_FN.to_string(),
            params: vec![Ty::Str],
            ret: Ty::Any,
            is_async: false,
        },
    ]
}

/// What each does, by key.
pub(crate) fn docs() -> impl Iterator<Item = (String, &'static str)> {
    [
        (STORE_FN, "Keep any value under a key for as long as the app runs: state no node owns, like a class's static variable."),
        (STORED_FN, "What `store` kept under a key; nil when nothing was."),
    ]
    .into_iter()
    .map(|(key, doc)| (key.to_string(), doc))
}

/// Whether `key` is one of these.
pub(crate) fn is_store(key: &str) -> bool {
    key == STORE_FN || key == STORED_FN
}

/// Store or read back, by `key`'s name.
pub(crate) fn call(key: &str, args: &dyn CallArgs) -> Result<L, String> {
    let Some(name) = args.text(0).map(str::to_string) else {
        return Err(format!("`{key}` wants a key as its first argument"));
    };
    if key == STORE_FN {
        let value = to_neutral(&args.value(1));
        STORE.with_borrow_mut(|store| {
            store.insert(name, value);
        });
        return Ok(L::Void);
    }
    let value = STORE.with_borrow(|store| store.get(&name).cloned());
    Ok(from_neutral(&value.unwrap_or(Value::Nil), &Ty::Any))
}
