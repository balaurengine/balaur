//! Statements by recursive descent, expressions by precedence climbing.
//!
//! A statement the parser cannot read becomes `Stmt::Raw` with its source
//! line rather than failing the file: a function translated in part is worth
//! more than one left whole as a comment.

use super::ast::{Expr, MatchArm, Stmt};
use super::lex::{Tok, Token};

/// Binding power per binary operator, loosest first. GDScript's table, which
/// is Python's with `&&`/`||` spelled both ways.
fn binding(op: &str) -> Option<(u8, &'static str)> {
    Some(match op {
        "||" | "or" => (1, "||"),
        "&&" | "and" => (2, "&&"),
        "==" => (4, "=="),
        "!=" => (4, "!="),
        "<" => (4, "<"),
        "<=" => (4, "<="),
        ">" => (4, ">"),
        ">=" => (4, ">="),
        "in" => (4, "in"),
        "|" => (5, "|"),
        "^" => (6, "^"),
        "&" => (7, "&"),
        "<<" => (8, "<<"),
        ">>" => (8, ">>"),
        "+" => (9, "+"),
        "-" => (9, "-"),
        "*" => (10, "*"),
        "/" => (10, "/"),
        "%" => (10, "%"),
        "**" => (12, "**"),
        _ => return None,
    })
}

const ASSIGNS: &[(&str, &str)] = &[
    ("=", "="),
    ("+=", "+="),
    ("-=", "-="),
    ("*=", "*="),
    ("/=", "/="),
    ("%=", "%="),
    ("**=", "**="),
    ("&=", "&="),
    ("|=", "|="),
    ("^=", "^="),
    ("<<=", "<<="),
    (">>=", ">>="),
];

