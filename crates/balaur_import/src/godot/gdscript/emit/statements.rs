//! A statement as a line or a block: a declaration with the type its value
//! or hint gives, a `for` over a counted range or the shim's list, and
//! GDScript's `match` as an `if` chain over `gd::same`.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::{Bt, Emitter, MARKER, discardable, map, quoted, safe, typed_zero};
use crate::godot::gdscript::ast::{Expr, MatchArm, Stmt};

impl Emitter<'_> {
    /// A block of statements at `depth` tabs of four spaces.
    pub(crate) fn block(&mut self, body: &[Stmt], depth: usize) -> String {
        self.scopes.push(BTreeSet::new());
        let mut out = String::new();
        for stmt in body {
            out.push_str(&self.statement(stmt, depth));
        }
        self.scopes.pop();
        out
    }

    fn statement(&mut self, stmt: &Stmt, depth: usize) -> String {
        let pad = "    ".repeat(depth);
        let outer_before = std::mem::take(&mut self.before);
        let outer_after = std::mem::take(&mut self.after);
        let body = self.statement_body(stmt, depth);
        let before = std::mem::replace(&mut self.before, outer_before);
        let after = std::mem::replace(&mut self.after, outer_after);
        let mut out = String::new();
        for line in before {
            let _ = writeln!(out, "{pad}{line}");
        }
        // What must run after a `return` runs before it, on the value kept.
        let returned = body
            .trim_start()
            .strip_prefix("return ")
            .and_then(|t| t.trim_end().strip_suffix(';'))
            .map(str::to_string);
        if let Some(value) = returned.filter(|_| !after.is_empty()) {
            let name = self.temp();
            let _ = writeln!(out, "{pad}let {name}: any = {value};");
            for line in after {
                let _ = writeln!(out, "{pad}{line}");
            }
            let _ = writeln!(out, "{pad}return {name};");
            return self.bare_modules(out);
        }
        out.push_str(&body);
        for line in after {
            let _ = writeln!(out, "{pad}{line}");
        }
        self.bare_modules(out)
    }

    /// A class named as a value, not an item of it: its module's file, which
    /// is what the shim knows a class by.
    fn bare_modules(&self, text: String) -> String {
        let mut out = text;
        for (name, path) in &self.modules {
            let prefix = format!("{name}::");
            let mut done = String::with_capacity(out.len());
            let mut rest = out.as_str();
            while let Some(at) = rest.find(&prefix) {
                let before = rest[..at].chars().last();
                let after = rest[at + prefix.len()..].chars().next();
                let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
                done.push_str(&rest[..at]);
                if word(before) || word(after) {
                    done.push_str(&prefix);
                } else {
                    done.push_str(&quoted(path));
                }
                rest = &rest[at + prefix.len()..];
            }
            done.push_str(rest);
            out = done;
        }
        out
    }

    fn statement_body(&mut self, stmt: &Stmt, depth: usize) -> String {
        let pad = "    ".repeat(depth);
        let mut out = String::new();
        match stmt {
            Stmt::Pass => {}
            Stmt::Break => {
                let _ = writeln!(out, "{pad}break;");
            }
            Stmt::Continue => {
                let _ = writeln!(out, "{pad}continue;");
            }
            Stmt::Return(None) => {
                let _ = writeln!(out, "{pad}return;");
            }
            Stmt::Return(Some(value)) => {
                let text = self.expression(value);
                // A function declared `-> bool` answers Godot's truth of what
                // it returns.
                let text = if self.context.returns.get(&self.enclosing) == Some(&Bt::Bool)
                    && !self.is_boolish(value)
                    && self.type_of(value) != Bt::Bool
                {
                    self.uses_shim = true;
                    format!("gd::truthy({text})")
                } else {
                    text
                };
                // Rune reads `return || f` as `(return) || f`.
                let text = if text.starts_with('|') {
                    format!("({text})")
                } else {
                    text
                };
                let _ = writeln!(out, "{pad}return {text};");
            }
            Stmt::Raw(text) => {
                self.note(format!("`{text}`: this line is not translated"));
                let _ = writeln!(out, "{pad}// {MARKER} {text}");
            }
            Stmt::Var { name, value, hint } => {
                let (text, known) = self.var_value(hint.as_deref(), value.as_ref());
                let bound = safe(name);
                let _ = writeln!(out, "{pad}let {bound}: {} = {text};", known.text());
                self.declare(name);
                self.local_types.insert(name.clone(), known);
                self.note_local_type(name, hint.as_deref(), value.as_ref());
            }
            Stmt::Assign { target, op, value } => {
                out.push_str(&self.assignment(target, op, value, &pad));
            }
            Stmt::Expr(value) => {
                self.bare = true;
                let text = self.expression(value);
                // A call that is nothing here leaves no statement.
                if text != "nil" {
                    let _ = writeln!(out, "{pad}{};", discardable(&text));
                }
            }
            Stmt::If { arms, other } => {
                for (index, (cond, body)) in arms.iter().enumerate() {
                    let cond = self.condition(cond);
                    let word = if index == 0 { "if" } else { "} else if" };
                    let _ = writeln!(out, "{pad}{word} {cond} {{");
                    out.push_str(&self.block(body, depth + 1));
                }
                if let Some(body) = other {
                    let _ = writeln!(out, "{pad}}} else {{");
                    out.push_str(&self.block(body, depth + 1));
                }
                let _ = writeln!(out, "{pad}}}");
            }
            Stmt::While { cond, body } => {
                let cond = self.condition(cond);
                let _ = writeln!(out, "{pad}while {cond} {{");
                out.push_str(&self.block(body, depth + 1));
                let _ = writeln!(out, "{pad}}}");
            }
            Stmt::For { name, iter, body } => {
                let bound = safe(name);
                let (iter, element) = self.iterated(iter);
                let _ = writeln!(out, "{pad}for {bound} in {iter} {{");
                self.scopes.push(BTreeSet::from([name.clone()]));
                self.local_types.insert(name.clone(), element);
                out.push_str(&self.block(body, depth + 1));
                self.scopes.pop();
                let _ = writeln!(out, "{pad}}}");
            }
            Stmt::Match { subject, arms } => out.push_str(&self.match_chain(subject, arms, depth)),
        }
        out
    }

    /// A declaration's value and its type: the hint's, else what the value is
    /// known to be, else Godot's Variant, since a later write may hold anything.
    fn var_value(&mut self, hint: Option<&str>, value: Option<&Expr>) -> (String, Bt) {
        let declared = hint.map(Bt::of_hint);
        let text = match value {
            // A typed int takes a float as its whole part, which is
            // what a number from JSON arrives as.
            Some(value) if hint == Some("int") && !matches!(value, Expr::Int(_)) => {
                let text = self.expression(value);
                self.uses_shim = true;
                format!("gd::int({text})")
            }
            Some(value) if declared == Some(Bt::Float) && self.type_of(value) == Bt::Int => {
                self.float_operand(value)
            }
            Some(value) => self.expression(value),
            // A declaration with no value: its type says what empty is.
            None => {
                let zero = typed_zero(hint.unwrap_or_default());
                self.uses_shim |= zero.contains(map::SHIM_MARK);
                zero.to_string()
            }
        };
        let known = declared.unwrap_or_else(|| match value {
            Some(value) => self.type_of(value),
            None => Bt::Any,
        });
        (text, known)
    }

    /// What a `for` walks, with the type of each element: `range(a, b)` and an
    /// int count in ints; anything else Godot walks (a dictionary's keys, a
    /// string's characters) the shim lays out as a list.
    fn iterated(&mut self, iter: &Expr) -> (String, Bt) {
        let counted = match iter {
            Expr::Call(callee, args)
                if matches!(&**callee, Expr::Name(n) if n == "range" && !self.is_local(n))
                    && (1..=2).contains(&args.len()) =>
            {
                // A range counts in ints; Godot truncates a float end.
                let ends: Vec<String> = args
                    .iter()
                    .map(|a| {
                        let text = self.expression(a);
                        if self.type_of(a) == Bt::Int {
                            text
                        } else {
                            self.uses_shim = true;
                            format!("gd::int({text})")
                        }
                    })
                    .collect();
                Some(if ends.len() == 1 {
                    format!("0..{}", ends[0])
                } else {
                    format!("{}..{}", ends[0], ends[1])
                })
            }
            _ => None,
        };
        // `for i in n` counts to an int's value.
        let counted = counted.or_else(|| {
            (self.type_of(iter) == Bt::Int).then(|| format!("0..{}", self.expression(iter)))
        });
        match counted {
            Some(range) => (range, Bt::Int),
            None => {
                let iter = self.expression(iter);
                self.uses_shim = true;
                (format!("gd::iter({iter})"), Bt::Any)
            }
        }
    }

    /// GDScript's `match` as an `if` chain: Rune has no or-patterns, and an
    /// arm here may carry several values.
    fn match_chain(&mut self, subject: &Expr, arms: &[MatchArm], depth: usize) -> String {
        let pad = "    ".repeat(depth);
        let mut out = String::new();
        let subject = self.expression(subject);
        let name = self.temp();
        let _ = writeln!(out, "{pad}let {name}: any = {subject};");
        self.declare(&name);
        let mut started = false;
        for arm in arms {
            let wildcard = arm
                .patterns
                .iter()
                .any(|p| matches!(p, Expr::Name(word) if word == "_"));
            if wildcard {
                let word = if started { "} else {" } else { "if true {" };
                let _ = writeln!(out, "{pad}{word}");
                out.push_str(&self.block(&arm.body, depth + 1));
                started = true;
                continue;
            }
            let tests: Vec<String> = arm
                .patterns
                .iter()
                .map(|pattern| {
                    let value = self.expression(pattern);
                    self.uses_shim = true;
                    format!("gd::same({name}, {value})")
                })
                .collect();
            let word = if started { "} else if" } else { "if" };
            let _ = writeln!(out, "{pad}{word} {} {{", tests.join(" || "));
            out.push_str(&self.block(&arm.body, depth + 1));
            started = true;
        }
        if started {
            let _ = writeln!(out, "{pad}}}");
        }
        out
    }
}
