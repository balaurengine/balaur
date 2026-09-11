//! A GDScript file as the Rune skeleton a hand port starts from.
//!
//! GDScript and Rune differ in their object models, their coroutines and
//! their numbers, so no line of a body is translated. What carries is what
//! the engine calls: `_ready` is `init`, `_process` is `update`, every other
//! function keeps its name so the scene's handlers still reach it, and every
//! `@export` becomes an entry of `exports()` with the default it was given.
//! Each body stays inside its function as a comment, and everything at the
//! top of the file above the first function sits in a comment there too.

use std::fmt::Write as _;

use crate::import_godot_exports::{Classes, split_top};

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

/// Rune's reserved words, from its lexer: a GDScript name that is one of
/// these is renamed with a trailing `_`, and the report says so.
const RESERVED: &[&str] = &[
    "abstract", "alignof", "as", "async", "await", "become", "break", "const", "continue", "crate",
    "default", "do", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
    "is", "let", "loop", "macro", "match", "mod", "move", "mut", "not", "offsetof", "override",
    "priv", "proc", "pub", "pure", "ref", "return", "select", "self", "Self", "sizeof", "static",
    "struct", "super", "true", "typeof", "unsafe", "use", "virtual", "while", "yield",
];

/// One `func` and the lines under it.
struct Function {
    name: String,
    params: Vec<String>,
    is_static: bool,
    lines: Vec<String>,
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
        "// Converted from {path} by `balaur import`. The hooks and the exports are\n\
         // Rune; every GDScript body is kept as a comment inside its function."
    );
    while header.last().is_some_and(|l| l.trim().is_empty()) {
        header.pop();
    }
    if !header.is_empty() {
        out.push('\n');
        for line in &header {
            push_comment(&mut out, line, "");
        }
    }
    let exports = crate::import_godot_exports::exports(source, classes);
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
    write_functions(&mut out, &functions, &mut notes);
    if source.contains("_input(") || source.contains("_unhandled_input(") {
        notes
            .push("an `_input` handler: read the `input` module from `update` instead".to_string());
    }
    if source.contains("await ") {
        notes.push("`await`: only `init` and event handlers may be async here".to_string());
    }
    Converted { rune: out, notes }
}

/// Each function as a Rune one: a hook renamed, a keyword name suffixed, a
/// name declared twice kept once, and every body inside as a comment.
fn write_functions(out: &mut String, functions: &[Function], notes: &mut Vec<String>) {
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
        let _ = write!(out, "\npub fn {name}({params}) {{\n");
        for line in &function.lines {
            push_comment(out, line, "    ");
        }
        out.push_str("}\n");
    }
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
    let inside = rest
        .find('(')
        .and_then(|open| {
            let close = rest.rfind(')')?;
            (close > open).then(|| &rest[open + 1..close])
        })
        .unwrap_or_default();
    let params = split_top(inside)
        .into_iter()
        .filter_map(|param| {
            let name: String = param
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect();
    Function {
        name,
        params,
        is_static,
        lines: Vec::new(),
    }
}

/// Whether a signature's parentheses have closed.
fn balanced(text: &str) -> bool {
    let open = text.matches('(').count();
    let close = text.matches(')').count();
    open <= close
}

/// A parameter or variable name Rune will take.
fn safe(name: &str) -> String {
    if RESERVED.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// One GDScript line as a comment, keeping its indentation after the `//`.
fn push_comment(out: &mut String, line: &str, indent: &str) {
    let line = line.replace('\t', "    ");
    if line.trim().is_empty() {
        let _ = writeln!(out, "{indent}//");
    } else {
        let _ = writeln!(out, "{indent}// {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::convert;
    use crate::import_godot_exports::Classes;

    const SHIP: &str = "extends Node\n\
class_name Ship\n\
\n\
signal sunk(depth)\n\
@export var speed := 2.0\n\
@export var label: String\n\
@export_range(0, 10) var crew: int = 4\n\
var hidden := 1\n\
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
        assert!(
            out.rune.contains("    // \tposition.x += speed * delta")
                || out.rune.contains("    //     position.x += speed * delta")
        );
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
}
