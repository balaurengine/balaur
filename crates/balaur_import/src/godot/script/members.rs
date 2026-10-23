//! A class's members: the fields that keep them, the defaults set as the
//! instance is made, and the `get`/`set` blocks a property declares.

use std::fmt::Write as _;

use super::render::Field;
use super::{
    Context, INPUT_FLAG, PHYSICS_PROCESS_FLAG, PROCESS_FLAG, assigned, expression, name_of, safe,
    top_level,
};
use crate::godot::exports::{Export, Kind};
use crate::godot::gdscript::{Bt, typed_zero};

/// What a class's members need: where each is kept and what sets it.
#[derive(Default)]
pub(crate) struct Plan {
    /// The struct's props: the exports a scene sets as a number or a flag.
    pub props: Vec<Field>,
    /// The members' class's fields, a literal default where one was declared.
    pub fields: Vec<Field>,
    /// Every member's declared value a field's default could not hold, in
    /// declaration order, a base's first: set before `_init` runs.
    pub defaults: Vec<String>,
    /// What the scene set, after `_init`: each prop copied in, and each
    /// export the scene filed in the node's `meta` read back.
    pub exports: Vec<String>,
    /// Every `@onready`, once the scene is in.
    pub ready: Vec<String>,
}

/// Whether an export is a struct prop: a number or a flag, which the scene
/// sets by name and the struct can hold without a string or a list.
pub(crate) fn is_prop(export: &Export) -> bool {
    matches!(export.kind, Some(Kind::Int | Kind::Float | Kind::Bool)) && !export.data
}

/// Each member's type, by its hint, or by the literal a `:=` infers it from.
pub(super) fn types(source: &str) -> Vec<(String, Bt)> {
    let mut out = Vec::new();
    for line in top_level(source) {
        let body = super::declaration_start(&line).unwrap_or(&line);
        let Some(rest) = body.strip_prefix("var ") else {
            continue;
        };
        let rest = declared(rest);
        let name = name_of(rest);
        out.push((name.clone(), member_type(rest, &name)));
    }
    out
}

// A declaration's type: its hint, else its `:=` literal's, else `any`.
fn member_type(rest: &str, name: &str) -> Bt {
    let after = rest[name.len()..].trim_start();
    if let Some(typed) = after.strip_prefix(":=") {
        return super::scalar_literal(typed).map_or(Bt::Any, |(kind, _)| kind);
    }
    let hint = after
        .strip_prefix(':')
        .map_or("", |t| t.split('=').next().unwrap_or_default().trim());
    Bt::of_hint(hint)
}

