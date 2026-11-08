//! The passes over a component's property table before `apply` sees it:
//! defaults filled in, a scene's values laid over them, records completed,
//! colours expanded and assets resolved to references.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;
use crate::vocabulary::keys;
use crate::vocabulary::words;

/// Every declared field of a `record` value, so a scene naming one of two
/// fields still hands `apply` both. A key no field answers to is dropped:
/// the record's shape is the schema's.
///
/// `props` are the properties holding a record ([`super::Passes::record`]);
/// no other one has anything to fill.
pub(super) fn fill_records(
    schema: &toml::Value,
    props: &[String],
    out: &mut toml::map::Map<String, toml::Value>,
) {
    for prop in props {
        if let (Some(spec), Some(value)) = (schema.get(prop), out.get_mut(prop)) {
            fill_record_value(spec, value);
        }
    }
}

fn fill_record_value(spec: &toml::Value, value: &mut toml::Value) {
    match spec.get(keys::TYPE).and_then(toml::Value::as_str) {
        Some(words::datatypes::RECORD) => {
            // A value that is no table is left for the component's own read
            // to refuse, rather than filled into a record it never was.
            let (Some(fields), Some(held)) = (
                spec.get(keys::FIELDS).and_then(toml::Value::as_table),
                value.as_table().cloned(),
            ) else {
                return;
            };
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
        Some(words::datatypes::VARIANT) => {
            if !value.is_table() {
                return;
            }
            let Some((kind, record)) = schema::active_variant(spec, value) else {
                return;
            };
            fill_record_value(record, value);
            if let Some(table) = value.as_table_mut() {
                table.insert(schema::KIND.into(), toml::Value::String(kind));
            }
        }
        Some(words::datatypes::LIST | words::datatypes::MAP) => {
            if let Some(of) = spec.get(keys::OF) {
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
///
/// `props` are the properties holding a colour ([`super::Passes::color`]).
pub(super) fn expand_colors(
    schema: &toml::Value,
    props: &[String],
    out: &mut toml::map::Map<String, toml::Value>,
) {
    for prop in props {
        if let (Some(spec), Some(value)) = (schema.get(prop), out.get_mut(prop)) {
            expand_color_value(prop, spec, value);
        }
    }
}

/// One value's colours, wherever the spec puts them: a colour held in a list
/// or a record is written the same way as one held by the property itself.
fn expand_color_value(prop: &str, spec: &toml::Value, value: &mut toml::Value) {
    // `type` is the spec's datatype key (see the module docs).
    match spec.get(keys::TYPE).and_then(toml::Value::as_str) {
        Some(words::datatypes::COLOR) => {
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
        Some(words::datatypes::LIST | words::datatypes::MAP) => {
            if let Some(of) = spec.get(keys::OF) {
                for inner in held_mut(value) {
                    expand_color_value(prop, of, inner);
                }
            }
        }
        Some(words::datatypes::RECORD | words::datatypes::VARIANT) => {
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

/// A `record` value's entries beside the spec each field declares, or a
/// `variant`'s beside its active variant's. A key no field answers to is
/// skipped: `validate_property` already refused it, and a variant's `kind`
/// is not a field.
pub(super) fn fields_mut<'a>(
    spec: &'a toml::Value,
    value: &'a mut toml::Value,
) -> Vec<(&'a toml::Value, &'a mut toml::Value)> {
    let record = match spec.get(keys::TYPE).and_then(toml::Value::as_str) {
        Some(words::datatypes::VARIANT) => match schema::active_variant(spec, value) {
            Some((_, record)) => record,
            None => return Vec::new(),
        },
        _ => spec,
    };
    let (Some(fields), Some(table)) = (
        record.get(keys::FIELDS).and_then(toml::Value::as_table),
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
    let passes = Passes::of(schema);
    let mut out = defaults_of(schema);
    overlay(schema, &mut out, params)?;
    fill_records(schema, &passes.record, &mut out);
    expand_colors(schema, &passes.color, &mut out);
    Ok(toml::Value::Table(out))
}

/// Every property the schema declares, at its declared default.
pub(super) fn defaults_of(schema: &toml::Value) -> toml::map::Map<String, toml::Value> {
    let mut out = toml::map::Map::new();
    if let Some(table) = schema.as_table() {
        for (prop, spec) in table {
            if let Some(default) = spec.get(keys::DEFAULT) {
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
                let merged = schema.get(k).and_then(|spec| {
                    // Nothing held yet: a variant's fields fall over its default.
                    over_variant(spec, out.get(k).or_else(|| spec.get(keys::DEFAULT)), v)
                });
                out.insert(k.clone(), merged.unwrap_or_else(|| v.clone()));
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
                Some(t) if other.is_str() && t.contains_key(words::KIND) => {
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

/// Whether a property is a `variant`, whose write is merged over what is held.
pub(super) fn is_variant(spec: &toml::Value) -> bool {
    spec.get(keys::TYPE).and_then(toml::Value::as_str) == Some(words::datatypes::VARIANT)
}

/// A `variant` written over the one held: a table naming no `kind`, or the
/// held one, changes only the fields it names, so a look writing a sphere's
/// `radius` keeps it a sphere. `None` where the value replaces what was
/// held: another kind, or a property of any other type.
pub(super) fn over_variant(
    spec: &toml::Value,
    held: Option<&toml::Value>,
    written: &toml::Value,
) -> Option<toml::Value> {
    if !is_variant(spec) {
        return None;
    }
    let (held, written) = (held?.as_table()?, written.as_table()?);
    let kind = written.get(schema::KIND);
    if kind.is_some_and(|kind| Some(kind) != held.get(schema::KIND)) {
        return None;
    }
    let mut merged = held.clone();
    for (key, value) in written {
        merged.insert(key.clone(), value.clone());
    }
    Some(toml::Value::Table(merged))
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
    let passes = Passes::of(schema);
    let mut out = defaults_of(schema);
    overlay(schema, &mut out, params)?;
    fill_records(schema, &passes.record, &mut out);
    expand_colors(schema, &passes.color, &mut out);
    resolve_assets(eng, schema, &passes.asset, &mut out)?;
    Ok(toml::Value::Table(out))
}

/// The table a whole write of a derived component takes: only what `params`
/// writes, its records filled, colours expanded and assets resolved. The
/// component's own `Default` is what its schema's defaults come from, so the
/// rest need not be written out for it to read back.
pub(crate) fn properties_known(
    eng: &Engine,
    schema: &toml::Value,
    passes: &Passes,
    params: Option<&toml::Value>,
) -> Result<toml::Value> {
    let mut out = toml::map::Map::new();
    overlay(schema, &mut out, params)?;
    fill_records(schema, &passes.record, &mut out);
    expand_colors(schema, &passes.color, &mut out);
    resolve_assets(eng, schema, &passes.asset, &mut out)?;
    Ok(toml::Value::Table(out))
}

/// A string is a reference; a table is a definition.
///
/// Done here once, so every asset type any plugin ever registers accepts both
/// spellings with no code of its own. An inline table is recorded in the asset
/// cache and replaced by the reference that now names it, which leaves the
/// property table pure TOML and every `apply` hook reading one shape.
///
/// `props` are the properties holding an asset ([`super::Passes::asset`]).
pub(super) fn resolve_assets(
    eng: &Engine,
    schema: &toml::Value,
    props: &[String],
    out: &mut toml::map::Map<String, toml::Value>,
) -> Result<()> {
    for prop in props {
        if let (Some(spec), Some(value)) = (schema.get(prop), out.get_mut(prop)) {
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
    match spec.get(keys::TYPE).and_then(toml::Value::as_str) {
        Some(words::datatypes::ASSET) => {
            let type_name = spec
                .get("asset")
                .and_then(toml::Value::as_str)
                .unwrap_or("");
            if let Some(reference) = asset_reference(eng, prop, type_name, value)? {
                *value = toml::Value::String(reference);
            }
        }
        Some(words::datatypes::LIST | words::datatypes::MAP) => {
            if let Some(of) = spec.get(keys::OF) {
                for inner in held_mut(value) {
                    resolve_asset_value(eng, prop, of, inner)?;
                }
            }
        }
        Some(words::datatypes::RECORD | words::datatypes::VARIANT) => {
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
        // Checked so a typo is reported where it was written. A reference that
        // reads nothing is only warned: a tool must be able to open a scene
        // whose files it cannot reach. One that reads is held to its schema.
        toml::Value::String(text) => {
            match crate::assets::written(eng, text) {
                Ok(written) => written
                    .refuse_unknown_keys(eng, type_name)
                    .with_context(|| format!("asset property '{prop}' names '{text}'"))?,
                Err(why) => tracing::warn!("asset property '{prop}': {why:#}"),
            }
            Ok(None)
        }
        // A table's own `type` wins over the schema's: `shape2d.mesh` takes a
        // `mesh` or a `path2d`, and the schema can only name one of them.
        toml::Value::Table(table) => {
            let declared = table
                .get(keys::TYPE)
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
