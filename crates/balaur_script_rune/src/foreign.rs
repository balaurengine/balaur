//! The editor's tooling on a file another language answers for: a `.bs`
//! beside the Rune scripts. Its answers come back in the shapes Rune's own
//! tooling gives, so the editor reads both the same way.

use balaur_script::{Value, tool_answer};

use crate::{RuneHost, value};

fn text(s: &str) -> Value {
    Value::Str(s.to_string())
}

fn num(n: usize) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// `script::<verb>`'s answer for `path`, when another language answers for
/// it; `args` is what came after the path.
fn ask(host: &RuneHost, verb: &str, path: &str, args: &[Value]) -> Option<rune::Value> {
    let key = RuneHost::normalize_key(path);
    let scripts = host.engine.script_host()?;
    let tools = scripts.tools_for(&key)?;
    let answer = tool_answer(tools, verb, &key, args)?;
    Some(
        value::from_neutral(&answer)
            .unwrap_or_else(|_| rune::to_value(()).expect("unit always converts")),
    )
}

/// `[#{ file, line, column, severity, message }]`.
pub(crate) fn check(host: &RuneHost, path: &str, source: &str) -> Option<rune::Value> {
    ask(host, "check", path, &[text(source)])
}

/// `[#{ label, kind, detail, doc, insert }]`.
pub(crate) fn complete(
    host: &RuneHost,
    path: &str,
    source: &str,
    line: usize,
    column: usize,
) -> Option<rune::Value> {
    ask(
        host,
        "complete",
        path,
        &[text(source), num(line), num(column)],
    )
}

/// `#{ title, detail, doc }`, or `()`.
pub(crate) fn hover(
    host: &RuneHost,
    path: &str,
    source: &str,
    line: usize,
    column: usize,
) -> Option<rune::Value> {
    ask(host, "hover", path, &[text(source), num(line), num(column)])
}

/// `#{ file, line, column, url }`, or `()`.
pub(crate) fn definition(
    host: &RuneHost,
    path: &str,
    source: &str,
    line: usize,
    column: usize,
) -> Option<rune::Value> {
    ask(
        host,
        "definition",
        path,
        &[text(source), num(line), num(column)],
    )
}

/// `[#{ name, kind, detail, line, column }]`.
pub(crate) fn symbols(host: &RuneHost, path: &str, source: &str) -> Option<rune::Value> {
    ask(host, "symbols", path, &[text(source)])
}

/// `#{ title, detail, doc, active }`, or `()`.
pub(crate) fn signature(
    host: &RuneHost,
    path: &str,
    source: &str,
    line: usize,
    column: usize,
) -> Option<rune::Value> {
    ask(
        host,
        "signature",
        path,
        &[text(source), num(line), num(column)],
    )
}

/// `[#{ file, line, column, url }]`.
pub(crate) fn references(
    host: &RuneHost,
    path: &str,
    source: &str,
    name: &str,
) -> Option<rune::Value> {
    ask(host, "references", path, &[text(source), text(name)])
}

/// `[#{ file, source }]`: the one file, renamed, or nothing.
pub(crate) fn rename(
    host: &RuneHost,
    path: &str,
    source: &str,
    from: &str,
    to: &str,
) -> Option<rune::Value> {
    ask(host, "rename", path, &[text(source), text(from), text(to)])
}

/// The source laid out, or as it was.
pub(crate) fn format(host: &RuneHost, path: &str, source: &str) -> Option<rune::Value> {
    ask(host, "format", path, &[text(source)])
}
