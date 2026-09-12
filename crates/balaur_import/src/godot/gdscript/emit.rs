//! The tree as Rune text.
//!
//! Three Rune rules shape what comes out, all of them from `AGENTS.md`:
//! `obj.field = a || b` overwrites `a` when `a` is a local, so a short-circuit
//! into a field goes through a temporary; `match` has no or-patterns, so a
//! GDScript `match` becomes an `if` chain; and `a[i] += v` is unsupported, so
//! an indexed compound assignment is expanded.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::ast::{Expr, MatchArm, Stmt};
use super::map;

/// What the emitter has to know about the class it is inside, so a bare name
/// resolves the way GDScript resolved it.
#[derive(Default)]
pub(crate) struct Context {
    /// Member variables, own and inherited: a bare one is `this.name`.
    pub members: BTreeSet<String>,
    /// Instance methods, own and inherited: a bare call takes `this`.
    pub methods: BTreeSet<String>,
    /// Static methods, which take no `this`.
    pub statics: BTreeSet<String>,
    /// Class constants, emitted as module constants and read bare.
    pub consts: BTreeSet<String>,
    /// Signals, so `sig.emit(x)` and `sig.connect(f)` are known to be signals.
    pub signals: BTreeSet<String>,
    /// Functions the async pass found, so a call to one gets `.await`.
    pub asyncs: BTreeSet<String>,
    /// A GDScript name that had to change, so calls reach the new one.
    pub renames: BTreeMap<String, String>,
    /// `class_name` to the `.rn` beside it, for a static call on a project
    /// class.
    pub classes: BTreeMap<String, String>,
}

/// Rune's reserved words: a GDScript name that is one gains a trailing `_`.
pub(crate) const RESERVED: &[&str] = &[
    "abstract", "alignof", "as", "async", "await", "become", "break", "const", "continue", "crate",
    "default", "do", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
    "is", "let", "loop", "macro", "match", "mod", "move", "mut", "not", "offsetof", "override",
    "priv", "proc", "pure", "ref", "return", "select", "self", "Self", "sizeof", "static",
    "struct", "super", "true", "typeof", "unsafe", "use", "virtual", "while", "yield",
];

pub(crate) fn safe(name: &str) -> String {
    if RESERVED.contains(&name) {
        return format!("{name}_");
    }
    name.to_string()
}

pub(crate) struct Emitter<'a> {
    pub context: &'a Context,
    pub notes: Vec<String>,
    /// Locals in scope, innermost last. A name found here is neither a member
    /// nor a global.
    scopes: Vec<BTreeSet<String>>,
    /// Names of temporaries already handed out, so nested ones do not collide.
    temps: usize,
    /// Set while emitting a function that awaits.
    pub awaits: bool,
    /// Set when the body reached a shim call, so the caller binds `gd`.
    pub uses_shim: bool,
}

impl<'a> Emitter<'a> {
    pub(crate) fn new(context: &'a Context) -> Self {
        Self {
            context,
            notes: Vec::new(),
            scopes: vec![BTreeSet::new()],
            temps: 0,
            awaits: false,
            uses_shim: false,
        }
    }

    pub(crate) fn declare(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }

    fn temp(&mut self) -> String {
        self.temps += 1;
        format!("tmp{}", self.temps)
    }

    /// A rewrite, with the shim flagged where one reached it.
    fn shimmed(&mut self, text: String) -> String {
        if text.contains(map::SHIM_MARK) {
            self.uses_shim = true;
        }
        text
    }

