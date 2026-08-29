//! Balaur engine core: the Rust data plane.
//!
//! The core owns the ECS world (hecs), the Godot-like scene tree layered on
//! top of it, the frame scheduler, the plugin API, and the Luau script host.
//! Script hot reloading and script precompilation are core services: every
//! plugin and every game built on Balaur gets them for free.

pub mod app;
pub mod collections;
pub mod components;
pub mod engine;
pub mod logbuf;
pub mod pack;
pub mod project;
pub mod resources;
pub mod scene;
pub mod script;

pub use app::{App, AppConfig, Plugin, Stage};
pub use collections::{DetHashMap, DetHashSet};
pub use components::{ComponentDef, ComponentRegistry};
pub use engine::{Command, Engine};
pub use pack::Pack;
pub use resources::Resources;
pub use scene::{Children, GlobalTransform, Name, Parent, ScriptAttachment, Transform};
pub use script::{LuaModule, NodeRef, ScriptHost};

pub use glamx;
pub use hecs;
pub use mlua;

/// Re-hydrate a script's node handle.
///
/// `balaur_script` keeps nodes opaque so it depends on nothing; this is where
/// the bits become an entity again. A stale handle is an error, not a panic:
/// scripts hold handles across frees.
pub fn entity_of(node: balaur_script::NodeId) -> anyhow::Result<hecs::Entity> {
    hecs::Entity::from_bits(node.0).ok_or_else(|| anyhow::anyhow!("stale node handle"))
}
