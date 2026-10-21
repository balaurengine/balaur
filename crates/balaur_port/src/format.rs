// `format!` and template strings, as `.bs` interpolation: `"{x}"`.

use crate::cx::{ATOM, Cx, Ex};
use crate::source::bs_string_body;
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;

impl Cx<'_> {
    pub(crate) fn macro_call(&mut self, mc: &ast::MacroCall) -> Ex {
        let is_template = matches!(
            &mc.path.first,
            ast::PathSegment::Ident(ast::Ident {
                source: ast::LitSource::BuiltIn(ast::BuiltIn::Template),
                ..
            })
        );
        let name = if is_template {
            "template".to_string()
        } else {
            self.text(mc.path.span()).to_string()
        };
        let Some(args) = macro_args(mc) else {
            self.flag(
                mc.span(),
                "macro",
                format!("`{name}!` with arguments the translator cannot read"),
            );
            return Ex::atom("\"\"", Ty::Str);
        };
        match name.as_str() {
            "template" => self.template(&args),
            "format" => self.format(mc, &args),
            "println" | "print" | "dbg" | "eprintln" => {
                let inner = if args.is_empty() {
                    Ex::atom("\"\"", Ty::Str)
                } else {
                    self.format(mc, &args)
                };
                Ex::new(format!("log::info({})", inner.s), Ty::Void, ATOM)
            }
            "panic" | "unreachable" | "todo" => {
                let inner = if args.is_empty() {
                    Ex::atom("\"\"", Ty::Str)
                } else {
                    self.format(mc, &args)
                };
                self.flag(
                    mc.span(),
                    "panic",
                    format!("`{name}!`: .bs has no panic; this logs instead"),
                );
                Ex::new(format!("log::error({})", inner.s), Ty::Void, ATOM)
            }
            _ => {
                let first = self
                    .text(mc.span())
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                self.flag(mc.span(), "macro", format!("`{first}`: .bs has no macros"));
                Ex::atom("nil", Ty::Any)
            }
        }
    }

    /// `` `a ${b}` ``: its text parts and its values, as `"a {b}"`.
    fn template(&mut self, args: &[ast::Expr]) -> Ex {
        let mut out = String::from("\"");
        for a in args {
            if let ast::Expr::Lit(ast::ExprLit {
                lit: ast::Lit::Str(s),
                ..
            }) = a
            {
                let raw = self.text(s.span).to_string();
                if !raw.starts_with('"') {
                    let (body, problem) = bs_string_body(&raw, true);
                    if let Some(p) = problem {
                        self.flag(s.span, "string", p);
                    }
                    out.push_str(&body);
                    continue;
                }
            }
            let ex = self.expr(a);
            out.push_str(&self.interp(&ex, a.span()));
        }
        out.push('"');
        Ex::atom(out, Ty::Str)
    }

    /// One `{..}` of an interpolated string.
    pub(crate) fn interp(&mut self, ex: &Ex, span: ast::Span) -> String {
        if ex.s.contains('\n') || ex.s.contains('"') && ex.s.contains('{') {
            // Hoisted: an `if` or a literal with braces inside `{..}` reads badly.
            if self.no_hoist == 0 {
                let name = self.fresh("part");
                self.pre.push(format!("let {name} = {};", ex.s));
                return format!("{{{name}}}");
            }
            self.flag(
                span,
                "interp",
                "a value too complex to interpolate in place",
            );
        }
        if let Ty::Shape(_) | Ty::Array(_) = ex.t {
            self.flag(
                span,
                "interp",
                format!(
                    "interpolating a `{}`: .bs writes only numbers, bools and strings",
                    self.ty_str(&ex.t)
                ),
            );
        }
        format!("{{{}}}", ex.s)
    }

    pub(crate) fn format(&mut self, mc: &ast::MacroCall, args: &[ast::Expr]) -> Ex {
        let Some(first) = args.first() else {
            return Ex::atom("\"\"", Ty::Str);
        };
        let Some(fmt) = self.lit_str_raw(first) else {
            self.flag(
                mc.span(),
                "format",
                "`format!` whose format is not a literal",
            );
            return Ex::atom("\"\"", Ty::Str);
        };
        let mut out = String::from("\"");
        let mut next = 1;
        let mut chars = fmt.chars().peekable();
        let mut text = String::new();
        let flush = |text: &mut String, out: &mut String, cx: &mut Self| {
            if !text.is_empty() {
                let (body, problem) = bs_string_body(text, false);
                if let Some(p) = problem {
                    cx.flag(mc.span(), "string", p);
                }
                out.push_str(&body);
                text.clear();
            }
        };
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    text.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    text.push('}');
                }
                '{' => {
                    flush(&mut text, &mut out, self);
                    let mut spec = String::new();
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                        spec.push(c);
                    }
                    let (name, fmtspec) = spec.split_once(':').unwrap_or((&spec, ""));
                    if !fmtspec.is_empty() {
                        self.flag(
                            mc.span(),
                            "format-spec",
                            format!("`{{:{fmtspec}}}`: .bs interpolation has no format specs"),
                        );
                    }
                    let ex = if name.is_empty() {
                        let e = args.get(next);
                        next += 1;
                        e.map(|e| (self.expr(e), e.span()))
                    } else if let Ok(i) = name.parse::<usize>() {
                        args.get(i + 1).map(|e| (self.expr(e), e.span()))
                    } else {
                        let v = self.lookup(name).cloned();
                        v.map(|v| (Ex::atom(v.out.clone(), self.local_ty(&v.name)), mc.span()))
                    };
                    match ex {
                        Some((ex, span)) => out.push_str(&self.interp(&ex, span)),
                        None => {
                            self.flag(
                                mc.span(),
                                "format",
                                format!("`{{{spec}}}` names no argument"),
                            );
                        }
                    }
                }
                c => text.push(c),
            }
        }
        flush(&mut text, &mut out, self);
        out.push('"');
        Ex::atom(out, Ty::Str)
    }

    /// The raw body of a string literal (escapes as written).
    pub(crate) fn lit_str_raw(&self, e: &ast::Expr) -> Option<String> {
        if let ast::Expr::Lit(l) = e
            && let ast::Lit::Str(s) = &l.lit
        {
            let raw = self.text(s.span);
            return Some(raw.strip_prefix('"')?.strip_suffix('"')?.to_string());
        }
        None
    }
}

/// A macro's arguments, comma-separated expressions; `None` when they are
/// something else.
fn macro_args(mc: &ast::MacroCall) -> Option<Vec<ast::Expr>> {
    let span = ast::Span::new(
        mc.open.span.range().start as u32,
        mc.close.span.range().end as u32,
    );
    let mut parser = rune::parse::Parser::from_token_stream(&mc.input, span);
    let mut args = vec![];
    while !parser.is_eof().unwrap_or(true) {
        args.push(parser.parse::<ast::Expr>().ok()?);
        if !parser.try_consume::<ast::Comma>().unwrap_or(false) && !parser.is_eof().unwrap_or(true)
        {
            return None;
        }
    }
    Some(args)
}
