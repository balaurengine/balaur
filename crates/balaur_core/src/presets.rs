//! Presets: a named set of components applied to one node.
//!
//! A preset is a recipe, not a type. Picking `rigid_body2d` adds `body2d` and
//! `collider2d` to a node and is then forgotten -- the node does not remember
//! which preset made it, and every component stays free to add or remove. That
//! is the whole difference from a class hierarchy, and it is deliberate: an
//! engine that records "this is a RigidBody2D" has to defend that claim
//! forever.
//!
//! One node, several components. Anything spanning several nodes is a scene,
//! which `scene.instantiate` already covers.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};

use crate::components;
use crate::hecs::Entity;
use crate::Engine;

/// One component of a preset, with the parameters that make the preset mean
/// something (a `rigid_body2d`'s body is dynamic; a `static_body2d`'s is not).
#[derive(Debug, Clone)]
pub struct PresetPart {
    pub component: String,
    pub params: Option<toml::Value>,
}

/// What a preset adds, and how to describe it.
#[derive(Debug, Clone)]
pub struct PresetDef {
    /// One line for the picker.
    pub description: String,
    /// Applied in order, so a part may rely on an earlier one existing.
    pub parts: Vec<PresetPart>,
    /// Same facets as components, so one filter drives both lists.
    pub tags: Vec<String>,
}

/// Appended during plugin build and by the project's `presets.toml`;
/// read-only afterwards.
#[derive(Default)]
pub struct PresetRegistry(pub BTreeMap<String, PresetDef>);

/// Build a preset from `(component, params)` pairs.
pub fn preset(
    description: &str,
    tags: &[&str],
    parts: &[(&str, Option<&str>)],
) -> Result<PresetDef> {
    let mut built = Vec::new();
    for (component, params) in parts {
        // A document, not a bare value: `parse` wants the latter and rejects
        // `kind = "dynamic"` outright.
        let params = match params {
            Some(text) => Some(
                toml::from_str::<toml::Value>(text)
                    .map_err(|e| anyhow!("preset '{description}' part '{component}': {e}"))?,
            ),
            None => None,
        };
        built.push(PresetPart {
            component: (*component).to_string(),
            params,
        });
    }
    Ok(PresetDef {
        description: description.to_string(),
        parts: built,
        tags: tags.iter().map(|t| (*t).to_string()).collect(),
    })
}

/// Apply every part of `name` to `entity`.
///
/// Partial application is left in place rather than rolled back: the failure
/// is a bad preset definition, and seeing which part landed is what tells you
/// which one is wrong.
pub fn apply(eng: &Engine, entity: Entity, name: &str) -> Result<()> {
    let registry = eng.resource::<PresetRegistry>();
    let registry = registry.borrow();
    let def = registry
        .0
        .get(name)
        .ok_or_else(|| anyhow!("no preset '{name}'"))?;
    for part in &def.parts {
        components::add(eng, entity, &part.component, part.params.as_ref())
            .map_err(|e| anyhow!("preset '{name}' part '{}': {e}", part.component))?;
    }
    Ok(())
}

/// Preset names, sorted.
pub fn names(eng: &Engine) -> Vec<String> {
    let registry = eng.resource::<PresetRegistry>();
    let registry = registry.borrow();
    registry.0.keys().cloned().collect()
}

/// Every component a node is missing something for, as `(component, expected)`.
///
/// Advisory: a script may add the missing piece on a later tick, so this
/// warns and never blocks. The editor shows it; the runtime ignores it.
pub fn unmet_expectations(eng: &Engine, entity: Entity) -> Vec<(String, Vec<String>)> {
    let present = components::present_on(eng, entity);
    let registry = eng.resource::<components::ComponentRegistry>();
    let registry = registry.borrow();
    let mut unmet = Vec::new();
    for name in &present {
        let Some(def) = registry.def(name) else {
            continue;
        };
        if def.expects.is_empty() {
            continue;
        }
        // Any one of them satisfies it -- `color` needs something to tint,
        // not every kind of thing at once.
        if def.expects.iter().any(|e| present.iter().any(|p| p == e)) {
            continue;
        }
        unmet.push((
            name.clone(),
            def.expects.iter().map(|e| (*e).to_string()).collect(),
        ));
    }
    unmet
}

/// Parse one preset out of `presets.toml`:
///
/// ```toml
/// [enemy]
/// description = "A patrolling enemy"
/// tags = ["2d"]
/// components = [
///   { component = "sprite", texture = "gfx/enemy.png" },
///   { component = "body2d", kind = "dynamic" },
/// ]
/// ```
///
/// Each entry names a `component` and carries that component's own
/// properties inline, so it reads like the scene file it stands in for.
pub fn from_toml(name: &str, body: &toml::Value) -> Result<PresetDef> {
    let table = body
        .as_table()
        .ok_or_else(|| anyhow!("preset '{name}' should be a table"))?;
    let description = table
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or(name)
        .to_string();
    let tags = table
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let entries = table
        .get("components")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("preset '{name}' needs a `components` array"))?;
    let mut parts = Vec::new();
    for entry in entries {
        let entry = entry
            .as_table()
            .ok_or_else(|| anyhow!("preset '{name}': each component is a table"))?;
        let component = entry
            .get("component")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("preset '{name}': a component entry needs `component`"))?;
        // Everything but the discriminant is the component's own properties.
        let mut params = entry.clone();
        params.remove("component");
        parts.push(PresetPart {
            component: component.to_string(),
            params: (!params.is_empty()).then_some(toml::Value::Table(params)),
        });
    }
    Ok(PresetDef {
        description,
        parts,
        tags,
    })
}
