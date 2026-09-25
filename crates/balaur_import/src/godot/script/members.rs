//! A class's members: the defaults `init` sets, and the `get`/`set` blocks
//! a property declares.

use std::fmt::Write as _;

use super::{
    Context, PHYSICS_PROCESS_FLAG, PROCESS_FLAG, assigned, gdscript, name_of, safe, shim_binding,
    top_level,
};

/// The members a Godot class declared with a value: the plain ones the
/// engine sets when the instance is made, and the ones that wait for the
/// scene — an exported vector's shape, and every `@onready`.
pub(super) fn write_members(
    out: &mut String,
    levels: &[String],
    context: &Context,
    exports: &[crate::godot::exports::Export],
    notes: &mut Vec<String>,
) -> Members {
    let mut assignments: Vec<String> = Vec::new();
    // What the scene has to land first: an export's shape, and `@onready`.
    let mut scened: Vec<String> = Vec::new();
    // First, so a default that fails still leaves the hooks running.
    for flag in [PROCESS_FLAG, PHYSICS_PROCESS_FLAG] {
        if context.members.contains(flag) {
            assignments.push(format!("    this.{flag} = true;"));
        }
    }
    // An exported vector or colour arrives as a list; the body reads `.x`.
    for export in exports {
        use crate::godot::exports::Kind;
        let field = safe(&export.name);
        let shape = match export.kind {
            Some(Kind::Vec2 | Kind::Vec3) => "vec_of",
            Some(Kind::Color) => "color_of",
            Some(_) => continue,
            // A kind the scene cannot set starts at its declared value.
            None => {
                let text = export
                    .value
                    .as_deref()
                    .and_then(|value| member_default(value, context))
                    .unwrap_or_else(|| gdscript::typed_zero(&export.hint).to_string());
                assignments.push(format!("    this.{field} = {text};"));
                continue;
            }
        };
        scened.push(format!("    this.{field} = (gd.{shape})(this.{field});"));
    }
    // Godot sets an `@onready` member once the scene is in: after the rest,
    // from the `init` hook, where this node's children are already built.
    let mut ready: Vec<String> = Vec::new();
    // A base's members first, so a derived class's own default is the last.
    for line in levels.iter().rev().flat_map(|level| top_level(level)) {
        let trimmed = line.as_str();
        // An exported member is set from the scene.
        if trimmed.starts_with("@export") {
            continue;
        }
        let onready = trimmed.starts_with("@onready");
        let body = trimmed.trim_start_matches("@onready").trim_start();
        let Some(rest) = body.strip_prefix("var ") else {
            continue;
        };
        let rest = declared(rest);
        let name = name_of(rest);
        // Godot's `var x: int` is 0 before it is written, and an untyped
        // `var x` is null, which a read answers rather than failing.
        let hint = rest[name.len()..]
            .trim_start()
            .strip_prefix(':')
            .map_or("", |t| t.split('=').next().unwrap_or_default().trim());
        assignments.push(format!(
            "    this.{} = {};",
            safe(&name),
            gdscript::typed_zero(hint)
        ));
        let Some(value) = assigned(rest) else {
            continue;
        };
        let Some(text) = member_default(value, context) else {
            notes.push(format!("`{name}`: its default did not translate"));
            continue;
        };
        let line = format!("    this.{} = {text};", safe(&name));
        if onready {
            ready.push(line);
        } else {
            assignments.push(line);
        }
    }
    scened.extend(ready);
    let held = Members {
        defaults: !assignments.is_empty(),
        scened: !scened.is_empty(),
    };
    write_block(
        out,
        &assignments,
        "pub fn defaults",
        "The defaults the class declared with its members, which the engine\n/// applies when the instance is made.",
    );
    write_block(
        out,
        &scened,
        "fn scene_defaults",
        "The members that wait for the scene: an exported vector arrives as a\n/// list, and `@onready` is Godot's \"once the tree is in\".",
    );
    held
}

/// Which halves of a class's members were written.
pub(crate) struct Members {
    pub defaults: bool,
    pub scened: bool,
}

fn write_block(out: &mut String, lines: &[String], signature: &str, doc: &str) {
    if lines.is_empty() {
        return;
    }
    let binding = if lines.iter().any(|line| line.contains("(gd.")) {
        shim_binding(1)
    } else {
        String::new()
    };
    let _ = write!(
        out,
        "\n/// {doc}\n{signature}(this) {{\n{binding}{}\n}}\n",
        lines.join("\n")
    );
}

/// A member's declared value as Rune, read inside `defaults(this)`.
fn member_default(value: &str, context: &Context) -> Option<String> {
    let body = gdscript::body(
        &[format!("var _x = {value}")],
        context,
        0,
        &[],
        false,
        false,
        "",
    );
    body.rune
        .trim()
        .strip_prefix("let _x = ")
        .and_then(|t| t.strip_suffix(';'))
        .map(str::to_string)
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
