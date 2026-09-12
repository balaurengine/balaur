//! A GDScript file as Rune.
//!
//! What the engine calls carries first: `_ready` is `init`, `_process` is
//! `update`, every other function keeps its name so the scene's handlers still
//! reach it, and every `@export` becomes an entry of `exports()`. The bodies
//! go through [`crate::godot::gdscript`], which needs to know what the class
//! declares — its members, its methods, its constants — so this module
//! collects that first and hands it over as a `Context`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::godot::exports::{Classes, split_top};
use crate::godot::gdscript::{self, Context, RESERVED, safe};

/// A skeleton, and what the port will have to deal with.
pub(crate) struct Converted {
    pub rune: String,
    pub notes: Vec<String>,
}

/// GDScript's lifecycle, as the hook the engine calls here, with its
/// parameters.
const HOOKS: &[(&str, &str, &str)] = &[
    ("_ready", "init", "this"),
    ("_process", "update", "this, dt"),
    ("_physics_process", "fixed_update", "this, dt"),
    ("_exit_tree", "on_free", "this"),
];

/// A hook the engine never calls asynchronously: an `await` inside one is
/// reported rather than emitted.
const SYNCHRONOUS: &[&str] = &["update", "fixed_update", "on_free"];

/// One `func` and the lines under it.
struct Function {
    name: String,
    params: Vec<String>,
    is_static: bool,
    /// The signature, then every line of the body.
    lines: Vec<String>,
}

impl Function {
    fn body(&self) -> &[String] {
        self.lines.get(1..).unwrap_or_default()
    }
}

pub(crate) fn convert(source: &str, path: &str, classes: &Classes) -> Converted {
    let mut notes = Vec::new();
    let mut header: Vec<String> = Vec::new();
    let mut functions: Vec<Function> = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let top = !line.starts_with([' ', '\t']) && !line.trim().is_empty();
        if top && (line.starts_with("func ") || line.starts_with("static func ")) {
            // A signature may run over lines until its closing `):`.
            let mut signature = line.to_string();
            while !balanced(&signature) && i + 1 < lines.len() {
                i += 1;
                signature.push(' ');
                signature.push_str(lines[i].trim());
            }
            let mut function = parse_signature(&signature, line.starts_with("static"));
            function.lines.push(signature);
            while i + 1 < lines.len() {
                let next = lines[i + 1];
                let continues = next.trim().is_empty()
                    || next.starts_with([' ', '\t'])
                    || next.starts_with('#');
                if !continues {
                    break;
                }
                i += 1;
                function.lines.push(next.to_string());
            }
            functions.push(function);
        } else {
            header.push(line.to_string());
        }
        i += 1;
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Converted from {path} by `balaur import`. Its hooks, exports and\n\
         // bodies are Rune; a line the importer could not read is marked."
    );
    let context = context(source, classes, &functions, &mut notes);
    let documentation: Vec<String> = header
        .iter()
        .filter(|line| line.trim_start().starts_with("##"))
        .cloned()
        .collect();
    if !documentation.is_empty() {
        out.push('\n');
        for line in &documentation {
            push_comment(&mut out, line, "");
        }
    }
    let exports = crate::godot::exports::exports(source, classes);
    let entries: Vec<String> = exports
        .iter()
        .filter_map(|e| Some(format!("\"{}\": {}", e.name, e.entry()?)))
        .collect();
    for export in exports.iter().filter(|e| e.kind.is_none()) {
        notes.push(format!(
            "export `{}` is a {}, which an `exports()` entry cannot hold; set it in `init`",
            export.name, export.hint
        ));
    }
    if !entries.is_empty() {
        let _ = write!(
            out,
            "\npub fn exports() {{\n    #{{ {} }}\n}}\n",
            entries.join(", ")
        );
    }
    write_enums(&mut out, source);
    write_constants(&mut out, source, &context, &mut notes);
    write_members(&mut out, source, &context, &mut notes);
    write_functions(&mut out, &functions, &context, &mut notes);
    if source.contains("_input(") || source.contains("_unhandled_input(") {
        notes
            .push("an `_input` handler: read the `input` module from `update` instead".to_string());
    }
    Converted { rune: out, notes }
}

