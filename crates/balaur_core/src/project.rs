//! Project manifest and declarative scene files.
//!
//! A project is a directory (Godot-style):
//!
//! ```text
//! project.toml          # name + main scene
//! scenes/main.toml      # node tree
//! scripts/*.luau|*.rn   # node scripts, in the project's language
//! ```
//!
//! Scene files declare the node tree; behavior lives in scripts. Keys the
//! core does not know are dispatched to plugin-registered handlers, so a
//! plugin can teach scenes new keys (e.g. `shape = "ball"`).

use std::collections::HashMap;

use crate::collections::DetHashMap;
use crate::components::StableId;
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
    /// Which scripting language this project is written in. The assembling
    /// crate maps the name to a backend; core does not know the set.
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "luau".to_string()
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
    /// Stable identity, assigned once and never reused.
    ///
    /// `parent` refers to this, so renaming a node cannot silently reparent
    /// its children and two siblings may share a display name. Omitted or
    /// duplicated ids are repaired at load; see [`repair_ids`].
    #[serde(default)]
    id: String,
    name: String,
    /// The parent's `id`. Omitted or empty means a root child.
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
///
/// Byte-identical to a component's `apply`, and `App::register_component`
/// wraps one into the other, so it is the same alias rather than a copy.
pub type SceneKeyHandler = crate::components::ApplyFn;

/// Handlers with their key, in plugin registration order. Application order
/// must be deterministic, and plugins know the right order for their own
/// keys (e.g. `shape` before `color`). Stored as an engine resource so
/// scenes can also be instantiated at runtime (from scripts, tools, the
/// editor).
#[derive(Default)]
pub struct SceneKeyRegistry(pub Vec<(String, SceneKeyHandler)>);

/// The project directory, as an engine resource (used by `fs`, audio, and
/// any plugin resolving project-relative paths).
pub struct ProjectRoot(pub std::path::PathBuf);

/// Instantiate a scene document under `base`. Nodes are created in
/// declaration order; scripts are attached (and `init` runs) after the whole
/// tree exists, so `init` can already look up sibling nodes.
/// `attach_scripts: false` builds the tree and plugin components only, which
/// is what an editor mirroring a foreign project wants.
pub fn instantiate_scene(
    eng: &Engine,
    source: &str,
    base: Entity,
    attach_scripts: bool,
) -> Result<()> {
    let doc: SceneDoc = toml::from_str(source).context("parsing scene")?;
    let registry = eng
        .try_resource::<SceneKeyRegistry>()
        .ok_or_else(|| anyhow!("scene key registry resource missing"))?;
    let registry = registry.borrow();
    let handlers = &registry.0;
    let root = base;
    let mut pending_scripts: Vec<(Entity, String)> = Vec::new();
    let mut by_id: DetHashMap<&str, Entity> = DetHashMap::default();
    let ids = repair_ids(&doc.nodes);
    for (index, node) in doc.nodes.iter().enumerate() {
        let parent = if node.parent.is_empty() {
            root
        } else {
            *by_id.get(node.parent.as_str()).ok_or_else(|| {
                anyhow!(
                    "node '{}' names parent id '{}', which no earlier node declares",
                    node.name,
                    node.parent
                )
            })?
        };
        let entity = scene::spawn_node(&mut eng.world_mut(), &node.name, parent);
        by_id.insert(ids[index].as_str(), entity);
        eng.world_mut()
            .insert_one(entity, StableId(ids[index].clone()))?;
        {
            let world = eng.world();
            // spawn_node inserts a Transform on every node it creates.
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
                handler(eng, entity, value)
                    .with_context(|| format!("scene key '{key}' on node '{}'", node.name))?;
            }
        }
        for key in node.extra.keys() {
            if !handlers.iter().any(|(k, _)| k == key) {
                tracing::warn!(
                    "scene key '{key}' on node '{}' has no registered handler",
                    node.name
                );
            }
        }
        if let Some(script) = &node.script {
            pending_scripts.push((entity, script.clone()));
        }
    }
    if attach_scripts && !pending_scripts.is_empty() {
        let host = eng.script_host().ok_or_else(|| {
            anyhow!("the scene attaches scripts but no script backend is running")
        })?;
        for (entity, script) in pending_scripts {
            host.attach(crate::node_id_of(entity), &script)?;
        }
    }
    Ok(())
}

/// Give every node a unique id, repairing what the file got wrong.
///
/// A scene should load even when hand-edited, so a missing or duplicated id is
/// fixed rather than fatal. Generation is by document order and node name, so
/// the same file always yields the same ids — an id that changed between runs
/// would defeat the point of having one. Repairs are logged: the fix is in
/// memory only, and the file still needs saving to make it permanent.
fn repair_ids(nodes: &[SceneNode]) -> Vec<String> {
    let mut taken: std::collections::BTreeSet<String> = nodes
        .iter()
        .filter(|n| !n.id.is_empty())
        .map(|n| n.id.clone())
        .collect();
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(nodes.len());

    for node in nodes {
        let reason = if node.id.is_empty() {
            Some("missing")
        } else if !seen.insert(node.id.as_str()) {
            Some("duplicate")
        } else {
            None
        };
        let Some(reason) = reason else {
            out.push(node.id.clone());
            continue;
        };
        let base = slug(&node.name);
        let mut candidate = base.clone();
        let mut n = 2;
        while !taken.insert(candidate.clone()) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        tracing::warn!(
            node = %node.name,
            id = %candidate,
            reason,
            "generated a scene node id; save the scene to keep it"
        );
        out.push(candidate);
    }
    out
}

fn slug(name: &str) -> String {
    let mut s = String::from("n_");
    let mut last_underscore = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.extend(c.to_lowercase());
            last_underscore = false;
        } else if !last_underscore {
            s.push('_');
            last_underscore = true;
        }
    }
    let trimmed = s.trim_end_matches('_');
    if trimmed.len() > 2 {
        trimmed.to_string()
    } else {
        String::from("n_node")
    }
}
