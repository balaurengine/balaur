//! `script::check(path, source)` and the editor's other questions about a
//! file: answered by whichever backend serves it, in the shapes the editor
//! reads (`balaur_script::tool_answer`).
use crate::api::Structs;
use crate::convert::{from_neutral_in, to_neutral};
use crate::doc_types::Cx;
use balaur_bs::runtime::Value as L;
use balaur_bs::{HostSignature, Ty};
use balaur_core::Engine;
use balaur_script::Value;
use std::collections::BTreeSet;

pub(crate) const MODULE: &str = "script::";

/// What the module is for, as the reference opens it.
pub(crate) const MODULE_DOC: (&str, &str) = (
    "script",
    "What a tool asks about a script file, and state no node owns.",
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
];

/// Each verb, what it takes after the path and gives back, and what it does.
#[rustfmt::skip]
const VERBS: &[(&str, &str, &str)] = &[
    ("check", "(source: string) -> [Diagnostic]", "Every compiler error in the source, read as the file at the path: one for each function that does not compile."),
    ("complete", "(source: string, line: int, column: int) -> [Completion]", "Every completion valid at that caret; an editor passes the buffer it is showing."),
    ("hover", "(source: string, line: int, column: int) -> Hover?", "What the name at that caret is: its type, or its signature; nil when it is nothing."),
    ("definition", "(source: string, line: int, column: int) -> Place?", "Where the name at that caret is declared in the file; nil when it is not."),
    ("symbols", "(source: string) -> [Symbol]", "What the file declares: its structs, their fields and methods, and its functions."),
    ("signature", "(source: string, line: int, column: int) -> Signature?", "The call the caret is inside, and which argument it is on."),
    ("references", "(source: string, name: string) -> [Place]", "Every use of that name in the file."),
    ("rename", "(source: string, from: string, to: string) -> [Edit]", "The file with a name renamed; nothing is written, so a caller can show it first."),
    ("find", "(source: string, needle: string) -> [Place]", "Every place that text appears in the file, matched as text rather than as a name."),
    ("replace", "(source: string, from: string, to: string) -> [Edit]", "The file with that text replaced; nothing is written, so a caller can show it first."),
    ("format", "(source: string) -> string", "The source laid out as `balaur fmt` lays it out; unchanged when it will not parse."),
    ("exports", "() -> list", "The `pub` fields of the script's struct, which a scene tunes, with their types and defaults."),
    ("api", "() -> list", "Every module a script can reach: its functions, their signatures and what each does, and its constants."),
];

// The verbs about the engine rather than a file: no path first.
const PATHLESS: &[&str] = &["api"];

/// Their signatures, read against the records every module declared.
pub(crate) fn signatures(records: &BTreeSet<String>) -> Vec<HostSignature> {
    let cx = Cx {
        module: MODULE.trim_end_matches("::"),
        records,
    };
    VERBS
        .iter()
        .filter_map(|(name, doc, _)| {
            let (params, ret) = crate::doc_types::signature(doc, &cx)?;
            let path = (!PATHLESS.contains(name)).then_some(Ty::Str);
            Some(HostSignature {
                key: format!("{MODULE}{name}"),
                params: path.into_iter().chain(params).collect(),
                ret,
                is_async: false,
            })
        })
        .collect()
}

/// What each verb does, keyed `script::name`.
pub(crate) fn docs() -> impl Iterator<Item = (String, &'static str)> {
    VERBS
        .iter()
        .map(|(name, _, doc)| (format!("{MODULE}{name}"), *doc))
}

/// The verb a key names, when it is one of these.
pub(crate) fn verb(key: &str) -> Option<&'static str> {
    let name = key.strip_prefix(MODULE)?;
    VERBS.iter().find(|(v, ..)| *v == name).map(|(v, ..)| *v)
}

/// What the file's backend answers; nil when no backend serves it.
pub(crate) fn call(
    eng: &Engine,
    verb: &str,
    args: &[L],
    ret: &Ty,
    structs: &Structs,
) -> Result<L, String> {
    if verb == "api" {
        let answer = crate::BsHost::of(eng).map_or(Value::List(vec![]), |host| host.reference());
        return Ok(from_neutral_in(&answer, ret, structs));
    }
    let Some(L::Str(path)) = args.first() else {
        return Err(format!("`script::{verb}` wants a path first"));
    };
    let key = crate::host::key_of(path);
    if verb == "exports" {
        return Ok(from_neutral_in(&exports(eng, &key), ret, structs));
    }
    let after: Vec<Value> = args[1..].iter().map(to_neutral).collect();
    let answer = eng.script_host().and_then(|scripts| {
        let tools = scripts.tools_for(&key)?;
        balaur_script::tool_answer(tools, verb, &key, &after)
    });
    Ok(from_neutral_in(&answer.unwrap_or(Value::Nil), ret, structs))
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
            let mut row = vec![("name".to_string(), Value::text(name))];
            if let Value::Map(fields) = spec {
                row.extend(fields);
            }
            Value::Map(row)
        })
        .collect();
    Value::List(rows)
}
