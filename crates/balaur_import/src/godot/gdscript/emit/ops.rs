//! Operators and literals: the tests Godot's truth needs, binary operators
//! regrouped where the tree lost their parentheses, dictionaries, lambdas.

use super::{Bt, Emitter, safe};
use crate::godot::gdscript::ast::{Expr, Stmt};
use std::collections::BTreeSet;

impl Emitter<'_> {
    /// A value tested for truth. Godot tests anything: null, zero, an empty
    /// string or collection, a freed node are false. Rune tests a bool.
    pub(super) fn condition(&mut self, value: &Expr) -> String {
        let text = self.expression(value);
        if self.is_boolish(value) || self.type_of(value) == Bt::Bool {
            return text;
        }
        self.uses_shim = true;
        format!("gd::truthy({text})")
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
        let (lt, rt) = (self.type_of(left), self.type_of(right));
        let typed = (lt.numeric() && rt.numeric()) || (lt == Bt::Str && rt == Bt::Str);
        // Godot answers false when two values are of different types, which
        // a game leans on when it compares a missing id with a string.
        if matches!(op, "==" | "!=")
            && !(literal(left) && literal(right))
            && !typed
            && !matches!(left, Expr::Nil)
            && !matches!(right, Expr::Nil)
        {
            let l = self.expression(left);
            let r = self.expression(right);
            self.uses_shim = true;
            let same = format!("gd::same({l}, {r})");
            return if op == "==" { same } else { format!("!{same}") };
        }
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
        // `"%s" % [a]` is formatting, not modulo. A literal says which it is;
        // anything else asks the shim, as Godot asks the value.
        if op == "%" && (matches!(left, Expr::Str(_)) || matches!(right, Expr::Array(_))) {
            let text = self.expression(left);
            let args = match right {
                Expr::Array(_) => self.expression(right),
                other => {
                    let one = self.expression(other);
                    super::map::any_list(&[one])
                }
            };
            self.uses_shim = true;
            return format!("gd::format({text}, {args})");
        }
        if op == "%" && !matches!(left, Expr::Int(_) | Expr::Float(_)) {
            let l = self.expression(left);
            let r = self.expression(right);
            self.uses_shim = true;
            return format!("gd::modulo({l}, {r})");
        }
        // `a + [b]`: Godot joins two arrays; Rune adds no lists.
        if op == "+" && (matches!(left, Expr::Array(_)) || matches!(right, Expr::Array(_))) {
            let left = self.expression(left);
            let right = self.expression(right);
            self.uses_shim = true;
            return format!("gd::concat({left}, {right})");
        }
        if op == "in" {
            let left = self.expression(left);
            let right = self.expression(right);
            self.uses_shim = true;
            return format!("gd::has({right}, {left})");
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
            let nil = format!("gd::is_nil({text})");
            return if op == "==" { nil } else { format!("!{nil}") };
        }
        if op == "**" {
            let left = self.float_operand(left);
            let right = self.float_operand(right);
            return format!("math::pow({left}, {right})");
        }
        // Balaur Script has no bitwise operators: the shim does them on ints.
        if let Some(verb) = bit_verb(op) {
            let left = self.expression(left);
            let right = self.expression(right);
            self.uses_shim = true;
            return format!("gd::{verb}({left}, {right})");
        }
        // Two literal strings are one.
        if op == "+"
            && let (Expr::Str(a), Expr::Str(b)) = (left, right)
        {
            return super::quoted(&format!("{a}{b}"));
        }
        // Strings joined, which `+` does not do here: one interpolation, a
        // literal part written in place and any other spliced in.
        if op == "+" && lt == Bt::Str && rt == Bt::Str {
            let mut parts = Vec::new();
            self.string_parts(left, &mut parts);
            self.string_parts(right, &mut parts);
            let mut text = String::from("\"");
            for part in parts {
                text.push_str(&part);
            }
            text.push('"');
            return text;
        }
        // Any other operand whose type is not known: Godot's operator on
        // whatever the value turns out to be.
        if !(lt.numeric() && rt.numeric()) && !matches!(op, "==" | "!=") {
            let verb = match op {
                "+" => Some("add"),
                "-" => Some("sub"),
                "*" => Some("mul"),
                "/" => Some("div"),
                "<" => Some("lt"),
                "<=" => Some("le"),
                ">" => Some("gt"),
                ">=" => Some("ge"),
                _ => None,
            };
            if let Some(verb) = verb {
                let left = self.expression(left);
                let right = self.expression(right);
                self.uses_shim = true;
                return format!("gd::{verb}({left}, {right})");
            }
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
        // An int beside a float is widened, as Godot widens it.
        let mixed = lt.numeric() && rt.numeric() && lt != rt;
        let left_text = if mixed && lt == Bt::Int {
            self.float_operand(left)
        } else {
            let text = self.expression(left);
            group(left, text, false)
        };
        let right_text = if mixed && rt == Bt::Int {
            self.float_operand(right)
        } else {
            let text = self.expression(right);
            group(right, text, true)
        };
        format!("{left_text} {op} {right_text}")
    }

    /// The pieces of a chain of strings joined by `+`, each as it stands
    /// inside one string literal.
    fn string_parts(&mut self, value: &Expr, parts: &mut Vec<String>) {
        if let Expr::Binary("+", left, right) = value
            && self.type_of(left) == Bt::Str
            && self.type_of(right) == Bt::Str
        {
            self.string_parts(left, parts);
            self.string_parts(right, parts);
            return;
        }
        if let Expr::Str(text) = value {
            let quoted = super::quoted(text);
            parts.push(quoted[1..quoted.len() - 1].to_string());
            return;
        }
        let text = self.expression(value);
        parts.push(format!("{{{text}}}"));
    }

    /// An operand as a float: an int widened.
    pub(super) fn float_operand(&mut self, value: &Expr) -> String {
        if let Expr::Int(text) = value {
            return format!("{text}.0");
        }
        let text = self.expression(value);
        match self.type_of(value) {
            Bt::Float => text,
            Bt::Int => format!("({text} as float)"),
            _ => {
                self.uses_shim = true;
                format!("gd::float({text})")
            }
        }
    }

    pub(super) fn dict(&mut self, pairs: &[(Expr, Expr)]) -> String {
        let literal = pairs.iter().all(|(key, _)| matches!(key, Expr::Str(_)));
        if literal {
            let parts: Vec<(String, String)> = pairs
                .iter()
                .map(|(key, value)| (self.expression(key), self.expression(value)))
                .collect();
            let mut out = "map()".to_string();
            for (key, value) in parts {
                out.push_str(&format!(".with({key}, {value})"));
            }
            return out;
        }
        // A key that is not a literal string cannot stand in an object
        // literal, so the shim builds the map from pairs.
        let mut flat = Vec::new();
        for (key, value) in pairs {
            flat.push(self.expression(key));
            flat.push(self.expression(value));
        }
        self.uses_shim = true;
        format!("gd::dict({})", super::map::any_list(&flat))
    }

    /// A lambda, lifted into a method of its own: Godot captures a local by
    /// value when the lambda is made, and so does this, in the map the
    /// callable carries. The method takes that map first.
    pub(super) fn lambda(&mut self, params: &[String], body: &[Stmt]) -> String {
        let mut used = BTreeSet::new();
        for stmt in body {
            names_in_stmt(stmt, &mut used);
        }
        let captured: Vec<String> = used
            .into_iter()
            .filter(|name| self.is_local(name) && !params.contains(name))
            .collect();
        // Named for the function it is in, and numbered across the file.
        let serial = self.context.lambdas.get();
        self.context.lambdas.set(serial + 1);
        let name = format!(
            "__lambda_{}_{serial}",
            self.enclosing.trim_start_matches('_')
        );
        let outer_scopes = std::mem::replace(&mut self.scopes, vec![BTreeSet::new()]);
        let outer_awaits = std::mem::replace(&mut self.awaits, false);
        for local in captured.iter().chain(params.iter()) {
            self.declare(local);
        }
        let mut text = String::new();
        for local in &captured {
            let bound = safe(local);
            text.push_str(&format!(
                "        let {bound} = cap.get({});\n",
                super::quoted(local)
            ));
        }
        // A lambda answers what it returns; a lone expression is a statement,
        // as it is in GDScript.
        let returns = body
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Return(Some(_))));
        text.push_str(&self.block(body, 2));
        if returns {
            text.push_str("        return nil;\n");
        }
        self.scopes = outer_scopes;
        let awaited = std::mem::replace(&mut self.awaits, outer_awaits);
        let mut args = vec!["cap: any".to_string()];
        args.extend(params.iter().map(|p| format!("{}: any", safe(p))));
        if self.context.object_class {
            args.insert(0, "this: any".to_string());
        }
        let word = if awaited { "pub async fn" } else { "pub fn" };
        let ret = if returns { " -> any" } else { "" };
        self.lifted.push(format!(
            "    {word} {name}({}){ret} {{\n{text}    }}\n",
            args.join(", ")
        ));
        let mut cap = "map()".to_string();
        for local in &captured {
            cap.push_str(&format!(".with({}, {})", super::quoted(local), safe(local)));
        }
        self.uses_shim = true;
        let target = if self.context.object_class {
            "this"
        } else {
            "self.node"
        };
        format!("gd::lambda({target}, {}, {cap})", super::quoted(&name))
    }
}

