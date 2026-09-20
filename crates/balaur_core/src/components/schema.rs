//! What a schema may say, and whether one says it.
//!
//! The closed vocabulary a property spec is written in -- the datatypes, the
//! units -- and the check that holds a spec to it. Split from the registry
//! because none of it runs after boot: a schema is validated once, when the
//! component registers, and read for its defaults thereafter.

use super::as_f64;

/// The datatypes a schema property may declare (rule N6). Closed: a plugin
/// that wants another one adds it here, so the editor's inspector and the
/// scene format learn about it at the same moment.
pub const PROPERTY_TYPES: [&str; 14] = [
    "float", "int", "bool", "string", "enum", "vec2", "vec3", "vec4", "color", "asset", "flags",
    "node", "nodes", "strings",
];

/// The tags a component or preset carries, which the editor's picker
/// filters by. One spelling for every crate that registers one.
/// The units a property may be drawn in. Closed like [`PROPERTY_TYPES`]: an
/// editor has to know how to convert one, so a name it has never seen would
/// draw the number unconverted and say nothing.
pub const UNITS: &[&str] = &["degrees"];

/// The closed set as prose, for a panic message.
fn type_list() -> String {
    PROPERTY_TYPES
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One property spec against the vocabulary in the module docs. The `Err` is
/// the reason alone; the caller prefixes the component and property.
///
/// Public because a script's `exports()` declares properties in this same
/// vocabulary and has to be held to the same rules, without going through
/// [`ComponentDef::parse_schema`], which panics where a script needs an error.
pub fn validate_property(spec: &toml::Value) -> Result<(), String> {
    let spec = spec.as_table().ok_or_else(|| {
        format!(
            "spec is {}, not a table like {{ type = \"float\", default = 0.0 }}",
            spec.type_str()
        )
    })?;
    let declared = match spec.get("type").map(toml::Value::as_str) {
        Some(Some(declared)) => declared,
        Some(None) => return Err(format!("`type` is not a string; expected {}", type_list())),
        None => return Err(format!("no `type` key; expected one of {}", type_list())),
    };
    if !PROPERTY_TYPES.contains(&declared) {
        return Err(format!(
            "`type = \"{declared}\"` is not one of {}",
            type_list()
        ));
    }
    let options = spec.get("options");
    let takes_options = declared == "enum" || declared == "flags";
    match (takes_options, options) {
        (true, None) => return Err(format!("`type = \"{declared}\"` needs an `options` list")),
        (false, Some(_)) => {
            return Err(format!(
                "`options` belongs to `type = \"enum\"` and `type = \"flags\"`, not `type = \
                 \"{declared}\"`"
            ));
        }
        _ => {}
    }
    // `type` is the datatype key, so the asset's own type name needs a second
    // key rather than reusing it (N6): `{ type = "asset", asset = "clip" }`.
    match (declared == "asset", spec.get("asset")) {
        (true, None) => {
            return Err(
                "`type = \"asset\"` needs an `asset` key naming the asset type it takes".into(),
            );
        }
        (true, Some(name)) if name.as_str().is_none() => {
            return Err(format!("`asset` is {}, not a type name", name.type_str()));
        }
        (false, Some(_)) => {
            return Err(format!(
                "`asset` belongs to `type = \"asset\"`, not `type = \"{declared}\"`"
            ));
        }
        _ => {}
    }
    // As `asset` names its asset type, a node property may name the component
    // the node it points at has to carry; the script is handed that handle.
    match (matches!(declared, "node" | "nodes"), spec.get("component")) {
        (true, Some(name)) if name.as_str().is_none() => {
            return Err(format!("`component` is {}, not a name", name.type_str()));
        }
        (false, Some(_)) => {
            return Err(format!(
                "`component` belongs to a node property, not `type = \"{declared}\"`"
            ));
        }
        _ => {}
    }
    if let Some(description) = spec.get("description")
        && description.as_str().is_none()
    {
        return Err(format!(
            "`description` is {}, not a string",
            description.type_str()
        ));
    }
    if let Some(group) = spec.get("group") {
        match group.as_str() {
            Some(name) if !name.trim().is_empty() => {}
            Some(_) => return Err("`group` is empty; leave it out to show the property".into()),
            None => return Err(format!("`group` is {}, not a name", group.type_str())),
        }
    }
    if let Some(unit) = spec.get("unit") {
        match unit.as_str() {
            Some(name) if UNITS.contains(&name) => {}
            Some(name) => {
                return Err(format!(
                    "`unit = \"{name}\"` is not one of {}",
                    UNITS.join(", ")
                ));
            }
            None => return Err(format!("`unit` is {}, not a unit name", unit.type_str())),
        }
    }
    let default = spec
        .get("default")
        .ok_or_else(|| format!("no `default`; every property needs one, of type `{declared}`"))?;
    check_default(declared, default, options)
}

/// `default` against its declared type.
fn check_default(
    declared: &str,
    default: &toml::Value,
    options: Option<&toml::Value>,
) -> Result<(), String> {
    let (ok, wanted) = match declared {
        "float" => (as_f64(default).is_some(), "a number"),
        // Not `as_f64`: a count written as 1.0 round-trips through TOML as a
        // float, and a reader that wants a whole number then refuses it.
        "int" => (default.as_integer().is_some(), "a whole number"),
        "bool" => (default.as_bool().is_some(), "true or false"),
        // An asset default is a reference, and a reference is a path string.
        // A node default is a scene-relative path, and a path is a string.
        "string" | "asset" | "node" => (default.as_str().is_some(), "a string"),
        "enum" => return check_enum_default(default, options),
        "flags" => return check_flags_default(default, options),
        "vec2" => return check_numbers(declared, default, &[2]),
        "vec3" => return check_numbers(declared, default, &[3]),
        "vec4" => return check_numbers(declared, default, &[4]),
        "color" => return check_color_default(default),
        "strings" => (
            default
                .as_array()
                .is_some_and(|items| items.iter().all(toml::Value::is_str)),
            "a list of strings",
        ),
        _ => (true, ""),
    };
    if ok {
        return Ok(());
    }
    Err(format!(
        "`default` is {}, but `type = \"{declared}\"` wants {wanted}",
        default.type_str()
    ))
}

/// An enum's `default` must be a string, and one the `options` list offers —
/// otherwise the editor opens a dropdown that cannot show its own value.
fn check_enum_default(default: &toml::Value, options: Option<&toml::Value>) -> Result<(), String> {
    let choices = options
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "`options` is not a list of strings".to_string())?;
    let choices: Vec<&str> = choices.iter().filter_map(toml::Value::as_str).collect();
    if choices.is_empty() {
        return Err("`options` is empty, so no `default` can be legal".into());
    }
    let default = default.as_str().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"enum\"` wants one of the `options` strings",
            default.type_str()
        )
    })?;
    if choices.contains(&default) {
        return Ok(());
    }
    Err(format!(
        "`default = \"{default}\"` is not in `options` {choices:?}"
    ))
}

