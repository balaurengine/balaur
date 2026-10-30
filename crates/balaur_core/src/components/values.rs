//! The passes over a component's property table before `apply` sees it:
//! defaults filled in, a scene's values laid over them, records completed,
//! colours expanded and assets resolved to references.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

/// Every declared field of a `record` value, so a scene naming one of two
/// fields still hands `apply` both. A key no field answers to is dropped:
/// the record's shape is the schema's.
pub(super) fn fill_records(schema: &toml::Value, out: &mut toml::map::Map<String, toml::Value>) {
    let Some(table) = schema.as_table() else {
        return;
    };
    for (prop, spec) in table {
        if let Some(value) = out.get_mut(prop) {
            fill_record_value(spec, value);
        }
    }
}

fn fill_record_value(spec: &toml::Value, value: &mut toml::Value) {
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("record") => {
            let Some(fields) = spec.get("fields").and_then(toml::Value::as_table) else {
                return;
            };
            let held = value.as_table().cloned().unwrap_or_default();
            let mut whole = toml::map::Map::new();
            for (name, field) in fields {
                let mut inner = held
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| schema::zero_of(field));
                fill_record_value(field, &mut inner);
                whole.insert(name.clone(), inner);
            }
            *value = toml::Value::Table(whole);
        }
        Some("list" | "map") => {
            if let Some(of) = spec.get("of") {
                for inner in held_mut(value) {
                    fill_record_value(of, inner);
                }
            }
        }
        _ => {}
    }
}

/// A colour written either way: `[r, g, b, a]` floats, or `#rrggbb` /
/// `#rrggbbaa`. A missing alpha is opaque.
///
/// Public because the node-level keys are colours too and are read outside
/// the schema path, which is where `expand_colors` does this.
#[must_use]
pub fn rgba(value: &toml::Value) -> Option<[f32; 4]> {
    if let Some(text) = value.as_str() {
        return hex_rgba(text).map(|c| c.map(|v| v as f32));
    }
    let array = value.as_array()?;
    let channel =
        |i: usize, default: f32| array.get(i).and_then(as_f64).map_or(default, |v| v as f32);
    Some([
        channel(0, 1.0),
        channel(1, 1.0),
        channel(2, 1.0),
        channel(3, 1.0),
    ])
}

/// Expand hex strings on `color`-typed properties into the float array every
/// `apply` hook reads.
///
/// Done once here rather than in each hook, so a hex value works on every
/// colour property that exists or is added later. Before this, `color =
/// "#ff0000"` on a node reached an `as_array()` that returned `None` and fell
/// through to the default grey without a word.
pub(super) fn expand_colors(schema: &toml::Value, out: &mut toml::map::Map<String, toml::Value>) {
    let Some(table) = schema.as_table() else {
        return;
    };
    for (prop, spec) in table {
        if let Some(value) = out.get_mut(prop) {
            expand_color_value(prop, spec, value);
        }
    }
}

/// One value's colours, wherever the spec puts them: a colour held in a list
/// or a record is written the same way as one held by the property itself.
fn expand_color_value(prop: &str, spec: &toml::Value, value: &mut toml::Value) {
    // `type` is the spec's datatype key (see the module docs).
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("color") => {
            let Some(text) = value.as_str() else {
                return;
            };
            if let Some(rgba) = hex_rgba(text) {
                *value = toml::Value::Array(rgba.iter().copied().map(toml::Value::Float).collect());
            } else {
                tracing::warn!(
                    property = prop,
                    value = text,
                    "not a colour; expected #rrggbb, #rrggbbaa or [r, g, b, a]"
                );
            }
        }
        Some("list" | "map") => {
            if let Some(of) = spec.get("of") {
                for inner in held_mut(value) {
                    expand_color_value(prop, of, inner);
                }
            }
        }
        Some("record") => {
            for (field, inner) in fields_mut(spec, value) {
                expand_color_value(prop, field, inner);
            }
        }
        _ => {}
    }
}

/// What a `list` or a `map` value holds, for a pass that rewrites entries.
pub(super) fn held_mut(value: &mut toml::Value) -> Vec<&mut toml::Value> {
    match value {
        toml::Value::Array(items) => items.iter_mut().collect(),
        toml::Value::Table(table) => table.iter_mut().map(|(_, inner)| inner).collect(),
        _ => Vec::new(),
    }
}

/// A `record` value's entries beside the spec each field declares. A key no
/// field answers to is skipped; `validate_property` already refused it.
fn fields_mut<'a>(
    spec: &'a toml::Value,
    value: &'a mut toml::Value,
) -> Vec<(&'a toml::Value, &'a mut toml::Value)> {
    let (Some(fields), Some(table)) = (
        spec.get("fields").and_then(toml::Value::as_table),
        value.as_table_mut(),
    ) else {
        return Vec::new();
    };
    table
        .iter_mut()
        .filter_map(|(name, inner)| Some((fields.get(name)?, inner)))
        .collect()
}

pub fn merge_defaults(schema: &toml::Value, params: Option<&toml::Value>) -> Result<toml::Value> {
    let mut out = defaults_of(schema);
    overlay(schema, &mut out, params)?;
    fill_records(schema, &mut out);
    expand_colors(schema, &mut out);
    Ok(toml::Value::Table(out))
}

