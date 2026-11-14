//! `script::check(path, source)` and the editor's other questions about a
//! file: answered by whichever backend serves it, in the shapes the editor
//! reads (`balaur_script::tool_answer`). A module a tool's binary installs
//! ([`install_tool_verbs`]); a game's runtime has none of it.
use crate::vocabulary::keys;
use crate::vocabulary::words;
use balaur_core::Engine;
use balaur_script::{FnDoc, Module, Value};

pub(crate) const MODULE: &str = "script::";

/// The module holding what a value's type is called.
pub(crate) const TYPES_MODULE: &str = "types::";

/// What `any.kind()` answers, as `types::KIND_*`. The prelude's `any.bs` spells
/// the same eight, and a test holds the two together.
pub(crate) const KINDS: &[(&str, &str)] = &[
    ("KIND_NIL", "nil"),
    ("KIND_BOOL", "bool"),
    ("KIND_INT", "int"),
    ("KIND_FLOAT", "float"),
    ("KIND_STRING", "str"),
    ("KIND_LIST", "list"),
    ("KIND_MAP", "map"),
    ("KIND_NODE", "node"),
];

/// What the module is for, as the reference opens it.
const MODULE_DOC: &str = "What a tool asks about a script file. The editor and `balaur check` have it; a game's runtime does not.";

/// What `types` is for, as the reference opens it.
pub(crate) const TYPES_MODULE_DOC: (&str, &str) = (
    "types",
    "The names of a value's type: what `kind()` answers on an `any`.",
);

/// What the answers are made of, as `balaur_script::tool_answer` builds them.
#[rustfmt::skip]
pub(crate) const RECORDS: &[(&str, &str)] = &[
    ("Diagnostic", "{ file: string, line: int, column: int, severity: string, message: string }"),
    ("Completion", "{ label: string, kind: string, detail: string, doc: string, insert: string }"),
    ("Hover", "{ title: string, detail: string, doc: string }"),
    ("Place", "{ file: string, line: int, column: int, url: string }"),
    ("Symbol", "{ name: string, kind: string, detail: string, line: int, column: int }"),
    ("Signature", "{ title: string, detail: string, doc: string, active: int }"),
    ("Edit", "{ file: string, source: string }"),
    ("ApiFunction", "{ name: string, signature: string, doc: string, acts_on: [string], methods: [string] }"),
    ("ApiConstant", "{ name: string, value: string }"),
    ("ApiModule", "{ name: string, doc: string, functions: [ApiFunction], constants: [ApiConstant] }"),
];

/// Each verb: what it takes and gives back, and what it does.
#[rustfmt::skip]
const VERBS: &[FnDoc] = &[
    ("check", &[], "(path: string, source: string) -> [Diagnostic]", "Every compiler error in the source, read as the file at the path: one for each function that does not compile."),
    ("complete", &[], "(path: string, source: string, line: int, column: int) -> [Completion]", "Every completion valid at that caret; an editor passes the buffer it is showing."),
    ("hover", &[], "(path: string, source: string, line: int, column: int) -> Hover?", "What the name at that caret is: its type, or its signature; nil when it is nothing."),
    ("definition", &[], "(path: string, source: string, line: int, column: int) -> Place?", "Where the name at that caret is declared in the file; nil when it is not."),
    ("symbols", &[], "(path: string, source: string) -> [Symbol]", "What the file declares: its structs, their fields and methods, its functions, and each `event` with its description for a detail."),
    ("signature", &[], "(path: string, source: string, line: int, column: int) -> Signature?", "The call the caret is inside, and which argument it is on."),
    ("references", &[], "(path: string, source: string, name: string) -> [Place]", "Every use of that name in the file."),
    ("rename", &[], "(path: string, source: string, from: string, to: string) -> [Edit]", "The file with a name renamed; nothing is written, so a caller can show it first."),
    ("find", &[], "(path: string, source: string, needle: string) -> [Place]", "Every place that text appears in the file, matched as text rather than as a name."),
    ("replace", &[], "(path: string, source: string, from: string, to: string) -> [Edit]", "The file with that text replaced; nothing is written, so a caller can show it first."),
    ("format", &[], "(path: string, source: string) -> string", "The source laid out as `balaur fmt` lays it out; unchanged when it will not parse."),
    (words::EXPORTS, &[], "(path: string) -> list", "The `pub` fields of the script's struct, which a scene tunes, with their types and defaults."),
    (words::API, &[], "() -> [ApiModule]", "Every module a script can reach: its functions, their signatures and what each does, and its constants."),
];

