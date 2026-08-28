//! Project manifest and declarative scene files.
//!
//! A project is a directory (Godot-style):
//!
//! ```text
//! project.toml          # name + main scene
//! scenes/main.toml      # node tree
//! scripts/*.luau        # node scripts
//! ```
//!
//! Scene files declare the node tree; behavior lives in scripts. Keys the
//! core does not know are dispatched to plugin-registered handlers, so a
//! plugin can teach scenes new vocabulary (e.g. `shape = "ball"`).

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use glamx::{EulerRot, Quat, Vec3};
use hecs::Entity;
use serde::Deserialize;

use crate::engine::Engine;
use crate::scene::{self, Transform};

#[derive(Deserialize, Clone)]
pub struct ProjectManifest {
    pub name: String,
    pub main_scene: String,
}

impl ProjectManifest {
    pub fn parse(source: &str) -> Result<Self> {
        toml::from_str(source).context("parsing project.toml")
    }
}

#[derive(Deserialize)]
struct SceneDoc {
    #[serde(default)]
    nodes: Vec<SceneNode>,
}

#[derive(Deserialize)]
struct SceneNode {
    name: String,
    /// Path relative to the scene root; omitted or empty means a root child.
    #[serde(default)]
    parent: String,
    position: Option<[f32; 3]>,
    rotation_euler: Option<[f32; 3]>,
    scale: Option<[f32; 3]>,
    script: Option<String>,
    /// Plugin-owned keys, dispatched to scene key handlers.
    #[serde(flatten)]
    extra: HashMap<String, toml::Value>,
}

/// A plugin hook for custom scene keys: `(engine, node, value)`.
pub type SceneKeyHandler = Box<dyn Fn(&Engine, Entity, &toml::Value) -> Result<()>>;

/// Handlers with their key, in plugin registration order. Application order
/// must be deterministic, and plugins know the right order for their own
/// keys (e.g. `shape` before `color`). Stored as an engine resource so
/// scenes can also be instantiated at runtime (from scripts, tools, the
/// editor).
#[derive(Default)]
pub struct SceneVocab(pub Vec<(String, SceneKeyHandler)>);

/// The project directory, as an engine resource (used by `fs`, audio, and
/// any plugin resolving project-relative paths).
pub struct ProjectRoot(pub std::path::PathBuf);

/// Instantiate a scene document under `base`. Nodes are created in
/// declaration order; scripts are attached (and `init` runs) after the whole
/// tree exists, so `init` can already look up sibling nodes.
/// `attach_scripts: false` builds the tree and plugin components only, which
/// is what an editor mirroring a foreign project wants.
pub fn instantiate_scene(
    engine: &Engine,
    source: &str,
    base: Entity,
    attach_scripts: bool,
) -> Result<()> {
    let doc: SceneDoc = toml::from_str(source).context("parsing scene")?;
    let vocab = engine
        .try_resource::<SceneVocab>()
        .ok_or_else(|| anyhow!("scene vocabulary resource missing"))?;
    let vocab = vocab.borrow();
    let handlers = &vocab.0;
    let root = base;
    let mut pending_scripts: Vec<(Entity, String)> = Vec::new();
    for node in &doc.nodes {
        let parent = if node.parent.is_empty() {
            root
        } else {
            scene::find_node(&engine.world(), root, &node.parent)
                .ok_or_else(|| anyhow!("unknown parent path '{}'", node.parent))?
        };
        let entity = scene::spawn_node(&mut engine.world_mut(), &node.name, parent);
        {
            let world = engine.world();
            let mut transform = world.get::<&mut Transform>(entity).unwrap();
            if let Some([x, y, z]) = node.position {
                transform.position = Vec3::new(x, y, z);
            }
            if let Some([roll, pitch, yaw]) = node.rotation_euler {
                transform.rotation = Quat::from_euler(EulerRot::ZYX, yaw, pitch, roll);
            }
            if let Some([x, y, z]) = node.scale {
                transform.scale = Vec3::new(x, y, z);
            }
        }
        for (key, handler) in handlers {
            if let Some(value) = node.extra.get(key) {
                handler(engine, entity, value)
                    .with_context(|| format!("scene key '{key}' on node '{}'", node.name))?;
            }
        }
        for key in node.extra.keys() {
            if !handlers.iter().any(|(k, _)| k == key) {
                log::warn!(
                    "scene key '{key}' on node '{}' has no registered handler",
                    node.name
                );
            }
        }
        if let Some(script) = &node.script {
            pending_scripts.push((entity, script.clone()));
        }
    }
    if attach_scripts {
        let host = engine
            .scripts()
            .ok_or_else(|| anyhow!("script host not running"))?;
        for (entity, script) in pending_scripts {
            host.attach(entity, &script)?;
        }
    }
    Ok(())
}
