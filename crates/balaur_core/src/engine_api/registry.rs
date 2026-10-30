//! What a script asks of the component and preset registries: the types,
//! their tags, schemas and expectations, the presets, and a node's warnings.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

/// The names of every registered component TYPE, not the components on any
/// node. Pairs with `scene.component_schema(name)`.
pub(super) fn component_types(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::components::names(eng)
            .into_iter()
            .map(Value::text)
            .collect(),
    ))
}

/// The facets a component belongs to, for filtering the palette.
pub(super) fn component_tags(eng: &Engine, args: &[Value]) -> Result<Value> {
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let registry = registry.borrow();
    Ok(registry.def(text(args, 0)?).map_or(Value::Nil, |def| {
        Value::List(
            def.tags
                .iter()
                .map(|t| Value::Str((*t).to_string()))
                .collect(),
        )
    }))
}

/// What a component declares it needs something from, for a tool ordering or
/// grouping its sections. `warnings` says whether one node meets it; this
/// answers it about the type.
pub(super) fn component_expects(eng: &Engine, args: &[Value]) -> Result<Value> {
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let registry = registry.borrow();
    Ok(registry.def(text(args, 0)?).map_or(Value::Nil, |def| {
        Value::List(
            def.expects
                .iter()
                .map(|t| Value::Str((*t).to_string()))
                .collect(),
        )
    }))
}

pub(super) fn presets(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::presets::names(eng)
            .into_iter()
            .map(Value::text)
            .collect(),
    ))
}

/// A preset's description, tags and the components it adds.
pub(super) fn preset_info(eng: &Engine, args: &[Value]) -> Result<Value> {
    let name = text(args, 0)?;
    let registry = eng.resource::<crate::presets::PresetRegistry>();
    let registry = registry.borrow();
    let Some(def) = registry.0.get(name) else {
        return Ok(Value::Nil);
    };
    let mut parts = Vec::new();
    for part in &def.parts {
        let params = part.params.as_ref().map(crate::node_api::from_toml);
        let params = params.transpose()?.unwrap_or(Value::Map(Vec::new()));
        parts.push((part.component.clone(), params));
    }
    Ok(Value::Map(vec![
        (
            "description".to_string(),
            Value::Str(def.description.clone()),
        ),
        (
            "tags".to_string(),
            Value::List(def.tags.iter().cloned().map(Value::text).collect()),
        ),
        (
            "components".to_string(),
            Value::List(
                def.parts
                    .iter()
                    .map(|p| Value::Str(p.component.clone()))
                    .collect(),
            ),
        ),
        ("parts".to_string(), Value::Map(parts)),
    ]))
}

pub(super) fn apply_preset(eng: &Engine, args: &[Value]) -> Result<Value> {
    let entity = optional_node(args, 0)?.ok_or_else(|| anyhow!("apply_preset needs a node"))?;
    crate::presets::apply(eng, entity, text(args, 1)?)?;
    Ok(Value::Nil)
}

/// What is off about this node, as a list of `{ component, property, message }`
/// with `property` nil for the component as a whole. Advisory: nothing blocks.
pub(super) fn warnings(eng: &Engine, args: &[Value]) -> Result<Value> {
    let entity = optional_node(args, 0)?.ok_or_else(|| anyhow!("warnings needs a node"))?;
    Ok(Value::List(
        crate::warnings::warnings(eng, entity)
            .into_iter()
            .map(|found| {
                let property = found.warning.property.map_or(Value::Nil, Value::Str);
                Value::Map(vec![
                    ("component".to_string(), Value::Str(found.component)),
                    ("property".to_string(), property),
                    ("message".to_string(), Value::Str(found.warning.message)),
                ])
            })
            .collect(),
    ))
}

pub(super) fn component_schema(eng: &Engine, args: &[Value]) -> Result<Value> {
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let registry = registry.borrow();
    // In the order the schema writes them, which is the order to show them.
    registry.def(text(args, 0)?).map_or(Ok(Value::Nil), |def| {
        Ok(Value::Map(
            def.schema
                .ordered()
                .map(|(name, spec)| Ok((name.into(), crate::node_api::from_toml(spec)?)))
                .collect::<Result<_>>()?,
        ))
    })
}
