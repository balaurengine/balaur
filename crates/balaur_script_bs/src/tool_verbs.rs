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

/// Each verb, and what it takes after the path and gives back.
#[rustfmt::skip]
const VERBS: &[(&str, &str)] = &[
    ("check", "(source: string) -> [Diagnostic]"),
    ("complete", "(source: string, line: int, column: int) -> [Completion]"),
    ("hover", "(source: string, line: int, column: int) -> Hover?"),
    ("definition", "(source: string, line: int, column: int) -> Place?"),
    ("symbols", "(source: string) -> [Symbol]"),
    ("signature", "(source: string, line: int, column: int) -> Signature?"),
    ("references", "(source: string, name: string) -> [Place]"),
    ("rename", "(source: string, from: string, to: string) -> [Edit]"),
    ("format", "(source: string) -> string"),
];

/// Their signatures, read against the records every module declared.
pub(crate) fn signatures(records: &BTreeSet<String>) -> Vec<HostSignature> {
    let cx = Cx {
        module: MODULE.trim_end_matches("::"),
        records,
    };
    VERBS
        .iter()
        .filter_map(|(name, doc)| {
            let (params, ret) = crate::doc_types::signature(doc, &cx)?;
            Some(HostSignature {
                key: format!("{MODULE}{name}"),
                params: std::iter::once(Ty::Str).chain(params).collect(),
                ret,
                is_async: false,
            })
        })
        .collect()
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
    let Some(L::Str(path)) = args.first() else {
        return Err(format!("`script::{verb}` wants a path first"));
    };
    let key = crate::host::key_of(path);
    let after: Vec<Value> = args[1..].iter().map(to_neutral).collect();
    let answer = eng.script_host().and_then(|scripts| {
        let tools = scripts.tools_for(&key)?;
        balaur_script::tool_answer(tools, verb, &key, &after)
    });
    Ok(from_neutral_in(&answer.unwrap_or(Value::Nil), ret, structs))
}
