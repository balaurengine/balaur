//! The editor tooling a backend gives its own files: a buffer's problems,
//! what may be typed at the cursor, what a name is, where it is declared.
//! Lines and columns count from 1.

use crate::Value;

/// A problem in a buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize,
    /// `"error"` or `"warning"`.
    pub severity: &'static str,
    pub message: String,
}

/// One thing that could be typed at the cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// `module`, `function`, `method`, `field`, `property`, `variable`,
    /// `struct` or `keyword`.
    pub kind: &'static str,
    /// A signature or a type; empty when there is none.
    pub detail: String,
}

/// Where a name is declared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

/// Something a file declares, for an outline: a `function` the engine may
/// call, a `property` a scene tunes, or a `struct`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub detail: String,
    pub line: usize,
    pub column: usize,
}

pub trait LanguageTools {
    fn check(&self, path: &str, source: &str) -> Vec<Diagnostic>;
    fn complete(&self, path: &str, source: &str, line: usize, column: usize) -> Vec<Item>;
    /// A type or a signature for the name at the cursor.
    fn hover(&self, path: &str, source: &str, line: usize, column: usize) -> Option<String>;
    fn definition(&self, path: &str, source: &str, line: usize, column: usize) -> Option<Location>;

    fn symbols(&self, path: &str, source: &str) -> Vec<Symbol> {
        let _ = (path, source);
        vec![]
    }

    /// The call the cursor is in, and which argument it is on.
    fn signature(
        &self,
        path: &str,
        source: &str,
        line: usize,
        column: usize,
    ) -> Option<(String, usize)> {
        let _ = (path, source, line, column);
        None
    }

    /// Every use of `name`.
    fn references(&self, path: &str, source: &str, name: &str) -> Vec<Location> {
        let _ = (path, source, name);
        vec![]
    }

    /// The source with `from` renamed `to`; `None` when that cannot be done.
    fn rename(&self, path: &str, source: &str, from: &str, to: &str) -> Option<String> {
        let _ = (path, source, from, to);
        None
    }

    /// The source laid out; `None` when the backend has no layout.
    fn format(&self, path: &str, source: &str) -> Option<String> {
        let _ = (path, source);
        None
    }
}

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

fn num(n: usize) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn place(l: Location) -> Value {
    Value::Map(vec![
        ("file".into(), text(l.file)),
        ("line".into(), num(l.line)),
        ("column".into(), num(l.column)),
        ("url".into(), text("")),
    ])
}

fn finding(key: &str, d: Diagnostic) -> Value {
    Value::Map(vec![
        ("file".into(), text(key)),
        ("line".into(), num(d.line)),
        ("column".into(), num(d.column)),
        ("severity".into(), text(d.severity)),
        ("message".into(), text(d.message)),
    ])
}

fn item(c: Item) -> Value {
    Value::Map(vec![
        ("label".into(), text(c.label.clone())),
        ("kind".into(), text(c.kind)),
        ("detail".into(), text(c.detail)),
        ("doc".into(), text("")),
        ("insert".into(), text(c.label)),
    ])
}

fn symbol(s: Symbol) -> Value {
    Value::Map(vec![
        ("name".into(), text(s.name)),
        ("kind".into(), text(s.kind)),
        ("detail".into(), text(s.detail)),
        ("line".into(), num(s.line)),
        ("column".into(), num(s.column)),
    ])
}

/// What `script::<verb>(path, ..)` answers for the file at `key`, in the
/// shapes the editor reads: `args` is what the script passed after the path.
/// `None` for a verb that is not one of these.
pub fn tool_answer(
    tools: &dyn LanguageTools,
    verb: &str,
    key: &str,
    args: &[Value],
) -> Option<Value> {
    let word = |i: usize| match args.get(i) {
        Some(Value::Str(s)) => s.as_str(),
        _ => "",
    };
    let count = |i: usize| match args.get(i) {
        Some(Value::Int(n)) => usize::try_from(*n).unwrap_or(0),
        Some(Value::Num(n)) => *n as usize,
        _ => 0,
    };
    let source = word(0);
    let (line, column) = (count(1), count(2));
    Some(match verb {
        // `[{ file, line, column, severity, message }]`.
        "check" => Value::List(
            tools
                .check(key, source)
                .into_iter()
                .map(|d| finding(key, d))
                .collect(),
        ),
        // `[{ label, kind, detail, doc, insert }]`.
        "complete" => Value::List(
            tools
                .complete(key, source, line, column)
                .into_iter()
                .map(item)
                .collect(),
        ),
        // `{ title, detail, doc }`, or nil.
        "hover" => tools
            .hover(key, source, line, column)
            .map_or(Value::Nil, |h| {
                Value::Map(vec![
                    ("title".into(), text(h)),
                    ("detail".into(), text("")),
                    ("doc".into(), text("")),
                ])
            }),
        // `{ file, line, column, url }`, or nil.
        "definition" => tools
            .definition(key, source, line, column)
            .map_or(Value::Nil, place),
        // `[{ name, kind, detail, line, column }]`.
        "symbols" => Value::List(tools.symbols(key, source).into_iter().map(symbol).collect()),
        // `{ title, detail, doc, active }`, or nil.
        "signature" => {
            tools
                .signature(key, source, line, column)
                .map_or(Value::Nil, |(sig, active)| {
                    Value::Map(vec![
                        ("title".into(), text(sig)),
                        ("detail".into(), text("")),
                        ("doc".into(), text("")),
                        ("active".into(), num(active)),
                    ])
                })
        }
        // `[{ file, line, column, url }]`.
        "references" => Value::List(
            tools
                .references(key, source, word(1))
                .into_iter()
                .map(place)
                .collect(),
        ),
        // `[{ file, source }]`: the one file, renamed, or nothing.
        "rename" => Value::List(
            tools
                .rename(key, source, word(1), word(2))
                .map(|renamed| {
                    Value::Map(vec![
                        ("file".into(), text(key)),
                        ("source".into(), text(renamed)),
                    ])
                })
                .into_iter()
                .collect(),
        ),
        // The source laid out, or as it was.
        "format" => text(
            tools
                .format(key, source)
                .unwrap_or_else(|| source.to_string()),
        ),
        _ => return None,
    })
}