/// Where the members go and what sets them. A node class keeps them as the
/// fields of its members' class; an object class, as its map's keys.
pub(super) fn plan(
    levels: &[String],
    context: &Context,
    exports: &[Export],
    notes: &mut Vec<String>,
) -> Plan {
    let mut plan = Plan::default();
    let object = context.object_class;
    let store = |name: &str, value: &str| {
        if object {
            format!("this.set({}, {value});", crate::godot::gdscript::quoted(name))
        } else {
            format!("self.{}.{} = {value};", crate::godot::gdscript::MY, safe(name))
        }
    };
    let mut seen = std::collections::BTreeSet::new();
    for flag in [PROCESS_FLAG, PHYSICS_PROCESS_FLAG, INPUT_FLAG] {
        if context.members.contains(flag) {
            seen.insert(flag.to_string());
            plan.fields.push(field(flag, Bt::Bool, Some("true".into())));
            if object {
                plan.defaults.push(store(flag, "true"));
            }
        }
    }
    // A derived class's declaration of a name is the one kept.
    let mut lines: Vec<String> = Vec::new();
    for level in levels {
        for line in top_level(level) {
            lines.push(line);
        }
    }
    let mut declared_names = Vec::new();
    for line in &lines {
        let body = super::declaration_start(line).unwrap_or(line);
        if line.starts_with("static ") || body.starts_with("static ") {
            continue;
        }
        let Some(rest) = body.strip_prefix("var ") else {
            continue;
        };
        let name = name_of(declared(rest));
        if seen.insert(name.clone()) {
            declared_names.push((name, line.clone()));
        }
    }
    // A base's members first, as Godot sets them.
    declared_names.reverse();
    for (name, line) in &declared_names {
        let body = super::declaration_start(line).unwrap_or(line);
        let rest = declared(body.strip_prefix("var ").unwrap_or_default());
        let kind = if object { Bt::Any } else { member_type(rest, name) };
        let hint = rest[name.len()..]
            .trim_start()
            .strip_prefix(':')
            .map_or("", |t| t.split('=').next().unwrap_or_default().trim());
        let export = exports.iter().find(|e| &e.name == name);
        let onready = line.starts_with("@onready");
        let value = assigned(rest);
        // A literal the field itself starts at, where its type takes one.
        let literal = value
            .and_then(super::scalar_literal)
            .and_then(|(lit, text)| fits(kind, lit, &text));
        let mut f = field(name, kind, None);
        if !object && !onready && let Some(text) = &literal {
            f.default = Some(text.clone());
        }
        plan.fields.push(f);
        if let Some(export) = export.filter(|e| is_prop(e)) {
            plan.props.push(Field {
                name: safe(name),
                ty: kind.text().to_string(),
                default: prop_default(export, kind),
                public: true,
            });
            plan.exports.push(store(name, &format!("self.{}", safe(name))));
        }
        let zero = typed_zero(hint);
        let text = match value {
            Some(value) if literal.is_none() || object => match expression(value, context, false) {
                Some((text, _)) => Some(text),
                None => {
                    notes.push(format!("`{name}`: its default did not translate"));
                    None
                }
            },
            Some(_) => None,
            // No value: Godot's zero for the type, which a field's own zero is
            // unless the type is a container or a vector.
            None if zero.contains("gd::") || zero == "map()" || object => Some(zero.to_string()),
            None => None,
        };
        if let Some(text) = text {
            let line = store(name, &text);
            if onready {
                plan.ready.push(line);
            } else {
                plan.defaults.push(line);
            }
        }
        if let Some(export) = export.filter(|e| !is_prop(e) && !object) {
            if let Some(read) = export_read(export, &store) {
                plan.exports.push(read);
            }
        }
    }
    plan
}

fn field(name: &str, kind: Bt, default: Option<String>) -> Field {
    Field {
        name: safe(name),
        ty: kind.text().to_string(),
        default,
        public: false,
    }
}

// A literal as a field of `kind` takes it: an int widens to a float.
fn fits(kind: Bt, literal: Bt, text: &str) -> Option<String> {
    match (kind, literal) {
        (Bt::Float, Bt::Int) => Some(format!("{text}.0")),
        (a, b) if a == b && a != Bt::Any && a != Bt::Node => Some(text.to_string()),
        _ => None,
    }
}

// A prop's default: the export's literal, fitted to the field.
fn prop_default(export: &Export, kind: Bt) -> Option<String> {
    let (lit, text) = super::scalar_literal(&export.default)?;
    fits(kind, lit, &text)
}

// An export the scene filed in the node's `meta`, read back over its
// default: a node by its path, a list of nodes by theirs.
fn export_read(export: &Export, store: &dyn Fn(&str, &str) -> String) -> Option<String> {
    let name = &export.name;
    let quoted = crate::godot::gdscript::quoted(name);
    let field = format!("self.{}.{}", crate::godot::gdscript::MY, safe(name));
    let read = match export.kind {
        Some(Kind::Node) => format!("gd::export_node(self.node, {quoted}, {field})"),
        Some(Kind::Nodes) => format!("gd::export_nodes(self.node, {quoted}, {field})"),
        _ => format!("gd::export_value(self.node, {quoted}, {field})"),
    };
    Some(store(name, &read))
}

/// A class's properties with accessors: `var hp: int:` then `get:` or
/// `set(v):` blocks, or `: get = f, set = g` after the value. Each becomes a
/// function, `__get_hp` or `__set_hp`, which every read or write of `hp`
/// outside it calls; inside it, `hp` is the stored value, as in Godot.
#[derive(Default)]
pub(super) struct Accessors {
    pub getters: std::collections::BTreeSet<String>,
    pub setters: std::collections::BTreeSet<String>,
    /// The function `get = f` or `set = f` names, to the property it keeps:
    /// inside it, Godot reads and writes the property's own storage.
    pub named: std::collections::BTreeMap<String, String>,
    /// The accessors as GDScript functions, for the translator to take.
    pub text: String,
    /// `static func` for a `static var`'s accessors, `func` otherwise.
    keyword: &'static str,
}

