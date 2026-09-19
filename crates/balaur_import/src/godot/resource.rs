//! A `.tres` a script loads, as the Rune module `<file>.tres.rn`.
//!
//! A resource carrying a script becomes an instance of that script's class
//! with the file's fields set on it; any other answers its own path, which is
//! what a scene reads where it names one.

use std::fmt::Write as _;

use crate::godot::Value;
use crate::godot::nodes::Resources;

/// The module a `.tres` is loaded through.
pub(crate) fn module_path(relative: &str) -> String {
    format!("{relative}.rn")
}

/// The module's text: `pub fn resource()` building the value.
pub(crate) fn convert(
    relative: &str,
    document: &crate::godot::Document,
    res: &Resources<'_>,
) -> String {
    let mut out =
        format!("// Converted from {relative} by `balaur import`.\n\npub fn resource() {{\n");
    let script = document
        .first("resource")
        .and_then(|section| Some((section, res.path(section.field("script")?)?)));
    let Some((section, script)) =
        script.filter(|(_, path)| crate::godot::files::has_extension(path, "gd"))
    else {
        let _ = write!(out, "    {}\n}}\n", quoted(relative));
        return out;
    };
    let module = format!("{}.rn", script.trim_end_matches(".gd"));
    out.push_str("    let gd = script::require(\"gd.rn\");\n");
    let _ = writeln!(
        out,
        "    let this = (script::require({}).new)();",
        quoted(&module)
    );
    for (key, value) in &section.fields {
        if key == "script" || key.starts_with("metadata/") {
            continue;
        }
        let _ = writeln!(
            out,
            "    (gd.set)(this, {}, {});",
            quoted(key),
            rune_value(value, res)
        );
    }
    out.push_str("    this\n}\n");
    out
}

/// One field's value as Rune.
fn rune_value(value: &Value, res: &Resources<'_>) -> String {
    match value {
        Value::Null | Value::Object { .. } => "()".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(n) => format!("{n:?}"),
        Value::Str(s) | Value::Name(s) => quoted(s),
        Value::Array(items) => list(items, res),
        Value::Dict(pairs) => {
            let parts: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("[{}, {}]", rune_value(k, res), rune_value(v, res)))
                .collect();
            format!("(gd.dict)([{}])", parts.join(", "))
        }
        Value::Call { name, args } => match name.as_str() {
            "ExtResource" => match res.path(value) {
                Some(path) if crate::godot::files::has_extension(path, "tres") => {
                    format!("(gd.resource)({})", quoted(&module_path(path)))
                }
                Some(path) => quoted(path),
                None => "()".into(),
            },
            "Vector2" | "Vector2i" => format!("(gd.vec2)({})", numbers(args)),
            "Vector3" | "Vector3i" => format!("(gd.vec3)({})", numbers(args)),
            "Color" => format!("(gd.color)({})", numbers(args)),
            "NodePath" | "StringName" => args.first().map_or("\"\"".into(), |a| rune_value(a, res)),
            // `PackedStringArray(..)` and its kin: a list, with the Godot 4
            // typed `Array[T]([..])` passing its one list through.
            _ if args.len() == 1 && matches!(args[0], Value::Array(_)) => rune_value(&args[0], res),
            _ => list(args, res),
        },
    }
}

fn list(items: &[Value], res: &Resources<'_>) -> String {
    let parts: Vec<String> = items.iter().map(|v| rune_value(v, res)).collect();
    format!("[{}]", parts.join(", "))
}

fn numbers(args: &[Value]) -> String {
    args.iter()
        .map(|a| format!("{:?}", a.as_f64().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
