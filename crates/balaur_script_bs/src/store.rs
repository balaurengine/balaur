//! `script::store(key, value)` and `script::stored(key)`: state no one node
//! owns, what Rune's `script::store` and a Godot `static var` are. What one
//! scene leaves for the next: a round's results for the summary that
//! follows it. Held as neutral values, so a map stored by one file is read
//! back by another; a `[T]` or a struct crosses as its neutral shape.
use crate::convert::{from_neutral, to_neutral};
use balaur_bs::runtime::Value as L;
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

/// Whether `key` is one of these.
pub(crate) fn is_store(key: &str) -> bool {
    key == STORE_FN || key == STORED_FN
}

/// Store or read back, by `key`'s name.
pub(crate) fn call(key: &str, args: &[L]) -> Result<L, String> {
    let name = match args.first() {
        Some(L::Str(name)) => name.clone(),
        _ => return Err(format!("`{key}` wants a key as its first argument")),
    };
    if key == STORE_FN {
        let value = args.get(1).map_or(Value::Nil, to_neutral);
        STORE.with_borrow_mut(|store| {
            store.insert(name, value);
        });
        return Ok(L::Void);
    }
    let value = STORE.with_borrow(|store| store.get(&name).cloned());
    Ok(from_neutral(&value.unwrap_or(Value::Nil), &Ty::Any))
}