pub(super) use crate::godot::gdscript::{GETTER, SETTER};

pub(super) fn accessors(source: &str) -> Accessors {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Accessors::default();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let code = line.split('#').next().unwrap_or_default().trim_end();
        let Some(at) = code.find("var ") else {
            continue;
        };
        let is_static = code.starts_with("static var ");
        if !(code.starts_with("var ") || code.starts_with('@') || is_static) {
            continue;
        }
        out.keyword = if is_static { "static func" } else { "func" };
        let rest = &code[at + 4..];
        let name = name_of(rest);
        if let Some(cut) = accessor_suffix(rest) {
            for part in rest[cut..].trim_start().trim_start_matches(':').split(',') {
                if let Some((kind, callee)) = part.split_once('=') {
                    out.named(kind.trim(), &name, callee.trim());
                }
            }
            continue;
        }
        if !code.ends_with(':') {
            continue;
        }
        let mut block = Vec::new();
        while i < lines.len() && (lines[i].trim().is_empty() || lines[i].starts_with([' ', '\t'])) {
            block.push(lines[i]);
            i += 1;
        }
        out.block(&name, &block);
    }
    out
}

impl Accessors {
    /// `get = f` or `set = f`: the accessor calls the named function.
    fn named(&mut self, kind: &str, name: &str, callee: &str) {
        match kind {
            "get" => {
                let _ = write!(
                    self.text,
                    "{} {GETTER}{name}():\n\treturn {callee}()\n",
                    self.keyword
                );
                self.getters.insert(name.to_string());
            }
            "set" => {
                let _ = write!(
                    self.text,
                    "{} {SETTER}{name}(value):\n\t{callee}(value)\n",
                    self.keyword
                );
                self.setters.insert(name.to_string());
            }
            _ => return,
        }
        self.named.insert(callee.to_string(), name.to_string());
    }

    /// The indented lines under `var name:`: `get:` and `set(v):` with their
    /// bodies, or the one-line `get = f` forms.
    fn block(&mut self, name: &str, block: &[&str]) {
        let width = |l: &str| l.len() - l.trim_start().len();
        let Some(unit) = block
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| width(l))
            .min()
        else {
            return;
        };
        let mut j = 0;
        while j < block.len() {
            let head = block[j].trim();
            let at_unit = !head.is_empty() && width(block[j]) == unit;
            j += 1;
            if !at_unit {
                continue;
            }
            let mut body: Vec<&str> = Vec::new();
            while j < block.len() && (block[j].trim().is_empty() || width(block[j]) > unit) {
                body.push(block[j]);
                j += 1;
            }
            let body = body.join("\n");
            let head = head.split('#').next().unwrap_or_default().trim();
            if head == "get:" || head == "get():" {
                let _ = write!(self.text, "{} {GETTER}{name}():\n{body}\n", self.keyword);
                self.getters.insert(name.to_string());
            } else if let Some(param) = head.strip_prefix("set(").and_then(|p| p.strip_suffix("):"))
            {
                let param = name_of(param);
                let _ = write!(
                    self.text,
                    "{} {SETTER}{name}({param}):\n{body}\n",
                    self.keyword
                );
                self.setters.insert(name.to_string());
            } else {
                for part in head.split(',') {
                    if let Some((kind, callee)) = part.split_once('=') {
                        self.named(kind.trim(), name, callee.trim());
                    }
                }
            }
        }
    }
}

/// Where `: get = f` or `: set = f` starts after a declaration's value.
pub(super) fn accessor_suffix(rest: &str) -> Option<usize> {
    let mut depth = 0i32;
    let bytes = rest.as_bytes();
    for (at, c) in rest.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 && bytes.get(at + 1) != Some(&b'=') => {
                let after = rest[at + 1..].trim_start();
                let word = after
                    .split(|c: char| !c.is_alphanumeric())
                    .next()
                    .unwrap_or_default();
                if matches!(word, "get" | "set")
                    && after[word.len()..].trim_start().starts_with('=')
                {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// A declaration without what only a property carries: the trailing `:`
/// that opens its accessor block, or its `: get = f` suffix.
pub(super) fn declared(rest: &str) -> &str {
    let rest = match accessor_suffix(rest) {
        Some(cut) => &rest[..cut],
        None => rest,
    };
    rest.trim_end().strip_suffix(':').unwrap_or(rest).trim_end()
}