/// A `flags` default: an array, each entry one of the `options` strings. The
/// empty array is legal and is what most flag properties default to.
fn check_flags_default(default: &toml::Value, options: Option<&toml::Value>) -> Result<(), String> {
    let choices = options
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "`options` is not a list of strings".to_string())?;
    let choices: Vec<&str> = choices.iter().filter_map(toml::Value::as_str).collect();
    if choices.is_empty() {
        return Err("`options` is empty, so the property can hold nothing".into());
    }
    let default = default.as_array().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"flags\"` wants an array of `options` strings",
            default.type_str()
        )
    })?;
    for entry in default {
        let Some(name) = entry.as_str() else {
            return Err(format!(
                "`default` holds {}, but `type = \"flags\"` wants `options` strings",
                entry.type_str()
            ));
        };
        if !choices.contains(&name) {
            return Err(format!(
                "`default` holds \"{name}\", which is not in `options` {choices:?}"
            ));
        }
    }
    Ok(())
}

/// A `vec2`/`vec3` default: an array of numbers of exactly the right length.
fn check_numbers(declared: &str, default: &toml::Value, lengths: &[usize]) -> Result<(), String> {
    let wanted = || {
        lengths
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(" or ")
    };
    let array = default.as_array().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"{declared}\"` wants an array of {} numbers",
            default.type_str(),
            wanted()
        )
    })?;
    if !lengths.contains(&array.len()) {
        return Err(format!(
            "`default` has {} entries, but `type = \"{declared}\"` wants {}",
            array.len(),
            wanted()
        ));
    }
    if array.iter().all(|v| as_f64(v).is_some()) {
        return Ok(());
    }
    Err(format!(
        "`default` holds something that is not a number, but `type = \"{declared}\"` wants numbers"
    ))
}

/// A `color` default, in either spelling the value form accepts.
fn check_color_default(default: &toml::Value) -> Result<(), String> {
    if let Some(text) = default.as_str() {
        return if hex_rgba(text).is_some() {
            Ok(())
        } else {
            Err(format!(
                "`default = \"{text}\"` is not #rrggbb or #rrggbbaa"
            ))
        };
    }
    check_numbers("color", default, &[3, 4])
}

/// `#rrggbb` or `#rrggbbaa` as `[r, g, b, a]` in 0..=1.
pub(super) fn hex_rgba(text: &str) -> Option<[f64; 4]> {
    let hex = text.strip_prefix('#')?;
    let channel = |i: usize| {
        u8::from_str_radix(hex.get(i..i + 2)?, 16)
            .ok()
            .map(|b| f64::from(b) / 255.0)
    };
    match hex.len() {
        6 => Some([channel(0)?, channel(2)?, channel(4)?, 1.0]),
        8 => Some([channel(0)?, channel(2)?, channel(4)?, channel(6)?]),
        _ => None,
    }
}
