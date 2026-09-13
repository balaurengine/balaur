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
    /// Constants whose value needs the shim or a node, so they are functions
    /// here and are read by calling them.
    pub lazy: BTreeSet<String>,
    /// Rune names a base's overridden copy was emitted under, so `super`
    /// reaches it. Godot's inheritance is flattened, so the copy is a
    /// function of this module named `<name>__base`.
    pub bases: BTreeSet<String>,
    /// Signals, so `sig.emit(x)` and `sig.connect(f)` are known to be signals.
    pub signals: BTreeSet<String>,
    /// Functions the async pass found, so a call to one gets `.await`.
    pub asyncs: BTreeSet<String>,
    /// A GDScript name that had to change, so calls reach the new one.
    pub renames: BTreeMap<String, String>,
    /// `class_name` to the `.rn` beside it, for a static call on a project
    /// class.
    pub classes: BTreeMap<String, String>,
    /// `static var` names to the Rune text of their default. A Rune module
    /// holds no state, so these live on the scene root under a key this
    /// prefix makes unique per script.
    pub static_vars: BTreeMap<String, String>,
    pub static_prefix: String,
}

/// The members that stand in for Godot's per-node process switch.
pub(crate) const PROCESS_FLAG: &str = "process_enabled";
pub(crate) const PHYSICS_PROCESS_FLAG: &str = "physics_process_enabled";

/// What a base's overridden function is named here, so `super` can reach it.
pub(crate) const BASE_SUFFIX: &str = "__base";

/// Rune's reserved words: a GDScript name that is one gains a trailing `_`.
pub(crate) const RESERVED: &[&str] = &[
    "abstract", "alignof", "as", "async", "await", "become", "break", "const", "continue", "crate",
    "default", "do", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
    "is", "let", "loop", "macro", "match", "mod", "move", "mut", "not", "offsetof", "override",
    "priv", "proc", "pure", "ref", "return", "select", "self", "Self", "sizeof", "static",
    "struct", "super", "true", "typeof", "unsafe", "use", "virtual", "while", "yield",
];

/// What marks a line in converted output that the port must finish. It is
/// the port's marker, in the port's repository, not debt in this one.
pub(crate) const MARKER: &str = "PORT(gdscript):";

pub(crate) fn safe(name: &str) -> String {
    if RESERVED.contains(&name) {
        return format!("{name}_");
    }
    name.to_string()
}

