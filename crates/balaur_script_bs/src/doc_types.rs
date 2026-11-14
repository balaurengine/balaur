// Types as the engine's docs spell them, read for the compiler: a signature
// `"(node: node) -> Rect"`, and a record's fields `"{ x: float, y: float }"`.

use crate::vocabulary::words;
use balaur_bs::Ty;
use std::collections::BTreeSet;

/// Where a documented type is read: the module a bare record name belongs
/// to, and every record any module declared, keyed `module::Name`.
pub(crate) struct Cx<'a> {
    pub module: &'a str,
    pub records: &'a BTreeSet<String>,
}

impl Cx<'_> {
    fn record(&self, word: &str) -> Option<Ty> {
        let key = if word.contains("::") {
            word.to_string()
        } else {
            format!("{}::{word}", self.module)
        };
        self.records.contains(&key).then_some(Ty::Struct(key))
    }
}

// The top-level pieces of a comma-separated list, `<..>` and `(..)` kept whole.
pub(crate) fn split_top(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (vec![], 0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter().filter(|p| !p.trim().is_empty()).collect()
}

// A tuple of two or three floats is a `Vec2` or `Vec3`, any other tuple of
// one type a `[T]`.
pub(crate) fn tuple(items: &[Ty]) -> Option<Ty> {
    let first = items.first()?.clone();
    if items.iter().any(|t| *t != first) {
        return None;
    }
    Some(match (first, items.len()) {
        (Ty::Float, 2) => Ty::Struct("Vec2".into()),
        (Ty::Float, 3) => Ty::Struct("Vec3".into()),
        (t, _) => Ty::Array(Box::new(t)),
    })
}

pub(crate) fn optional(t: Ty) -> Ty {
    match t {
        Ty::Any | Ty::Optional(_) => t,
        t => Ty::Optional(Box::new(t)),
    }
}

// A schema property's `type`, in the component vocabulary.
pub(crate) fn schema_ty(t: &str) -> Option<Ty> {
    Some(match t {
        words::doc_types::FLOAT | words::doc_types::NUMBER => Ty::Float,
        words::doc_types::INT => Ty::Int,
        words::doc_types::BOOL => Ty::Bool,
        words::doc_types::STRING | words::doc_types::ENUM | words::doc_types::ASSET => Ty::Str,
        words::doc_types::VEC2 => Ty::Struct("Vec2".into()),
        words::doc_types::VEC3 => Ty::Struct("Vec3".into()),
        words::doc_types::COLOR => Ty::Struct("Color".into()),
        words::doc_types::NODE => Ty::Node,
        words::doc_types::LIST
        | words::doc_types::MAP
        | words::doc_types::VARIANT
        | words::doc_types::TABLE
        | words::doc_types::ANY => Ty::Any,
        _ => return None,
    })
}

// A documented type: a schema word, a record, `[T]`, a tuple, `()` for nothing
// or `T?`. `bytes` are a list of ints, and `a | b` reads as nothing (N25).
fn doc_ty(t: &str, cx: &Cx<'_>) -> Option<Ty> {
    let t = t.trim();
    if let Some(inner) = t.strip_suffix('?') {
        return Some(optional(doc_ty(inner, cx)?));
    }
    if t.contains('|') {
        return None;
    }
    match t {
        words::doc_types::BYTES => return Some(Ty::Array(Box::new(Ty::Int))),
        words::doc_types::STR => return Some(Ty::Str),
        "Vec2" | "Vec3" | "Color" | "Transform2d" | "Transform3d" => {
            return Some(Ty::Struct(t.into()));
        }
        _ => {}
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        return Some(Ty::Array(Box::new(doc_ty(inner, cx)?)));
    }
    if t == "()" {
        return Some(Ty::Void);
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        let items: Vec<Ty> = split_top(inner)
            .into_iter()
            .map(|i| doc_ty(i, cx))
            .collect::<Option<_>>()?;
        return tuple(&items);
    }
    schema_ty(t).or_else(|| cx.record(t))
}

// `name: type`, or a bare name: `node` is a node, anything else untyped.
fn doc_param(p: &str, cx: &Cx<'_>) -> Option<Ty> {
    match p.split_once(':') {
        Some((_, t)) => doc_ty(t, cx),
        None if p.trim() == words::doc_types::NODE => Some(Ty::Node),
        None => Some(Ty::Any),
    }
}

/// "(message: string)", "(n: int?)" or "(pattern: string, text: string) ->
/// bool": the parameters and result, if every type reads. With no `->` the
/// function gives nothing back, as in Rust.
pub(crate) fn signature(s: &str, cx: &Cx<'_>) -> Option<(Vec<Ty>, Ty)> {
    let (args, ret) = s.split_once("->").map_or((s, None), |(a, r)| (a, Some(r)));
    let inner = args.trim().strip_prefix('(')?.strip_suffix(')')?;
    let params = split_top(inner)
        .into_iter()
        .map(|p| doc_param(p, cx))
        .collect::<Option<_>>()?;
    Some((params, ret.map_or(Some(Ty::Void), |r| doc_ty(r, cx))?))
}

/// `"-> Rect"`, or the result of a whole signature: what a typed function's
/// `Value` result holds. A whole signature with no `->` gives nothing back.
pub(crate) fn result(s: &str, cx: &Cx<'_>) -> Option<Ty> {
    match s.split_once("->") {
        Some((_, r)) => doc_ty(r, cx),
        None if s.trim_start().starts_with('(') => Some(Ty::Void),
        None => None,
    }
}

/// `"{ x: float, label: string? }"`: a record's fields, if every type reads.
pub(crate) fn fields(spec: &str, cx: &Cx<'_>) -> Option<Vec<(String, Ty)>> {
    let inner = spec.trim().strip_prefix('{')?.strip_suffix('}')?;
    split_top(inner)
        .into_iter()
        .map(|f| {
            let (name, t) = f.split_once(':')?;
            Some((name.trim().to_string(), doc_ty(t, cx)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Cx, fields, result, signature};
    use balaur_bs::Ty;
    use std::collections::BTreeSet;

    fn list(t: Ty) -> Ty {
        Ty::Array(Box::new(t))
    }

    fn records() -> BTreeSet<String> {
        ["ui::Rect".to_string(), "scene::Warning".to_string()].into()
    }

    #[test]
    fn a_documented_signature_reads_as_what_a_script_passes_and_gets() {
        let records = records();
        let cx = Cx {
            module: "ui",
            records: &records,
        };
        let read = |s| signature(s, &cx).unwrap_or_else(|| panic!("`{s}` does not read"));
        assert_eq!(read("()"), (vec![], Ty::Void), "no `->` gives nothing back");
        assert_eq!(read("(message: string)"), (vec![Ty::Str], Ty::Void));
        assert_eq!(
            read("(n: int?)"),
            (vec![Ty::Optional(Box::new(Ty::Int))], Ty::Void),
            "`nil` leaves it out"
        );
        assert_eq!(
            read("(node, on: bool)"),
            (vec![Ty::Node, Ty::Bool], Ty::Void)
        );
        assert_eq!(read("(on)"), (vec![Ty::Any], Ty::Void));
        assert_eq!(
            read("(path: string, lines: [int])"),
            (vec![Ty::Str, list(Ty::Int)], Ty::Void)
        );
        assert_eq!(read("(data: bytes)"), (vec![list(Ty::Int)], Ty::Void));
        assert!(
            signature("(data: bytes | string)", &cx).is_none(),
            "a function has one shape"
        );
        assert_eq!(
            read("(at: Transform3d) -> Vec3"),
            (
                vec![Ty::Struct("Transform3d".into())],
                Ty::Struct("Vec3".into())
            )
        );
        assert_eq!(
            read("(pattern: string, text: string) -> bool"),
            (vec![Ty::Str, Ty::Str], Ty::Bool)
        );
        assert_eq!(
            read("(from: string, to: string) -> [string]"),
            (vec![Ty::Str, Ty::Str], list(Ty::Str))
        );
        assert_eq!(
            read("(name: string) -> string?"),
            (vec![Ty::Str], Ty::Optional(Box::new(Ty::Str)))
        );
        assert_eq!(read("() -> map"), (vec![], Ty::Any));
        assert_eq!(read("() -> bytes"), (vec![], list(Ty::Int)));
        assert_eq!(
            read("() -> (float, float)"),
            (vec![], Ty::Struct("Vec2".into()))
        );
        assert_eq!(
            signature("(f: fn)", &cx),
            None,
            "a callback has no .bs type"
        );
    }

    #[test]
    fn a_record_is_named_bare_in_its_module_and_qualified_elsewhere() {
        let records = records();
        let ui = Cx {
            module: "ui",
            records: &records,
        };
        let rect = Ty::Struct("ui::Rect".into());
        assert_eq!(
            signature("(node: node) -> Rect", &ui),
            Some((vec![Ty::Node], rect.clone()))
        );
        assert_eq!(
            signature("() -> [scene::Warning]", &ui),
            Some((vec![], list(Ty::Struct("scene::Warning".into()))))
        );
        assert_eq!(
            result("-> Rect?", &ui),
            Some(Ty::Optional(Box::new(rect.clone())))
        );
        assert_eq!(
            result("(node: node) -> [Rect]", &ui),
            Some(list(rect.clone()))
        );
        assert_eq!(result("(node: node)", &ui), Some(Ty::Void), "nothing back");
        assert_eq!(result("", &ui), None, "no docs");
        assert_eq!(signature("() -> Nothing", &ui), None, "an unknown name");
        assert_eq!(
            fields("{ area: Rect, label: string?, tags: [string] }", &ui),
            Some(vec![
                ("area".into(), rect),
                ("label".into(), Ty::Optional(Box::new(Ty::Str))),
                ("tags".into(), list(Ty::Str)),
            ])
        );
    }
}
