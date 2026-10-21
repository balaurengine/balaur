// Literals with parts: objects become structs (or `any` maps where the
// data is free-form), arrays stay arrays, tuples become `_0`/`_1` structs.

use crate::cx::{Cx, Ex};
use crate::expr::indent;
use crate::project::default_of;
use crate::source::{pascal, singular};
use crate::ty::{Shape, Ty};
use rune::ast;
use rune::ast::Spanned;
use std::fmt::Write as _;

impl Cx<'_> {
    pub(crate) fn object(&mut self, o: &ast::ExprObject) -> Ex {
        let span = o.span();
        let named = matches!(o.ident, ast::ObjectIdent::Named(_));
        if named {
            self.flag(span, "struct", "a Rune struct literal");
        }
        let src_lines = self.text(span).lines().count();
        let hint = self.hint.take();
        let engine_arg = std::mem::take(&mut self.engine_arg);
        let (fields, weird_key) = self.object_fields(o);
        // One field a line only where the source had about that.
        let multi = src_lines > 2 && src_lines * 2 >= fields.len();
        let id = if engine_arg || named || weird_key {
            None
        } else {
            self.literal_shape(span, fields.len(), false, hint.as_ref())
        };
        // Walk the values, naming nested literals by their keys.
        let mut vals: Vec<(String, Ex, Vec<String>)> = vec![];
        for (k, v, fspan) in &fields {
            let before = if multi {
                self.comments_before(fspan.range().start)
            } else {
                vec![]
            };
            let saved = self.hint.replace(crate::project::name_hint(k));
            let ex = match v {
                Some(v) => self.expr(v),
                None => self
                    .resolve_short(k)
                    .unwrap_or_else(|| Ex::atom(k.clone(), Ty::Unknown)),
            };
            self.hint = saved;
            if let Some(id) = id
                && self.rec()
            {
                self.p.shapes.set_field(id, k, ex.t.clone());
            }
            vals.push((k.clone(), ex, before));
        }
        if let Some(Ty::Shape(id)) = id.map(|id| self.p.norm(&Ty::Shape(id))) {
            return self.struct_literal(id, &vals, multi);
        }
        if !engine_arg
            && !self.learn
            && let Some(id) = id
        {
            let why = self.p.shapes.free_reason(id);
            self.note(span, "any", format!("an object kept as `any`: {why}"));
        }
        Ex::atom(map_literal(&vals, multi), Ty::Any)
    }

    /// An object's keys and values, and whether a key is not a name.
    #[allow(clippy::type_complexity)] // one tuple a field, read where it is made
    fn object_fields<'e>(
        &self,
        o: &'e ast::ExprObject,
    ) -> (Vec<(String, Option<&'e ast::Expr>, ast::Span)>, bool) {
        let mut fields = vec![];
        let mut weird_key = false;
        for (fa, _) in &o.assignments {
            let (key, ok) = match &fa.key {
                ast::ObjectKey::Path(p) => (self.text(p.span()).trim().to_string(), true),
                ast::ObjectKey::LitStr(s) => {
                    let raw = self.text(s.span).trim_matches('"').to_string();
                    let ok = raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && raw.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
                    (raw, ok)
                }
                other => (self.text(other.span()).to_string(), false),
            };
            weird_key |= !ok;
            // `#{ a }` is `#{ a: a }`: no value, and the key names it.
            fields.push((key, fa.assign.as_ref().map(|(_, v)| v), fa.span()));
        }
        (fields, weird_key)
    }

    /// The shape of the literal at `span`, made on its first walk.
    fn literal_shape(
        &mut self,
        span: ast::Span,
        keys: usize,
        tuple: bool,
        hint: Option<&(u8, String)>,
    ) -> Option<usize> {
        let key = (self.m, span.range().start);
        let at = span.range().start as u32;
        let id = if let Some(id) = self.p.lit_shapes.get(&key) {
            *id
        } else if self.rec() {
            let id = self.p.shapes.add(Shape {
                free: self.p.free_sites.get(&key).cloned(),
                home: Some((self.m, at)),
                tuple,
                literals: 1,
                sites: [(self.m, (1, at, keys))].into_iter().collect(),
                site_keys: [((self.m, at), keys)].into_iter().collect(),
                ..Default::default()
            });
            self.p.lit_shapes.insert(key, id);
            id
        } else {
            return None;
        };
        if let (Some((strength, name)), true) = (hint, self.rec()) {
            self.p.shapes.hint(id, *strength, name);
        }
        Some(id)
    }

    /// `Name { .. }` naming every field, in the declaration's order.
    fn struct_literal(&self, id: usize, vals: &[(String, Ex, Vec<String>)], multi: bool) -> Ex {
        let name = self.ty_str(&Ty::Shape(id));
        let order = self.p.shapes.get(id).order.clone();
        let mut parts = vec![];
        for f in &order {
            let want = self.p.shapes.field(id, f).unwrap_or(Ty::Unknown);
            match vals.iter().find(|(k, _, _)| k == f) {
                Some((_, ex, before)) => {
                    let v = self.coerce(ex, &want);
                    let mut part = before.join("\n");
                    if !part.is_empty() {
                        part.push('\n');
                    }
                    let _ = write!(part, "{f}: {v}");
                    parts.push(part);
                }
                None => parts.push(format!("{f}: {}", self.default_value(&want, 0))),
            }
        }
        Ex::atom(layout(&name, &parts, multi, ", ", " "), Ty::Shape(id))
    }

    pub(crate) fn resolve_short(&mut self, k: &str) -> Option<Ex> {
        let v = self.lookup(k)?.clone();
        let t = self.local_ty(&v.name);
        Some(Ex::atom(v.out, t))
    }

    /// A value of `t`, for a field a literal left out.
    pub(crate) fn default_value(&self, t: &Ty, depth: u32) -> String {
        match self.p.norm(t) {
            Ty::Shape(id) if depth < 3 => {
                let name = self.ty_str(&Ty::Shape(id));
                let s = self.p.shapes.get(id);
                let parts: Vec<String> = s
                    .order
                    .iter()
                    .map(|f| {
                        let ft = s.fields.get(f).cloned().unwrap_or(Ty::Unknown);
                        format!("{f}: {}", self.default_value(&ft, depth + 1))
                    })
                    .collect();
                format!("{name} {{ {} }}", parts.join(", "))
            }
            t => default_of(&t),
        }
    }

    pub(crate) fn vec(&mut self, v: &ast::ExprVec) -> Ex {
        let multi = self.text(v.span()).contains('\n');
        let hint = self
            .hint
            .take()
            .map(|(s, n)| (s, pascal(&singular(&n.to_lowercase()))));
        let mut items = vec![];
        let mut t = Ty::Unknown;
        for (e, _) in &v.items {
            let before = if multi {
                self.comments_before(e.span().range().start)
            } else {
                vec![]
            };
            self.hint.clone_from(&hint);
            let ex = self.expr(e);
            self.hint = None;
            t = self.p.shapes.join(&t, &ex.t);
            let mut s = ex.s;
            if !before.is_empty() {
                s = format!("{}\n{s}", before.join("\n"));
            }
            items.push(s);
        }
        let t = self.p.norm(&t);
        let text = if multi && !items.is_empty() {
            format!("[\n{},\n]", indent(&items.join(",\n")))
        } else {
            format!("[{}]", items.join(", "))
        };
        Ex::atom(text, Ty::Array(Box::new(t)))
    }

    pub(crate) fn tuple(&mut self, t: &ast::ExprTuple) -> Ex {
        if t.items.is_empty() {
            return Ex::atom("nil", Ty::Nil);
        }
        let span = t.span();
        let hint = self.hint.take();
        let id = self.literal_shape(span, t.items.len(), true, hint.as_ref());
        let mut vals = vec![];
        for (i, (e, _)) in t.items.iter().enumerate() {
            let ex = self.expr(e);
            if let Some(id) = id
                && self.rec()
            {
                self.p.shapes.set_field(id, &format!("_{i}"), ex.t.clone());
            }
            vals.push(ex);
        }
        if let Some(Ty::Shape(id)) = id.map(|id| self.p.norm(&Ty::Shape(id))) {
            let name = self.ty_str(&Ty::Shape(id));
            let parts: Vec<String> = vals
                .iter()
                .enumerate()
                .map(|(i, ex)| {
                    let want = self
                        .p
                        .shapes
                        .field(id, &format!("_{i}"))
                        .unwrap_or(Ty::Unknown);
                    format!("_{i}: {}", self.coerce(ex, &want))
                })
                .collect();
            Ex::atom(format!("{name} {{ {} }}", parts.join(", ")), Ty::Shape(id))
        } else {
            let parts: Vec<String> = vals.iter().map(|v| v.s.clone()).collect();
            self.flag(
                span,
                "tuple",
                "a tuple of mixed kinds, kept as an `any` list",
            );
            Ex::atom(format!("[{}]", parts.join(", ")), Ty::Any)
        }
    }
}

/// `map().with("k", v)..`: an object kept as `any`.
fn map_literal(vals: &[(String, Ex, Vec<String>)], multi: bool) -> String {
    let mut s = "map()".to_string();
    for (k, ex, before) in vals {
        let line = format!(".with(\"{k}\", {})", ex.s);
        if multi {
            for b in before {
                s.push('\n');
                s.push_str(&indent(b));
            }
            s.push('\n');
            s.push_str(&indent(&line));
        } else {
            s.push_str(&line);
        }
    }
    s
}

/// `Name { a: 1, b: 2 }`, on one line or one field a line.
fn layout(name: &str, parts: &[String], multi: bool, sep: &str, pad: &str) -> String {
    if parts.is_empty() {
        return format!("{name} {{}}");
    }
    let one = format!("{name} {{{pad}{}{pad}}}", parts.join(sep));
    if !multi && !one.contains('\n') && one.len() <= 140 {
        return one;
    }
    format!("{name} {{\n{},\n}}", indent(&parts.join(",\n")))
}
