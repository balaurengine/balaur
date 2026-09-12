//! A GDScript `@export`, typed the way a Rune `exports()` entry is.
//!
//! The scene converter writes an export's value into `[nodes.script.props]`
//! and the script converter declares it in `exports()`, and the engine checks
//! one against the other, so both read exports through here and agree on what
//! each one is. A Godot type with no counterpart here — a Dictionary, a
//! Callable, an array of numbers — is not exported, and the report says so.

use std::collections::BTreeMap;
use std::path::Path;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::Value;

/// A project's own `class_name`s: the class each extends, so an export typed
/// by one can be traced back to a node or a resource, and the file each is
/// declared in, so a script extending one inherits its exports.
#[derive(Default)]
pub(crate) struct Classes {
    pub bases: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    pub root: std::path::PathBuf,
}

/// What an export holds, in the types an `exports()` spec has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Int,
    Float,
    Bool,
    Str,
    /// A node in the scene, by path.
    Node,
    /// A resource, by its project path.
    Path,
    Vec2,
    Vec3,
    Color,
    /// A list of nodes in the scene, each by path.
    Nodes,
    /// A list of paths or names.
    Strings,
}

/// One `@export`: its name, its GDScript type, what it is here, and its
/// default as a Rune literal.
#[derive(Debug)]
pub(crate) struct Export {
    pub name: String,
    pub hint: String,
    pub kind: Option<Kind>,
    pub default: String,
}

impl Export {
    /// The `exports()` entry, or `None` for a type this does not carry.
    pub(crate) fn entry(&self) -> Option<String> {
        let kind = self.kind?;
        // Quoted keys: `default` is a Rune keyword and cannot stand bare.
        let typed = |ty: &str| format!("#{{ \"type\": \"{ty}\", \"default\": {} }}", self.default);
        Some(match kind {
            // A bare default already says what these are.
            Kind::Int | Kind::Float | Kind::Bool | Kind::Str => self.default.clone(),
            Kind::Node => typed("node"),
            Kind::Path => typed("string"),
            Kind::Vec2 => typed("vec2"),
            Kind::Vec3 => typed("vec3"),
            Kind::Color => typed("color"),
            Kind::Strings => typed("strings"),
            Kind::Nodes => typed("nodes"),
        })
    }
}

/// Every `@export` a GDScript file has: its base's first, then its own, a
/// redeclared name keeping the later one, as GDScript's inheritance does.
pub(crate) fn exports(source: &str, classes: &Classes) -> Vec<Export> {
    let mut chain = vec![own(source, classes)];
    let mut base = extended(source, classes);
    // A chain longer than this is a cycle, which Godot refuses too.
    for _ in 0..16 {
        let Some(file) = base else { break };
        let Ok(text) = std::fs::read_to_string(classes.root.join(&file)) else {
            break;
        };
        chain.push(own(&text, classes));
        base = extended(&text, classes);
    }
    let mut out: Vec<Export> = Vec::new();
    for level in chain.into_iter().rev() {
        for export in level {
            out.retain(|e| e.name != export.name);
            out.push(export);
        }
    }
    out
}

/// The file a script's `extends` names, when it is a script of this project:
/// `extends "res://base.gd"` or `extends SomeClassName`.
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

/// The `@export`s one file declares itself.
fn own(source: &str, classes: &Classes) -> Vec<Export> {
    let mut out = Vec::new();
    let mut pending = false;
    for line in source.lines() {
        let top = !line.starts_with([' ', '\t']);
        if !top {
            continue;
        }
        let exporting = line.trim_start().starts_with("@export");
        if !exporting && !pending {
            continue;
        }
        match parse(line, classes) {
            Some(export) => {
                out.push(export);
                pending = false;
            }
            // `@export` alone on a line annotates the `var` on the next.
            None => pending = exporting,
        }
    }
    out
}

