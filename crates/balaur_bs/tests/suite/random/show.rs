//! A generated program as Balaur Script source.

use super::{DECLARATIONS, E, Func, S, T};
use std::fmt::Write as _;

fn ty(t: T) -> &'static str {
    match t {
        T::Int => "int",
        T::Float => "float",
        T::Bool => "bool",
        T::Str => "str",
        T::Ints => "[int]",
        T::Point => "Point",
        T::Shade => "Shade",
        T::Ages => "{str: int}",
        T::Maybe => "int?",
        T::Any => "any",
    }
}

fn list(items: &[E]) -> String {
    items.iter().map(expr).collect::<Vec<_>>().join(", ")
}

// What a method, an index or a key is read off: bare when it binds tighter
// than `.`, else in parentheses.
fn receiver(e: &E) -> String {
    match e {
        E::Var(_) | E::Call(..) | E::Method(..) | E::Index(..) | E::NilAny | E::MapAny => expr(e),
        _ => format!("({})", expr(e)),
    }
}

pub(super) fn expr(e: &E) -> String {
    match e {
        E::Int(i) if *i == i64::MIN => "(-9223372036854775807 - 1)".into(),
        E::Int(i) if *i < 0 => format!("({i})"),
        E::Int(i) => i.to_string(),
        E::Float(f) => {
            let text = format!("{f:?}");
            let text = if text.contains(['.', 'e', 'E']) {
                text
            } else {
                format!("{text}.0")
            };
            if f.is_sign_negative() {
                format!("({text})")
            } else {
                text
            }
        }
        E::Bool(b) => b.to_string(),
        E::Str(s) => format!("\"{s}\""),
        E::Var(v) => v.clone(),
        E::Bin(op, a, b) => format!("({} {} {})", expr(a), op.text(), expr(b)),
        E::Neg(a) => format!("(-{})", expr(a)),
        E::Not(a) => format!("(!{})", expr(a)),
        E::If(c, a, b) => format!("(if {} {{ {} }} else {{ {} }})", expr(c), expr(a), expr(b)),
        E::Call(f, args) => format!("{f}({})", list(args)),
        E::Len(a) => format!("{a}.len()"),
        E::At(a, i) => format!("{a}[((({}) % {a}.len()) + {a}.len()) % {a}.len()]", expr(i)),
        E::StrLen(s) => format!("({}).len()", expr(s)),
        E::Interp(pieces) => {
            let mut out = String::from("\"");
            for (text, v) in pieces {
                let _ = write!(out, "{text}{{{v}}}");
            }
            out.push('"');
            out
        }
        E::Array(items) => format!("[{}]", list(items)),
        E::Point(x, y) => format!("Point {{ x: {}, y: {} }}", expr(x), expr(y)),
        E::Field(v, f) => format!("{v}.{f}"),
        E::Dark => "Shade::Dark".into(),
        E::Lit(e) => format!("Shade::Lit({})", expr(e)),
        E::Match(v, dark, n, lit) => format!(
            "(match {v} {{ Shade::Dark => {}, Shade::Lit({n}) => {} }})",
            expr(dark),
            expr(lit)
        ),
        E::Ages(pairs) => format!(
            "{{{}}}",
            pairs
                .iter()
                .map(|(k, e)| format!("\"{k}\": {}", expr(e)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        E::Get(v, k, d) => format!("{v}[\"{k}\"].or({})", expr(d)),
        E::Count(v) => format!("{v}.len()"),
        E::Nil => "nil".into(),
        E::Or(v, d) => format!("{v}.or({})", expr(d)),
        E::Method(r, name, args) => format!("{}.{name}({})", receiver(r), list(args)),
        E::Index(r, k) => format!("{}[{}]", receiver(r), expr(k)),
        E::NilAny => "any_nil()".into(),
        E::MapAny => "any_map()".into(),
    }
}

fn stmts(body: &[S], indent: usize, out: &mut String) {
    let pad = "    ".repeat(indent);
    for st in body {
        match st {
            S::Let(n, e) => {
                let _ = writeln!(out, "{pad}let {n} = {};", expr(e));
            }
            S::Set(n, e) => {
                let _ = writeln!(out, "{pad}{n} = {};", expr(e));
            }
            S::Push(n, e) => {
                let _ = writeln!(out, "{pad}{n}.push({});", expr(e));
            }
            S::SetAt(n, i, e) => {
                let _ = writeln!(
                    out,
                    "{pad}{n}[((({}) % {n}.len()) + {n}.len()) % {n}.len()] = {};",
                    expr(i),
                    expr(e)
                );
            }
            S::If(c, a, b) => {
                let _ = writeln!(out, "{pad}if {} {{", expr(c));
                stmts(a, indent + 1, out);
                let _ = writeln!(out, "{pad}}} else {{");
                stmts(b, indent + 1, out);
                let _ = writeln!(out, "{pad}}}");
            }
            S::For(i, k, body) => {
                let _ = writeln!(out, "{pad}for {i} in 0..{k} {{");
                stmts(body, indent + 1, out);
                let _ = writeln!(out, "{pad}}}");
            }
            S::While(c, k, body) => {
                let _ = writeln!(out, "{pad}let {c} = 0;\n{pad}while {c} < {k} {{");
                stmts(body, indent + 1, out);
                let _ = writeln!(out, "{pad}    {c} += 1;\n{pad}}}");
            }
            S::LetTyped(n, t, e) => {
                let _ = writeln!(out, "{pad}let {n}: {} = {};", ty(*t), expr(e));
            }
            S::SetField(v, f, e) => {
                let _ = writeln!(out, "{pad}{v}.{f} = {};", expr(e));
            }
            S::SetKey(v, k, e) => {
                let _ = writeln!(out, "{pad}{v}[\"{k}\"] = {};", expr(e));
            }
            S::Put(target, e) => {
                let _ = writeln!(out, "{pad}{} = {};", expr(target), expr(e));
            }
            S::Do(e) => {
                let _ = writeln!(out, "{pad}{};", expr(e));
            }
            S::Each(x, r, body) => {
                let _ = writeln!(out, "{pad}for {x} in {} {{", receiver(r));
                stmts(body, indent + 1, out);
                let _ = writeln!(out, "{pad}}}");
            }
        }
    }
}

pub(super) fn source(funcs: &[Func]) -> String {
    let mut out = String::from(DECLARATIONS);
    for f in funcs {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                let by_ref = if p.by_ref { "&mut " } else { "" };
                format!("{}: {by_ref}{}", p.name, ty(p.t))
            })
            .collect();
        let public = if f.name == "main" { "pub " } else { "" };
        let _ = writeln!(
            out,
            "{public}fn {}({}) -> {} {{",
            f.name,
            params.join(", "),
            ty(f.ret)
        );
        stmts(&f.body, 1, &mut out);
        let _ = writeln!(out, "    {}\n}}", expr(&f.tail));
    }
    out
}