/// Everything a bare name in a body needs to resolve against, this file's and
/// its bases'.
fn context(
    source: &str,
    classes: &Classes,
    functions: &[Function],
    notes: &mut Vec<String>,
) -> Context {
    let mut context = Context {
        classes: classes
            .files
            .iter()
            .map(|(name, file)| (name.clone(), file.replace(".gd", ".rn")))
            .collect(),
        ..Context::default()
    };
    for text in chain(source, classes) {
        let level = declarations(&text);
        context.members.extend(level.members);
        context.consts.extend(level.consts);
        context.signals.extend(level.signals);
        context.methods.extend(level.methods);
        context.statics.extend(level.statics);
    }
    for function in functions {
        let hook = HOOKS.iter().find(|(godot, _, _)| *godot == function.name);
        let name = match hook {
            Some((_, here, _)) => (*here).to_string(),
            None if RESERVED.contains(&function.name.as_str()) => format!("{}_", function.name),
            None => function.name.clone(),
        };
        if name != function.name {
            context.renames.insert(function.name.clone(), name);
        }
    }
    close_asyncs(&mut context, functions, notes);
    context
}

/// Which functions are async: the ones that await, then everything that calls
/// one, to a fixed point.
fn close_asyncs(context: &mut Context, functions: &[Function], notes: &mut Vec<String>) {
    let names: BTreeSet<String> = functions.iter().map(|f| f.name.clone()).collect();
    let calls: BTreeMap<String, BTreeSet<String>> = functions
        .iter()
        .map(|f| (f.name.clone(), gdscript::called(f.body(), &names)))
        .collect();
    for function in functions {
        if gdscript::awaits(function.body()) {
            context.asyncs.insert(function.name.clone());
        }
    }
    // A caller of an async function is async too; the chain is short, and a
    // pass that changes nothing ends it.
    for _ in 0..names.len().max(1) {
        let mut grew = false;
        for (name, called) in &calls {
            if context.asyncs.contains(name) {
                continue;
            }
            if called.iter().any(|target| context.asyncs.contains(target)) {
                context.asyncs.insert(name.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    for function in functions {
        let hook = HOOKS
            .iter()
            .find(|(godot, _, _)| *godot == function.name)
            .map(|(_, here, _)| *here);
        if let Some(here) = hook
            && SYNCHRONOUS.contains(&here)
            && context.asyncs.contains(&function.name)
        {
            notes.push(format!(
                "`{}` awaits, and `{here}` cannot: move the wait to an event handler",
                function.name
            ));
        }
    }
}

/// A file's source, then each of its bases', nearest first.
fn chain(source: &str, classes: &Classes) -> Vec<String> {
    let mut out = vec![source.to_string()];
    let mut base = extended(source, classes);
    for _ in 0..16 {
        let Some(file) = base else { break };
        let Ok(text) = std::fs::read_to_string(classes.root.join(&file)) else {
            break;
        };
        base = extended(&text, classes);
        out.push(text);
    }
    out
}

fn extended(source: &str, classes: &Classes) -> Option<String> {
    let line = source.lines().find(|l| l.starts_with("extends "))?;
    let target = line["extends ".len()..].trim();
    if let Some(path) = target.strip_prefix('"').and_then(|t| t.split('"').next()) {
        return Some(path.strip_prefix("res://").unwrap_or(path).to_string());
    }
    let name: String = target
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    classes.files.get(&name).cloned()
}

#[derive(Default)]
struct Declarations {
    members: BTreeSet<String>,
    consts: BTreeSet<String>,
    signals: BTreeSet<String>,
    methods: BTreeSet<String>,
    statics: BTreeSet<String>,
}

/// What one file declares at its top level.
fn declarations(source: &str) -> Declarations {
    let mut out = Declarations::default();
    for line in source.lines() {
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let trimmed = line.trim_start_matches('@');
        let body = match trimmed.find(' ') {
            // An annotated declaration: `@export var speed := 2.0`.
            Some(_) if trimmed.starts_with("export") || trimmed.starts_with("onready") => {
                match trimmed.split_once(' ') {
                    Some((_, rest)) => rest,
                    None => continue,
                }
            }
            _ => line,
        };
        let body = body.trim_start();
        if let Some(rest) = body.strip_prefix("var ") {
            out.members.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("const ") {
            out.consts.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("signal ") {
            out.signals.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("static func ") {
            out.statics.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("func ") {
            out.methods.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("enum ") {
            let name = name_of(rest);
            if name.is_empty() {
                // An unnamed enum's members are constants of their own.
                for (member, _) in enum_members(rest.split('{').nth(1).unwrap_or_default()) {
                    out.consts.insert(member);
                }
            } else {
                out.consts.insert(name);
            }
        }
    }
    out
}

/// The identifier a declaration opens with.
fn name_of(rest: &str) -> String {
    rest.trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// A GDScript `enum` as a module constant: named, an object whose fields are
/// its members, so `StageState.IDLE` reads unchanged; unnamed, one constant
/// per member.
fn write_enums(out: &mut String, source: &str) {
    let text = source.replace('\t', " ");
    let mut rest = text.as_str();
    while let Some(at) = rest.find("enum ") {
        // Only a declaration at column 0 is the class's own.
        let starts_line = rest[..at].ends_with('\n') || at == 0;
        rest = &rest[at + 5..];
        if !starts_line {
            continue;
        }
        let Some(open) = rest.find('{') else { continue };
        let name = rest[..open].trim().to_string();
        let Some(close) = rest.find('}') else { continue };
        let members = enum_members(&rest[open + 1..close]);
        rest = &rest[close + 1..];
        if members.is_empty() {
            continue;
        }
        let entries: Vec<String> = members
            .iter()
            .map(|(name, value)| format!("\"{name}\": {value}"))
            .collect();
        out.push('\n');
        if name.is_empty() {
            for (member, value) in &members {
                let _ = writeln!(out, "pub const {member} = {value};");
            }
            continue;
        }
        let _ = writeln!(out, "pub const {name} = #{{ {} }};", entries.join(", "));
    }
}

/// An enum's members and their values, numbered from zero where Godot left
/// them implicit.
fn enum_members(body: &str) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    let mut next = 0;
    for entry in body.split(',') {
        let entry = entry.split('#').next().unwrap_or_default().trim();
        if entry.is_empty() {
            continue;
        }
        let (name, value) = match entry.split_once('=') {
            Some((name, value)) => match value.trim().parse::<i64>() {
                Ok(value) => (name.trim(), value),
                Err(_) => continue,
            },
            None => (entry, next),
        };
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        next = value + 1;
        out.push((name.to_string(), value));
    }
    out
}

/// A class constant is a module constant here, so a body reads it bare.
fn write_constants(out: &mut String, source: &str, context: &Context, notes: &mut Vec<String>) {
    let mut wrote = false;
    for line in source.lines() {
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some(rest) = line.trim_start().strip_prefix("const ") else {
            continue;
        };
        let name = name_of(rest);
        let Some((_, value)) = rest.split_once('=') else {
            continue;
        };
        let value = value.trim();
        // A constant whose value is itself an expression over other names is
        // left to the port: a module constant here is a literal.
        let Some(literal) = literal(value, context) else {
            notes.push(format!(
                "`const {name}` is not a literal; declare it in `init` instead"
            ));
            continue;
        };
        if !wrote {
            out.push('\n');
            wrote = true;
        }
        let _ = writeln!(out, "pub const {name} = {literal};");
    }
}

/// A GDScript literal as a Rune one, or `None` where it is an expression.
fn literal(value: &str, context: &Context) -> Option<String> {
    let mut body = gdscript::body(&[format!("var _x = {value}")], context, 0);
    if !body.notes.is_empty() {
        return None;
    }
    let text = body.rune.trim().strip_prefix("let _x = ")?.to_string();
    body.rune.clear();
    let text = text.strip_suffix(';')?.to_string();
    // Only self-contained values become constants: a name in here would be
    // resolved at load, before the engine has a node.
    if text.contains("(gd.") {
        return None;
    }
    let plain = text.chars().all(|c| {
        c.is_alphanumeric()
            || " \t\"'.,:_-+#{}[]()".contains(c)
            || c == '\\'
    });
    plain.then_some(text)
}

/// The members a Godot class declared with a value, set on `this` at `init`.
fn write_members(out: &mut String, source: &str, context: &Context, notes: &mut Vec<String>) {
    let mut assignments: Vec<String> = Vec::new();
    for line in source.lines() {
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let trimmed = line.trim_start();
        // An exported member is set from the scene, and an `@onready` one is
        // a node lookup the port has to place.
        if trimmed.starts_with("@export") {
            continue;
        }
        let onready = trimmed.starts_with("@onready");
        let body = trimmed.trim_start_matches("@onready").trim_start();
        let Some(rest) = body.strip_prefix("var ") else {
            continue;
        };
        let name = name_of(rest);
        let Some((_, value)) = rest.split_once('=') else {
            continue;
        };
        if onready {
            notes.push(format!(
                "`@onready var {name}` reads the scene at load; set it in `init`"
            ));
            continue;
        }
        let value = value.trim_start_matches('=').trim();
        let body = gdscript::body(&[format!("var _x = {value}")], context, 0);
        let Some(text) = body
            .rune
            .trim()
            .strip_prefix("let _x = ")
            .and_then(|t| t.strip_suffix(';'))
        else {
            continue;
        };
        assignments.push(format!("    this.{} = {text};", safe(&name)));
    }
    if assignments.is_empty() {
        return;
    }
    let binding = assignments
        .iter()
        .any(|line| line.contains("(gd."))
        .then(|| shim_binding(1))
        .unwrap_or_default();
    let _ = write!(
        out,
        "\n/// The defaults the class declared with its members.\nfn defaults(this) {{\n{binding}{}\n}}\n",
        assignments.join("\n")
    );
}

/// Each function as a Rune one: a hook renamed, a keyword name suffixed, a
/// name declared twice kept once, and its body translated.
fn write_functions(
    out: &mut String,
    functions: &[Function],
    context: &Context,
    notes: &mut Vec<String>,
) {
    let mut seen: Vec<String> = Vec::new();
    for function in functions {
        let hook = HOOKS.iter().find(|(godot, _, _)| *godot == function.name);
        let (name, params) = if let Some((_, here, params)) = hook {
            ((*here).to_string(), (*params).to_string())
        } else {
            let mut name = function.name.clone();
            if RESERVED.contains(&name.as_str()) {
                notes.push(format!(
                    "`{name}` is a Rune keyword; the function is `{name}_` here and its callers need the new name"
                ));
                name.push('_');
            }
            let mut params: Vec<String> = function.params.iter().map(|p| safe(p)).collect();
            if !function.is_static {
                params.insert(0, "this".to_string());
            }
            (name, params.join(", "))
        };
        if seen.contains(&name) {
            notes.push(format!(
                "`{name}` is declared twice; the second is kept as a comment only"
            ));
            out.push('\n');
            for line in &function.lines {
                push_comment(out, line, "");
            }
            continue;
        }
        seen.push(name.clone());
        let mut body = gdscript::body(function.body(), context, 1);
        notes.extend(body.notes.drain(..).map(|note| {
            format!("`{}`: {note}", function.name)
        }));
        if body.uses_shim {
            body.rune.insert_str(0, &shim_binding(1));
        }
        // A hook the engine calls synchronously cannot be async; the note is
        // already written, and the body keeps its `.await` for the port.
        let synchronous = SYNCHRONOUS.contains(&name.as_str());
        let asynchronous = context.asyncs.contains(&function.name) && !synchronous;
        let word = if asynchronous { "pub async fn" } else { "pub fn" };
        let _ = write!(out, "\n{word} {name}({params}) {{\n");
        out.push_str(&body.rune);
        out.push_str("}\n");
    }
}

/// The line that binds the Variant shim, for a body that calls it.
fn shim_binding(depth: usize) -> String {
    format!(
        "{}let gd = script::require(\"{}\");\n",
        "    ".repeat(depth),
        gdscript::SHIM_PATH
    )
}

/// `func name(a: int, b := 2) -> T:` as its name and parameter names.
fn parse_signature(signature: &str, is_static: bool) -> Function {
    let rest = signature
        .trim_start_matches("static ")
        .trim_start_matches("func ")
        .trim_start();
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let params = rest
        .split_once('(')
        .map(|(_, tail)| tail)
        .and_then(|tail| tail.rsplit_once(')'))
        .map(|(inside, _)| inside.to_string())
        .unwrap_or_default();
    let params = split_top(&params)
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            p.trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|p| !p.is_empty())
        .collect();
    Function {
        name,
        params,
        is_static,
        lines: Vec::new(),
    }
}

/// Whether a signature's brackets are closed, so one running over lines is
/// read whole.
fn balanced(text: &str) -> bool {
    let opens = text.matches('(').count();
    let closes = text.matches(')').count();
    opens > 0 && opens == closes
}

fn push_comment(out: &mut String, line: &str, indent: &str) {
    if line.trim().is_empty() {
        let _ = writeln!(out, "{indent}//");
        return;
    }
    let _ = writeln!(out, "{indent}// {}", line.replace('\t', "    "));
}

#[cfg(test)]
mod tests {
    use super::convert;
    use crate::godot::exports::Classes;

    const SHIP: &str = "extends Node\n\
class_name Ship\n\
\n\
signal sunk(depth)\n\
@export var speed := 2.0\n\
@export var label: String\n\
@export_range(0, 10) var crew: int = 4\n\
var hidden := 1\n\
const LIMIT = 9\n\
\n\
func _ready() -> void:\n\
\tprint(\"ahoy\")\n\
\n\
func _process(delta: float) -> void:\n\
\tposition.x += speed * delta\n\
\n\
func _on_go_pressed(\n\
\t\tforce: float,\n\
\t\tloud := true) -> void:\n\
\tsunk.emit(3)\n\
\n\
static func knots(v):\n\
\treturn v * 1.94\n\
\n\
func match(a):\n\
\tpass\n";

    #[test]
    fn hooks_are_renamed_and_every_other_function_keeps_its_name() {
        let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
        assert!(out.rune.contains("pub fn init(this) {"), "{}", out.rune);
        assert!(out.rune.contains("pub fn update(this, dt) {"));
        assert!(
            out.rune
                .contains("pub fn _on_go_pressed(this, force, loud) {"),
            "a signature over three lines, and the handler name a scene points at: {}",
            out.rune
        );
        assert!(
            out.rune.contains("pub fn knots(v) {"),
            "a static function takes no `this`"
        );
        assert!(out.rune.contains("pub fn match_(this, a) {"));
        assert!(out.notes.iter().any(|n| n.contains("`match`")));
    }

    #[test]
    fn exports_carry_their_defaults_and_their_types_fill_the_rest() {
        let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
        assert!(
            out.rune
                .contains("#{ \"speed\": 2.0, \"label\": \"\", \"crew\": 4 }"),
            "{}",
            out.rune
        );
        assert!(
            !out.rune.contains("\"hidden\""),
            "a plain var is not an export"
        );
    }

    #[test]
    fn bodies_are_translated_rather_than_commented() {
        let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
        assert!(
            out.rune.contains(r#"log::info((gd.str_all)(["ahoy"]))"#),
            "{}",
            out.rune
        );
        assert!(
            out.rune.contains(r#"this.node.emit("sunk", 3);"#),
            "{}",
            out.rune
        );
        assert!(
            out.rune.contains("return v * 1.94;"),
            "a static body too: {}",
            out.rune
        );
    }

    #[test]
    fn a_body_that_calls_the_shim_binds_it_first() {
        let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
        assert!(
            out.rune.contains(r#"    let gd = script::require("gd.rn");"#),
            "{}",
            out.rune
        );
    }

    #[test]
    fn a_class_constant_becomes_a_module_constant() {
        let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
        assert!(out.rune.contains("pub const LIMIT = 9;"), "{}", out.rune);
    }

    #[test]
    fn a_member_with_a_value_lands_in_defaults() {
        let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
        assert!(out.rune.contains("this.hidden = 1;"), "{}", out.rune);
    }

    #[test]
    fn awaiting_makes_a_function_async_and_its_callers_too() {
        let source = "extends Node\n\
func outer():\n\
\tinner()\n\
\n\
func inner():\n\
\tawait ready\n";
        let out = convert(source, "scripts/a.gd", &Classes::default());
        assert!(out.rune.contains("pub async fn inner(this)"), "{}", out.rune);
        assert!(out.rune.contains("pub async fn outer(this)"), "{}", out.rune);
        assert!(out.rune.contains("inner(this).await;"), "{}", out.rune);
    }
}
