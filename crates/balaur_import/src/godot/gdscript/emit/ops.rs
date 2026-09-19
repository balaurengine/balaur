//! Operators and literals: the tests Godot's truth needs, binary operators
//! regrouped where the tree lost their parentheses, dictionaries, lambdas.

use super::{Emitter, safe};
use crate::godot::gdscript::ast::{Expr, Stmt};

impl Emitter<'_> {
    /// A value tested for truth. Godot tests anything: null, zero, an empty
    /// string or collection, a freed node are false. Rune tests a bool.
    pub(super) fn condition(&mut self, value: &Expr) -> String {
        let text = self.expression(value);
        if self.is_boolish(value) {
            return text;
        }
        self.uses_shim = true;
        format!("(gd.truthy)({text})")
    }

    pub(super) fn is_boolish(&self, value: &Expr) -> bool {
        match value {
            Expr::Bool(_) | Expr::Is(..) | Expr::Unary("!", _) => true,
            Expr::Binary(op, ..) => matches!(
                *op,
                "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||" | "in"
            ),
            Expr::Name(name) if self.is_local(name) => self.bool_locals.contains(name),
            Expr::Name(name) => self.context.bools.contains(name),
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => {
                self.context.bools.contains(name)
            }
            Expr::Call(callee, _) => match &**callee {
                Expr::Name(name) if !self.is_local(name) => self.context.bools.contains(name),
                Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => {
                    self.context.bools.contains(name)
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub(super) fn binary(&mut self, op: &str, left: &Expr, right: &Expr) -> String {
        if matches!(op, "&&" | "||") {
            let wrap = |e: &Expr, t: String| {
                if matches!(e, Expr::Binary("&&" | "||", ..)) && e_op(e) != op {
                    format!("({t})")
                } else {
                    t
                }
            };
            let l = self.condition(left);
            let r = self.condition(right);
            return format!("{} {op} {}", wrap(left, l), wrap(right, r));
        }
        // `"%s" % [a]` is formatting, not modulo. Godot decides at run time;
        // a string on the left or a list on the right decides it here.
        if op == "%" && (matches!(left, Expr::Str(_)) || matches!(right, Expr::Array(_))) {
            let text = self.expression(left);
            let args = match right {
                Expr::Array(_) => self.expression(right),
                other => {
                    let one = self.expression(other);
                    format!("[{one}]")
                }
            };
            self.uses_shim = true;
            return format!("(gd.format)({text}, {args})");
        }
        // `a + [b]`: Godot joins two arrays; Rune adds no lists.
        if op == "+" && (matches!(left, Expr::Array(_)) || matches!(right, Expr::Array(_))) {
            let left = self.expression(left);
            let right = self.expression(right);
            self.uses_shim = true;
            return format!("(gd.concat)({left}, {right})");
        }
        if op == "in" {
            let left = self.expression(left);
            let right = self.expression(right);
            self.uses_shim = true;
            return format!("(gd.has)({right}, {left})");
        }
        // `x == null` is `is_nil`: Rune does not compare against unit.
        if matches!(op, "==" | "!=") && (matches!(left, Expr::Nil) || matches!(right, Expr::Nil)) {
            let value = if matches!(left, Expr::Nil) {
                right
            } else {
                left
            };
            let text = self.expression(value);
            self.uses_shim = true;
            let nil = format!("(gd.is_nil)({text})");
            return if op == "==" { nil } else { format!("!{nil}") };
        }
        if op == "**" {
            let left = self.expression(left);
            let right = self.expression(right);
            return format!("math::pow({left}, {right})");
        }
        let comparison = matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=");
        let power =
            |op: &str| crate::godot::gdscript::parse::binding(op).map_or(0, |(power, _)| power);
        // The tree holds no parentheses, so they come back wherever the
        // operators alone would regroup it; Rune also refuses `a < b < c`.
        let group = |value: &Expr, text: String, right: bool| {
            let Expr::Binary(inner, ..) = value else {
                return text;
            };
            let chained = comparison && matches!(*inner, "==" | "!=" | "<" | "<=" | ">" | ">=");
            let looser = power(inner) < power(op) || (right && power(inner) == power(op));
            if chained || looser {
                return format!("({text})");
            }
            text
        };
        let left_text = self.expression(left);
        let left_text = group(left, left_text, false);
        let right_text = self.expression(right);
        let right_text = group(right, right_text, true);
        format!("{left_text} {op} {right_text}")
    }

    pub(super) fn dict(&mut self, pairs: &[(Expr, Expr)]) -> String {
        let literal = pairs.iter().all(|(key, _)| matches!(key, Expr::Str(_)));
        if literal {
            let parts: Vec<String> = pairs
                .iter()
                .map(|(key, value)| {
                    let key = self.expression(key);
                    let value = self.expression(value);
                    format!("{key}: {value}")
                })
                .collect();
            // An empty one may be keyed by anything later, which only the
            // shim's hash map takes.
            if parts.is_empty() {
                self.uses_shim = true;
                return "(gd.dict)([])".into();
            }
            return format!("#{{ {} }}", parts.join(", "));
        }
        // A key that is not a literal string cannot stand in an object
        // literal, so the shim builds the map from pairs.
        let parts: Vec<String> = pairs
            .iter()
            .map(|(key, value)| {
                let key = self.expression(key);
                let value = self.expression(value);
                format!("[{key}, {value}]")
            })
            .collect();
        self.uses_shim = true;
        format!("(gd.dict)([{}])", parts.join(", "))
    }

    pub(super) fn lambda(&mut self, params: &[String], body: &[Stmt]) -> String {
        self.scopes.push(params.iter().cloned().collect());
        let bound: Vec<String> = params.iter().map(|name| safe(name)).collect();
        let outer = std::mem::replace(&mut self.awaits, false);
        // A one-expression lambda reads as one, which is what most of these are.
        let out = if let [Stmt::Return(Some(value)) | Stmt::Expr(value)] = body {
            let text = self.expression(value);
            format!("|{}| {{ {text} }}", bound.join(", "))
        } else {
            let text = self.block(body, 2);
            format!("|{}| {{\n{text}    }}", bound.join(", "))
        };
        self.scopes.pop();
        // A closure that waits is async, and its own waiting does not make the
        // function around it async.
        let inner = std::mem::replace(&mut self.awaits, outer);
        if inner {
            return format!("async {out}");
        }
        out
    }
}

/// The operator of a binary expression, for grouping a mixed `and`/`or`.
fn e_op(value: &Expr) -> &'static str {
    match value {
        Expr::Binary(op, ..) => op,
        _ => "",
    }
}
