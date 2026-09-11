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

/// Rune's reserved words: a GDScript function with one of these names is
/// renamed with a trailing `_`, and the report says so.
const RESERVED: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "is", "let", "loop", "match", "mod", "move", "not",
    "priv", "pub", "return", "select", "self", "static", "struct", "super", "true", "use",
    "while", "yield",
];

/// One `func` and the lines under it.
struct Function {
    name: String,
    params: Vec<String>,
    is_static: bool,
    lines: Vec<String>,
}

pub(crate) fn convert(source: &str, path: &str) -> Converted {
    let mut notes = Vec::new();
    let mut header: Vec<String> = Vec::new();
    let mut functions: Vec<Function> = Vec::new();
    let mut exports: Vec<(String, String)> = Vec::new();
    let mut pending_export = false;
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
            if top && (line.contains("@export") || pending_export) {
                match exported(line) {
                    Some(entry) => {
                        exports.push(entry);
                        pending_export = false;
                    }
                    None => pending_export = line.trim().starts_with("@export"),
                }
            }
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
    if !exports.is_empty() {
        out.push_str("\npub fn exports() {\n    #{");
        for (index, (name, value)) in exports.iter().enumerate() {
            let sep = if index == 0 { " " } else { ", " };
            let _ = write!(out, "{sep}\"{name}\": {value}");
        }
        out.push_str(" }\n}\n");
    }
    let mut seen: Vec<String> = Vec::new();
    for function in &functions {
        let hook = HOOKS.iter().find(|(godot, _, _)| *godot == function.name);
        let (name, params) = match hook {
            Some((_, here, params)) => ((*here).to_string(), (*params).to_string()),
            None => {
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
            }
        };
        if seen.contains(&name) {
            notes.push(format!("`{name}` is declared twice; the second is kept as a comment only"));
            out.push('\n');
            for line in &function.lines {
                push_comment(&mut out, line, "");
            }
            continue;
        }
        seen.push(name.clone());
        let _ = write!(out, "\npub fn {name}({params}) {{\n");
        for line in &function.lines {
            push_comment(&mut out, line, "    ");
        }
        out.push_str("}\n");
    }
    if source.contains("_input(") || source.contains("_unhandled_input(") {
        notes.push("an `_input` handler: read the `input` module from `update` instead".to_string());
    }
    if source.contains("await ") {
        notes.push("`await`: only `init` and event handlers may be async here".to_string());
    }
    Converted { rune: out, notes }
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

/// Split on commas that are not inside brackets or quotes.
fn split_top(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quoted = None;
    let mut current = String::new();
    for c in text.chars() {
        match (quoted, c) {
            (Some(q), c) if c == q => quoted = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quoted = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// A parameter or variable name Rune will take.
fn safe(name: &str) -> String {
    if RESERVED.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// `@export var name: Type = value` as `(name, Rune value)`.
fn exported(line: &str) -> Option<(String, String)> {
    let at = line.find("var ")?;
    let rest = &line[at + 4..];
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    let rest = rest[name.len()..].trim();
    let (hint, value) = match rest.split_once('=') {
        Some((before, after)) => (before.trim_start_matches(':').trim(), Some(after.trim())),
        None => (rest.trim_start_matches(':').trim(), None),
    };
    // `:=` leaves the `:` on the hint side, which is no type.
    let hint = hint.trim_end_matches(':').trim();
    // A trailing comment or setter block is not part of the value.
    let value = value.map(|v| v.split(" #").next().unwrap_or(v).trim_end_matches(':').trim());
    let rune = value
        .and_then(literal)
        .unwrap_or_else(|| default_of(hint).to_string());
    Some((name, rune))
}

/// A GDScript literal as Rune, or `None` for an expression this cannot read.
fn literal(text: &str) -> Option<String> {
    let text = text.trim();
    if text == "true" || text == "false" {
        return Some(text.to_string());
    }
    if text == "null" {
        return Some("()".to_string());
    }
    if let Some(inner) = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))
        .or_else(|| text.strip_prefix("&\"").and_then(|t| t.strip_suffix('"')))
        .or_else(|| text.strip_prefix("^\"").and_then(|t| t.strip_suffix('"')))
    {
        return Some(format!("\"{}\"", inner.replace('\\', "\\\\").replace('"', "\\\"")));
    }
    let numeric = text.trim_start_matches('-');
    if !numeric.is_empty() && numeric.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '_')) {
        let clean = text.replace('_', "");
        return if clean.contains(['.', 'e', 'E']) {
            let float = if clean.starts_with('.') || clean.starts_with("-.") {
                clean.replacen('.', "0.", 1)
            } else {
                clean
            };
            Some(float)
        } else {
            Some(clean)
        };
    }
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        let items: Option<Vec<String>> = split_top(inner)
            .iter()
            .filter(|s| !s.trim().is_empty())
            .map(|s| literal(s))
            .collect();
        return items.map(|items| format!("[{}]", items.join(", ")));
    }
    if text == "{}" {
        return Some("#{}".to_string());
    }
    for vector in ["Vector2", "Vector2i", "Vector3", "Vector3i", "Color"] {
        if let Some(args) = text
            .strip_prefix(vector)
            .and_then(|t| t.trim().strip_prefix('('))
            .and_then(|t| t.strip_suffix(')'))
        {
            let numbers: Option<Vec<String>> = split_top(args)
                .iter()
                .map(|a| {
                    let n = literal(a)?;
                    Some(if n.contains('.') { n } else { format!("{n}.0") })
                })
                .collect();
            return numbers.map(|n| format!("[{}]", n.join(", ")));
        }
    }
    None
}

