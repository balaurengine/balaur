// `match` and its patterns.

use super::{Parser, Tok, err, interpolate};
use crate::Error;
use crate::ast::{Arm, Expr, ExprKind, Pattern};
use crate::vocabulary::words;

impl Parser {
    // `a | b | ..`, read here rather than by `binary`, where `|` is a bit-or.
    fn pattern(&mut self) -> Result<Pattern, Error> {
        let first = self.alternative()?;
        if !self.is("|") {
            return Ok(first);
        }
        let mut alternatives = vec![first];
        while self.eat("|") {
            alternatives.push(self.alternative()?);
        }
        Ok(Pattern::Or(alternatives))
    }

    // `_`, a literal, or a path: a variant with what its fields are bound to,
    // a constant, or a name to bind, which `enums::lower` tells apart.
    fn alternative(&mut self) -> Result<Pattern, Error> {
        if self.eat_kw("_") {
            return Ok(Pattern::Any);
        }
        if let Some(value) = self.literal_pattern()? {
            return Ok(Pattern::Value(value));
        }
        let mut path = self.ident()?;
        while self.eat("::") {
            path = format!("{path}::{}", self.ident()?);
        }
        if !path.contains("::") && !self.is("(") && !self.is("{") {
            return Ok(Pattern::Bind(path));
        }
        let mut binds = Vec::new();
        if self.eat("(") {
            while !self.eat(")") {
                binds.push((binds.len().to_string(), self.ident()?));
                if !self.eat(",") {
                    self.want(")")?;
                    break;
                }
            }
        } else if self.eat("{") {
            while !self.eat("}") {
                let field = self.ident()?;
                let bound = if self.eat(":") {
                    self.ident()?
                } else {
                    field.clone()
                };
                binds.push((field, bound));
                if !self.eat(",") {
                    self.want("}")?;
                    break;
                }
            }
        }
        Ok(Pattern::Variant { path, binds })
    }

    fn literal_pattern(&mut self) -> Result<Option<Expr>, Error> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            Tok::Int(v) => ExprKind::Int(v),
            Tok::P("-") if matches!(self.peek_at(1), Tok::Int(_)) => {
                self.i += 1;
                let Tok::Int(v) = self.peek().clone() else {
                    unreachable!("the token after `-` was just seen to be an int");
                };
                ExprKind::Int(-v)
            }
            Tok::Str(s, at) => match interpolate(&s, at)? {
                ExprKind::Str(s) => ExprKind::Str(s),
                _ => return err(pos, "a pattern's string holds no `{..}`"),
            },
            Tok::Ident(s) if s == words::keywords::TRUE || s == words::keywords::FALSE => {
                ExprKind::Bool(s == words::keywords::TRUE)
            }
            Tok::Ident(s) if s == words::keywords::NIL => ExprKind::Nil,
            Tok::Float(_) => {
                return err(pos, "a float is no pattern: compare it with `<` or `>`");
            }
            _ => return Ok(None),
        };
        self.i += 1;
        Ok(Some(Expr { kind, pos }))
    }

    // `match subject { pattern => value, .. }`, past `match`.
    pub(super) fn match_expr(&mut self) -> Result<ExprKind, Error> {
        let subject = self.expr(true)?;
        self.want("{")?;
        let mut arms = Vec::new();
        while !self.eat("}") {
            let pos = self.pos();
            let pattern = self.pattern()?;
            self.want("=>")?;
            let body = if self.is("{") {
                let at = self.pos();
                Expr {
                    kind: ExprKind::Block(self.block()?),
                    pos: at,
                }
            } else {
                self.expr(false)?
            };
            let ends = matches!(body.kind, ExprKind::Block(_));
            arms.push(Arm { pattern, body, pos });
            if !self.eat(",") && !ends {
                self.want("}")?;
                break;
            }
        }
        Ok(ExprKind::Match(Box::new(subject), arms))
    }
}
