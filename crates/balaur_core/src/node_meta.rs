//! `meta`: values filed on a node by name, Godot's `set_meta`. State that
//! belongs to the node rather than to the script on it, so a helper with no
//! instance of its own — a static fade, a pool — can find it again.
//!
//! The one component with no schema: every key is the author's, so a scene
//! writes `[nodes.meta] fade_seconds = 0.3`, a script reads
//! `node.meta["fade_seconds"]`, and both go through the same component path
//! every other value on a node takes. Component data is TOML, so a node
//! handle cannot be filed here; a stable id can.

use anyhow::{Result, anyhow};
use hecs::Entity;

use crate::app::App;
use crate::components::ComponentDef;
use crate::engine::Engine;

pub const COMPONENT: &str = "meta";

/// A node's named values, sorted by name as a TOML table is.
#[derive(Clone, Debug, Default)]
pub struct Meta(pub toml::map::Map<String, toml::Value>);

pub(crate) fn register_meta_component(app: &mut App) {
    app.register_component(
        COMPONENT,
        ComponentDef {
            doc: "Named values filed on the node, like Godot's `set_meta`. It has no fixed properties; every key is the author's.",
            schema: ComponentDef::parse_schema(COMPONENT, ""),
            tags: &["interaction"],
            expects: &[],
            apply: Box::new(|eng: &Engine, entity: Entity, params: &toml::Value| {
                let table = params.as_table().cloned().unwrap_or_default();
                let mut world = eng.world_mut();
                if let Ok(mut meta) = world.get::<&mut Meta>(entity) {
                    meta.0 = table;
                    return Ok(());
                }
                world
                    .insert_one(entity, Meta(table))
                    .map_err(|_| anyhow!("node is dead"))?;
                Ok(())
            }),
            remove: Box::new(|eng: &Engine, entity: Entity| {
                let _ = eng.world_mut().remove_one::<Meta>(entity);
                Ok(())
            }),
            get: Box::new(|eng: &Engine, entity: Entity| {
                let world = eng.world();
                let meta = world.get::<&Meta>(entity).ok()?;
                Some(toml::Value::Table(meta.0.clone()))
            }),
        },
    );
}

/// What the node has filed under `name`, nil when it has nothing.
pub fn get(eng: &Engine, entity: Entity, name: &str) -> Option<toml::Value> {
    let world = eng.world();
    let meta = world.get::<&Meta>(entity).ok()?;
    meta.0.get(name).cloned()
}

/// File one value, leaving the rest of the node's alone.
pub fn set(eng: &Engine, entity: Entity, name: &str, value: toml::Value) -> Result<()> {
    let mut table = toml::map::Map::new();
    table.insert(name.to_string(), value);
    crate::components::patch(eng, entity, COMPONENT, &toml::Value::Table(table))
}

/// Drop one value; a name the node never had is left alone.
pub fn remove(eng: &Engine, entity: Entity, name: &str) {
    let world = eng.world_mut();
    if let Ok(mut meta) = world.get::<&mut Meta>(entity) {
        meta.0.remove(name);
    }
}