/// The value a typed export starts at when it was given none.
fn default_of(hint: &str) -> &'static str {
    match hint {
        "int" => "0",
        "float" => "0.0",
        "bool" => "false",
        "String" | "StringName" | "NodePath" => "\"\"",
        "Vector2" | "Vector2i" => "[0.0, 0.0]",
        "Vector3" | "Vector3i" => "[0.0, 0.0, 0.0]",
        "Color" => "[1.0, 1.0, 1.0, 1.0]",
        "Dictionary" => "#{}",
        h if h.starts_with("Array") || (h.starts_with("Packed") && h.ends_with("Array")) => "[]",
        _ => "()",
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
    use super::{convert, exported, literal};

    const SHIP: &str = "extends Node2D\n\
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
        let out = convert(SHIP, "scripts/ship.gd");
        assert!(out.rune.contains("pub fn init(this) {"), "{}", out.rune);
        assert!(out.rune.contains("pub fn update(this, dt) {"));
        assert!(
            out.rune.contains("pub fn _on_go_pressed(this, force, loud) {"),
            "a signature over three lines, and the handler name a scene points at: {}",
            out.rune
        );
        assert!(out.rune.contains("pub fn knots(v) {"), "a static function takes no `this`");
        assert!(out.rune.contains("pub fn match_(this, a) {"));
        assert!(out.notes.iter().any(|n| n.contains("`match`")));
        assert!(out.rune.contains("    // \tposition.x += speed * delta") || out.rune.contains("    //     position.x += speed * delta"));
    }

    #[test]
    fn exports_carry_their_defaults_and_their_types_fill_the_rest() {
        let out = convert(SHIP, "scripts/ship.gd");
        assert!(
            out.rune.contains("#{ \"speed\": 2.0, \"label\": \"\", \"crew\": 4 }"),
            "{}",
            out.rune
        );
        assert!(!out.rune.contains("\"hidden\""), "a plain var is not an export");
    }

    #[test]
    fn a_literal_reads_as_rune_and_an_expression_does_not() {
        assert_eq!(literal("Vector2(1, 2.5)").as_deref(), Some("[1.0, 2.5]"));
        assert_eq!(literal(".5").as_deref(), Some("0.5"));
        assert_eq!(literal("&\"boats\"").as_deref(), Some("\"boats\""));
        assert_eq!(literal("[1, 2]").as_deref(), Some("[1, 2]"));
        assert_eq!(literal("preload(\"res://x.tscn\")"), None);
        assert_eq!(
            exported("@export var scene: PackedScene"),
            Some(("scene".to_string(), "()".to_string()))
        );
    }
}
