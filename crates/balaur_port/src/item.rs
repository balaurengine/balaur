// A file's items: its struct (what `this` held), the structs its object
// literals make, consts, functions and methods, in source order with the
// comments between them.

use crate::cx::Cx;
use crate::expr::indent;
use crate::project::Project;
use crate::stmt::{block_diverges, fn_hint};
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;
use std::fmt::Write as _;

fn item_start(item: &ast::Item, cx: &Cx<'_>) -> usize {
    let _ = cx;
    match item {
        ast::Item::Fn(f) => f.name.span.range().start,
        ast::Item::Const(c) => c.name.span.range().start,
        other => other.span().range().start,
    }
}

/// Walks one file; in the writing round returns its `.bs`.
pub(crate) fn module(p: &mut Project, m: usize, file: &ast::File, learn: bool) -> String {
    p.modules[m].ccur = 0;
    if !learn {
        p.modules[m].needs.clear();
    }
    let script = p.modules[m].script;
    let mut out: Vec<String> = vec![];
    let mut impl_open = false;
    let mut after_impl: Vec<String> = vec![];
    let script_name = script.map(|s| p.shape_name(s)).unwrap_or_default();
    let mut header_comments: Option<Vec<String>> = None;
    for (item, _) in &file.items {
        let mut cx = Cx::new(p, m, learn);
        let start = item_start(item, &cx);
        if header_comments.is_none() {
            // The file's opening comment stays at the top, above the structs.
            let (head, rest) = cx.header_comments(start);
            header_comments = Some(head);
            push_comments(&mut out, impl_open, &rest);
        } else {
            let comments = cx.comments_before(start);
            push_comments(&mut out, impl_open, &comments);
        }
        match item {
            ast::Item::Fn(f) => {
                let name = cx.ident(&f.name);
                let Some(sig) = cx.p.modules[m].fns.get(&name).copied() else {
                    continue;
                };
                if script.is_some() && name == "exports" && !cx.p.sigs[sig].method {
                    cx.exports(f);
                    continue;
                }
                let method = cx.p.sigs[sig].method;
                if method != impl_open {
                    if impl_open {
                        out.push("}".into());
                        out.append(&mut after_impl);
                    } else {
                        out.push(format!("impl {script_name} {{"));
                    }
                    impl_open = method;
                }
                let text = cx.function(f, sig);
                let lifted = std::mem::take(&mut cx.lifted);
                if method {
                    out.push(indent(&text));
                    for (is_method, l) in lifted {
                        if is_method {
                            out.push(indent(&l));
                        } else {
                            after_impl.push(l);
                        }
                    }
                } else {
                    out.push(text);
                    for (_, l) in lifted {
                        out.push(l);
                    }
                }
            }
            ast::Item::Const(c) => {
                if impl_open {
                    out.push("}".into());
                    out.append(&mut after_impl);
                    impl_open = false;
                }
                out.push(cx.konst(c));
            }
            ast::Item::Use(_) => {}
            ast::Item::Mod(md) => {
                let name = cx.ident(&md.name);
                cx.note(
                    md.span(),
                    "mod",
                    format!(
                        "`mod {name};` declares a file of the crate: a `use` where it is called"
                    ),
                );
            }
            other => {
                if impl_open {
                    out.push("}".into());
                    out.append(&mut after_impl);
                    impl_open = false;
                }
                let text = cx.text(other.span()).to_string();
                cx.flag(
                    other.span(),
                    "item",
                    "an item the translator does not port (struct, enum, impl or macro)",
                );
                let notes = std::mem::take(&mut cx.notes);
                out.extend(notes);
                out.push(
                    text.lines()
                        .map(|l| format!("// {l}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
        }
    }
    if impl_open {
        out.push("}".into());
        out.append(&mut after_impl);
    }
    let mut cx = Cx::new(p, m, learn);
    let tail = cx.comments_before(usize::MAX);
    out.extend(tail);
    if learn {
        return String::new();
    }
    assemble(p, m, &header_comments.unwrap_or_default(), out)
}

fn push_comments(out: &mut Vec<String>, in_impl: bool, comments: &[String]) {
    if comments.is_empty() {
        return;
    }
    let text = comments.join("\n");
    out.push(if in_impl { indent(&text) } else { text });
}

fn assemble(p: &mut Project, m: usize, header: &[String], body: Vec<String>) -> String {
    let module = &p.modules[m];
    let mut s = String::new();
    let _ = write!(
        s,
        "// Ported from {} by balaur-port: a draft. Each {} is something\n// the translator could not map; fix those, then delete this line.\n",
        module.path,
        crate::MARK
    );
    if !header.is_empty() {
        s.push('\n');
        s.push_str(&header.join("\n"));
        s.push('\n');
    }
    let mut needs: Vec<String> = module.needs.iter().cloned().collect();
    // A struct another file declares, named in a type: that file is used.
    let text = body.join("\n");
    for key in shape_homes(p) {
        if key != module.key && !needs.contains(&key) && names_type_of(&text, &key) {
            needs.push(key);
        }
    }
    needs.sort();
    if !needs.is_empty() {
        s.push('\n');
        for n in needs {
            let _ = writeln!(s, "use {n};");
        }
    }
    let structs = structs(p, m);
    for st in structs {
        s.push('\n');
        s.push_str(&st);
        s.push('\n');
    }
    // A blank line between items, none after a comment that heads the next
    // item, none just inside an `impl`.
    let mut glued = false;
    for b in body {
        let is_comment = b.lines().all(|l| l.trim_start().starts_with("//"));
        if !glued && b != "}" {
            s.push('\n');
        }
        s.push_str(&b);
        s.push('\n');
        glued = is_comment || b.starts_with("impl ");
    }
    s
}

/// The files that declare a struct.
fn shape_homes(p: &Project) -> std::collections::BTreeSet<String> {
    p.shapes
        .data
        .iter()
        .filter_map(|s| s.home.map(|(m, _)| p.modules[m].key.clone()))
        .filter(|k| !k.contains("::"))
        .collect()
}

/// Whether `text` names one of `key`'s structs: `key::Upper`, not part of a
/// longer path.
fn names_type_of(text: &str, key: &str) -> bool {
    let pat = format!("{key}::");
    text.match_indices(&pat).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + pat.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':')
            && after.is_some_and(char::is_uppercase)
    })
}

/// The structs this file declares: its own, then those its literals make.
fn structs(p: &mut Project, m: usize) -> Vec<String> {
    let mut out = vec![];
    let mut ids: Vec<usize> = (0..p.shapes.data.len())
        .filter(|i| p.shapes.find(*i) == *i)
        .filter(|i| {
            let s = &p.shapes.data[*i];
            !p.shapes.is_free(*i)
                && p.reach.contains(i)
                && s.home.map(|h| h.0) == Some(m)
                && (!s.fields.is_empty() || s.script)
        })
        .collect();
    ids.sort_by_key(|i| {
        let s = &p.shapes.data[*i];
        (!s.script, s.home.map(|h| h.1))
    });
    for id in ids {
        let name = p.shape_name(id);
        let s = p.shapes.data[id].clone();
        let mut lines = vec![];
        let mut order = s.order.clone();
        if s.script {
            // Exported fields first, as a scene reads them, then the node.
            order.sort_by_key(|f| (!s.defaults.contains_key(f), f != "node"));
        }
        for f in &order {
            let t = s.fields.get(f).cloned().unwrap_or(Ty::Unknown);
            let nt = p.norm(&t);
            let ts = p.ty_str(&nt, m);
            let line = s
                .home
                .map_or(0, |h| p.modules[h.0].lines.line(h.1 as usize));
            if matches!(nt, Ty::Unknown | Ty::Nil) && !s.any_fields.contains(f) {
                lines.push(format!(
                    "// {}: no type was inferred for `{f}`",
                    crate::MARK
                ));
                p.modules[m].flag(
                    line,
                    "field-type",
                    format!("`{name}.{f}`: no type inferred"),
                );
            }
            if let Ty::Fn(_) = nt {
                p.modules[m].note(line, "fn-field", format!("`{name}.{f}` holds a function"));
            }
            let field = match s.defaults.get(f) {
                Some(d) => format!("pub {f}: {ts} = {d},"),
                None => format!("{f}: {ts},"),
            };
            lines.push(field);
        }
        let vis = "pub ";
        let kind = if crate::project::class_name(&name) && !s.script {
            "class"
        } else {
            "struct"
        };
        out.push(format!(
            "{vis}{kind} {name} {{\n{}\n}}",
            indent(&lines.join("\n"))
        ));
    }
    out
}

impl Cx<'_> {
    /// `pub fn exports() { #{ speed: 2.0 } }`: the struct's tunable fields.
    fn exports(&mut self, f: &ast::ItemFn) {
        let Some(script) = self.p.modules[self.m].script else {
            return;
        };
        let Some(ast::Stmt::Expr(ast::Expr::Object(o))) = f.body.statements.last() else {
            self.flag(
                f.span(),
                "exports",
                "`exports()` that is not one object literal",
            );
            return;
        };
        for (fa, _) in &o.assignments {
            let key = match &fa.key {
                ast::ObjectKey::Path(p) => self.text(p.span()).trim().to_string(),
                ast::ObjectKey::LitStr(s) => self.text(s.span).trim_matches('"').to_string(),
                other => self.text(other.span()).to_string(),
            };
            let Some((_, v)) = &fa.assign else { continue };
            let ex = self.expr(v);
            if self.rec() {
                self.p.shapes.set_field(script, &key, ex.t.clone());
            }
            self.p.shapes.get_mut(script).defaults.insert(key, ex.s);
        }
        let _ = self.comments_before(f.body.close.span.range().end);
    }

    fn konst(&mut self, c: &ast::ItemConst) -> String {
        let name = self.ident(&c.name);
        self.hint = Some(crate::project::name_hint(&name.to_lowercase()));
        let ex = self.expr(&c.expr);
        self.hint = None;
        let notes = std::mem::take(&mut self.notes);
        let pre = std::mem::take(&mut self.pre);
        let (literal, public) = {
            let k = &self.p.modules[self.m].consts[&name];
            (k.literal, k.public)
        };
        if self.rec() {
            let old = self.p.modules[self.m].consts[&name].ty.clone();
            let j = self.p.shapes.join(&old, &ex.t);
            if j != old {
                if let Some(c) = self.p.modules[self.m].consts.get_mut(&name) {
                    c.ty = j;
                }
                self.p.changed = true;
            }
        }
        let t = self.p.norm(&self.p.modules[self.m].consts[&name].ty);
        let ts = self.ty_str(&t);
        let vis = if public { "pub " } else { "" };
        let mut lines = notes;
        if literal {
            lines.push(format!("{vis}const {name}: {ts} = {};", ex.s));
        } else {
            self.note(c.span(), "const-fn", format!("`{name}` is not a literal, so it is a function: .bs takes only literals for `const`"));
            let mut body = pre;
            body.push(self.coerce(&ex, &t));
            lines.push(format!(
                "{vis}fn {name}() -> {ts} {{\n{}\n}}",
                indent(&body.join("\n"))
            ));
        }
        lines.join("\n")
    }

    pub(crate) fn function(&mut self, f: &ast::ItemFn, sig: usize) -> String {
        self.sig = Some(sig);
        self.method = self.p.sigs[sig].method;
        self.push_scope();
        if self.method {
            self.bind("this", false);
        }
        let names: Vec<String> = self.p.sigs[sig]
            .params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let outs: Vec<String> = names.iter().map(|n| self.bind(n, false)).collect();
        let ret_now = self.p.norm(&self.p.sigs[sig].ret);
        let value = self.learn || !matches!(ret_now, Ty::Void | Ty::Unknown | Ty::Nil);
        let saved_hint = self.hint.take();
        self.hint = None;
        let (body, t) = {
            // A literal in tail position is named for the function.
            let tail_hint = fn_hint(&self.p.modules[self.m].key, &self.p.sigs[sig].name);
            self.tail_hint = Some(tail_hint);
            let r = self.block(&f.body, value);
            self.tail_hint = None;
            r
        };
        self.hint = saved_hint;
        if !block_diverges(&f.body) {
            self.learn_ret(&t);
        }
        self.pop_scope();
        let s = &self.p.sigs[sig];
        let mut header_notes = vec![];
        let mut async_writes = vec![];
        let mut params = vec![];
        for (p, o) in s.params.iter().zip(&outs) {
            let t = self.p.norm(&p.ty);
            if matches!(t, Ty::Unknown | Ty::Nil) && !p.saw_any && !p.name.starts_with('_') {
                header_notes.push(p.name.clone());
            }
            // A struct or list the function writes, or hands on to be
            // written, is the caller's own: Rune shared it.
            let i = s
                .params
                .iter()
                .position(|q| std::ptr::eq(q, p))
                .unwrap_or(0);
            let by_ref = p.mutated && matches!(t, Ty::Shape(_) | Ty::Array(_));
            let by_ref = by_ref || self.p.by_ref(s, i);
            if by_ref && s.is_async {
                async_writes.push(p.name.clone());
            }
            let amp = if by_ref && !s.is_async { "&mut " } else { "" };
            params.push(format!("{o}: {amp}{}", self.p.ty_str(&t, self.m)));
        }
        let ret = self.p.norm(&s.ret);
        let ret_text = match &ret {
            // Only ever `nil`: what a caller gets instead of a value, an `any`.
            Ty::Nil => " -> any".to_string(),
            Ty::Void | Ty::Unknown => String::new(),
            t => format!(" -> {}", self.p.ty_str(t, self.m)),
        };
        let vis = if s.public { "pub " } else { "" };
        let asy = if s.is_async { "async " } else { "" };
        let out_name = s.out.clone();
        let mut lines = std::mem::take(&mut self.notes);
        for n in async_writes {
            let span = f.name.span;
            self.flag(
                span,
                "async-write",
                format!("`{n}` is written here, but an `async fn` takes no `&mut`: pass it back or keep it on `self`"),
            );
            lines.extend(std::mem::take(&mut self.notes));
        }
        for n in header_notes {
            let span = f.name.span;
            self.flag(
                span,
                "param-type",
                format!("no type was inferred for parameter `{n}`"),
            );
            lines.extend(std::mem::take(&mut self.notes));
        }
        lines.push(format!(
            "{vis}{asy}fn {out_name}({}){ret_text} {body}",
            params.join(", ")
        ));
        lines.join("\n")
    }
}
