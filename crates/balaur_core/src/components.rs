//! The named-component registry: how plugins make their data editable.
//!
//! A plugin registers a component under a name with a *schema* (property
//! names, kinds, defaults — declared as TOML) plus apply/get/remove hooks.
//! Registration buys three things at once:
//!
//! 1. a scene-file key (`body = { kind = "dynamic" }`) applied at
//!    instantiation, in registration order;
//! 2. a runtime Lua API on every node (`node:add_component`,
//!    `set_component`, `get_component`, `remove_component`,
//!    `node:component_names()`), plus `scene.components()` /
//!    `scene.component_schema(name)` for enumeration;
//! 3. editor support for free: the balaur editor builds its "Add component"
//!    list and its property inspectors from the schemas, so third-party
//!    plugin components are addable and editable without editor changes.
//!
//! Property specs (`schema` is a TOML table of `name = { ... }`):
//!   kind = "float" | "bool" | "str" | "enum" | "vec2" | "vec3" | "color"
//!   default = ...          (required)
//!   options = [...]        (enum only)
//!   min/max/step/decimals  (float, optional)

use anyhow::{anyhow, Context, Result};
use hecs::Entity;

use crate::engine::Engine;

/// Read a numeric TOML value as f64, integers included: schemas say
/// "float" but scene authors naturally write `14`, which TOML parses as an
/// integer (`Value::as_float` alone would reject it).
pub fn as_f64(value: &toml::Value) -> Option<f64> {
    match value {
        toml::Value::Float(f) => Some(*f),
        toml::Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

pub struct ComponentDef {
    /// TOML table of property specs (see module docs).
    pub schema: toml::Value,
    /// Insert-or-update the component from a full property table.
    pub apply: Box<dyn Fn(&Engine, Entity, &toml::Value) -> Result<()>>,
    pub remove: Box<dyn Fn(&Engine, Entity) -> Result<()>>,
    /// Current property table, or None when the entity lacks the component.
    pub get: Box<dyn Fn(&Engine, Entity) -> Option<toml::Value>>,
}

impl ComponentDef {
    /// Parse a schema from TOML text (panics on invalid text: schemas are
    /// compile-time constants written by plugin authors).
    pub fn parse_schema(text: &str) -> toml::Value {
        toml::from_str(text).expect("invalid component schema TOML")
    }
}

/// Ordered by registration: scene keys and editor sections follow it.
#[derive(Default)]
pub struct ComponentRegistry(pub Vec<(String, ComponentDef)>);

impl ComponentRegistry {
    pub fn def(&self, name: &str) -> Option<&ComponentDef> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, def)| def)
    }
}

/// Merge `params` over the schema's defaults, producing the full property
/// table `apply` hooks receive. Unknown keys pass through untouched (schemas
/// evolve; scenes may carry newer keys).
pub fn merge_defaults(schema: &toml::Value, params: Option<&toml::Value>) -> toml::Value {
    let mut out = toml::map::Map::new();
    if let Some(table) = schema.as_table() {
        for (prop, spec) in table {
            if let Some(default) = spec.get("default") {
                out.insert(prop.clone(), default.clone());
            }
        }
    }
    match params {
        Some(toml::Value::Table(params)) => {
            for (k, v) in params {
                out.insert(k.clone(), v.clone());
            }
        }
        // Scalar/array shorthand (`body = "fixed"`, `color = [1, 0, 0]`)
        // lands on the prop marked `shorthand = true` in the schema.
        Some(other) => {
            if let Some(table) = schema.as_table() {
                for (prop, spec) in table {
                    if spec.get("shorthand").and_then(|v| v.as_bool()) == Some(true) {
                        out.insert(prop.clone(), other.clone());
                        break;
                    }
                }
            }
        }
        None => {}
    }
    toml::Value::Table(out)
}

pub fn add(engine: &Engine, entity: Entity, name: &str, params: Option<&toml::Value>) -> Result<()> {
    let registry = engine
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let registry = registry.borrow();
    let def = registry
        .def(name)
        .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    let full = merge_defaults(&def.schema, params);
    (def.apply)(engine, entity, &full).with_context(|| format!("applying component '{name}'"))
}

pub fn remove(engine: &Engine, entity: Entity, name: &str) -> Result<()> {
    let registry = engine
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let registry = registry.borrow();
    let def = registry
        .def(name)
        .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    (def.remove)(engine, entity)
}

pub fn get(engine: &Engine, entity: Entity, name: &str) -> Option<toml::Value> {
    let registry = engine.try_resource::<ComponentRegistry>()?;
    let registry = registry.borrow();
    registry.def(name).and_then(|def| (def.get)(engine, entity))
}

pub fn names(engine: &Engine) -> Vec<String> {
    engine
        .try_resource::<ComponentRegistry>()
        .map(|r| r.borrow().0.iter().map(|(n, _)| n.clone()).collect())
        .unwrap_or_default()
}

pub fn present_on(engine: &Engine, entity: Entity) -> Vec<String> {
    let Some(registry) = engine.try_resource::<ComponentRegistry>() else {
        return Vec::new();
    };
    let registry = registry.borrow();
    registry
        .0
        .iter()
        .filter(|(_, def)| (def.get)(engine, entity).is_some())
        .map(|(n, _)| n.clone())
        .collect()
}