/// `@export var name: Type = value`.
fn parse(line: &str, classes: &Classes) -> Option<Export> {
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
    let hint = hint.trim_end_matches(':').trim().to_string();
    // A trailing comment or a setter block is not part of the value.
    let value = value.map(|v| {
        v.split(" #")
            .next()
            .unwrap_or(v)
            .trim_end_matches(':')
            .trim()
    });
    let written = value.and_then(literal);
    let kind = if hint.is_empty() {
        written.as_deref().and_then(kind_of_literal)
    } else {
        kind_of_hint(&hint, classes)
    };
    let default = match (kind, written) {
        (Some(Kind::Float), Some(n)) if !n.contains(['.', 'e', 'E']) => format!("{n}.0"),
        (Some(Kind::Strings | Kind::Nodes), Some(list)) if list.starts_with('[') => list,
        (Some(Kind::Node | Kind::Path), _) => "\"\"".to_string(),
        (Some(_), Some(literal)) => literal,
        (Some(kind), None) => zero(kind).to_string(),
        (None, _) => String::new(),
    };
    Some(Export {
        name,
        hint,
        kind,
        default,
    })
}

/// What a GDScript type is here.
fn kind_of_hint(hint: &str, classes: &Classes) -> Option<Kind> {
    Some(match hint {
        "int" => Kind::Int,
        "float" => Kind::Float,
        "bool" => Kind::Bool,
        "String" | "StringName" => Kind::Str,
        "NodePath" => Kind::Node,
        "Vector2" | "Vector2i" => Kind::Vec2,
        "Vector3" | "Vector3i" => Kind::Vec3,
        "Color" => Kind::Color,
        "PackedStringArray" => Kind::Strings,
        h => {
            if let Some(element) = h.strip_prefix("Array[").and_then(|e| e.strip_suffix(']')) {
                return match class_kind(element, classes) {
                    Some(Kind::Node) => Some(Kind::Nodes),
                    Some(Kind::Path | Kind::Str) => Some(Kind::Strings),
                    _ => None,
                };
            }
            return class_kind(h, classes);
        }
    })
}

/// A class name as a node reference, a resource path, or nothing here.
fn class_kind(class: &str, classes: &Classes) -> Option<Kind> {
    match class {
        "String" | "StringName" | "NodePath" => return Some(Kind::Str),
        "int" | "float" | "bool" | "Dictionary" | "Array" | "Callable" | "Signal" | "Variant" => {
            return None;
        }
        _ => {}
    }
    let mut current = class;
    // A project class extends another, eventually a Godot one. Its own name
    // says nothing: `PirateShipAnimation` is a node, not an `Animation`.
    for _ in 0..16 {
        if let Some(base) = classes.bases.get(current) {
            current = base;
            continue;
        }
        if is_resource(current) {
            return Some(Kind::Path);
        }
        if is_node(current) {
            return Some(Kind::Node);
        }
        return None;
    }
    None
}

fn is_resource(class: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        "Texture",
        "Texture2D",
        "Shape2D",
        "Shape3D",
        "Mesh",
        "Material",
        "Stream",
        "Font",
        "FontFile",
        "FontVariation",
        "Theme",
        "Gradient",
        "Curve",
        "Scene",
        "Resource",
        "Animation",
        "Library",
        "Frames",
        "Environment",
        "TileSet",
        "Image",
        "Script",
        "Shader",
        "Settings",
        "Translation",
    ];
    class.starts_with("StyleBox") || SUFFIXES.iter().any(|s| class.ends_with(s))
}

fn is_node(class: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        "2D",
        "3D",
        "Container",
        "Button",
        "Label",
        "Rect",
        "Bar",
        "Edit",
        "Slider",
        "Separator",
        "Dialog",
        "Player",
        "Timer",
        "Layer",
        "Box",
        "Panel",
        "Tree",
        "List",
        "Window",
        "Viewport",
        "Request",
        "Control",
        "Node",
        "CanvasItem",
        "TabBar",
    ];
    SUFFIXES.iter().any(|s| class.ends_with(s))
}

