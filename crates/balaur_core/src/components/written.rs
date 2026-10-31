//! A component as a scene writes it. The component's own struct is the
//! record of what was asked of it, so a save reads it back through `get` and
//! spells each value the way an author writes one: nothing beside the
//! component remembers the request.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

/// What component `index` of `entity` holds, spelled for a scene: an inline
/// asset as its definition, a colour as `#rrggbb` (`#rrggbbaa` when it is not
/// opaque) where its channels are bytes, and no read-only property, which
/// the engine fills.
pub(crate) fn written_at(eng: &Engine, entity: Entity, index: usize) -> Option<toml::Value> {
    let schema = schema_at(eng, index)?;
    let mut held = get_at(eng, entity, index)?;
    spell_table(eng, &schema, &mut held);
    Some(held)
}

/// [`written_at`] by name, for a script reading a component as a save would
/// write it. `None` when the node does not carry it.
#[must_use]
pub fn written(eng: &Engine, entity: Entity, name: &str) -> Option<toml::Value> {
    let index = index_of(eng, name)?;
    if !attached_of(eng, entity).has(index) {
        return None;
    }
    written_at(eng, entity, index)
}

/// Each component the node carries, by name in registration order, as a
/// scene file writes it: [`written_at`] less every property at its default,
/// which a scene leaves out. A component at its defaults is an empty table.
pub fn written_of(eng: &Engine, entity: Entity) -> Vec<(String, toml::Value)> {
    let Some(registry) = eng.try_resource::<ComponentRegistry>() else {
        return Vec::new();
    };
    let present: Vec<(usize, String, Rc<Schema>)> = {
        let registry = registry.borrow();
        let bits = attached_of(eng, entity);
        registry
            .iter()
            .enumerate()
            .filter(|(index, _)| bits.has(*index))
            .map(|(index, (name, def))| (index, name.to_string(), def.schema.clone()))
            .collect()
    };
    present
        .into_iter()
        .map(|(index, name, schema)| {
            let mut held = get_at(eng, entity, index)
                .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
            // What `add` hands `apply` for a component nobody asked anything
            // of: the defaults through the same passes, so a colour or an asset
            // compares in the shape the component holds it.
            let defaults = properties(eng, &schema, None).ok();
            if let (Some(table), Some(toml::Value::Table(defaults))) =
                (held.as_table_mut(), defaults.as_ref())
            {
                table.retain(|key, value| defaults.get(key).is_none_or(|was| !same(value, was)));
            }
            spell_table(eng, &schema, &mut held);
            (name, held)
        })
        .collect()
}

/// Whether two values are the same, a number within the error an `f32`
/// round trip makes.
pub(crate) fn same(a: &toml::Value, b: &toml::Value) -> bool {
    match (a, b) {
        (toml::Value::Array(a), toml::Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (toml::Value::Table(a), toml::Value::Table(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same(value, other)))
        }
        _ => match (as_f64(a), as_f64(b)) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-5 * x.abs().max(1.0),
            _ => a == b,
        },
    }
}

fn schema_at(eng: &Engine, index: usize) -> Option<Rc<Schema>> {
    let registry = eng.try_resource::<ComponentRegistry>()?;
    let registry = registry.borrow();
    registry.at(index).map(|(_, def)| def.schema.clone())
}

/// Spell every property of a component's table, and drop the read-only ones.
fn spell_table(eng: &Engine, schema: &Schema, held: &mut toml::Value) {
    let (Some(specs), Some(table)) = (schema.as_table(), held.as_table_mut()) else {
        return;
    };
    table.retain(|prop, _| !is_readonly(specs.get(prop)));
    for (prop, value) in table.iter_mut() {
        if let Some(spec) = specs.get(prop.as_str()) {
            spell(eng, spec, value);
        }
    }
}

/// One value as an author writes it, wherever the spec puts it.
fn spell(eng: &Engine, spec: &toml::Value, value: &mut toml::Value) {
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("color") => {
            if let Some(hex) = hex_of(value) {
                *value = toml::Value::String(hex);
            }
        }
        Some("asset") => {
            if let Some(body) = value
                .as_str()
                .and_then(|reference| crate::assets::inline_body(eng, reference))
            {
                *value = body;
            }
        }
        Some("record" | "variant") => {
            for (field, inner) in super::values::fields_mut(spec, value) {
                spell(eng, field, inner);
            }
        }
        Some("list" | "map") => {
            if let Some(of) = spec.get("of") {
                for inner in super::values::held_mut(value) {
                    spell(eng, of, inner);
                }
            }
        }
        _ => {}
    }
}

/// `[r, g, b, a]` as `#rrggbb` or `#rrggbbaa`, where every channel is a byte
/// read the way `hex_rgba` reads one; `None` for any other colour, which
/// stays a list.
pub(crate) fn hex_of(value: &toml::Value) -> Option<String> {
    let channels = value.as_array()?;
    if channels.len() != 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    for (byte, channel) in bytes.iter_mut().zip(channels) {
        let held = as_f64(channel)? as f32;
        let rounded = (f64::from(held) * 255.0).round();
        // Bit for bit: a byte channel reads back exactly what `hex_rgba` made.
        if !(0.0..=255.0).contains(&rounded)
            || ((rounded / 255.0) as f32).to_bits() != held.to_bits()
        {
            return None;
        }
        *byte = rounded as u8;
    }
    let [r, g, b, a] = bytes;
    Some(if a == u8::MAX {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    })
}
