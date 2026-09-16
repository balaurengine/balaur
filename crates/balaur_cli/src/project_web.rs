//! `project.*` in a browser tab: the projects this browser keeps, and the
//! handshake that opens one.
//!
//! A tab cannot start a second editor, so opening a project is a request
//! rather than a launch: the screen leaves the project's id in
//! `sessionStorage` under [`ASKED`] and loads the page again, and the page
//! boots the editor on what it finds there. A browser runs one engine per
//! page, so the page is what restarts, not the engine inside it.
//!
//! The list itself is read before the editor boots, because the store is
//! asynchronous and a script verb is not.

use std::cell::RefCell;
use std::path::Path;

use balaur_script::Value;
use wasm_bindgen::JsValue;

/// One project this browser keeps, as its record names it.
struct Kept {
    id: String,
    name: String,
    /// When it was last opened, in seconds since the epoch.
    modified: f64,
}

thread_local! {
    static KEPT: RefCell<Vec<Kept>> = const { RefCell::new(Vec::new()) };
}

/// Where the screen leaves the project it picked, for the page that loads
/// next. The page's own half of this is `src/play.ts` in the website.
const ASKED: &str = "balaur-open-project";

/// Take the rows [`crate::web_store::list`] answered, newest first.
pub(crate) fn set_kept(rows: &js_sys::Array) {
    let read = |row: &JsValue, field: &str| {
        js_sys::Reflect::get(row, &field.into()).unwrap_or(JsValue::UNDEFINED)
    };
    let mut out = Vec::with_capacity(rows.length() as usize);
    for index in 0..rows.length() {
        let row = rows.get(index);
        let Some(id) = read(&row, "id").as_string() else {
            continue;
        };
        let name = read(&row, "name").as_string().unwrap_or_else(|| id.clone());
        out.push(Kept {
            id,
            name,
            modified: read(&row, "modified").as_f64().unwrap_or_default(),
        });
    }
    KEPT.with(|kept| *kept.borrow_mut() = out);
}

/// The rows the start screen draws, in the shape a desktop's list has: the
/// id stands where a path does, because that is what opening one takes.
pub(crate) fn recent() -> Vec<Value> {
    let now = js_sys::Date::now() / 1000.0;
    KEPT.with(|kept| {
        kept.borrow()
            .iter()
            .map(|row| {
                Value::Map(vec![
                    ("path".into(), Value::Str(row.id.clone())),
                    ("name".into(), Value::Str(row.name.clone())),
                    ("opened".into(), Value::Num(row.modified)),
                    (
                        "when".into(),
                        Value::Str(crate::project_api::said_ago(
                            (now - row.modified).max(0.0) as i64
                        )),
                    ),
                    ("version".into(), Value::Str(String::new())),
                    ("exists".into(), Value::Bool(true)),
                ])
            })
            .collect()
    })
}

/// Ask the page to open `id` and load itself again. The screen says nothing
/// after this: the page it drew in is on its way out.
pub(crate) fn open(id: &Path) -> Value {
    let id = id.to_string_lossy().into_owned();
    if !KEPT.with(|kept| kept.borrow().iter().any(|row| row.id == id)) {
        return Value::Map(vec![(
            "error".into(),
            Value::Str(format!("this browser keeps no project called {id}")),
        )]);
    }
    match ask_for(&id) {
        Ok(()) => Value::Map(Vec::new()),
        Err(e) => Value::Map(vec![("error".into(), Value::Str(e))]),
    }
}

/// Leave the id where the next page load will find it, and load it.
fn ask_for(id: &str) -> Result<(), String> {
    let said = |e: JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or_else(|| "no window to load again".to_string())?;
    let store = window
        .session_storage()
        .map_err(said)?
        .ok_or_else(|| "this browser keeps nothing for the page".to_string())?;
    store.set_item(ASKED, id).map_err(said)?;
    window.location().reload().map_err(said)
}

/// Drop a project from this browser: the files and the record that named it.
pub(crate) fn forget(id: &Path) -> Value {
    let id = id.to_string_lossy().into_owned();
    KEPT.with(|kept| kept.borrow_mut().retain(|row| row.id != id));
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = crate::web_store::delete(&id).await {
            tracing::warn!("forgetting {id} failed: {e:?}");
        }
    });
    Value::Map(Vec::new())
}