pub(crate) struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    /// The source, by line, so an unreadable statement keeps its own text.
    lines: &'a [&'a str],
    /// Set inside a lambda written on one line, where a statement ends at the
    /// bracket or comma that follows it rather than at a newline.
    inline: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(tokens: &'a [Token], lines: &'a [&'a str]) -> Self {
        Self {
            tokens,
            at: 0,
            lines,
            inline: 0,
        }
    }

    /// Every statement of one indented block, to its `Dedent`.
    pub(crate) fn block(&mut self) -> Vec<Stmt> {
        let mut out = Vec::new();
        if !self.eat(&Tok::Indent) {
            return out;
        }
        while !self.at_end() && !self.check(&Tok::Dedent) {
            if self.eat(&Tok::Newline) {
                continue;
            }
            out.push(self.statement());
        }
        self.eat(&Tok::Dedent);
        out
    }

    /// The statements of a file's top level, for a lambda body written inline.
    pub(crate) fn statements(&mut self) -> Vec<Stmt> {
        let mut out = Vec::new();
        while !self.at_end() && !self.check(&Tok::Dedent) {
            if self.eat(&Tok::Newline) {
                continue;
            }
            out.push(self.statement());
        }
        out
    }

    fn statement(&mut self) -> Stmt {
        let line = self.line();
        let start = self.at;
        let stmt = self.try_statement();
        if let Some(stmt) = stmt {
            stmt
        } else {
            self.skip_statement(start);
            Stmt::Raw(self.source_line(line))
        }
    }

    fn try_statement(&mut self) -> Option<Stmt> {
        if matches!(self.peek(), Tok::Annotation(_)) {
            self.next();
            if self.eat(&Tok::Op("(")) {
                let mut depth = 1;
                while depth > 0 && !self.at_end() {
                    match self.next() {
                        Tok::Op("(") => depth += 1,
                        Tok::Op(")") => depth -= 1,
                        _ => {}
                    }
                }
            }
            self.eat(&Tok::Newline);
            return Some(Stmt::Pass);
        }
        if let Tok::Name(word) = self.peek().clone() {
            match word.as_str() {
                "var" | "const" => return self.var(),
                "if" => return self.if_chain(),
                "while" => return self.while_loop(),
                "for" => return self.for_loop(),
                "match" => return self.match_block(),
                "return" => {
                    self.next();
                    if self.check(&Tok::Newline) || self.at_end() {
                        self.eat(&Tok::Newline);
                        return Some(Stmt::Return(None));
                    }
                    let value = self.expression(0)?;
                    self.end_of_statement()?;
                    return Some(Stmt::Return(Some(value)));
                }
                "break" => {
                    self.next();
                    self.end_of_statement()?;
                    return Some(Stmt::Break);
                }
                "continue" => {
                    self.next();
                    self.end_of_statement()?;
                    return Some(Stmt::Continue);
                }
                "pass" => {
                    self.next();
                    self.end_of_statement()?;
                    return Some(Stmt::Pass);
                }
                // Frame-ordering and declaration keywords with no counterpart:
                // the caller reports them and keeps the line.
                "breakpoint" | "assert" | "class" | "signal" | "enum" | "func" | "static"
                | "class_name" | "extends" => return None,
                _ => {}
            }
        }
        let target = self.expression(0)?;
        if let Tok::Op(op) = self.peek()
            && let Some((_, op)) = ASSIGNS.iter().find(|(spelling, _)| spelling == op)
        {
            self.next();
            let value = self.expression(0)?;
            self.end_of_statement()?;
            return Some(Stmt::Assign { target, op, value });
        }
        self.end_of_statement()?;
        Some(Stmt::Expr(target))
    }

    fn var(&mut self) -> Option<Stmt> {
        self.next();
        let name = self.name()?;
        let mut hint = None;
        let mut value = None;
        // `var x := 1` infers its type; `var x: int = 1` states one.
        if self.eat(&Tok::Op(":=")) {
            value = Some(self.expression(0)?);
        } else {
            if self.eat(&Tok::Op(":")) && !self.check(&Tok::Op("=")) {
                hint = Some(self.type_name()?);
            }
            if self.eat(&Tok::Op("=")) {
                value = Some(self.expression(0)?);
            }
        }
        // `var x` with a setter body is a property; its block is skipped and
        // the caller reports it.
        if self.check(&Tok::Op(":")) {
            return None;
        }
        self.end_of_statement()?;
        Some(Stmt::Var { name, value, hint })
    }

    fn if_chain(&mut self) -> Option<Stmt> {
        let mut arms = Vec::new();
        let mut other = None;
        self.next();
        let cond = self.expression(0)?;
        self.consume(&Tok::Op(":"))?;
        arms.push((cond, self.body()));
        while let Tok::Name(word) = self.peek().clone() {
            match word.as_str() {
                "elif" => {
                    self.next();
                    let cond = self.expression(0)?;
                    self.consume(&Tok::Op(":"))?;
                    arms.push((cond, self.body()));
                }
                "else" => {
                    self.next();
                    self.consume(&Tok::Op(":"))?;
                    other = Some(self.body());
                    break;
                }
                _ => break,
            }
        }
        Some(Stmt::If { arms, other })
    }

    fn while_loop(&mut self) -> Option<Stmt> {
        self.next();
        let cond = self.expression(0)?;
        self.consume(&Tok::Op(":"))?;
        Some(Stmt::While {
            cond,
            body: self.body(),
        })
    }

    fn for_loop(&mut self) -> Option<Stmt> {
        self.next();
        let name = self.name()?;
        if self.eat(&Tok::Op(":")) {
            self.type_name()?;
        }
        match self.peek() {
            Tok::Name(word) if word == "in" => self.next(),
            _ => return None,
        };
        let iter = self.expression(0)?;
        self.consume(&Tok::Op(":"))?;
        Some(Stmt::For {
            name,
            iter,
            body: self.body(),
        })
    }

    fn match_block(&mut self) -> Option<Stmt> {
        self.next();
        let subject = self.expression(0)?;
        self.consume(&Tok::Op(":"))?;
        self.eat(&Tok::Newline);
        if !self.eat(&Tok::Indent) {
            return None;
        }
        let mut arms = Vec::new();
        while !self.at_end() && !self.check(&Tok::Dedent) {
            if self.eat(&Tok::Newline) {
                continue;
            }
            let mut patterns = Vec::new();
            loop {
                patterns.push(self.expression(0)?);
                if !self.eat(&Tok::Op(",")) {
                    break;
                }
            }
            self.consume(&Tok::Op(":"))?;
            arms.push(MatchArm {
                patterns,
                body: self.body(),
            });
        }
        self.eat(&Tok::Dedent);
        Some(Stmt::Match { subject, arms })
    }

    /// What follows a `:`: an indented block, or one statement on the same
    /// line.
    fn body(&mut self) -> Vec<Stmt> {
        if self.check(&Tok::Newline) {
            self.next();
            return self.block();
        }
        vec![self.statement()]
    }

    pub(crate) fn expression(&mut self, least: u8) -> Option<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Op(op) => (*op).to_string(),
                Tok::Name(word) if matches!(word.as_str(), "or" | "and" | "in" | "as" | "is") => {
                    word.clone()
                }
                Tok::Name(word) if word == "if" => {
                    // `then if cond else other`, GDScript's ternary.
                    if least > 0 {
                        break;
                    }
                    self.next();
                    let cond = self.expression(1)?;
                    match self.peek() {
                        Tok::Name(word) if word == "else" => self.next(),
                        _ => return None,
                    };
                    let other = self.expression(0)?;
                    left = Expr::Ternary {
                        then: Box::new(left),
                        cond: Box::new(cond),
                        other: Box::new(other),
                    };
                    continue;
                }
                _ => break,
            };
            if op == "as" {
                self.next();
                let name = self.type_name()?;
                left = Expr::Cast(Box::new(left), name);
                continue;
            }
            if op == "is" {
                self.next();
                let name = self.type_name()?;
                left = Expr::Is(Box::new(left), name, false);
                continue;
            }
            let Some((power, spelling)) = binding(&op) else {
                break;
            };
            if power < least {
                break;
            }
            self.next();
            // `**` is right-associative; everything else binds left.
            let next = if spelling == "**" { power } else { power + 1 };
            let right = self.expression(next)?;
            left = Expr::Binary(spelling, Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<Expr> {
        match self.peek().clone() {
            Tok::Op("-") => {
                self.next();
                Some(Expr::Unary("-", Box::new(self.unary()?)))
            }
            Tok::Op("+") => {
                self.next();
                self.unary()
            }
            Tok::Op("!") => {
                self.next();
                Some(Expr::Unary("!", Box::new(self.unary()?)))
            }
            Tok::Op("~") => {
                self.next();
                Some(Expr::Unary("~", Box::new(self.unary()?)))
            }
            Tok::Name(word) if word == "not" => {
                self.next();
                let inner = self.unary()?;
                // `not x is T` is GDScript's negated type test, which reads
                // better emitted as one thing than as a negation of one.
                if let Expr::Is(value, name, _) = inner {
                    return Some(Expr::Is(value, name, true));
                }
                Some(Expr::Unary("!", Box::new(inner)))
            }
            Tok::Name(word) if word == "await" => {
                self.next();
                Some(Expr::Await(Box::new(self.unary()?)))
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Option<Expr> {
        let mut value = self.primary()?;
        loop {
            match self.peek().clone() {
                Tok::Op(".") => {
                    self.next();
                    let name = self.name()?;
                    value = Expr::Field(Box::new(value), name);
                }
                Tok::Op("(") => {
                    self.next();
                    let args = self.arguments()?;
                    value = Expr::Call(Box::new(value), args);
                }
                Tok::Op("[") => {
                    self.next();
                    let index = self.expression(0)?;
                    self.consume(&Tok::Op("]"))?;
                    value = Expr::Index(Box::new(value), Box::new(index));
                }
                _ => break,
            }
        }
        Some(value)
    }

    fn arguments(&mut self) -> Option<Vec<Expr>> {
        let mut out = Vec::new();
        if self.eat(&Tok::Op(")")) {
            return Some(out);
        }
        loop {
            out.push(self.expression(0)?);
            if self.eat(&Tok::Op(",")) {
                // A trailing comma before the bracket is allowed.
                if self.eat(&Tok::Op(")")) {
                    return Some(out);
                }
                continue;
            }
            self.consume(&Tok::Op(")"))?;
            return Some(out);
        }
    }

    fn primary(&mut self) -> Option<Expr> {
        let token = self.peek().clone();
        match token {
            Tok::Int(text) => {
                self.next();
                Some(Expr::Int(text))
            }
            Tok::Float(text) => {
                self.next();
                Some(Expr::Float(text))
            }
            Tok::Str(text) => {
                self.next();
                Some(Expr::Str(text))
            }
            Tok::NodePath(path) => {
                self.next();
                Some(Expr::NodePath(path))
            }
            Tok::Unique(name) => {
                self.next();
                Some(Expr::Unique(name))
            }
            Tok::Op("(") => {
                self.next();
                let inner = self.expression(0)?;
                self.consume(&Tok::Op(")"))?;
                Some(inner)
            }
            Tok::Op("[") => {
                self.next();
                let mut items = Vec::new();
                if self.eat(&Tok::Op("]")) {
                    return Some(Expr::Array(items));
                }
                loop {
                    items.push(self.expression(0)?);
                    if self.eat(&Tok::Op(",")) {
                        if self.eat(&Tok::Op("]")) {
                            break;
                        }
                        continue;
                    }
                    self.consume(&Tok::Op("]"))?;
                    break;
                }
                Some(Expr::Array(items))
            }
            Tok::Op("{") => {
                self.next();
                let mut pairs = Vec::new();
                if self.eat(&Tok::Op("}")) {
                    return Some(Expr::Dict(pairs));
                }
                loop {
                    let key = self.expression(0)?;
                    self.consume(&Tok::Op(":"))?;
                    let value = self.expression(0)?;
                    pairs.push((key, value));
                    if self.eat(&Tok::Op(",")) {
                        if self.eat(&Tok::Op("}")) {
                            break;
                        }
                        continue;
                    }
                    self.consume(&Tok::Op("}"))?;
                    break;
                }
                Some(Expr::Dict(pairs))
            }
            Tok::Name(word) => match word.as_str() {
                "true" => {
                    self.next();
                    Some(Expr::Bool(true))
                }
                "false" => {
                    self.next();
                    Some(Expr::Bool(false))
                }
                "null" => {
                    self.next();
                    Some(Expr::Nil)
                }
                "self" => {
                    self.next();
                    Some(Expr::SelfRef)
                }
                "func" => self.lambda(),
                _ => {
                    self.next();
                    Some(Expr::Name(word))
                }
            },
            _ => None,
        }
    }

    /// `func(a, b): body`, GDScript's lambda. A multi-line one indents, and a
    /// one-liner sits on the same line.
    fn lambda(&mut self) -> Option<Expr> {
        self.next();
        self.consume(&Tok::Op("("))?;
        let mut params = Vec::new();
        if !self.eat(&Tok::Op(")")) {
            loop {
                let name = self.name()?;
                if self.eat(&Tok::Op(":")) {
                    self.type_name()?;
                }
                if self.eat(&Tok::Op("=")) {
                    self.expression(0)?;
                }
                params.push(name);
                if self.eat(&Tok::Op(",")) {
                    continue;
                }
                self.consume(&Tok::Op(")"))?;
                break;
            }
        }
        if self.eat(&Tok::Op("->")) {
            self.type_name()?;
        }
        self.consume(&Tok::Op(":"))?;
        let body = if self.check(&Tok::Newline) {
            self.next();
            self.block()
        } else {
            self.inline += 1;
            let body = vec![self.statement()];
            self.inline -= 1;
            body
        };
        Some(Expr::Lambda { params, body })
    }

    /// A type, which may be `Array[int]` or `A.B`. Only its head is kept: the
    /// emitter has no use for the rest.
    fn type_name(&mut self) -> Option<String> {
        let head = self.name()?;
        while self.eat(&Tok::Op(".")) {
            self.name()?;
        }
        if self.eat(&Tok::Op("[")) {
            let mut depth = 1;
            while depth > 0 && !self.at_end() {
                match self.next() {
                    Tok::Op("[") => depth += 1,
                    Tok::Op("]") => depth -= 1,
                    _ => {}
                }
            }
        }
        Some(head)
    }

    fn name(&mut self) -> Option<String> {
        match self.next() {
            Tok::Name(word) => Some(word),
            _ => None,
        }
    }

    fn end_of_statement(&mut self) -> Option<()> {
        self.eat(&Tok::Op(";"));
        if self.check(&Tok::Newline) || self.at_end() || self.check(&Tok::Dedent) {
            self.eat(&Tok::Newline);
            return Some(());
        }
        // A one-line lambda's body ends where its call's argument does.
        let closes = matches!(self.peek(), Tok::Op("," | ")" | "]" | "}"));
        (self.inline > 0 && closes).then_some(())
    }

    /// After an unreadable statement, skip to the next line at the same depth
    /// and drop any block it opened.
    fn skip_statement(&mut self, start: usize) {
        self.at = start;
        while !self.at_end() && !self.check(&Tok::Newline) && !self.check(&Tok::Indent) {
            self.next();
        }
        self.eat(&Tok::Newline);
        if self.check(&Tok::Indent) {
            let mut depth = 0;
            loop {
                match self.next() {
                    Tok::Indent => depth += 1,
                    Tok::Dedent => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Tok::Eof => break,
                    _ => {}
                }
            }
        }
    }

    fn source_line(&self, line: usize) -> String {
        self.lines
            .get(line.saturating_sub(1))
            .map(|text| text.trim().to_string())
            .unwrap_or_default()
    }

    fn line(&self) -> usize {
        self.tokens.get(self.at).map(|t| t.line).unwrap_or_default()
    }

    fn peek(&self) -> &Tok {
        self.tokens.get(self.at).map_or(&Tok::Eof, |t| &t.kind)
    }

    fn next(&mut self) -> Tok {
        let kind = self.peek().clone();
        if self.at < self.tokens.len() {
            self.at += 1;
        }
        kind
    }

    fn check(&self, kind: &Tok) -> bool {
        self.peek() == kind
    }

    fn eat(&mut self, kind: &Tok) -> bool {
        if self.check(kind) {
            self.at += 1;
            return true;
        }
        false
    }

    fn consume(&mut self, kind: &Tok) -> Option<()> {
        self.eat(kind).then_some(())
    }

    fn at_end(&self) -> bool {
        self.check(&Tok::Eof)
    }
}

#[cfg(test)]
mod tests {
    use super::super::ast::{Expr, Stmt};
    use super::super::lex::lex;
    use super::Parser;

    fn parse(source: &str) -> Vec<Stmt> {
        let tokens = lex(source).expect("lexes");
        let lines: Vec<&str> = source.lines().collect();
        Parser::new(&tokens, &lines).statements()
    }

    #[test]
    fn an_if_chain_keeps_every_arm() {
        let out = parse("if a:\n\tpass\nelif b:\n\tpass\nelse:\n\tpass\n");
        let Some(Stmt::If { arms, other }) = out.first() else {
            panic!("{out:?}");
        };
        assert_eq!(arms.len(), 2);
        assert!(other.is_some());
    }

    #[test]
    fn precedence_binds_multiplication_tighter_than_addition() {
        let out = parse("var x = 1 + 2 * 3\n");
        let Some(Stmt::Var {
            value: Some(Expr::Binary("+", _, right)),
            ..
        }) = out.first()
        else {
            panic!("{out:?}");
        };
        assert!(matches!(**right, Expr::Binary("*", _, _)), "{right:?}");
    }

    #[test]
    fn a_ternary_reads_as_condition_then_branches() {
        let out = parse("var x = 1 if ok else 2\n");
        let Some(Stmt::Var {
            value: Some(Expr::Ternary { .. }),
            ..
        }) = out.first()
        else {
            panic!("{out:?}");
        };
    }

    #[test]
    fn an_unreadable_line_keeps_its_text_and_the_rest_parses() {
        let out = parse("var a = 1\nsignal done(x)\nvar b = 2\n");
        assert!(
            matches!(out.get(1), Some(Stmt::Raw(text)) if text.starts_with("signal")),
            "{out:?}"
        );
        assert!(matches!(out.get(2), Some(Stmt::Var { .. })), "{out:?}");
    }

    #[test]
    fn a_lambda_carries_its_parameters_and_body() {
        let out = parse("var f = func(a, b): return a + b\n");
        let Some(Stmt::Var {
            value: Some(Expr::Lambda { params, body }),
            ..
        }) = out.first()
        else {
            panic!("{out:?}");
        };
        assert_eq!(params, &["a", "b"]);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn match_arms_carry_every_pattern() {
        let out = parse("match kind:\n\t\"a\", \"b\":\n\t\tpass\n\t_:\n\t\tpass\n");
        let Some(Stmt::Match { arms, .. }) = out.first() else {
            panic!("{out:?}");
        };
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].patterns.len(), 2);
    }

    #[test]
    fn await_and_casts_parse_as_their_own_shapes() {
        let out = parse("var a = await thing.go() as Ship\n");
        let Some(Stmt::Var {
            value: Some(Expr::Cast(inner, name)),
            ..
        }) = out.first()
        else {
            panic!("{out:?}");
        };
        assert_eq!(name, "Ship");
        assert!(matches!(**inner, Expr::Await(_)), "{inner:?}");
    }
}
