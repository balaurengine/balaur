//! Balaur engine core: the ECS world every plugin builds on.
//!
//! The core owns the ECS world (hecs), the Godot-like scene tree layered on
//! top of it, the frame scheduler, and the plugin API. Scripting lives
//! behind `balaur_script`; a backend crate supplies it.
//! Script hot reloading and script precompilation are core services: every
//! plugin and every game built on Balaur gets them for free.

pub mod app;
pub mod asset_index;
pub mod assets;
mod batteries_api;
pub mod collections;
pub mod components;
#[cfg(not(target_family = "wasm"))]
pub mod dap;
pub mod debug_lines;
pub mod debugger_api;
// Opening a URL or a folder is the OS's job, and a browser tab has neither.
#[cfg(not(target_family = "wasm"))]
pub mod desktop;
pub mod desktop_api;
pub mod digest;
pub mod engine;
pub mod engine_api;
pub mod events;
pub mod facts;
pub mod file_api;
pub mod files;
pub mod geometry2d;
pub mod glb;
pub mod handler;
pub mod heightfield;
pub mod ids;
pub mod logbuf;
pub mod math_api;
pub mod mesh;
pub mod netsession;
pub mod node_api;
pub mod pack;
pub mod plugins;
pub mod presets;
pub mod project;
pub mod replay;
pub mod replay_api;
pub mod resources;
pub mod rng;
pub mod rollback;
pub mod rollback_api;
pub mod save;
pub mod scene;
pub mod settings;
pub mod settings_api;
pub mod skeleton;
pub mod snapshot;
pub mod standalone;
pub mod strings;
pub mod time;
pub mod timers;
pub mod timings;
pub mod transport;
pub mod triangulate;
pub mod voxels;

pub use app::{
    App, AppConfig, FIXED_DT, MAX_SUBSTEPS, ScriptArgs, ScriptHostFactory, ScriptSetup, Stage,
    TICK_HZ,
};
pub use assets::{AssetRef, AssetState, AssetTypeRegistry};
pub use collections::{DetHashMap, DetHashSet};
pub use components::{ComponentDef, ComponentRegistry, StableId};
pub use digest::{Digest, DigestRegistry};
pub use engine::{Command, Engine};
pub use handler::Handler;
pub use ids::IdAllocator;
pub use netsession::{Desync, NetSession};
pub use pack::Pack;
pub use plugins::{PluginInfo, PluginRegistry};
pub use project::{PluginChoice, PluginConfigs};
pub use replay::{ReplayMode, ReplayRegistry};
pub use resources::Resources;
pub use rollback::Session;
pub use scene::{
    Appearance, Children, GlobalAppearance, GlobalTransform, Name, Parent, ScriptAttachment,
    Transform,
};
pub use snapshot::{Snapshot, SnapshotRegistry, SnapshotRing};
pub use transport::{Delivery, Faults, Faulty, LinkState, Received, Transport};

/// Inserted by a windowed backend on the first key, button or touch: the
/// moment a browser lets a page start audio, and the only fact about the
/// user a backend needs to share.
pub struct UserActivation;

pub use glamx;
pub use hecs;

/// Re-hydrate a script's node handle.
///
/// `balaur_script` keeps nodes opaque so it depends on nothing; this is where
/// the bits become an entity again. A stale handle is an error, not a panic:
/// scripts hold handles across frees.
pub fn entity_of(node: balaur_script::NodeId) -> anyhow::Result<hecs::Entity> {
    hecs::Entity::from_bits(node.0).ok_or_else(|| anyhow::anyhow!("stale node handle"))
}

/// The script-facing handle for an entity, the inverse of [`entity_of`].
pub fn node_id_of(entity: hecs::Entity) -> balaur_script::NodeId {
    balaur_script::NodeId(entity.to_bits().get())
}
