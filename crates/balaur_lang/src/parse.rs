use crate::Error;
use crate::ast::{
    BinOp, Block, Expr, ExprKind, ExternDecl, Field, FnDecl, Param, Part, Pos, Program, Stmt,
    StmtKind, Ty, UnOp,
};

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String, Pos),
    P(&'static str),
    Eof,
}

const PUNCT: [&str; 34] = [
    "..", "->", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "::", "(", ")",
    "{", "}", "[", "]", ",", ";", ":", ".", "&", "+", "-", "*", "/", "%", "=", "<", ">", "!",
];

fn err<T>(pos: Pos, msg: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        pos,
        msg: msg.into(),
    })
}

fn lex(src: &str, at: Pos) -> Result<Vec<(Tok, Pos)>, Error> {
    let b = src.as_bytes();
    let (mut i, mut line, mut col) = (0usize, at.line, at.col);
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        let pos = Pos { line, col };
        if c == b'\n' {
            i += 1;
            line += 1;
            col = 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            col += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((Tok::Ident(src[start..i].to_string()), pos));
        } else if c.is_ascii_digit() {
            let (tok, end) = lex_number(src, start, pos)?;
            i = end;
            out.push((tok, pos));
        } else if c == b'"' {
            let (text, end) = lex_string(src, start, pos)?;
            i = end;
            out.push((Tok::Str(text, Pos { line, col: col + 1 }), pos));
        } else if let Some(p) = PUNCT.iter().find(|p| src[i..].starts_with(**p)) {
            i += p.len();
            out.push((Tok::P(p), pos));
        } else {
            return err(pos, format!("unexpected character `{}`", c as char));
        }
        col += (i - start) as u32;
    }
    out.push((Tok::Eof, Pos { line, col }));
    Ok(out)
}

// A number from `start`: an int, or a float when it has a fraction or an exponent.
fn lex_number(src: &str, start: usize, pos: Pos) -> Result<(Tok, usize), Error> {
    let b = src.as_bytes();
    let digits = |mut i: usize| {
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
            i += 1;
        }
        i
    };
    let mut i = digits(start);
    let mut float = false;
    if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
        float = true;
        i = digits(i + 1);
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        float = true;
        i += 1;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        i = digits(i);
    }
    let text = src[start..i].replace('_', "");
    let tok = if float {
        Tok::Float(text.parse().map_err(|_| Error {
            pos,
            msg: "bad number".into(),
        })?)
    } else {
        Tok::Int(text.parse().map_err(|_| Error {
            pos,
            msg: "integer too large".into(),
        })?)
    };
    Ok((tok, i))
}

// A string literal from its opening quote; returns its text and the index past the closing one.
fn lex_string(src: &str, start: usize, pos: Pos) -> Result<(String, usize), Error> {
    let b = src.as_bytes();
    let mut i = start + 1;
    let mut s = String::new();
    loop {
        match b.get(i) {
            None | Some(b'\n') => return err(pos, "unterminated string"),
            Some(b'"') => return Ok((s, i + 1)),
            Some(b'\\') => {
                let e = b.get(i + 1).copied().unwrap_or(b' ');
                s.push(match e {
                    b'n' => '\n',
                    b't' => '\t',
                    b'"' => '"',
                    b'\\' => '\\',
                    _ => {
                        return err(
                            Pos {
                                line: pos.line,
                                col: pos.col + (i - start) as u32,
                            },
                            format!("unknown escape `\\{}`", e as char),
                        );
                    }
                });
                i += 2;
            }
            Some(_) => {
                let ch = src[i..].chars().next().unwrap_or(' ');
                s.push(ch);
                i += ch.len_utf8();
            }
        }
    }
}

struct Parser {
    toks: Vec<(Tok, Pos)>,
    i: usize,
}