// Four flags, each a different question about where the emitter is: they are
// read independently and never together.
#[allow(clippy::struct_excessive_bools)]
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
    /// False inside a hook the engine calls synchronously, where a wait
    /// cannot be emitted at all.
    pub allow_await: bool,
    /// Set inside a `static func`, which has no `this` to read a member from.
    pub in_static: bool,
    /// The function being emitted, as Rune names it, so a bare `super(..)`
    /// knows which base copy it means.
    pub enclosing: String,
    /// Lines the statement in progress needs before and after it: a call on a
    /// static reads it into a local first and writes the local back, because
    /// the store hands out a value rather than a place.
    before: Vec<String>,
    after: Vec<String>,
    /// Set while emitting the operand of an explicit `await`, so a call that
    /// is already awaited is not awaited twice.
    awaited_here: bool,
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
            allow_await: true,
            in_static: false,
            enclosing: String::new(),
            awaited_here: false,
            before: Vec::new(),
            after: Vec::new(),
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
        let outer_before = std::mem::take(&mut self.before);
        let outer_after = std::mem::take(&mut self.after);
        let body = self.statement_body(stmt, depth);
        let before = std::mem::replace(&mut self.before, outer_before);
        let after = std::mem::replace(&mut self.after, outer_after);
        let mut out = String::new();
        for line in before {
            let _ = writeln!(out, "{pad}{line}");
        }
        out.push_str(&body);
        for line in after {
            let _ = writeln!(out, "{pad}{line}");
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
                let _ = writeln!(out, "{pad}return {text};");
            }
            Stmt::Raw(text) => {
                self.note(format!("`{text}`: this line is not translated"));
                let _ = writeln!(out, "{pad}// {MARKER} {text}");
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
                let _ = writeln!(out, "{pad}{};", discardable(&text));
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
            let _ = writeln!(out, "{pad}let {name} = {text};");
            self.declare(&name);
            let bound = Expr::Name(name.clone());
            if let Some(write) = self.property_write(target, op, &bound, pad) {
                out.push_str(&write);
                return out;
            }
            let place = self.expression(target);
            let _ = writeln!(out, "{pad}{place} {op} {name};");
            return out;
        }
        if let Some(text) = self.property_write(target, op, value, pad) {
            return text;
        }
        let place = self.expression(target);
        let text = self.expression(value);
        // A target that resolved to nothing, or to another module's own item,
        // is not a place: the whole line becomes a stub.
        if place.starts_with("(gd.todo)") || place.starts_with("(script::require(") {
            let _ = writeln!(out, "{pad}{};", discardable(&place));
            return out;
        }
        let _ = writeln!(out, "{pad}{place} {op} {text};");
        out
    }

    /// A chain as somewhere to write, rather than as a value to read: a field
    /// on a local table is a field, not a call into the shim.
    fn place(&mut self, value: &Expr) -> String {
        match value {
            Expr::Field(object, field) => {
                let base = self.place(object);
                format!("{base}.{}", safe(field))
            }
            Expr::Index(object, index) => {
                let base = self.place(object);
                let index = self.expression(index);
                format!("{base}[{index}]")
            }
            other => self.expression(other),
        }
    }

    /// `node.x += 1` on a Godot property: read through the getter, change the
    /// value, write it back through the setter.
    fn compound_write(
        &mut self,
        object: &str,
        field: &str,
        op: &str,
        value: &Expr,
        pad: &str,
    ) -> Option<String> {
        let text = self.expression(value);
        let operator = op.trim_end_matches('=');
        let plain = object.starts_with("(script::require(") || object == "this";
        let getter = match map::property(object, field) {
            Some(getter) => getter,
            None if plain => return None,
            None => {
                self.uses_shim = true;
                format!("(gd.field)({object}, {})", quoted(field))
            }
        };
        let changed = format!("{getter} {operator} {text}");
        if let Some(write) = map::setter(object, field, &changed) {
            self.uses_shim |= write.contains(map::SHIM_MARK);
            return Some(format!("{pad}{};\n", discardable(&write)));
        }
        if plain {
            return None;
        }
        self.uses_shim = true;
        let write = format!("(gd.set_field)({object}, {}, {changed})", quoted(field));
        Some(format!("{pad}{};\n", discardable(&write)))
    }

    /// The static variable an index or field chain is rooted at, if any.
    fn static_root(&self, value: &Expr) -> Option<String> {
        match value {
            Expr::Name(name) => (!self.is_local(name)
                && self.context.static_vars.contains_key(name))
            .then(|| name.clone()),
            Expr::Index(object, _) | Expr::Field(object, _) => self.static_root(object),
            _ => None,
        }
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
        // A write *into* a static — `_cache[key] = v` — reads it out, changes
        // the copy and writes it back, because the store hands out a value.
        if let Some(root) = self.static_root(target)
            && !matches!(target, Expr::Name(_))
        {
            let fallback = self
                .context
                .static_vars
                .get(&root)
                .cloned()
                .unwrap_or_default();
            let key = quoted(&format!("{}:{root}", self.context.static_prefix));
            let name = self.temp();
            self.declare(&name);
            let place = self.place(&replace_root(target, &name));
            let text = self.expression(value);
            self.uses_shim = true;
            let mut out = String::new();
            let _ = writeln!(out, "{pad}let {name} = (gd.static_get)({key}, {fallback});");
            let _ = writeln!(out, "{pad}{place} {op} {text};");
            let _ = writeln!(out, "{pad}let _ = (gd.static_set)({key}, {name});");
            return Some(out);
        }
        if let Expr::Name(name) = target
            && !self.is_local(name)
            && self.context.static_vars.contains_key(name)
        {
            let key = quoted(&format!("{}:{name}", self.context.static_prefix));
            let text = if op == "=" {
                self.expression(value)
            } else {
                let read = self.expression(target);
                let other = self.expression(value);
                format!("{read} {} {other}", op.trim_end_matches('='))
            };
            self.uses_shim = true;
            return Some(format!("{pad}let _ = (gd.static_set)({key}, {text});\n"));
        }
        let (object, field) = match target {
            Expr::Name(name)
                if !self.is_local(name)
                    && !self.context.members.contains(name)
                    && !self.context.consts.contains(name) =>
            {
                if self.in_static {
                    self.uses_shim = true;
                    let stub = map::todo(name);
                    self.note(format!("`{name}`: a static function writes no member here"));
                    return Some(format!("{pad}{};\n", discardable(&stub)));
                }
                ("this.node".to_string(), name.clone())
            }
            Expr::Field(object, field) => {
                if matches!(**object, Expr::SelfRef)
                    && (self.context.members.contains(field) || self.context.consts.contains(field))
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
            return self.compound_write(&object, &field, op, value, pad);
        }
        let text = self.expression(value);
        if let Some(write) = map::setter(&object, &field, &text) {
            self.uses_shim |= write.contains(map::SHIM_MARK);
            return Some(format!("{pad}{};\n", discardable(&write)));
        }
        // Another script's property: written through the setter the converted
        // script carries.
        if object.starts_with("(script::require(") || object == "this" {
            return None;
        }
        self.uses_shim = true;
        let write = format!("(gd.set_field)({object}, {}, {text})", quoted(&field));
        Some(format!("{pad}{};\n", discardable(&write)))
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
                let was = self.awaited_here;
                self.awaited_here = true;
                let text = self.expression(inner);
                self.awaited_here = was;
                if !self.allow_await {
                    self.note(
                        "a wait inside a hook the engine calls synchronously; it is dropped"
                            .to_string(),
                    );
                    return text;
                }
                self.awaits = true;
                format!("{text}.await")
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
        if let Some(fallback) = self.context.static_vars.get(name).cloned() {
            self.uses_shim = true;
            let key = quoted(&format!("{}:{name}", self.context.static_prefix));
            return format!("(gd.static_get)({key}, {fallback})");
        }
        if self.context.lazy.contains(name) {
            return format!("{name}()");
        }
        if self.context.consts.contains(name) {
            return name.to_string();
        }
        if self.context.members.contains(name) {
            if self.in_static && !self.context.static_vars.contains_key(name) {
                self.note(format!(
                    "`{name}` is a member, and a static function has no instance to read it from"
                ));
                self.uses_shim = true;
                return map::todo(name);
            }
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
            return self.shimmed(text);
        }
        if let Some(text) = map::global_constant(name) {
            return text.to_string();
        }
        // A capitalised name with no class and no constant is a Godot type
        // used as a value, which nothing here carries.
        self.note(format!("`{name}`: no value of that name here"));
        self.uses_shim = true;
        map::todo(name)
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
            if self.context.lazy.contains(field) {
                return format!("{field}()");
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
        // A module's own item is a field; anything else may be a node whose
        // script holds the value, so the read goes through the shim.
        if text.starts_with("(script::require(") || text == "this" {
            return format!("{text}.{}", safe(field));
        }
        self.uses_shim = true;
        format!("(gd.field)({text}, {})", quoted(field))
    }

    /// A call the tables do not carry, as a stub that compiles and says so.
    fn unresolved(&mut self, what: &str) -> String {
        self.note(format!("`{what}`: no engine call of that name"));
        self.uses_shim = true;
        map::todo(what)
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> String {
        // `self.method(..)` and a bare `method(..)` are the same call here.
        let own = match callee {
            Expr::Name(name) if !self.is_local(name) => Some(name.clone()),
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => Some(name.clone()),
            _ => None,
        };
        // Godot's `set_process` is a per-node switch the engine does not have;
        // it becomes a flag the frame hook reads, declared by `script.rs`.
        if let Some(text) = self.process_verb(callee, args) {
            return text;
        }
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
        if let Some(text) = self.super_call(callee, &parts) {
            return text;
        }
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
            return self.unresolved(&format!("{class}.{method}()"));
        }
        if let Expr::Field(object, method) = callee
            && let Expr::Name(root) = &**object
            && !self.is_local(root)
            && let Some(fallback) = self.context.static_vars.get(root).cloned()
        {
            let key = quoted(&format!("{}:{root}", self.context.static_prefix));
            let name = self.temp();
            self.declare(&name);
            self.before
                .push(format!("let {name} = (gd.static_get)({key}, {fallback});"));
            self.after
                .push(format!("let _ = (gd.static_set)({key}, {name});"));
            self.uses_shim = true;
            if let Some(text) = map::method(&name, method, &parts) {
                return self.shimmed(text);
            }
            return format!("{name}.{}({})", safe(method), parts.join(", "));
        }
        if let Expr::Field(object, method) = callee {
            let receiver = self.expression(object);
            if let Some(text) = map::method(&receiver, method, &parts) {
                return self.shimmed(text);
            }
            // A required module's functions are its fields, and a field
            // holding a function is called in parentheses.
            if receiver.starts_with("script::require(") {
                return format!("({receiver}.{})({})", safe(method), parts.join(", "));
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
            if let Some(text) = map::implicit_self(name, &parts) {
                return self.shimmed(text);
            }
            return self.unresolved(&format!("{name}()"));
            // Godot resolved a bare call on `self` when the class had no
            // function of that name, and most of those are node verbs.
        }
        let head = self.expression(callee);
        // A closure in a local is called through parentheses; a bare name
        // there reads as a module item.
        if matches!(callee, Expr::Name(name) if self.is_local(name)) {
            return format!("({head})({})", parts.join(", "));
        }
        format!("{head}({})", parts.join(", "))
    }

    /// `set_process(false)` and its kin, called on this script itself.
    fn process_verb(&mut self, callee: &Expr, args: &[Expr]) -> Option<String> {
        let name = match callee {
            Expr::Name(name) if !self.is_local(name) => name,
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => name,
            _ => return None,
        };
        let flag = match name.as_str() {
            "set_process" | "is_processing" => PROCESS_FLAG,
            "set_physics_process" | "is_physics_processing" => PHYSICS_PROCESS_FLAG,
            _ => return None,
        };
        if !self.context.members.contains(flag) {
            return None;
        }
        if name.starts_with("is_") {
            return Some(format!("this.{flag}"));
        }
        let value = args
            .first()
            .map_or("true".to_string(), |a| self.expression(a));
        Some(format!("this.{flag} = {value}"))
    }

    /// `super(..)` and `super.name(..)`: the base's copy, which §4's
    /// flattening emitted under a suffixed name when this class overrode it.
    fn super_call(&mut self, callee: &Expr, parts: &[String]) -> Option<String> {
        let base = match callee {
            Expr::Name(name) if name == "super" => self.enclosing.clone(),
            Expr::Field(object, name) if matches!(**object, Expr::Name(ref n) if n == "super") => {
                self.context
                    .renames
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| safe(name))
            }
            _ => return None,
        };
        let suffixed = format!("{base}{BASE_SUFFIX}");
        let target = if self.context.bases.contains(&suffixed) {
            suffixed
        } else {
            self.note(format!("`super` reaches `{base}`, which no base declares"));
            base
        };
        let mut all = vec!["this".to_string()];
        all.extend(parts.iter().cloned());
        Some(format!("{target}({})", all.join(", ")))
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
        if self.awaited_here || !self.allow_await {
            return text;
        }
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
        let group = |value: &Expr, text: String| {
            let nested = matches!(value, Expr::Binary(inner, ..)
                if matches!(*inner, "==" | "!=" | "<" | "<=" | ">" | ">="));
            if comparison && nested {
                return format!("({text})");
            }
            text
        };
        let left_text = self.expression(left);
        let left_text = group(left, left_text);
        let right_text = self.expression(right);
        let right_text = group(right, right_text);
        format!("{left_text} {op} {right_text}")
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

/// The same chain with its root name swapped for a local, so a read-modify-
/// write touches the copy the store handed out.
fn replace_root(value: &Expr, name: &str) -> Expr {
    match value {
        Expr::Name(_) => Expr::Name(name.to_string()),
        Expr::Index(object, index) => Expr::Index(
            Box::new(replace_root(object, name)),
            Box::new((**index).clone()),
        ),
        Expr::Field(object, field) => {
            Expr::Field(Box::new(replace_root(object, name)), field.clone())
        }
        other => other.clone(),
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

/// A statement that would start with `(` or `[` is bound instead: a block
/// before it would otherwise read as a call or an index, which is Rune's
/// parse, not a runtime error.
pub(crate) fn discardable(text: &str) -> String {
    if text.starts_with('(') || text.starts_with('[') {
        return format!("let _ = {text}");
    }
    text.to_string()
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