/// The shim's name for a bitwise operator.
pub(super) fn bit_verb(op: &str) -> Option<&'static str> {
    Some(match op {
        "|" => "bit_or",
        "&" => "bit_and",
        "^" => "bit_xor",
        "<<" => "shift_left",
        ">>" => "shift_right",
        _ => return None,
    })
}

/// Whether an expression is a literal, whose type is known where it stands.
fn literal(value: &Expr) -> bool {
    matches!(
        value,
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_)
    )
}

/// The operator of a binary expression, for grouping a mixed `and`/`or`.
fn e_op(value: &Expr) -> &'static str {
    match value {
        Expr::Binary(op, ..) => op,
        _ => "",
    }
}

// Every bare name a statement reads, for what a lambda captures.
fn names_in_stmt(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match stmt {
        Stmt::Var { value: Some(v), .. } | Stmt::Expr(v) | Stmt::Return(Some(v)) => {
            names_in(v, out);
        }
        Stmt::Assign { target, value, .. } => {
            names_in(target, out);
            names_in(value, out);
        }
        Stmt::If { arms, other } => {
            for (cond, body) in arms {
                names_in(cond, out);
                body.iter().for_each(|s| names_in_stmt(s, out));
            }
            other.iter().flatten().for_each(|s| names_in_stmt(s, out));
        }
        Stmt::While { cond, body } => {
            names_in(cond, out);
            body.iter().for_each(|s| names_in_stmt(s, out));
        }
        Stmt::For { iter, body, .. } => {
            names_in(iter, out);
            body.iter().for_each(|s| names_in_stmt(s, out));
        }
        Stmt::Match { subject, arms } => {
            names_in(subject, out);
            for arm in arms {
                arm.patterns.iter().for_each(|p| names_in(p, out));
                arm.body.iter().for_each(|s| names_in_stmt(s, out));
            }
        }
        _ => {}
    }
}

fn names_in(value: &Expr, out: &mut BTreeSet<String>) {
    match value {
        Expr::Name(name) => {
            out.insert(name.clone());
        }
        Expr::Field(object, _) | Expr::Unary(_, object) | Expr::Await(object) => {
            names_in(object, out)
        }
        Expr::Cast(object, _) | Expr::Is(object, ..) => names_in(object, out),
        Expr::Index(a, b) | Expr::Binary(_, a, b) => {
            names_in(a, out);
            names_in(b, out);
        }
        Expr::Call(callee, args) => {
            names_in(callee, out);
            args.iter().for_each(|a| names_in(a, out));
        }
        Expr::Ternary { then, cond, other } => {
            names_in(then, out);
            names_in(cond, out);
            names_in(other, out);
        }
        Expr::Array(items) => items.iter().for_each(|a| names_in(a, out)),
        Expr::Dict(pairs) => pairs.iter().for_each(|(k, v)| {
            names_in(k, out);
            names_in(v, out);
        }),
        Expr::Lambda { body, .. } => body.iter().for_each(|s| names_in_stmt(s, out)),
        _ => {}
    }
}