/// Every property the schema declares, at its declared default.
pub(super) fn defaults_of(schema: &toml::Value) -> toml::map::Map<String, toml::Value> {
    let mut out = toml::map::Map::new();
    if let Some(table) = schema.as_table() {
        for (prop, spec) in table {
            if let Some(default) = spec.get("default") {
                out.insert(prop.clone(), default.clone());
            }
        }
    }
    out
}

/// Write `params` over whatever `out` already holds, leaving every property
/// `params` does not mention alone.
pub(super) fn overlay(
    schema: &toml::Value,
    out: &mut toml::map::Map<String, toml::Value>,
    params: Option<&toml::Value>,
) -> Result<()> {
    match params {
        Some(toml::Value::Table(params)) => {
            for (k, v) in params {
                out.insert(k.clone(), v.clone());
            }
        }
        // A component is written as a table and nothing else, so a bare
        // value is an error rather than a default the file did not ask for.
        Some(other) => {
            let shown = if other.is_array() {
                "[...]".to_string()
            } else {
                other.to_string()
            };
            let hint = match schema.as_table() {
                Some(t) if other.is_str() && t.contains_key("kind") => {
                    format!("{{ kind = {shown} }}")
                }
                Some(t) if t.len() == 1 => match t.keys().next() {
                    Some(only) => format!("{{ {only} = {shown} }}"),
                    None => "{ property = value }".to_string(),
                },
                _ => "{ property = value }".to_string(),
            };
            bail!("expected a table of properties, got {shown}; write `{hint}`");
        }
        None => {}
    }
    Ok(())
}

/// The full property table an `apply` hook receives: schema defaults, the
/// scene's or script's own values merged over them, and every asset-typed
/// property resolved to a reference.
///
/// [`merge_defaults`] is the half that needs no engine; this is the whole
/// thing, and it is what both entry points into `apply` call.
pub fn properties(
    eng: &Engine,
    schema: &toml::Value,
    params: Option<&toml::Value>,
) -> Result<toml::Value> {
    resolved(eng, schema, merge_defaults(schema, params)?)
}

/// Every asset-typed property of an already-merged table turned into a
/// reference, which is the last thing that happens before `apply` sees it.
pub(super) fn resolved(
    eng: &Engine,
    schema: &toml::Value,
    mut full: toml::Value,
) -> Result<toml::Value> {
    if let Some(table) = full.as_table_mut() {
        resolve_assets(eng, schema, table)?;
    }
    Ok(full)
}

/// A string is a reference; a table is a definition.
///
/// Done here once, so every asset type any plugin ever registers accepts both
/// spellings with no code of its own. An inline table is recorded in the asset
/// cache and replaced by the reference that now names it, which leaves the
/// property table pure TOML and every `apply` hook reading one shape.
fn resolve_assets(
    eng: &Engine,
    schema: &toml::Value,
    out: &mut toml::map::Map<String, toml::Value>,
) -> Result<()> {
    let Some(table) = schema.as_table() else {
        return Ok(());
    };
    for (prop, spec) in table {
        if let Some(value) = out.get_mut(prop) {
            resolve_asset_value(eng, prop, spec, value)?;
        }
    }
    Ok(())
}

/// One value's assets, wherever the spec puts them.
fn resolve_asset_value(
    eng: &Engine,
    prop: &str,
    spec: &toml::Value,
    value: &mut toml::Value,
) -> Result<()> {
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("asset") => {
            let type_name = spec
                .get("asset")
                .and_then(toml::Value::as_str)
                .unwrap_or("");
            if let Some(reference) = asset_reference(eng, prop, type_name, value)? {
                *value = toml::Value::String(reference);
            }
        }
        Some("list" | "map") => {
            if let Some(of) = spec.get("of") {
                for inner in held_mut(value) {
                    resolve_asset_value(eng, prop, of, inner)?;
                }
            }
        }
        Some("record") => {
            for (field, inner) in fields_mut(spec, value) {
                resolve_asset_value(eng, prop, field, inner)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The reference an asset property should carry, or `None` when what it
/// already carries is one.
fn asset_reference(
    eng: &Engine,
    prop: &str,
    type_name: &str,
    value: &toml::Value,
) -> Result<Option<String>> {
    match value {
        // The empty default means "no asset", and resolving it would be an
        // error about a reference the author never wrote.
        toml::Value::String(text) if text.trim().is_empty() => Ok(None),
        // Checked so a typo is reported where it was written, but only warned:
        // a tool must be able to open a scene whose files it cannot reach.
        toml::Value::String(text) => {
            if let Err(why) = crate::assets::definition(eng, text)
                .with_context(|| format!("asset property '{prop}'"))
            {
                tracing::warn!("{why:#}");
            }
            Ok(None)
        }
        // A table's own `type` wins over the schema's: `shape2d.mesh` takes a
        // `mesh` or a `path2d`, and the schema can only name one of them.
        toml::Value::Table(table) => {
            let declared = table
                .get("type")
                .and_then(toml::Value::as_str)
                .unwrap_or(type_name);
            Ok(Some(
                crate::assets::define_inline(eng, declared, value.clone())?.to_string(),
            ))
        }
        other => Err(anyhow!(
            "property '{prop}' is {}; an asset property takes either a reference string or a \
             definition table",
            other.type_str()
        )),
    }
}
