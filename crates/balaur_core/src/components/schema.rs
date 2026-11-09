//! What a schema may say, and whether one says it.
//!
//! The closed vocabulary a property spec is written in -- the datatypes, the
//! units -- and the check that holds a spec to it. Split from the registry
//! because none of it runs after boot: a schema is validated once, when the
//! component registers, and read for its defaults thereafter.

use super::as_f64;
use crate::vocabulary::keys;
use crate::vocabulary::words;

const KEY: &str = "key";

/// The datatypes a schema property may declare (rule N6). Closed: a plugin
/// that wants another one adds it here, so the editor's inspector and the
/// scene format learn about it at the same moment.
pub const PROPERTY_TYPES: [&str; 16] = [
    "float",
    words::datatypes::INT,
    words::datatypes::BOOL,
    words::datatypes::STRING,
    words::datatypes::ENUM,
    words::datatypes::VEC2,
    words::datatypes::VEC3,
    words::datatypes::VEC4,
    words::datatypes::COLOR,
    words::datatypes::ASSET,
    words::datatypes::FLAGS,
    words::datatypes::NODE,
    words::datatypes::LIST,
    words::datatypes::MAP,
    words::datatypes::RECORD,
    words::datatypes::VARIANT,
];

/// The key a `variant` value names its variant by: `{ kind = "box", size =
/// [1.0, 1.0, 1.0] }`.
pub const KIND: &str = "kind";

/// The record spec of the variant `value` names, or of the default's where
/// the value names none. `None` for any other type, and for a name no
/// variant answers to.
#[must_use]
pub fn active_variant<'a>(
    spec: &'a toml::Value,
    value: &toml::Value,
) -> Option<(String, &'a toml::Value)> {
    let variants = spec.get(keys::VARIANTS)?.as_table()?;
    let kind = value
        .get(KIND)
        .or_else(|| spec.get(keys::DEFAULT)?.get(KIND))
        .and_then(toml::Value::as_str)
        .or_else(|| variants.keys().next().map(String::as_str))?
        .to_owned();
    let record = variants.get(&kind)?;
    Some((kind, record))
}