/// Declare the `script` module: every verb above, on whichever backend
/// serves the file it is asked about.
pub fn install_tool_verbs(m: &mut Module<Engine>) {
    m.module_doc(MODULE_DOC);
    m.describe(VERBS);
    for &(verb, ..) in VERBS {
        m.function_raw(verb, Box::new(move |eng, args| Ok(call(eng, verb, args))));
    }
}

/// What the file's backend answers; nil when no backend serves it.
fn call(eng: &Engine, verb: &str, args: &[Value]) -> Value {
    if verb == words::API {
        let answer = crate::BsHost::of(eng).map_or(Value::List(vec![]), |host| host.reference());
        return by_function(&answer);
    }
    let Some(Value::Str(path)) = args.first() else {
        return Value::Nil;
    };
    let key = crate::host::key_of(path);
    if verb == words::EXPORTS {
        return exports(eng, &key);
    }
    let answer = eng.script_host().and_then(|scripts| {
        let tools = scripts.tools_for(&key)?;
        balaur_script::tool_answer(tools, verb, &key, &args[1..])
    });
    answer.unwrap_or(Value::Nil)
}

// `balaur api`'s reference as a script reads it: a row a function, where the
// file keeps its signatures and docs in maps by name.
fn by_function(reference: &Value) -> Value {
    let Value::List(modules) = reference else {
        return Value::List(vec![]);
    };
    let rows = modules.iter().map(|module| {
        let functions = list_at(module, keys::FUNCTIONS)
            .iter()
            .filter_map(|function| {
                let Value::Str(name) = function else {
                    return None;
                };
                let of = |key: &str| at(at(module, key), name).cloned().unwrap_or(Value::Nil);
                // What a script writes after `node.`, a handle each.
                let methods = match of(keys::METHODS) {
                    Value::Map(on) => on
                        .iter()
                        .filter_map(|(comp, method)| match method {
                            Value::Str(method) => Some(Value::text(format!("{comp}.{method}"))),
                            _ => None,
                        })
                        .collect(),
                    _ => vec![],
                };
                Some(Value::Map(vec![
                    (keys::NAME.into(), function.clone()),
                    ("signature".into(), of("signatures")),
                    (keys::DOC.into(), of("docs")),
                    (keys::ACTS_ON.into(), of(keys::ACTS_ON)),
                    (keys::METHODS.into(), Value::List(methods)),
                ]))
            });
        Value::Map(vec![
            (
                keys::NAME.into(),
                at(module, keys::NAME).cloned().unwrap_or(Value::Nil),
            ),
            (
                keys::DOC.into(),
                at(module, keys::DOC).cloned().unwrap_or(Value::Nil),
            ),
            (keys::FUNCTIONS.into(), Value::List(functions.collect())),
            (
                keys::CONSTANTS.into(),
                Value::List(list_at(module, keys::CONSTANTS).to_vec()),
            ),
        ])
    });
    Value::List(rows.collect())
}

fn at<'a>(map: impl Into<Option<&'a Value>>, key: &str) -> Option<&'a Value> {
    match map.into()? {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn list_at<'a>(map: &'a Value, key: &str) -> &'a [Value] {
    match at(map, key) {
        Some(Value::List(items)) => items,
        _ => &[],
    }
}

/// What the script at `key` declares tunable, one row a property in name
/// order: its `name`, then its spec's `type`, `default` and the rest.
fn exports(eng: &Engine, key: &str) -> Value {
    let Some(scripts) = eng.script_host() else {
        return Value::List(vec![]);
    };
    let mut declared = match scripts.exports(key) {
        Ok(declared) => declared,
        Err(err) => {
            tracing::error!("script::exports({key}): {err:#}");
            return Value::List(vec![]);
        }
    };
    declared.sort_by(|a, b| a.0.cmp(&b.0));
    let rows = declared
        .into_iter()
        .map(|(name, spec)| {
            let mut row = vec![(keys::NAME.to_string(), Value::text(name))];
            if let Value::Map(fields) = spec {
                row.extend(fields);
            }
            Value::Map(row)
        })
        .collect();
    Value::List(rows)
}