/// What an untyped export is, from the literal it was given.
fn kind_of_literal(literal: &str) -> Option<Kind> {
    match literal {
        "true" | "false" => Some(Kind::Bool),
        l if l.starts_with('"') => Some(Kind::Str),
        l if l.starts_with("[\"") || l == "[]" => Some(Kind::Strings),
        l if l.starts_with('[') => None,
        l if l.contains(['.', 'e', 'E']) => Some(Kind::Float),
        l if l
            .trim_start_matches('-')
            .chars()
            .all(|c| c.is_ascii_digit()) =>
        {
            Some(Kind::Int)
        }
        _ => None,
    }
}

fn zero(kind: Kind) -> &'static str {
    match kind {
        Kind::Int => "0",
        Kind::Float => "0.0",
        Kind::Bool => "false",
        Kind::Str | Kind::Node | Kind::Path => "\"\"",
        Kind::Vec2 => "[0.0, 0.0]",
        Kind::Vec3 => "[0.0, 0.0, 0.0]",
        Kind::Color => "[1.0, 1.0, 1.0, 1.0]",
        Kind::Strings | Kind::Nodes => "[]",
    }
}

/// A value a scene gave an export, as the kind that export has.
pub(crate) fn scene_value(
    kind: Kind,
    value: &Value,
    path_of: &dyn Fn(&Value) -> Option<String>,
) -> Option<Toml> {
    let text = |v: &Value| -> Option<String> {
        v.call("NodePath")
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| path_of(v))
            .or_else(|| v.as_str().map(str::to_string))
    };
    Some(match kind {
        Kind::Int => Toml::Integer(value.as_i64()?),
        Kind::Float => Toml::Float(value.as_f64()?),
        Kind::Bool => match value {
            Value::Bool(b) => Toml::Boolean(*b),
            _ => return None,
        },
        Kind::Str | Kind::Node | Kind::Path => Toml::String(text(value)?),
        Kind::Vec2 | Kind::Vec3 | Kind::Color => {
            Toml::Array(value.numbers()?.into_iter().map(Toml::Float).collect())
        }
        Kind::Strings | Kind::Nodes => {
            let items = value
                .as_array()
                .or_else(|| value.call("PackedStringArray"))?;
            Toml::Array(
                items
                    .iter()
                    .filter_map(|v| text(v).map(Toml::String))
                    .collect(),
            )
        }
    })
}

/// Every `class_name` in the project's scripts, to the class it extends and
/// the file that declares it.
pub(crate) fn class_index(root: &Path, files: &[String]) -> Classes {
    let mut classes = Classes {
        root: root.to_path_buf(),
        ..Classes::default()
    };
    for file in files
        .iter()
        .filter(|f| crate::godot::files::has_extension(f, "gd"))
    {
        let Ok(source) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        let word = |prefix: &str| {
            source.lines().find_map(|line| {
                let rest = line.strip_prefix(prefix)?;
                let name: String = rest
                    .trim()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                (!name.is_empty()).then_some(name)
            })
        };
        if let Some(name) = word("class_name ") {
            if let Some(base) = word("extends ") {
                classes.bases.insert(name.clone(), base);
            }
            classes.files.insert(name, file.clone());
        }
    }
    classes
}