/// The specs a composite holds: what a `list` or a `map` holds, every field
/// of a `record`, or every variant of a `variant`, each a record. Empty for
/// everything else, which is most properties.
#[must_use]
pub fn inner_specs(spec: &toml::Value) -> Vec<&toml::Value> {
    match spec.get(keys::TYPE).and_then(toml::Value::as_str) {
        Some(words::datatypes::LIST | words::datatypes::MAP) => {
            spec.get(keys::OF).into_iter().collect()
        }
        Some(words::datatypes::RECORD) => spec
            .get(keys::FIELDS)
            .and_then(toml::Value::as_table)
            .map(|fields| fields.values().collect())
            .unwrap_or_default(),
        Some(words::datatypes::VARIANT) => spec
            .get(keys::VARIANTS)
            .and_then(toml::Value::as_table)
            .map(|variants| variants.values().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

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
/// [`super::ComponentDef::parse_schema`], which panics where a script needs an
/// error.
pub fn validate_property(spec: &toml::Value) -> Result<(), String> {
    check_spec(spec, true)
}

/// One spec, with `default` required only at the top: a nested spec's default
/// is what a new entry starts at, and [`zero_of`] serves where it is left out.
fn check_spec(spec: &toml::Value, needs_default: bool) -> Result<(), String> {
    let spec = spec.as_table().ok_or_else(|| {
        format!(
            "spec is {}, not a table like {{ type = \"float\", default = 0.0 }}",
            spec.type_str()
        )
    })?;
    let declared = match spec.get(keys::TYPE).map(toml::Value::as_str) {
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
    check_siblings(declared, spec)?;
    check_composite(declared, spec)?;
    let Some(default) = spec.get(keys::DEFAULT) else {
        return if needs_default {
            Err(format!(
                "no `default`; every property needs one, of type `{declared}`"
            ))
        } else {
            Ok(())
        };
    };
    check_default(declared, default, spec)
}

/// The keys a non-composite type may bring with it, each tied to the one type
/// it belongs to.
fn check_siblings(
    declared: &str,
    spec: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    let options = spec.get(keys::OPTIONS);
    let takes_options = declared == words::datatypes::ENUM || declared == words::datatypes::FLAGS;
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
    match (declared == words::datatypes::ASSET, spec.get("asset")) {
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
    match (
        declared == words::datatypes::NODE,
        spec.get(keys::COMPONENT),
    ) {
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
    if let Some(description) = spec.get(keys::DESCRIPTION)
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
    Ok(())
}

/// A composite names what it holds in one key, and nothing else may carry
/// that key: `of` for a `list` or a `map`, `fields` for a `record`.
fn check_composite(
    declared: &str,
    spec: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    match (
        matches!(declared, words::datatypes::LIST | words::datatypes::MAP),
        spec.get(keys::OF),
    ) {
        (true, None) => {
            return Err(format!(
                "`type = \"{declared}\"` needs an `of` key saying what it holds"
            ));
        }
        (true, Some(inner)) => check_spec(inner, false).map_err(|why| format!("`of`: {why}"))?,
        (false, Some(_)) => {
            return Err(format!(
                "`of` belongs to a `list` or a `map`, not `type = \"{declared}\"`"
            ));
        }
        _ => {}
    }
    match (
        declared == words::datatypes::VARIANT,
        spec.get(keys::VARIANTS),
    ) {
        (true, None) => {
            return Err(
                "`type = \"variant\"` needs a `variants` table, one record spec a variant".into(),
            );
        }
        (true, Some(variants)) => check_variants(variants)?,
        (false, Some(_)) => {
            return Err(format!(
                "`variants` belongs to `type = \"variant\"`, not `type = \"{declared}\"`"
            ));
        }
        _ => {}
    }
    match (declared == words::datatypes::RECORD, spec.get(keys::FIELDS)) {
        (true, None) => {
            return Err("`type = \"record\"` needs a `fields` table, one spec a field".into());
        }
        (true, Some(fields)) => check_fields(fields)?,
        (false, Some(_)) => {
            return Err(format!(
                "`fields` belongs to `type = \"record\"`, not `type = \"{declared}\"`"
            ));
        }
        _ => {}
    }
    // As an `asset` names its asset type, a record may name the script class
    // whose shape it is; the host hands that script an instance of it.
    match (declared == words::datatypes::RECORD, spec.get("class")) {
        (true, Some(name)) if name.as_str().is_none() => {
            return Err(format!("`class` is {}, not a name", name.type_str()));
        }
        (false, Some(_)) => {
            return Err(format!(
                "`class` belongs to `type = \"record\"`, not `type = \"{declared}\"`"
            ));
        }
        _ => {}
    }
    match (declared == words::datatypes::MAP, spec.get(KEY)) {
        (true, Some(kind)) => match kind.as_str() {
            Some(words::datatypes::STRING | words::datatypes::INT) => {}
            _ => return Err("`key` is not \"string\" or \"int\"".into()),
        },
        (false, Some(_)) => {
            return Err(format!(
                "`key` belongs to `type = \"map\"`, not `type = \"{declared}\"`"
            ));
        }
        _ => {}
    }
    Ok(())
}

/// A `variant`'s `variants`: a table of record specs, one per variant, and
/// at least one. A variant's name is what a value's `kind` says.
fn check_variants(variants: &toml::Value) -> Result<(), String> {
    let table = variants.as_table().ok_or_else(|| {
        format!(
            "`variants` is {}, not a table of record specs",
            variants.type_str()
        )
    })?;
    if table.is_empty() {
        return Err("`variants` is empty, so the value can be nothing".into());
    }
    for (name, record) in table {
        if record.get(keys::TYPE).and_then(toml::Value::as_str) != Some(words::datatypes::RECORD) {
            return Err(format!(
                "variant '{name}' is not a `type = \"record\"` spec"
            ));
        }
        let fields = record.get(keys::FIELDS).and_then(toml::Value::as_table);
        if fields.is_some_and(|fields| fields.contains_key(KIND)) {
            return Err(format!(
                "variant '{name}' has a field named `{KIND}`, which names the variant"
            ));
        }
        // A kind with nothing more to say holds no fields, which a record
        // on its own may not.
        if fields.is_some_and(toml::map::Map::is_empty) {
            continue;
        }
        check_spec(record, false).map_err(|why| format!("variant '{name}': {why}"))?;
    }
    Ok(())
}

/// A `record`'s `fields`: a table of specs, one per field, and at least one.
fn check_fields(fields: &toml::Value) -> Result<(), String> {
    let table = fields
        .as_table()
        .ok_or_else(|| format!("`fields` is {}, not a table of specs", fields.type_str()))?;
    if table.is_empty() {
        return Err("`fields` is empty, so the record can hold nothing".into());
    }
    for (name, spec) in table {
        check_spec(spec, false).map_err(|why| format!("field '{name}': {why}"))?;
    }
    Ok(())
}

/// `default` against its declared type, and against the specs a composite
/// holds.
fn check_default(
    declared: &str,
    default: &toml::Value,
    spec: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    let options = spec.get(keys::OPTIONS);
    let (ok, wanted) = match declared {
        words::datatypes::FLOAT => (as_f64(default).is_some(), "a number"),
        // Not `as_f64`: a count written as 1.0 round-trips through TOML as a
        // float, and a reader that wants a whole number then refuses it.
        words::datatypes::INT => (default.as_integer().is_some(), "a whole number"),
        words::datatypes::BOOL => (default.as_bool().is_some(), "true or false"),
        // An asset default is a reference, and a reference is a path string.
        // A node default is a scene-relative path, and a path is a string.
        words::datatypes::STRING | words::datatypes::ASSET | words::datatypes::NODE => {
            (default.as_str().is_some(), "a string")
        }
        words::datatypes::ENUM => return check_enum_default(default, options),
        words::datatypes::FLAGS => return check_flags_default(default, options),
        words::datatypes::VEC2 => return check_numbers(declared, default, &[2]),
        words::datatypes::VEC3 => return check_numbers(declared, default, &[3]),
        words::datatypes::VEC4 => return check_numbers(declared, default, &[4]),
        words::datatypes::COLOR => return check_color_default(default),
        words::datatypes::LIST => return check_list_default(default, spec),
        words::datatypes::MAP => return check_map_default(default, spec),
        words::datatypes::RECORD => return check_record_default(default, spec),
        words::datatypes::VARIANT => return check_variant_default(default, spec),
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

/// What a new entry in a composite starts at: its spec's own `default`, or
/// the type's zero where a nested spec left one out.
#[must_use]
pub fn zero_of(spec: &toml::Value) -> toml::Value {
    if let Some(default) = spec.get(keys::DEFAULT) {
        return default.clone();
    }
    let numbers = |n: usize| toml::Value::Array(vec![toml::Value::Float(0.0); n]);
    match spec
        .get(keys::TYPE)
        .and_then(toml::Value::as_str)
        .unwrap_or("")
    {
        words::datatypes::INT => toml::Value::Integer(0),
        words::datatypes::FLOAT => toml::Value::Float(0.0),
        words::datatypes::BOOL => toml::Value::Boolean(false),
        words::datatypes::ENUM => spec
            .get(keys::OPTIONS)
            .and_then(toml::Value::as_array)
            .and_then(|options| options.first())
            .cloned()
            .unwrap_or_else(|| toml::Value::String(String::new())),
        words::datatypes::VEC2 => numbers(2),
        words::datatypes::VEC3 => numbers(3),
        words::datatypes::VEC4 => numbers(4),
        // A missing channel is opaque, which is what `rgba` reads.
        words::datatypes::COLOR => toml::Value::Array(vec![toml::Value::Float(1.0); 4]),
        words::datatypes::FLAGS | words::datatypes::LIST => toml::Value::Array(Vec::new()),
        words::datatypes::MAP => toml::Value::Table(toml::map::Map::new()),
        words::datatypes::VARIANT => spec
            .get(keys::VARIANTS)
            .and_then(toml::Value::as_table)
            .and_then(|variants| variants.iter().next())
            .map_or_else(
                || toml::Value::Table(toml::map::Map::new()),
                |(name, record)| {
                    let mut zero = zero_of(record);
                    if let Some(table) = zero.as_table_mut() {
                        table.insert(KIND.into(), toml::Value::String(name.clone()));
                    }
                    zero
                },
            ),
        words::datatypes::RECORD => toml::Value::Table(
            spec.get(keys::FIELDS)
                .and_then(toml::Value::as_table)
                .map(|fields| {
                    fields
                        .iter()
                        .map(|(name, field)| (name.clone(), zero_of(field)))
                        .collect()
                })
                .unwrap_or_default(),
        ),
        _ => toml::Value::String(String::new()),
    }
}

/// Write the `default` a nested spec left out, so every reader finds one at
/// every depth. The inspector needs one to add an entry.
pub fn complete_property(spec: &mut toml::Value) {
    match spec.get(keys::TYPE).and_then(toml::Value::as_str) {
        Some(words::datatypes::LIST | words::datatypes::MAP) => {
            if let Some(of) = spec.get_mut(keys::OF) {
                complete_inner(of);
            }
        }
        Some(words::datatypes::RECORD) => {
            if let Some(fields) = spec
                .get_mut(keys::FIELDS)
                .and_then(toml::Value::as_table_mut)
            {
                for (_, field) in fields.iter_mut() {
                    complete_inner(field);
                }
            }
        }
        Some(words::datatypes::VARIANT) => {
            if let Some(variants) = spec
                .get_mut(keys::VARIANTS)
                .and_then(toml::Value::as_table_mut)
            {
                for (_, record) in variants.iter_mut() {
                    complete_property(record);
                }
            }
        }
        _ => {}
    }
}

fn complete_inner(spec: &mut toml::Value) {
    complete_property(spec);
    if spec.get(keys::DEFAULT).is_some() {
        return;
    }
    let zero = zero_of(spec);
    if let Some(table) = spec.as_table_mut() {
        table.insert(keys::DEFAULT.into(), zero);
    }
}

/// One value against the spec that governs it, wherever it sits.
///
/// Public because a script inferring a list's `of` from its first entry has
/// to hold the rest to it, and that is this question.
pub fn validate_value(spec: &toml::Value, value: &toml::Value) -> Result<(), String> {
    let table = spec
        .as_table()
        .ok_or_else(|| format!("spec is {}, not a table", spec.type_str()))?;
    let declared = table
        .get(keys::TYPE)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "spec has no `type`".to_string())?;
    check_default(declared, value, table)
}

/// A `list` default: an array, every entry of the type `of` declares.
fn check_list_default(
    default: &toml::Value,
    spec: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    let items = default.as_array().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"list\"` wants an array",
            default.type_str()
        )
    })?;
    let of = spec.get(keys::OF).ok_or_else(|| "no `of`".to_string())?;
    for (at, item) in items.iter().enumerate() {
        validate_value(of, item).map_err(|why| format!("entry {at}: {why}"))?;
    }
    Ok(())
}

/// A `map` default: a table whose keys parse as `key` says and whose values
/// are what `of` declares.
fn check_map_default(
    default: &toml::Value,
    spec: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    let table = default.as_table().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"map\"` wants a table",
            default.type_str()
        )
    })?;
    let of = spec.get(keys::OF).ok_or_else(|| "no `of`".to_string())?;
    let keyed_by_int = spec.get(KEY).and_then(toml::Value::as_str) == Some(words::datatypes::INT);
    for (key, value) in table {
        if keyed_by_int && key.parse::<i64>().is_err() {
            return Err(format!("key \"{key}\" is not a whole number"));
        }
        validate_value(of, value).map_err(|why| format!("key \"{key}\": {why}"))?;
    }
    Ok(())
}

/// A `record` default: a table of the declared fields, each of its own type.
/// A field left out takes that field's own default, so only a name no field
/// answers to is an error.
fn check_record_default(
    default: &toml::Value,
    spec: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    let table = default.as_table().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"record\"` wants a table",
            default.type_str()
        )
    })?;
    let fields = spec
        .get(keys::FIELDS)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "no `fields`".to_string())?;
    for (name, value) in table {
        let field = fields
            .get(name)
            .ok_or_else(|| format!("`default` holds '{name}', which is not a declared field"))?;
        validate_value(field, value).map_err(|why| format!("field '{name}': {why}"))?;
    }
    Ok(())
}

/// A `variant` default: a table whose `kind` names a variant, the rest the
/// fields of that variant's record.
fn check_variant_default(
    default: &toml::Value,
    spec: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    let table = default.as_table().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"variant\"` wants a table",
            default.type_str()
        )
    })?;
    let variants = spec
        .get(keys::VARIANTS)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "no `variants`".to_string())?;
    let kind = table
        .get(KIND)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("`default` names no `{KIND}`"))?;
    let record = variants.get(kind).ok_or_else(|| {
        format!(
            "`{KIND} = \"{kind}\"` is none of {}",
            variants.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })?;
    let mut fields = table.clone();
    fields.remove(KIND);
    validate_value(record, &toml::Value::Table(fields)).map_err(|why| format!("{kind}: {why}"))
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
    check_numbers(words::datatypes::COLOR, default, &[3, 4])
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
