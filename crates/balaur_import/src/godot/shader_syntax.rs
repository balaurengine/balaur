//! The parse half of the shader translation in `godot::shader`:
//! Godot's shading language as tokens, then as statements and expressions.
//! Only the shapes a `canvas_item` shader writes are read; the rest fail
//! with what was found.

use std::collections::BTreeSet;

use anyhow::{Result, anyhow, bail};

use crate::godot::shader::Uniform;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Ident(String),
    Num(String),
    Punct(&'static str),
}

pub(crate) const PUNCTS: &[&str] = &[
    "<<=", ">>=", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "==", "!=", "<=",
    ">=", "&&", "||", "^^", "<<", ">>", "+", "-", "*", "/", "%", "=", "<", ">", "!", "~", "&", "|",
    "^", "?", ":", ";", ",", ".", "(", ")", "[", "]", "{", "}",
];

pub(crate) fn lex(source: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if c == '#' {
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            let line: String = chars[start..i].iter().collect();
            bail!("the preprocessor has no equivalent: `{}`", line.trim());
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric()
                    || chars[i] == '.'
                    || ((chars[i] == '-' || chars[i] == '+')
                        && matches!(chars[i - 1], 'e' | 'E')
                        && !chars[start..i].contains(&'x')))
            {
                i += 1;
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
        } else if c == '"' {
            // Only a hint's argument is ever a string, and hints are dropped.
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            i += 1;
            out.push(Tok::Ident("_string".into()));
        } else {
            let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
            let Some(punct) = PUNCTS.iter().find(|p| rest.starts_with(**p)) else {
                bail!("unexpected character `{c}`");
            };
            out.push(Tok::Punct(punct));
            i += punct.len();
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Ident(String),
    Num(String),
    Call(String, Vec<Expr>),
    /// `type[n](…)`; the count is the arguments' when left out.
    Array(String, Option<usize>, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Assign(&'static str, Box<Expr>, Box<Expr>),
    /// `x++` and `++x` alike: only ever a statement here.
    Step(&'static str, Box<Expr>),
}

/// An array suffix: `[n]`, or `[]` sized by its initializer.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Array {
    Sized(usize),
    Unsized,
}

/// One struct field: its type, its name, and its array suffix.
pub(crate) type Field = (String, String, Option<Array>);

#[derive(Clone, Debug)]
pub(crate) struct Declarator {
    pub(crate) name: String,
    pub(crate) array: Option<Array>,
    pub(crate) init: Option<Expr>,
}

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    Block(Vec<Stmt>),
    Decl {
        constant: bool,
        ty: String,
        array: Option<Array>,
        vars: Vec<Declarator>,
    },
    Expr(Expr),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Return(Option<Expr>),
    Break,
    Continue,
    Discard,
}

#[derive(Clone, Debug)]
pub(crate) struct Param {
    pub(crate) by_ref: bool,
    pub(crate) ty: String,
    pub(crate) name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub(crate) ret: String,
    pub(crate) name: String,
    pub(crate) params: Vec<Param>,
    pub(crate) body: Vec<Stmt>,
}

#[derive(Default, Debug)]
pub(crate) struct Module {
    pub(crate) render_modes: Vec<String>,
    pub(crate) uniforms: Vec<(Uniform, Option<Expr>)>,
    pub(crate) varyings: Vec<(String, String)>,
    pub(crate) consts: Vec<Stmt>,
    pub(crate) structs: Vec<(String, Vec<Field>)>,
    pub(crate) functions: Vec<Function>,
}

pub(crate) const TYPES: &[&str] = &[
    "void",
    "bool",
    "int",
    "uint",
    "float",
    "vec2",
    "vec3",
    "vec4",
    "ivec2",
    "ivec3",
    "ivec4",
    "uvec2",
    "uvec3",
    "uvec4",
    "bvec2",
    "bvec3",
    "bvec4",
    "mat2",
    "mat3",
    "mat4",
    "sampler2D",
    "isampler2D",
    "usampler2D",
    "samplerCube",
    "sampler2DArray",
    "sampler3D",
];

pub(crate) const QUALIFIERS: &[&str] = &["lowp", "mediump", "highp", "flat", "smooth", "in"];

pub(crate) struct Parser {
    pub(crate) tokens: Vec<Tok>,
    pub(crate) at: usize,
    pub(crate) structs: BTreeSet<String>,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.at)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Tok> {
        self.tokens.get(self.at + ahead)
    }

    fn next(&mut self) -> Result<Tok> {
        let tok = self
            .tokens
            .get(self.at)
            .cloned()
            .ok_or_else(|| anyhow!("the shader ends early"))?;
        self.at += 1;
        Ok(tok)
    }

    fn is(&self, punct: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(p)) if *p == punct)
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(w)) if w == word)
    }

    fn eat(&mut self, punct: &str) -> bool {
        if self.is(punct) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn want(&mut self, punct: &str) -> Result<()> {
        if self.eat(punct) {
            Ok(())
        } else {
            bail!("expected `{punct}`, found {:?}", self.peek())
        }
    }

    fn ident(&mut self) -> Result<String> {
        match self.next()? {
            Tok::Ident(word) => Ok(word),
            other => bail!("expected a name, found {other:?}"),
        }
    }

    fn skip_qualifiers(&mut self) {
        while let Some(Tok::Ident(word)) = self.peek() {
            if QUALIFIERS.contains(&word.as_str()) {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn is_type(&self, word: &str) -> bool {
        TYPES.contains(&word) || self.structs.contains(word)
    }

    /// `[n]` or `[]` after a type or a name.
    fn array_suffix(&mut self) -> Result<Option<Array>> {
        if !self.eat("[") {
            return Ok(None);
        }
        if self.eat("]") {
            return Ok(Some(Array::Unsized));
        }
        let Tok::Num(n) = self.next()? else {
            bail!("an array's size has to be a number here");
        };
        self.want("]")?;
        let size = n.parse().map_err(|_| anyhow!("array size `{n}`"))?;
        Ok(Some(Array::Sized(size)))
    }

    pub(crate) fn module(&mut self) -> Result<Module> {
        let mut module = Module::default();
        while self.peek().is_some() {
            let word = self.ident()?;
            match word.as_str() {
                "shader_type" => {
                    let kind = self.ident()?;
                    if kind != "canvas_item" {
                        bail!("a `{kind}` shader: only `canvas_item` draws in 2D here");
                    }
                    self.want(";")?;
                }
                "render_mode" => {
                    loop {
                        module.render_modes.push(self.ident()?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.want(";")?;
                }
                "group_uniforms" => {
                    while !self.eat(";") {
                        self.next()?;
                    }
                }
                "global" | "instance" => bail!("a `{word} uniform` has no equivalent"),
                "uniform" => module.uniforms.push(self.uniform()?),
                "varying" => {
                    self.skip_qualifiers();
                    let ty = self.ident()?;
                    let name = self.ident()?;
                    self.want(";")?;
                    module.varyings.push((ty, name));
                }
                "const" => {
                    let decl = self.declaration(true)?;
                    module.consts.push(decl);
                }
                "struct" => {
                    let name = self.ident()?;
                    self.structs.insert(name.clone());
                    self.want("{")?;
                    let mut fields = Vec::new();
                    while !self.eat("}") {
                        self.skip_qualifiers();
                        let ty = self.ident()?;
                        loop {
                            let field = self.ident()?;
                            let array = self.array_suffix()?;
                            fields.push((ty.clone(), field, array));
                            if !self.eat(",") {
                                break;
                            }
                        }
                        self.want(";")?;
                    }
                    self.want(";")?;
                    module.structs.push((name, fields));
                }
                _ => {
                    self.at -= 1;
                    self.skip_qualifiers();
                    let ret = self.ident()?;
                    let name = self.ident()?;
                    if !self.is("(") {
                        bail!(
                            "`{ret} {name}` at the top level is neither a function nor a uniform"
                        );
                    }
                    module.functions.push(self.function(ret, name)?);
                }
            }
        }
        Ok(module)
    }

    fn uniform(&mut self) -> Result<(Uniform, Option<Expr>)> {
        self.skip_qualifiers();
        let ty = self.ident()?;
        if self.is("[") {
            bail!("a uniform array has no equivalent");
        }
        let name = self.ident()?;
        if self.is("[") {
            bail!("a uniform array has no equivalent");
        }
        let mut source_color = false;
        let mut screen = false;
        if self.eat(":") {
            loop {
                let hint = self.ident()?;
                source_color |= hint == "source_color";
                screen |= hint == "hint_screen_texture";
                if self.eat("(") {
                    let mut depth = 1;
                    while depth > 0 {
                        match self.next()? {
                            Tok::Punct("(") => depth += 1,
                            Tok::Punct(")") => depth -= 1,
                            _ => {}
                        }
                    }
                }
                if !self.eat(",") {
                    break;
                }
            }
        }
        let init = if self.eat("=") {
            Some(self.expr()?)
        } else {
            None
        };
        self.want(";")?;
        Ok((
            Uniform {
                name,
                ty,
                source_color,
                default: None,
                slot: None,
                screen,
            },
            init,
        ))
    }

    fn function(&mut self, ret: String, name: String) -> Result<Function> {
        self.want("(")?;
        let mut params = Vec::new();
        if !self.eat(")") {
            loop {
                if self.is_word("void") && matches!(self.peek_at(1), Some(Tok::Punct(")"))) {
                    self.at += 1;
                    self.want(")")?;
                    break;
                }
                let mut by_ref = false;
                while let Some(Tok::Ident(word)) = self.peek() {
                    match word.as_str() {
                        "out" | "inout" => by_ref = true,
                        "in" | "const" | "lowp" | "mediump" | "highp" => {}
                        _ => break,
                    }
                    self.at += 1;
                }
                let ty = self.ident()?;
                if ty.starts_with("sampler") {
                    bail!("a function taking a sampler has no equivalent");
                }
                let name = self.ident()?;
                if self.array_suffix()?.is_some() {
                    bail!("a function taking an array has no equivalent");
                }
                params.push(Param { by_ref, ty, name });
                if self.eat(")") {
                    break;
                }
                self.want(",")?;
            }
        }
        let Stmt::Block(body) = self.block()? else {
            unreachable!("a block parses as a block")
        };
        Ok(Function {
            ret,
            name,
            params,
            body,
        })
    }

    fn block(&mut self) -> Result<Stmt> {
        self.want("{")?;
        let mut body = Vec::new();
        while !self.eat("}") {
            body.push(self.statement()?);
        }
        Ok(Stmt::Block(body))
    }

    /// Whether what comes next declares a variable: a type, then a name or
    /// an array suffix.
    fn at_declaration(&self) -> bool {
        let mut ahead = 0;
        while let Some(Tok::Ident(word)) = self.peek_at(ahead) {
            if QUALIFIERS.contains(&word.as_str()) || word == "const" {
                ahead += 1;
            } else {
                break;
            }
        }
        matches!(
            (self.peek_at(ahead), self.peek_at(ahead + 1)),
            (Some(Tok::Ident(ty)), Some(Tok::Ident(_) | Tok::Punct("[")))
                if self.is_type(ty)
        )
    }

    fn declaration(&mut self, constant: bool) -> Result<Stmt> {
        let mut constant = constant;
        while let Some(Tok::Ident(word)) = self.peek() {
            if word == "const" {
                constant = true;
            } else if !QUALIFIERS.contains(&word.as_str()) {
                break;
            }
            self.at += 1;
        }
        let ty = self.ident()?;
        let array = self.array_suffix()?;
        let mut vars = Vec::new();
        loop {
            let name = self.ident()?;
            let own = self.array_suffix()?;
            let init = if self.eat("=") {
                Some(self.assignment()?)
            } else {
                None
            };
            vars.push(Declarator {
                name,
                array: own,
                init,
            });
            if !self.eat(",") {
                break;
            }
        }
        self.want(";")?;
        Ok(Stmt::Decl {
            constant,
            ty,
            array,
            vars,
        })
    }

    fn statement(&mut self) -> Result<Stmt> {
        if self.is("{") {
            return self.block();
        }
        if self.at_declaration() {
            return self.declaration(false);
        }
        if let Some(Tok::Ident(word)) = self.peek().cloned()
            && let Some(stmt) = self.control(&word)?
        {
            return Ok(stmt);
        }
        if self.eat(";") {
            return Ok(Stmt::Block(Vec::new()));
        }
        let e = self.expr()?;
        self.want(";")?;
        Ok(Stmt::Expr(e))
    }

    /// A statement a keyword opens: a branch, a loop, a switch, a jump.
    /// `None` when `word` opens none of them.
    fn control(&mut self, word: &str) -> Result<Option<Stmt>> {
        match word {
            "if" => {
                self.at += 1;
                self.want("(")?;
                let cond = self.expr()?;
                self.want(")")?;
                let then = self.statement()?;
                let other = if self.is_word("else") {
                    self.at += 1;
                    Some(Box::new(self.statement()?))
                } else {
                    None
                };
                return Ok(Some(Stmt::If(cond, Box::new(then), other)));
            }
            "for" => return self.for_loop().map(Some),
            "while" => {
                self.at += 1;
                self.want("(")?;
                let cond = self.expr()?;
                self.want(")")?;
                let body = self.statement()?;
                return Ok(Some(Stmt::While(cond, Box::new(body))));
            }
            "do" => {
                self.at += 1;
                let body = self.statement()?;
                if self.ident()? != "while" {
                    bail!("`do` without its `while`");
                }
                self.want("(")?;
                let cond = self.expr()?;
                self.want(")")?;
                self.want(";")?;
                return Ok(Some(Stmt::DoWhile(Box::new(body), cond)));
            }
            "switch" => return self.switch().map(Some),
            "return" => {
                self.at += 1;
                let value = if self.is(";") {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.want(";")?;
                return Ok(Some(Stmt::Return(value)));
            }
            "break" | "continue" | "discard" => {
                self.at += 1;
                self.want(";")?;
                return Ok(Some(match word {
                    "break" => Stmt::Break,
                    "continue" => Stmt::Continue,
                    _ => Stmt::Discard,
                }));
            }
            _ => {}
        }
        Ok(None)
    }

    /// `for (init; cond; step) body`, each of the three optional.
    fn for_loop(&mut self) -> Result<Stmt> {
        self.at += 1;
        self.want("(")?;
        let init = if self.eat(";") {
            None
        } else if self.at_declaration() {
            Some(Box::new(self.declaration(false)?))
        } else {
            let e = self.expr()?;
            self.want(";")?;
            Some(Box::new(Stmt::Expr(e)))
        };
        let cond = if self.is(";") {
            None
        } else {
            Some(self.expr()?)
        };
        self.want(";")?;
        let step = if self.is(")") {
            None
        } else {
            Some(self.expr()?)
        };
        self.want(")")?;
        let body = self.statement()?;
        Ok(Stmt::For(init, cond, step, Box::new(body)))
    }

    /// `switch (on) { case x: ... default: ... }`, arms in source order.
    fn switch(&mut self) -> Result<Stmt> {
        self.at += 1;
        self.want("(")?;
        let on = self.expr()?;
        self.want(")")?;
        self.want("{")?;
        let mut arms = Vec::new();
        while !self.eat("}") {
            let label = match self.ident()?.as_str() {
                "case" => Some(self.expr()?),
                "default" => None,
                other => bail!("`{other}` in a switch"),
            };
            self.want(":")?;
            let mut body = Vec::new();
            while !self.is("}") && !self.is_word("case") && !self.is_word("default") {
                body.push(self.statement()?);
            }
            arms.push((label, body));
        }
        Ok(Stmt::Switch(on, arms))
    }

    fn expr(&mut self) -> Result<Expr> {
        let first = self.assignment()?;
        if self.is(",") {
            bail!("the comma operator has no equivalent");
        }
        Ok(first)
    }

    fn assignment(&mut self) -> Result<Expr> {
        let lhs = self.ternary()?;
        for op in [
            "=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=",
        ] {
            if self.is(op) {
                let op = PUNCTS.iter().find(|p| **p == op).copied().unwrap_or("=");
                self.at += 1;
                let rhs = self.assignment()?;
                return Ok(Expr::Assign(op, Box::new(lhs), Box::new(rhs)));
            }
        }
        Ok(lhs)
    }

    fn ternary(&mut self) -> Result<Expr> {
        let cond = self.binary(1)?;
        if !self.eat("?") {
            return Ok(cond);
        }
        let then = self.assignment()?;
        self.want(":")?;
        let other = self.assignment()?;
        Ok(Expr::Ternary(
            Box::new(cond),
            Box::new(then),
            Box::new(other),
        ))
    }

    fn binary(&mut self, min: u8) -> Result<Expr> {
        let mut left = self.unary()?;
        while let Some(Tok::Punct(op)) = self.peek().cloned() {
            let Some(prec) = precedence(op) else { break };
            if prec < min {
                break;
            }
            self.at += 1;
            let right = self.binary(prec + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr> {
        for op in ["-", "+", "!", "~", "++", "--"] {
            if self.is(op) {
                let op = PUNCTS.iter().find(|p| **p == op).copied().unwrap_or("-");
                self.at += 1;
                let inner = self.unary()?;
                return Ok(match op {
                    "++" | "--" => Expr::Step(op, Box::new(inner)),
                    "+" => inner,
                    _ => Expr::Unary(op, Box::new(inner)),
                });
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.eat("[") {
                let index = self.expr()?;
                self.want("]")?;
                e = Expr::Index(Box::new(e), Box::new(index));
            } else if self.eat(".") {
                let member = self.ident()?;
                if self.is("(") {
                    bail!("a method call `.{member}()` has no equivalent");
                }
                e = Expr::Member(Box::new(e), member);
            } else if self.is("++") || self.is("--") {
                let op = if self.eat("++") {
                    "++"
                } else {
                    self.at += 1;
                    "--"
                };
                e = Expr::Step(op, Box::new(e));
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn args(&mut self) -> Result<Vec<Expr>> {
        self.want("(")?;
        let mut args = Vec::new();
        if self.eat(")") {
            return Ok(args);
        }
        loop {
            args.push(self.assignment()?);
            if self.eat(")") {
                return Ok(args);
            }
            self.want(",")?;
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        match self.next()? {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Punct("(") => {
                let e = self.expr()?;
                self.want(")")?;
                Ok(e)
            }
            Tok::Ident(word) => {
                if self.is_type(&word) && self.is("[") {
                    let size = match self.array_suffix()? {
                        Some(Array::Sized(n)) => Some(n),
                        _ => None,
                    };
                    let args = self.args()?;
                    return Ok(Expr::Array(word, size, args));
                }
                if self.is("(") {
                    let args = self.args()?;
                    return Ok(Expr::Call(word, args));
                }
                Ok(Expr::Ident(word))
            }
            other @ Tok::Punct(_) => bail!("unexpected {other:?} in an expression"),
        }
    }
}

pub(crate) fn precedence(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "^^" => 2,
        "&&" => 3,
        "|" => 4,
        "^" => 5,
        "&" => 6,
        "==" | "!=" => 7,
        "<" | ">" | "<=" | ">=" => 8,
        "<<" | ">>" => 9,
        "+" | "-" => 10,
        "*" | "/" | "%" => 11,
        _ => return None,
    })
}