    fn note(&mut self, text: String) {
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

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
                let _ = writeln!(out, "{pad}return {text};");
            }
            Stmt::Raw(text) => {
                self.note(format!("`{text}`: this line is not translated"));
                let _ = writeln!(out, "{pad}// TODO(gdscript): {text}");
            }
            Stmt::Var { name, value, hint } => {
                let text = match value {
                    Some(value) => self.expression(value),
                    // A declaration with no value: its type says what empty is.
                    None => empty_for(hint.as_deref()).to_string(),
                };
                let bound = safe(name);
                let _ = writeln!(out, "{pad}let {bound} = {text};");
                self.declare(name);
            }
            Stmt::Assign { target, op, value } => {
                out.push_str(&self.assignment(target, op, value, &pad));
            }
            Stmt::Expr(value) => {
                let text = self.expression(value);
                let _ = writeln!(out, "{pad}{text};");
            }
            Stmt::If { arms, other } => {
                for (index, (cond, body)) in arms.iter().enumerate() {
                    let cond = self.expression(cond);
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
                let cond = self.expression(cond);
                let _ = writeln!(out, "{pad}while {cond} {{");
                out.push_str(&self.block(body, depth + 1));
                let _ = writeln!(out, "{pad}}}");
            }
            Stmt::For { name, iter, body } => {
                let iter = self.expression(iter);
                let bound = safe(name);
                let _ = writeln!(out, "{pad}for {bound} in {iter} {{");
                self.scopes.push(BTreeSet::from([name.clone()]));
                out.push_str(&self.block(body, depth + 1));
                self.scopes.pop();
                let _ = writeln!(out, "{pad}}}");
            }
            Stmt::Match { subject, arms } => out.push_str(&self.match_chain(subject, arms, depth)),
        }
        out
    }

    /// GDScript's `match` as an `if` chain: Rune has no or-patterns, and an
    /// arm here may carry several values.
    fn match_chain(&mut self, subject: &Expr, arms: &[MatchArm], depth: usize) -> String {
        let pad = "    ".repeat(depth);
        let mut out = String::new();
        let subject = self.expression(subject);
        let name = self.temp();
        let _ = writeln!(out, "{pad}let {name} = {subject};");
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
                    format!("{name} == {value}")
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

    fn assignment(&mut self, target: &Expr, op: &str, value: &Expr, pad: &str) -> String {
        let mut out = String::new();
        let indexed = matches!(target, Expr::Index(..));
        // `a[i] += v` is not supported; expanding it is the whole fix.
        if indexed && op != "=" {
            let place = self.expression(target);
            let text = self.expression(value);
            let operator = op.trim_end_matches('=');
            let _ = writeln!(out, "{pad}{place} = {place} {operator} {text};");
            return out;
        }
        let short_circuit = matches!(value, Expr::Binary("||" | "&&", ..));
        let place_is_plain = matches!(target, Expr::Name(name) if self.is_local(name));
        // A short-circuit assigned into a field or an index lands in the left
        // operand's slot too, so it goes through a temporary.
        if short_circuit && !place_is_plain {
            let text = self.expression(value);
            let name = self.temp();
            let place = self.expression(target);
            let _ = writeln!(out, "{pad}let {name} = {text};");
            let _ = writeln!(out, "{pad}{place} {op} {name};");
            self.declare(&name);
            return out;
        }
        if let Some(text) = self.property_write(target, op, value, pad) {
            return text;
        }
        let place = self.expression(target);
        let text = self.expression(value);
        let _ = writeln!(out, "{pad}{place} {op} {text};");
        out
    }

    /// The receiver and property name where an expression is itself a Godot
    /// property read: `modulate` on the node, or `node.modulate`.
    fn property_base(&mut self, value: &Expr) -> Option<(String, String)> {
        match value {
            Expr::Name(name)
                if !self.is_local(name)
                    && !self.context.members.contains(name)
                    && !self.context.consts.contains(name)
                    && map::setter("x", name, "y").is_some() =>
            {
                Some(("this.node".to_string(), name.clone()))
            }
            Expr::Field(base, property) if map::setter("x", property, "y").is_some() => {
                if matches!(**base, Expr::SelfRef) && self.context.members.contains(property) {
                    return None;
                }
                let receiver = self.expression(base);
                Some((receiver, property.clone()))
            }
            _ => None,
        }
    }

    /// Writing a Godot property, which is a call here rather than a place. A
    /// component of one — `modulate.a = 0.5` — is read, changed and written
    /// back, because the getter hands out a copy.
    fn property_write(
        &mut self,
        target: &Expr,
        op: &str,
        value: &Expr,
        pad: &str,
    ) -> Option<String> {
        let (object, field) = match target {
            Expr::Name(name)
                if !self.is_local(name)
                    && !self.context.members.contains(name)
                    && !self.context.consts.contains(name) =>
            {
                ("this.node".to_string(), name.clone())
            }
            Expr::Field(object, field) => {
                if matches!(**object, Expr::SelfRef)
                    && (self.context.members.contains(field)
                        || self.context.consts.contains(field))
                {
                    return None;
                }
                // `node.modulate.a = x`: the base is a property, so its value
                // is read out, changed, and written back.
                if let Some((receiver, property)) = self.property_base(object) {
                    let getter = map::property(&receiver, &property)?;
                    let name = self.temp();
                    let text = self.expression(value);
                    let write = map::setter(&receiver, &property, &name)?;
                    self.uses_shim |= write.contains(map::SHIM_MARK);
                    self.declare(&name);
                    return Some(format!(
                        "{pad}let {name} = {getter};\n{pad}{name}.{field} {op} {text};\n{pad}{write};\n"
                    ));
                }
                (self.expression(object), field.clone())
            }
            _ => return None,
        };
        if op != "=" {
            // A compound write reads through the getter first.
            let getter = map::property(&object, &field)?;
            let text = self.expression(value);
            let operator = op.trim_end_matches('=');
            let write = map::setter(&object, &field, &format!("{getter} {operator} {text}"))?;
            self.uses_shim |= write.contains(map::SHIM_MARK);
            return Some(format!("{pad}{write};\n"));
        }
        let text = self.expression(value);
        let write = map::setter(&object, &field, &text)?;
        self.uses_shim |= write.contains(map::SHIM_MARK);
        Some(format!("{pad}{write};\n"))
    }

    pub(crate) fn expression(&mut self, value: &Expr) -> String {
        match value {
            Expr::Int(text) => text.clone(),
            Expr::Float(text) => {
                // Rune never mixes ints and floats, so a float literal must
                // read as one: `1e3` and `2.` are not float tokens there.
                if text.contains(['e', 'E']) && !text.contains('.') {
                    return format!("{text}f64");
                }
                if let Some(head) = text.strip_suffix('.') {
                    return format!("{head}.0");
                }
                text.clone()
            }
            Expr::Str(text) => quoted(text),
            Expr::Bool(true) => "true".into(),
            Expr::Bool(false) => "false".into(),
            Expr::Nil => "()".into(),
            Expr::SelfRef => "this.node".into(),
            Expr::Name(name) => self.name(name),
            Expr::Field(object, field) => self.field(object, field),
            Expr::Index(object, index) => {
                let object = self.expression(object);
                let index = self.expression(index);
                format!("{object}[{index}]")
            }
            Expr::Call(callee, args) => self.call(callee, args),
            Expr::Unary(op, inner) => {
                let inner = self.expression(inner);
                format!("{op}{inner}")
            }
            Expr::Binary(op, left, right) => self.binary(op, left, right),
            Expr::Ternary { then, cond, other } => {
                let cond = self.expression(cond);
                let then = self.expression(then);
                let other = self.expression(other);
                // Parenthesised: a bare `}` followed by `(` or `[` parses as a
                // call or an index in Rune.
                format!("(if {cond} {{ {then} }} else {{ {other} }})")
            }
            Expr::Array(items) => {
                let items: Vec<String> = items.iter().map(|item| self.expression(item)).collect();
                format!("[{}]", items.join(", "))
            }
            Expr::Dict(pairs) => self.dict(pairs),
            Expr::Lambda { params, body } => self.lambda(params, body),
            Expr::NodePath(path) => format!("this.node.get_node({})", quoted(path)),
            Expr::Unique(name) => format!("this.node.get_node({})", quoted(&format!("%{name}"))),
            Expr::Await(inner) => {
                self.awaits = true;
                let inner = self.expression(inner);
                format!("{inner}.await")
            }
            Expr::Cast(inner, name) => {
                let text = self.expression(inner);
                let cast = map::cast(&text, name);
                self.shimmed(cast)
            }
            Expr::Is(inner, name, negated) => {
                let inner = self.expression(inner);
                let test = self.shimmed(map::type_test(&inner, name));
                if *negated {
                    return format!("!{test}");
                }
                test
            }
        }
    }

    fn name(&mut self, name: &str) -> String {
        if name == "_" {
            return "_".into();
        }
        if self.is_local(name) {
            return safe(name);
        }
        if self.context.consts.contains(name) {
            return name.to_string();
        }
        if self.context.members.contains(name) {
            return format!("this.{}", safe(name));
        }
        if self.context.signals.contains(name) {
            return quoted(name);
        }
        if let Some(text) = map::constant(name) {
            return self.shimmed(text);
        }
        if self.context.classes.contains_key(name) {
            return self.class_module(name);
        }
        // Godot read a bare name off `self` when the class declared none, and
        // the node's own properties are what is left.
        if let Some(text) = map::property("this.node", name) {
            return text;
        }
        safe(name)
    }

    /// A project class used as a value: the module beside it, required once
    /// per use. `script::require` is cached by the engine, so this is a lookup
    /// rather than a load.
    fn class_module(&mut self, name: &str) -> String {
        match self.context.classes.get(name) {
            Some(path) => format!("script::require({})", quoted(path)),
            None => safe(name),
        }
    }

    fn field(&mut self, object: &Expr, field: &str) -> String {
        if matches!(object, Expr::SelfRef) {
            if self.context.members.contains(field) {
                return format!("this.{}", safe(field));
            }
            if self.context.consts.contains(field) {
                return field.to_string();
            }
            if self.context.signals.contains(field) {
                return quoted(field);
            }
        }
        if let Expr::Name(class) = object
            && !self.is_local(class)
            && !self.context.members.contains(class)
        {
            // A value on one of Godot's own types: `Vector2.ZERO`.
            if let Some(text) = map::static_value(class, field) {
                return self.shimmed(text);
            }
            // A static on a project class reads through its module, where the
            // function is a field and must be called in parentheses.
            if self.context.classes.contains_key(class) {
                let module = self.class_module(class);
                return format!("({module}.{})", safe(field));
            }
        }
        let text = self.expression(object);
        if let Some(mapped) = map::property(&text, field) {
            return self.shimmed(mapped);
        }
        format!("{text}.{}", safe(field))
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> String {
        // `self.method(..)` and a bare `method(..)` are the same call here.
        let own = match callee {
            Expr::Name(name) if !self.is_local(name) => Some(name.clone()),
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => Some(name.clone()),
            _ => None,
        };
        if let Some(name) = own {
            if self.context.methods.contains(&name) {
                let bound = self.method_name(&name);
                let mut parts = vec!["this".to_string()];
                parts.extend(args.iter().map(|arg| self.expression(arg)));
                let text = format!("{bound}({})", parts.join(", "));
                return self.awaited(&name, text);
            }
            if self.context.statics.contains(&name) {
                let bound = self.method_name(&name);
                let parts: Vec<String> = args.iter().map(|arg| self.expression(arg)).collect();
                let text = format!("{bound}({})", parts.join(", "));
                return self.awaited(&name, text);
            }
        }
        // A signal's own verbs: `sig.emit(..)`, `sig.connect(..)`.
        if let Expr::Field(object, verb) = callee
            && let Some(signal) = self.signal_of(object)
        {
            let parts: Vec<String> = args.iter().map(|arg| self.expression(arg)).collect();
            if let Some(text) = map::signal_verb(&signal, verb, &parts) {
                return self.shimmed(text);
            }
        }
        let parts: Vec<String> = args.iter().map(|arg| self.expression(arg)).collect();
        if let Expr::Field(object, method) = callee
            && let Expr::Name(class) = &**object
            && !self.is_local(class)
            && !self.context.members.contains(class)
            && !self.context.classes.contains_key(class)
            && class.chars().next().is_some_and(char::is_uppercase)
        {
            if let Some(text) = map::static_call(class, method, &parts) {
                return self.shimmed(text);
            }
            self.note(format!("`{class}.{method}()`: no engine call for it"));
        }
        if let Expr::Field(object, method) = callee {
            let receiver = self.expression(object);
            if let Some(text) = map::method(&receiver, method, &parts) {
                return self.shimmed(text);
            }
            return format!("{receiver}.{}({})", safe(method), parts.join(", "));
        }
        if let Expr::Name(name) = callee
            && !self.is_local(name)
        {
            if let Some(text) = map::global(name, &parts) {
                return self.shimmed(text);
            }
            if let Some(head) = map::value_type(name) {
                self.uses_shim = true;
                return format!("{head}({})", parts.join(", "));
            }
            // Godot resolved a bare call on `self` when the class had no
            // function of that name, and most of those are node verbs.
            if let Some(text) = map::implicit_self(name, &parts) {
                return self.shimmed(text);
            }
            self.note(format!("`{name}()`: no engine call of that name"));
        }
        let head = self.expression(callee);
        format!("{head}({})", parts.join(", "))
    }

    /// The signal a `sig.emit(..)` was written on, where the receiver names
    /// one this class declares.
    fn signal_of(&self, object: &Expr) -> Option<String> {
        let name = match object {
            Expr::Name(name) if !self.is_local(name) => name,
            Expr::Field(inner, name) if matches!(**inner, Expr::SelfRef) => name,
            _ => return None,
        };
        self.context.signals.contains(name).then(|| name.clone())
    }

    fn method_name(&self, name: &str) -> String {
        match self.context.renames.get(name) {
            Some(bound) => bound.clone(),
            None => safe(name),
        }
    }

    /// A call to a function the async pass marked gets its `.await` here, so
    /// callers need no `await` of their own in the source.
    fn awaited(&mut self, name: &str, text: String) -> String {
        if self.context.asyncs.contains(name) {
            self.awaits = true;
            return format!("{text}.await");
        }
        text
    }

    fn binary(&mut self, op: &str, left: &Expr, right: &Expr) -> String {
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
        if op == "in" {
            let left = self.expression(left);
            let right = self.expression(right);
            self.uses_shim = true;
            return format!("(gd.has)({right}, {left})");
        }
        if op == "**" {
            let left = self.expression(left);
            let right = self.expression(right);
            return format!("math::pow({left}, {right})");
        }
        let left = self.expression(left);
        let right = self.expression(right);
        format!("{left} {op} {right}")
    }

    fn dict(&mut self, pairs: &[(Expr, Expr)]) -> String {
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
            if parts.is_empty() {
                return "#{}".into();
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

    fn lambda(&mut self, params: &[String], body: &[Stmt]) -> String {
        self.scopes.push(params.iter().cloned().collect());
        let bound: Vec<String> = params.iter().map(|name| safe(name)).collect();
        let out = match body {
            // A one-expression lambda reads as one, which is what most of
            // these are.
            [Stmt::Return(Some(value))] | [Stmt::Expr(value)] => {
                let text = self.expression(value);
                format!("|{}| {{ {text} }}", bound.join(", "))
            }
            _ => {
                let text = self.block(body, 2);
                format!("|{}| {{\n{text}    }}", bound.join(", "))
            }
        };
        self.scopes.pop();
        out
    }
}

/// A declared type's empty value, so `var x: float` is not an int.
fn empty_for(hint: Option<&str>) -> &'static str {
    match hint {
        Some("float") => "0.0",
        Some("int") => "0",
        Some("bool") => "false",
        Some("String" | "StringName") => "\"\"",
        Some("Array" | "PackedStringArray" | "PackedFloat32Array" | "PackedInt32Array") => "[]",
        Some("Dictionary") => "#{}",
        _ => "()",
    }
}

/// A Rune string literal for arbitrary text.
pub(crate) fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
