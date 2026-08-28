//! Balaur engine core: the Rust data plane.
//!
//! The core owns the ECS world (hecs), the Godot-like scene tree layered on
//! top of it, the frame scheduler, the plugin API, and the Luau script host.
//! Script hot reloading and script precompilation are core services: every
//! plugin and every game built on Balaur gets them for free.

pub mod app;
pub mod collections;
pub mod components;
pub mod logbuf;
pub mod engine;
pub mod pack;
pub mod project;
pub mod resources;
pub mod scene;
pub mod script;

pub use app::{App, AppConfig, Plugin, Stage};
pub use engine::{Command, Engine};
pub use pack::Pack;
pub use collections::{DetHashMap, DetHashSet};
pub use components::{ComponentDef, ComponentRegistry};
pub use resources::Resources;
pub use scene::{Children, GlobalTransform, Name, Parent, ScriptAttachment, Transform};
pub use script::{LuaModule, NodeRef, ScriptHost};

pub use hecs;
pub use mlua;
pub use glamx;