/// A GDScript literal as Rune, or `None` for an expression this cannot read.
pub(crate) fn literal(text: &str) -> Option<String> {
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
        return Some(format!(
            "\"{}\"",
            inner.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    let numeric = text.trim_start_matches('-');
    if !numeric.is_empty()
        && numeric
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '_'))
    {
        let clean = text.replace('_', "");
        return Some(if clean.starts_with('.') || clean.starts_with("-.") {
            clean.replacen('.', "0.", 1)
        } else {
            clean
        });
    }
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        let items: Option<Vec<String>> = split_top(inner)
            .iter()
            .filter(|s| !s.trim().is_empty())
            .map(|s| literal(s))
            .collect();
        return items.map(|items| format!("[{}]", items.join(", ")));
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

/// Split on commas that are not inside brackets or quotes.
pub(crate) fn split_top(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quoted = None;
    let mut current = String::new();
    for c in text.chars() {
        match (quoted, c) {
            (Some(q), c) if c == q => quoted = None,
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

#[cfg(test)]
mod tests {
    use super::{Classes, Kind, exports, literal};

    fn one(line: &str, classes: &Classes) -> (Option<Kind>, Option<String>) {
        let export = exports(line, classes).pop().expect("an export");
        (export.kind, export.entry())
    }

    #[test]
    fn each_godot_type_is_the_spec_this_engine_checks() {
        let none = Classes::default();
        assert_eq!(
            one("@export var speed := 2.0", &none),
            (Some(Kind::Float), Some("2.0".into()))
        );
        assert_eq!(
            one("@export var hp: float = 3", &none).1.as_deref(),
            Some("3.0")
        );
        assert_eq!(
            one("@export var go: Button", &none).1.as_deref(),
            Some("#{ \"type\": \"node\", \"default\": \"\" }")
        );
        assert_eq!(
            one("@export var art: Texture2D", &none).1.as_deref(),
            Some("#{ \"type\": \"string\", \"default\": \"\" }")
        );
        assert_eq!(
            one("@export var hearts: Array[CanvasItem]", &none)
                .1
                .as_deref(),
            Some("#{ \"type\": \"nodes\", \"default\": [] }")
        );
        assert_eq!(
            one("@export var names: Array[String]", &none).1.as_deref(),
            Some("#{ \"type\": \"strings\", \"default\": [] }")
        );
        assert_eq!(
            one("@export var at: Vector2 = Vector2(1, 2)", &none)
                .1
                .as_deref(),
            Some("#{ \"type\": \"vec2\", \"default\": [1.0, 2.0] }")
        );
        assert_eq!(one("@export var table: Dictionary", &none), (None, None));
        assert_eq!(
            one("@export var weights: Array[float]", &none),
            (None, None)
        );
    }

    /// `PirateShipAnimation extends Node2D`: a project class is what it
    /// extends, whatever its name ends in.
    #[test]
    fn a_project_class_named_like_a_resource_is_still_a_node() {
        let mut classes = Classes::default();
        classes.bases.insert("PirateShipAnimation".into(), "Node2D".into());
        assert_eq!(super::class_kind("PirateShipAnimation", &classes), Some(Kind::Node));
        assert_eq!(super::class_kind("Animation", &classes), Some(Kind::Path));
    }

    /// `Controllers extends Node` in this game: a project class is traced
    /// back to the Godot class it comes from.
    #[test]
    fn a_project_class_is_whatever_it_extends() {
        let mut classes = Classes::default();
        classes.bases.insert("Controllers".into(), "Node".into());
        classes.bases.insert("Meta".into(), "RefCounted".into());
        assert_eq!(
            one("@export var c: Controllers", &classes).0,
            Some(Kind::Node)
        );
        assert_eq!(one("@export var m: Meta", &classes).0, None);
    }

    /// `CountrySelectPanel extends PagePanel` in this game, and `close_button`
    /// is PagePanel's: a script carries every export up its chain.
    #[test]
    fn a_script_inherits_the_exports_of_the_class_it_extends() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("page.gd"),
            "class_name PagePanel\nextends Control\n@export var close_button: Button\n@export var title := \"x\"\n",
        )
        .unwrap();
        let classes = super::class_index(dir.path(), &["page.gd".to_string()]);
        let names: Vec<String> = exports(
            "extends PagePanel\n@export var title := \"mine\"\n@export var flag := true\n",
            &classes,
        )
        .into_iter()
        .map(|e| format!("{}={}", e.name, e.default))
        .collect();
        assert_eq!(
            names,
            vec!["close_button=\"\"", "title=\"mine\"", "flag=true"]
        );
    }

    #[test]
    fn a_literal_reads_as_rune_and_an_expression_does_not() {
        assert_eq!(literal("Vector2(1, 2.5)").as_deref(), Some("[1.0, 2.5]"));
        assert_eq!(literal(".5").as_deref(), Some("0.5"));
        assert_eq!(literal("&\"boats\"").as_deref(), Some("\"boats\""));
        assert_eq!(literal("preload(\"res://x.tscn\")"), None);
    }
}