pub(crate) fn parse(src: &str) -> Result<Program, Error> {
    let mut p = Parser {
        toks: lex(src, Pos { line: 1, col: 1 })?,
        i: 0,
    };
    let mut prog = Program::default();
    while p.peek() != &Tok::Eof {
        p.item(&mut prog)?;
    }
    Ok(prog)
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i].0
    }
    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.i + n).min(self.toks.len() - 1)].0
    }
    fn pos(&self) -> Pos {
        self.toks[self.i].1
    }
    fn fail<T>(&self, msg: &str) -> Result<T, Error> {
        let found = match self.peek() {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Int(v) => v.to_string(),
            Tok::Float(v) => v.to_string(),
            Tok::Str(..) => "a string".into(),
            Tok::P(p) => format!("`{p}`"),
            Tok::Eof => "the end of the file".into(),
        };
        err(self.pos(), format!("{msg}, found {found}"))
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::P(x) if *x == p)
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Ident(x) if x == k)
    }
    fn eat(&mut self, p: &str) -> bool {
        let hit = self.is(p);
        self.i += usize::from(hit);
        hit
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        let hit = self.is_kw(k);
        self.i += usize::from(hit);
        hit
    }
    fn want(&mut self, p: &str) -> Result<(), Error> {
        if self.eat(p) {
            Ok(())
        } else {
            self.fail(&format!("expected `{p}`"))
        }
    }
    fn ident(&mut self) -> Result<String, Error> {
        if let Tok::Ident(s) = self.peek().clone() {
            self.i += 1;
            Ok(s)
        } else {
            self.fail("expected a name")
        }
    }

    fn item(&mut self, prog: &mut Program) -> Result<(), Error> {
        let public = self.eat_kw("pub");
        if self.eat_kw("struct") {
            let name = self.ident()?;
            self.want("{")?;
            let mut fields = Vec::new();
            while !self.eat("}") {
                let public = self.eat_kw("pub");
                let field = self.ident()?;
                self.want(":")?;
                let ty = self.ty()?;
                let default = if self.eat("=") {
                    Some(self.expr(false)?)
                } else {
                    None
                };
                fields.push(Field {
                    name: field,
                    ty,
                    public,
                    default,
                });
                if !self.eat(",") {
                    self.want("}")?;
                    break;
                }
            }
            prog.structs.push((name, fields));
        } else if self.eat_kw("impl") {
            let owner = self.ident()?;
            self.want("{")?;
            while !self.eat("}") {
                let public = self.eat_kw("pub");
                let f = self.func(public, Some(owner.clone()))?;
                prog.fns.push(f);
            }
        } else if self.eat_kw("extern") {
            let is_async = self.eat_kw("async");
            if !self.eat_kw("fn") {
                return self.fail("expected `fn` after `extern`");
            }
            let name = self.ident()?;
            let params = self.params()?;
            let ret = if self.eat("->") { self.ty()? } else { Ty::Void };
            self.want(";")?;
            prog.externs.push(ExternDecl {
                name,
                is_async,
                params,
                ret,
            });
        } else if self.eat_kw("const") {
            let name = self.ident()?;
            if self.eat(":") {
                self.ty()?;
            }
            self.want("=")?;
            let value = self.expr(false)?;
            self.want(";")?;
            prog.consts.push((name, value));
        } else {
            let f = self.func(public, None)?;
            prog.fns.push(f);
        }
        Ok(())
    }

    fn func(&mut self, public: bool, owner: Option<String>) -> Result<FnDecl, Error> {
        let pos = self.pos();
        let is_async = self.eat_kw("async");
        if !self.eat_kw("fn") {
            return self.fail("expected `fn`");
        }
        let name = self.ident()?;
        let params = self.params()?;
        let ret = if self.eat("->") { self.ty()? } else { Ty::Void };
        let body = self.block()?;
        Ok(FnDecl {
            name,
            owner,
            public,
            is_async,
            params,
            ret,
            body,
            pos,
        })
    }

    // A Rust-style `self`, `&self` or `&mut self` is accepted and means nothing:
    // every function in an `impl` already has a mutable `self`.
    fn params(&mut self) -> Result<Vec<Param>, Error> {
        self.want("(")?;
        let mut params = Vec::new();
        while !self.eat(")") {
            if self.eat("&") {
                self.eat_kw("mut");
            }
            if !self.eat_kw("self") {
                let name = self.ident()?;
                self.want(":")?;
                params.push(Param {
                    name,
                    ty: self.ty()?,
                });
            }
            if !self.eat(",") {
                self.want(")")?;
                break;
            }
        }
        Ok(params)
    }

    fn ty(&mut self) -> Result<Ty, Error> {
        if self.eat("[") {
            let t = self.ty()?;
            self.want("]")?;
            return Ok(Ty::Array(Box::new(t)));
        }
        let n = self.ident()?;
        Ok(match n.as_str() {
            "int" => Ty::Int,
            "float" => Ty::Float,
            "bool" => Ty::Bool,
            "str" => Ty::Str,
            "any" => Ty::Any,
            "Node" => Ty::Node,
            _ => Ty::Struct(n),
        })
    }

    fn block(&mut self) -> Result<Block, Error> {
        self.want("{")?;
        let mut b = Block::default();
        while !self.eat("}") {
            let pos = self.pos();
            let kind = if self.eat_kw("let") {
                self.eat_kw("mut");
                let name = self.ident()?;
                let ty = if self.eat(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.want("=")?;
                let init = self.expr(false)?;
                self.want(";")?;
                StmtKind::Let { name, ty, init }
            } else if self.eat_kw("while") {
                let cond = self.expr(true)?;
                StmtKind::While {
                    cond,
                    body: self.block()?,
                }
            } else if self.eat_kw("loop") {
                StmtKind::Loop(self.block()?)
            } else if self.eat_kw("for") {
                let var = self.ident()?;
                if !self.eat_kw("in") {
                    return self.fail("expected `in`");
                }
                let first = self.expr(true)?;
                if self.eat("..") {
                    let to = self.expr(true)?;
                    StmtKind::ForRange {
                        var,
                        from: first,
                        to,
                        body: self.block()?,
                    }
                } else {
                    StmtKind::ForEach {
                        var,
                        items: first,
                        body: self.block()?,
                    }
                }
            } else if self.eat_kw("return") {
                let v = if self.is(";") {
                    None
                } else {
                    Some(self.expr(false)?)
                };
                self.want(";")?;
                StmtKind::Return(v)
            } else if self.eat_kw("break") {
                self.want(";")?;
                StmtKind::Break
            } else if self.eat_kw("continue") {
                self.want(";")?;
                StmtKind::Continue
            } else {
                let block_like = self.is_kw("if");
                let e = self.expr(false)?;
                let op = match self.peek() {
                    Tok::P("=") => Some(None),
                    Tok::P("+=") => Some(Some(BinOp::Add)),
                    Tok::P("-=") => Some(Some(BinOp::Sub)),
                    Tok::P("*=") => Some(Some(BinOp::Mul)),
                    Tok::P("/=") => Some(Some(BinOp::Div)),
                    Tok::P("%=") => Some(Some(BinOp::Rem)),
                    _ => None,
                };
                if let Some(op) = op {
                    self.i += 1;
                    let value = self.expr(false)?;
                    self.want(";")?;
                    StmtKind::Assign {
                        target: e,
                        op,
                        value,
                    }
                } else if self.eat(";") || (block_like && !self.is("}")) {
                    StmtKind::Expr(e)
                } else if self.is("}") {
                    b.tail = Some(Box::new(e));
                    continue;
                } else {
                    return self.fail("expected `;`");
                }
            };
            b.stmts.push(Stmt { kind, pos });
        }
        Ok(b)
    }

    fn args(&mut self) -> Result<Vec<Expr>, Error> {
        self.want("(")?;
        let mut v = Vec::new();
        while !self.eat(")") {
            v.push(self.expr(false)?);
            if !self.eat(",") {
                self.want(")")?;
                break;
            }
        }
        Ok(v)
    }

    // `no_struct`: inside a condition `Name {` opens the body, not a struct, as in Rust.
    fn expr(&mut self, no_struct: bool) -> Result<Expr, Error> {
        self.binary(0, no_struct)
    }

    fn binary(&mut self, min: u8, ns: bool) -> Result<Expr, Error> {
        let mut lhs = self.unary(ns)?;
        loop {
            let (op, prec) = match self.peek() {
                Tok::P("||") => (BinOp::Or, 1),
                Tok::P("&&") => (BinOp::And, 2),
                Tok::P("==") => (BinOp::Eq, 3),
                Tok::P("!=") => (BinOp::Ne, 3),
                Tok::P("<") => (BinOp::Lt, 3),
                Tok::P("<=") => (BinOp::Le, 3),
                Tok::P(">") => (BinOp::Gt, 3),
                Tok::P(">=") => (BinOp::Ge, 3),
                Tok::P("+") => (BinOp::Add, 4),
                Tok::P("-") => (BinOp::Sub, 4),
                Tok::P("*") => (BinOp::Mul, 5),
                Tok::P("/") => (BinOp::Div, 5),
                Tok::P("%") => (BinOp::Rem, 5),
                _ => break,
            };
            if prec < min {
                break;
            }
            let pos = self.pos();
            self.i += 1;
            let rhs = self.binary(prec + 1, ns)?;
            lhs = Expr {
                kind: ExprKind::Bin(op, Box::new(lhs), Box::new(rhs)),
                pos,
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self, ns: bool) -> Result<Expr, Error> {
        let pos = self.pos();
        if self.eat("-") {
            return Ok(Expr {
                kind: ExprKind::Unary(UnOp::Neg, Box::new(self.unary(ns)?)),
                pos,
            });
        }
        if self.eat("!") {
            return Ok(Expr {
                kind: ExprKind::Unary(UnOp::Not, Box::new(self.unary(ns)?)),
                pos,
            });
        }
        if self.eat_kw("await") {
            return Ok(Expr {
                kind: ExprKind::Await(Box::new(self.unary(ns)?)),
                pos,
            });
        }
        let mut e = self.primary(ns)?;
        loop {
            let pos = self.pos();
            if self.eat(".") {
                let name = self.ident()?;
                if self.is("(") {
                    let args = self.args()?;
                    e = Expr {
                        kind: ExprKind::Method(Box::new(e), name, args),
                        pos,
                    };
                } else {
                    e = Expr {
                        kind: ExprKind::Field(Box::new(e), name),
                        pos,
                    };
                }
            } else if self.eat("[") {
                let idx = self.expr(false)?;
                self.want("]")?;
                e = Expr {
                    kind: ExprKind::Index(Box::new(e), Box::new(idx)),
                    pos,
                };
            } else if self.eat_kw("as") {
                let t = self.ty()?;
                e = Expr {
                    kind: ExprKind::Cast(Box::new(e), t),
                    pos,
                };
            } else {
                break;
            }
        }
        Ok(e)
    }

    // After the `[`: `[a, b]` or `[value; count]`.
    fn array(&mut self) -> Result<ExprKind, Error> {
        let mut items = Vec::new();
        if !self.eat("]") {
            let first = self.expr(false)?;
            if self.eat(";") {
                let n = self.expr(false)?;
                self.want("]")?;
                return Ok(ExprKind::ArrayRepeat(Box::new(first), Box::new(n)));
            }
            items.push(first);
            while self.eat(",") && !self.is("]") {
                items.push(self.expr(false)?);
            }
            self.want("]")?;
        }
        Ok(ExprKind::ArrayLit(items))
    }

    fn primary(&mut self, ns: bool) -> Result<Expr, Error> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            Tok::Int(v) => {
                self.i += 1;
                ExprKind::Int(v)
            }
            Tok::Float(v) => {
                self.i += 1;
                ExprKind::Float(v)
            }
            Tok::Str(s, at) => {
                self.i += 1;
                interpolate(&s, at)?
            }
            Tok::P("(") => {
                self.i += 1;
                let e = self.expr(false)?;
                self.want(")")?;
                return Ok(e);
            }
            Tok::P("[") => {
                self.i += 1;
                self.array()?
            }
            Tok::Ident(s) if s == "true" || s == "false" => {
                self.i += 1;
                ExprKind::Bool(s == "true")
            }
            Tok::Ident(s) if s == "self" => {
                self.i += 1;
                ExprKind::SelfRef
            }
            Tok::Ident(s) if s == "if" => {
                self.i += 1;
                let cond = self.expr(true)?;
                let then = self.block()?;
                let els = if self.eat_kw("else") {
                    if self.is_kw("if") {
                        let inner = self.primary(false)?;
                        Some(Block {
                            stmts: vec![],
                            tail: Some(Box::new(inner)),
                        })
                    } else {
                        Some(self.block()?)
                    }
                } else {
                    None
                };
                ExprKind::If(Box::new(cond), then, els)
            }
            Tok::Ident(s) => {
                self.i += 1;
                let mut s = s;
                while self.eat("::") {
                    s = format!("{s}::{}", self.ident()?);
                }
                if self.is("(") {
                    ExprKind::Call(s, self.args()?)
                } else if s.contains("::") {
                    return self.fail("expected `(` after a module function");
                } else if !ns
                    && self.is("{")
                    && s.starts_with(|c: char| c.is_ascii_uppercase())
                    && matches!(self.peek_at(1), Tok::Ident(_) | Tok::P("}"))
                {
                    self.i += 1;
                    let mut fields = Vec::new();
                    while !self.eat("}") {
                        let f = self.ident()?;
                        self.want(":")?;
                        fields.push((f, self.expr(false)?));
                        if !self.eat(",") {
                            self.want("}")?;
                            break;
                        }
                    }
                    ExprKind::StructLit(s, fields)
                } else {
                    ExprKind::Var(s)
                }
            }
            _ => return self.fail("expected an expression"),
        };
        Ok(Expr { kind, pos })
    }
}

// "hp {self.hp}" is parts; `{{` and `}}` are literal braces.
fn interpolate(s: &str, at: Pos) -> Result<ExprKind, Error> {
    if !s.contains(['{', '}']) {
        return Ok(ExprKind::Str(s.to_string()));
    }
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '{' if chars.peek().map(|p| p.1) == Some('{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek().map(|p| p.1) == Some('}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let close = s[i..].find('}').ok_or(Error {
                    pos: at,
                    msg: "`{` without `}` in a string".into(),
                })? + i;
                let inner_at = Pos {
                    line: at.line,
                    col: at.col + i as u32 + 1,
                };
                let mut p = Parser {
                    toks: lex(&s[i + 1..close], inner_at)?,
                    i: 0,
                };
                let e = p.expr(false)?;
                if p.peek() != &Tok::Eof {
                    return p.fail("expected `}` after the value");
                }
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                parts.push(Part::Value(e));
                while chars.peek().is_some_and(|p| p.0 <= close) {
                    chars.next();
                }
            }
            '}' => return err(at, "`}` without `{` in a string; write `}}`"),
            c => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(ExprKind::Interp(parts))
}
