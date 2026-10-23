//! A `.tres` a script loads, as the module `<file>_tres.bs`.
//!
//! A resource carrying a script becomes an instance of that script's class
//! with the file's fields set on it; any other answers its own path, which is
//! what a scene reads where it names one.

use std::fmt::Write as _;

use crate::godot::Value;
use crate::godot::nodes::Resources;

/// The module a `.tres` is loaded through: `a/b.tres` is `a/b_tres.bs`.
pub(crate) fn module_path(relative: &str) -> String {
    let stem = relative.strip_suffix(".tres").unwrap_or(relative);
    crate::godot::scene::module_file(&format!("{stem}_tres"))
}

/// The module's text: `pub fn resource()` building the value.
pub(crate) fn convert(
    relative: &str,
    document: &crate::godot::Document,
    res: &Resources<'_>,
) -> String {
    let mut out = format!("// Converted from {relative} by `balaur import`.\n");
    let script = document
        .first("resource")
        .and_then(|section| Some((section, res.path(section.field("script")?)?)));
    let Some((section, script)) =
        script.filter(|(_, path)| crate::godot::files::has_extension(path, "gd"))
    else {
        let _ = write!(
            out,
            "\npub fn resource() -> any {{\n    let path: any = {};\n    path\n}}\n",
            quoted(relative)
        );
        return out;
    };
    let mut body = String::new();
    let module = crate::godot::scene::script_path(script);
    let _ = writeln!(body, "    let this: any = classes::make({});", quoted(&module));
    let mut uses = std::collections::BTreeSet::from(["classes".to_string()]);
    for (key, value) in &section.fields {
        if key == "script" || key.starts_with("metadata/") {
            continue;
        }
        let value = bs_value(value, res, &mut uses);
        let _ = writeln!(body, "    gd::set(this, {}, {value});", quoted(key));
        uses.insert("gd".to_string());
    }
    out.push('\n');
    for module in &uses {
        let _ = writeln!(out, "use {module};");
    }
    let _ = write!(out, "\npub fn resource() -> any {{\n{body}    this\n}}\n");
    out
}

/// One field's value as Balaur Script, with the modules it reaches.
fn bs_value(
    value: &Value,
    res: &Resources<'_>,
    uses: &mut std::collections::BTreeSet<String>,
) -> String {
    match value {
        Value::Null | Value::Object { .. } => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(n) => format!("{n:?}"),
        Value::Str(s) | Value::Name(s) => quoted(s),
        Value::Array(items) => list(items, res, uses),
        Value::Dict(pairs) => {
            let mut flat = Vec::new();
            for (k, v) in pairs {
                flat.push(bs_value(k, res, uses));
                flat.push(bs_value(v, res, uses));
            }
            uses.insert("gd".into());
            format!("gd::dict(gd::list_of([{}]))", flat.join(", "))
        }
        Value::Call { name, args } => match name.as_str() {
            "ExtResource" => match res.path(value) {
                Some(path) if crate::godot::files::has_extension(path, "tres") => {
                    let module = module_path(path);
                    uses.insert("gd".into());
                    uses.insert(crate::godot::gdscript::module_use(&module));
                    format!(
                        "gd::resource_of({}, {}::resource)",
                        quoted(path),
                        crate::godot::gdscript::module_name(&module)
                    )
                }
                Some(path) => quoted(path),
                None => "nil".into(),
            },
            "Vector2" | "Vector2i" => {
                uses.insert("gd".into());
                format!("gd::vec2({})", numbers(args))
            }
            "Vector3" | "Vector3i" => {
                uses.insert("gd".into());
                format!("gd::vec3({})", numbers(args))
            }
            "Color" => {
                uses.insert("gd".into());
                format!("gd::color({})", numbers(args))
            }
            "NodePath" | "StringName" => args
                .first()
                .map_or("\"\"".into(), |a| bs_value(a, res, uses)),
            // `PackedStringArray(..)` and its kin: a list, with the Godot 4
            // typed `Array[T]([..])` passing its one list through.
            _ if args.len() == 1 && matches!(args[0], Value::Array(_)) => {
                bs_value(&args[0], res, uses)
            }
            _ => list(args, res, uses),
        },
    }
}

// A list of values of any kind: an `any` list, which the shim's list is.
fn list(
    items: &[Value],
    res: &Resources<'_>,
    uses: &mut std::collections::BTreeSet<String>,
) -> String {
    let parts: Vec<String> = items.iter().map(|v| bs_value(v, res, uses)).collect();
    uses.insert("gd".into());
    format!("gd::list_of([{}])", parts.join(", "))
}

fn numbers(args: &[Value]) -> String {
    args.iter()
        .map(|a| format!("{:?}", a.as_f64().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn quoted(text: &str) -> String {
    crate::godot::gdscript::quoted(text)
}
