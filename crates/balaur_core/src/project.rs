//! Project manifest and declarative scene files.
//!
//! A project is a directory (Godot-style):
//!
//! ```text
//! project.toml          # name + main scene
//! scenes/main.toml      # node tree
//! scripts/*.rn          # node scripts
//! ```
//!
//! Scene files declare the node tree; behavior lives in scripts. Keys the
//! core does not know are dispatched to plugin-registered handlers, so a
//! plugin can teach scenes new keys (e.g. `shape = "ball"`).

use std::collections::{BTreeMap, HashMap};

use crate::assets::SceneAsset;
use crate::collections::DetHashMap;
use crate::components::StableId;
use anyhow::{Context, Result, anyhow, bail};
use balaur_script::Value;
use glamx::{EulerRot, Quat, Vec3};
use hecs::Entity;
use serde::Deserialize;

use crate::engine::Engine;
use crate::scene::{self, Appearance, Tags, Transform};

/// A project's manifest, `project.toml`.
///
/// What the game is lives under `[application]`, so the file reads the way
/// the settings screen addresses it: `application/name` is
/// `[application] name`. `[plugins]` is its own table because it already is
/// one — a map of plugin names to whether they load.
#[derive(Clone)]
pub struct ProjectManifest {
    pub name: String,
    pub main_scene: String,
    /// Which scripting language this project is written in. The assembling
    /// crate maps the name to a backend; core does not know the set.
    pub language: String,
    /// Where a shipped game may read assets from. Only bites once packed;
    /// a dev run always reads the source tree.
    pub assets: AssetSource,
    /// Which plugins this project wants. Every module the build linked in
    /// loads unless it is named `false` here.
    pub plugins: BTreeMap<String, PluginChoice>,
    /// A project-relative picture the runtime shows over the first frames,
    /// on every target; empty shows none.
    pub splash: String,
    /// How long the splash stays, in seconds of engine time.
    pub splash_seconds: f32,
}

/// What a project says about one plugin: whether it wants it, and the
/// settings it hands over.
///
/// A table means "on, with these", read by the plugin through
/// `Registry::config`. Untagged, so `http = false` and
/// `http = { timeout = 5 }` are the same key spelled two ways.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum PluginChoice {
    Wanted(bool),
    Configured(toml::Table),
}

impl PluginChoice {
    /// Only a bare `false` turns a plugin off; a table is an instruction,
    /// not a refusal.
    #[must_use]
    pub fn wanted(&self) -> bool {
        !matches!(self, Self::Wanted(false))
    }

    /// Whether the project asked for it outright, which is what makes an
    /// absent plugin an error rather than a silence.
    #[must_use]
    pub fn asked_for(&self) -> bool {
        match self {
            Self::Wanted(on) => *on,
            Self::Configured(_) => true,
        }
    }

    #[must_use]
    pub fn config(&self) -> Option<&toml::Table> {
        match self {
            Self::Wanted(_) => None,
            Self::Configured(table) => Some(table),
        }
    }
}

/// What each plugin was handed in `[plugins]`, for `Registry::config`.
#[derive(Default)]
pub struct PluginConfigs(pub BTreeMap<String, toml::Table>);

/// The file's shape. Flattened into [`ProjectManifest`] so every reader
/// keeps saying `manifest.name`, and only the parser knows about the table.
#[derive(Deserialize)]
struct RawManifest {
    application: Application,
    #[serde(default)]
    plugins: BTreeMap<String, PluginChoice>,
}

#[derive(Deserialize)]
struct Application {
    name: String,
    main_scene: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    assets: AssetSource,
    #[serde(default)]
    splash: String,
    #[serde(default = "default_splash_seconds")]
    splash_seconds: f32,
}

fn default_splash_seconds() -> f32 {
    1.5
}

impl From<RawManifest> for ProjectManifest {
    fn from(raw: RawManifest) -> Self {
        Self {
            name: raw.application.name,
            main_scene: raw.application.main_scene,
            language: raw.application.language,
            assets: raw.application.assets,
            plugins: raw.plugins,
            splash: raw.application.splash,
            splash_seconds: raw.application.splash_seconds.max(0.0),
        }
    }
}

impl<'de> Deserialize<'de> for ProjectManifest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        RawManifest::deserialize(deserializer).map(Self::from)
    }
}

fn default_language() -> String {
    "rune".to_string()
}

impl ProjectManifest {
    pub fn parse(source: &str) -> Result<Self> {
        toml::from_str(source).context("parsing project.toml")
    }
}

#[derive(Deserialize)]
struct SceneDoc {
    /// Assets this scene owns, addressable as `#id` from any node in it —
    /// Godot's `[sub_resource]`. See `crate::assets`.
    #[serde(default)]
    assets: Vec<SceneAsset>,
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
    /// The parent's `id`, or a `/`-separated path of names from the scene's
    /// root (`"World/Ground"`). Omitted or empty means a root child.
    #[serde(default)]
    parent: String,
    position: Option<[f32; 3]>,
    rotation_euler: Option<[f32; 3]>,
    scale: Option<[f32; 3]>,
    /// Hides the node and everything under it. Physics is unaffected.
    visible: Option<bool>,
    z_index: Option<i32>,
    /// False makes `z_index` absolute rather than added to the parent's.
    z_relative: Option<bool>,
    /// Names the node is filed under, for `scene.tagged`.
    #[serde(default)]
    tags: Vec<String>,
    script: Option<ScriptRef>,
    /// A prefab: another scene file, built as this node's children.
    ///
    /// The node keeps its own name, transform and components — they are the
    /// instance's, not the prefab's — and the prefab's roots become its
    /// children, which is what `scene::instantiate` does from a script.
    instance: Option<String>,
    /// Per-node edits inside the instance, keyed by path from this node:
    /// `[nodes.overrides."Body/Arm"]`. Each holds scene keys, applied after
    /// the prefab is built, in key order.
    #[serde(default)]
    overrides: toml::Table,
    /// Plugin-owned keys, dispatched to scene key handlers.
    #[serde(flatten)]
    extra: HashMap<String, toml::Value>,
}

/// A node's `script`: a path, or a path with the properties this node sets.
///
/// `props` holds only what differs from the script's exported defaults, so a
/// changed default reaches every node that did not override it.
#[derive(Deserialize)]
#[serde(untagged)]
enum ScriptRef {
    Source(String),
    Tuned {
        /// Absent in an override, which retunes the script the prefab already
        /// gave the node rather than replacing it. A node's own `script` must
        /// name one, and is told so if it does not.
        #[serde(default)]
        source: String,
        #[serde(default)]
        props: toml::Table,
    },
}

impl ScriptRef {
    fn source(&self) -> &str {
        match self {
            Self::Source(path) | Self::Tuned { source: path, .. } => path,
        }
    }

    /// The node's overrides, as the host takes them. Order is the table's,
    /// which `toml` keeps sorted, so two runs write the same instance.
    fn props(&self) -> Result<Vec<(String, Value)>> {
        let Self::Tuned { props, .. } = self else {
            return Ok(Vec::new());
        };
        props
            .iter()
            .map(|(k, v)| Ok((k.clone(), crate::node_api::from_toml(v)?)))
            .collect()
    }
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

/// The manifest's own text, kept because `ProjectManifest` is typed and a
/// plugin's table is not one of its fields.
///
/// `ProjectFiles::read("project.toml")` is not the answer: a pack carries the
/// manifest beside the assets rather than among them, so a shipped game would
/// find nothing there and silently take every default.
pub struct ManifestSource(pub String);

/// The manifest text, or `None` before the project has loaded.
#[must_use]
pub fn manifest_source(eng: &Engine) -> Option<String> {
    eng.try_resource::<ManifestSource>()
        .map(|m| m.borrow().0.clone())
}

/// The project directory, as an engine resource (used by `fs`, audio, and
/// any plugin resolving project-relative paths).
pub struct ProjectRoot(pub std::path::PathBuf);

/// Where a project is allowed to read its bytes from, set by `assets` in
/// `project.toml`. The default is deliberately the strict one: a shipped game
/// that quietly falls back to the working directory runs on the machine that
/// built it and nowhere else.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetSource {
    /// The pack only. A miss is an error naming the file.
    #[default]
    Embedded,
    /// The project directory only, ignoring anything packed.
    Files,
    /// The pack first, then the directory — loose DLC, mods, or an override
    /// folder shipped beside the executable.
    #[serde(rename = "embedded+files")]
    EmbeddedThenFiles,
}

/// Where a project's bytes come from: the source tree while developing, the
/// pack itself in a shipped game. Every reader of a binary asset goes through
/// this, which is what lets a packed game ship as one file.
pub struct ProjectFiles {
    root: std::path::PathBuf,
    packed: std::collections::BTreeMap<String, Vec<u8>>,
    source: AssetSource,
    /// Where loose files come from. Held rather than reached for through the
    /// engine, because an asset loader has the files and not the engine.
    fs: std::rc::Rc<dyn crate::files::FileBackend>,
}

impl ProjectFiles {
    /// Reads from the source tree. Nothing is packed, so `assets` cannot
    /// forbid the only source there is.
    #[must_use]
    pub fn directory(root: std::path::PathBuf) -> Self {
        Self {
            root,
            packed: std::collections::BTreeMap::new(),
            source: AssetSource::Files,
            fs: crate::files::default_backend(),
        }
    }

    /// Serves a pack's assets under the project's `assets` rule.
    #[must_use]
    pub fn packed(
        root: std::path::PathBuf,
        assets: std::collections::BTreeMap<String, Vec<u8>>,
        source: AssetSource,
    ) -> Self {
        Self {
            root,
            packed: assets,
            source,
            fs: crate::files::default_backend(),
        }
    }

    /// Serve loose files from `fs` rather than the disk.
    #[must_use]
    pub fn on(mut self, fs: std::rc::Rc<dyn crate::files::FileBackend>) -> Self {
        self.fs = fs;
        self
    }

    #[must_use]
    pub const fn source(&self) -> AssetSource {
        self.source
    }

    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// When the file behind a path last changed, in the backend's own units,
    /// or `None` for one only the pack holds — the pack never changes.
    #[must_use]
    pub fn mtime(&self, path: &str) -> Option<f64> {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            return self.fs.mtime(p);
        }
        if self.source == AssetSource::Embedded {
            return None;
        }
        self.fs.mtime(&self.root.join(p))
    }

    /// The bytes for a project-relative path. An absolute path is read from
    /// disk as given, so a tool pointing outside the project still works.
    ///
    /// # Errors
    /// If no permitted source has the file. The message names every place
    /// that was tried, because "asset not found" without a location is the
    /// least useful sentence a shipped game can print.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            return self.fs.read(p);
        }
        // Separators are normalised because a pack is keyed the way it was
        // built, which is always with forward slashes.
        let key = path.replace('\\', "/");
        let embedded = matches!(
            self.source,
            AssetSource::Embedded | AssetSource::EmbeddedThenFiles
        );
        if embedded && let Some(bytes) = self.packed.get(&key) {
            return Ok(bytes.clone());
        }
        if self.source != AssetSource::Embedded {
            let full = self.root.join(p);
            if let Ok(bytes) = self.fs.read(&full) {
                return Ok(bytes);
            }
            if embedded {
                return Err(anyhow!(
                    "no asset '{path}': not in the pack, and not at {}",
                    full.display()
                ));
            }
            return Err(anyhow!("no asset '{path}': nothing at {}", full.display()));
        }
        Err(anyhow!(
            "no asset '{path}' in the pack. It ships only what `balaur export` \
             collected; set `assets = \"embedded+files\"` in project.toml to also \
             read files beside the game."
        ))
    }

    /// Project-relative paths directly under `dir`, from the pack and from
    /// disk, sorted and deduplicated. A packed game has no directory to walk,
    /// so anything that discovers files by scanning one asks here instead.
    #[must_use]
    pub fn list(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        let mut out: Vec<String> = if self.source == AssetSource::Files {
            Vec::new()
        } else {
            self.packed
                .keys()
                .filter(|k| k.starts_with(&prefix) && !k[prefix.len()..].contains('/'))
                .cloned()
                .collect()
        };
        if self.source != AssetSource::Embedded {
            for (name, is_dir) in self.fs.list(&self.root.join(dir)) {
                if !is_dir {
                    out.push(format!("{prefix}{name}"));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// A node's script, held until the whole tree exists: the node, the path, and
/// the properties the scene set on it.
type PendingScript = (Entity, String, Vec<(String, Value)>);

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
    let mut build = Build {
        prefix: String::new(),
        open: Vec::new(),
        pending: Vec::new(),
        attach_scripts,
    };
    build_scene(eng, source, base, &mut build)?;
    attach_pending(eng, &build)
}

/// One scene being built, and everything a prefab inside it needs to know.
struct Build {
    /// Prepended to every stable id, one `<instance id>/` per enclosing
    /// instance. This is what keeps two instances of one prefab apart, and
    /// what a replay or a replication layer ends up addressing.
    prefix: String,
    /// The prefab files open above this one, so a prefab that contains itself
    /// is an error naming the cycle rather than a hang.
    open: Vec<String>,
    /// Scripts wait for the whole tree — the outermost one, not the prefab —
    /// so `init` can already look up anything the scene declares.
    pending: Vec<PendingScript>,
    attach_scripts: bool,
}

/// Parse and build one scene document under `base`.
fn build_scene(eng: &Engine, source: &str, base: Entity, build: &mut Build) -> Result<()> {
    let doc: SceneDoc = toml::from_str(source).context("parsing scene")?;
    // A scene's `[[assets]]` are in scope only while it is being built, so
    // `#id` never resolves against a sibling scene; a prefab's own blocks nest
    // inside that rather than accumulating.
    let previous = crate::assets::enter_scene_scope(eng, source, &doc.assets)?;
    let outcome = instantiate_nodes(eng, &doc, base, build);
    crate::assets::leave_scene_scope(eng, previous);
    outcome
}

/// A scene file's text: from the pack in a packed run, from disk otherwise —
/// the same resolution an asset document gets.
pub fn scene_text(eng: &Engine, path: &str) -> Result<String> {
    if let Some(source) = eng.script_host().and_then(|host| host.scene_source(path)) {
        return Ok(source);
    }
    let root = eng
        .try_resource::<ProjectRoot>()
        .map(|r| r.borrow().0.clone())
        .unwrap_or_default();
    let full = root.join(path);
    let bytes = crate::files::backend(eng)
        .read(&full)
        .with_context(|| format!("reading scene file '{path}'"))?;
    String::from_utf8(bytes).with_context(|| format!("scene file '{path}' is not UTF-8"))
}

fn attach_pending(eng: &Engine, build: &Build) -> Result<()> {
    if !build.attach_scripts || build.pending.is_empty() {
        return Ok(());
    }
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("the scene attaches scripts but no script backend is running"))?;
    for (entity, script, props) in &build.pending {
        scene::remember_script_props(eng, *entity, props);
        host.attach_with_props(crate::node_id_of(*entity), script, props)?;
    }
    Ok(())
}

fn instantiate_nodes(eng: &Engine, doc: &SceneDoc, base: Entity, build: &mut Build) -> Result<()> {
    let registry = eng
        .try_resource::<SceneKeyRegistry>()
        .ok_or_else(|| anyhow!("scene key registry resource missing"))?;
    let registry = registry.borrow();
    let handlers = &registry.0;
    let root = base;
    let mut by_id: DetHashMap<&str, Entity> = DetHashMap::default();
    let ids = repair_ids(&doc.nodes);
    for (index, node) in doc.nodes.iter().enumerate() {
        let parent = resolve_parent(eng, node, root, &by_id)?;
        let entity = scene::spawn_node(&mut eng.world_mut(), &node.name, parent);
        by_id.insert(ids[index].as_str(), entity);
        eng.world_mut()
            .insert_one(entity, StableId(format!("{}{}", build.prefix, ids[index])))?;
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
            // spawn_node inserts an Appearance on every node it creates.
            let mut appearance = world.get::<&mut Appearance>(entity).unwrap();
            if let Some(on) = node.visible {
                appearance.visible = on;
            }
            if let Some(z) = node.z_index {
                appearance.z_index = z;
            }
            if let Some(on) = node.z_relative {
                appearance.z_relative = on;
            }
        }
        if !node.tags.is_empty() {
            let mut tags = Tags::default();
            for tag in &node.tags {
                tags.add(tag);
            }
            eng.world_mut().insert_one(entity, tags)?;
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
            if script.source().is_empty() {
                bail!("node '{}' has a script key with no source", node.name);
            }
            let props = script
                .props()
                .with_context(|| format!("script properties on node '{}'", node.name))?;
            build
                .pending
                .push((entity, script.source().to_string(), props));
        }
        if let Some(prefab) = &node.instance {
            build_instance(eng, node, &ids[index], entity, build)
                .with_context(|| format!("instance '{prefab}' on node '{}'", node.name))?;
            apply_overrides(eng, node, entity, handlers, build);
        }
    }
    Ok(())
}

/// Build a prefab under the node that names it, with every id inside it
/// prefixed by this node's own.
fn build_instance(
    eng: &Engine,
    node: &SceneNode,
    id: &str,
    entity: Entity,
    build: &mut Build,
) -> Result<()> {
    let prefab = node.instance.as_deref().unwrap_or_default();
    if build.open.iter().any(|open| open == prefab) {
        let mut chain = build.open.clone();
        chain.push(prefab.to_string());
        bail!("a prefab cannot contain itself: {}", chain.join(" -> "));
    }
    let source = scene_text(eng, prefab)?;
    let inner = format!("{}{}/", build.prefix, id);
    let outer = std::mem::replace(&mut build.prefix, inner);
    build.open.push(prefab.to_string());
    let outcome = build_scene(eng, &source, entity, build);
    build.open.pop();
    build.prefix = outer;
    outcome
}

/// Apply this node's `overrides` to the instance it just built.
///
/// A path that no longer names anything is reported and kept: the prefab may
/// have moved a node, and dropping the edit would lose work the file still
/// holds. Everything else is dispatched exactly as a node's own keys are, so
/// an override reaches components and the transform both.
fn apply_overrides(
    eng: &Engine,
    node: &SceneNode,
    entity: Entity,
    handlers: &[(String, SceneKeyHandler)],
    build: &mut Build,
) {
    for (path, table) in &node.overrides {
        let Some(target) = scene::find_node(&eng.world(), entity, path) else {
            tracing::warn!(
                "override '{path}' on node '{}' names nothing inside the instance",
                node.name
            );
            continue;
        };
        let Some(table) = table.as_table() else {
            tracing::warn!(
                "override '{path}' on node '{}' is not a table of scene keys",
                node.name
            );
            continue;
        };
        apply_node_keys(eng, target, table);
        if let Some(script) = table.get("script")
            && let Err(err) = override_script(build, target, script)
        {
            tracing::error!("override '{path}.script' on node '{}': {err:#}", node.name);
        }
        for (key, handler) in handlers {
            let Some(value) = table.get(key) else {
                continue;
            };
            // An override *patches* one property of a component the prefab
            // already described. The scene key handler would rebuild it from
            // the schema defaults, resetting every property it does not name.
            let applied = if crate::components::is_registered(eng, key) {
                crate::components::patch(eng, target, key, value)
            } else {
                handler(eng, target, value)
            };
            if let Err(err) = applied {
                tracing::error!("override '{path}.{key}' on node '{}': {err:#}", node.name);
            }
        }
        for key in table.keys() {
            if key != "script"
                && !NODE_KEYS.contains(&key.as_str())
                && !handlers.iter().any(|(k, _)| k == key)
            {
                tracing::warn!(
                    "override '{path}.{key}' on node '{}' has no registered handler",
                    node.name
                );
            }
        }
    }
}

/// Retune a prefab node's script from the instance that named it: the
/// override's `props` are merged over the ones the prefab set, and its
/// `source` replaces the file.
///
/// This edits the pending attachment rather than the node, because a script
/// is attached once the whole tree exists — which has not happened yet. An
/// override on a node the prefab gave no script to is one nothing can act on,
/// so it says so.
fn override_script(build: &mut Build, target: Entity, value: &toml::Value) -> Result<()> {
    let over: ScriptRef = value.clone().try_into().context("reading the script key")?;
    let props = over.props()?;
    let Some(pending) = build.pending.iter_mut().find(|(e, _, _)| *e == target) else {
        bail!("the node it names has no script to retune");
    };
    if !over.source().is_empty() {
        pending.1 = over.source().to_string();
    }
    for (name, value) in props {
        match pending.2.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => pending.2.push((name, value)),
        }
    }
    Ok(())
}

/// The keys every node has, which an override may set like any other.
const NODE_KEYS: [&str; 7] = [
    "position",
    "rotation_euler",
    "scale",
    "visible",
    "z_index",
    "z_relative",
    "tags",
];

fn apply_node_keys(eng: &Engine, entity: Entity, table: &toml::Table) {
    let world = eng.world();
    if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
        if let Some([x, y, z]) = triple(table.get("position")) {
            transform.position = Vec3::new(x, y, z);
        }
        if let Some([roll, pitch, yaw]) = triple(table.get("rotation_euler")) {
            transform.rotation = Quat::from_euler(EulerRot::ZYX, yaw, pitch, roll);
        }
        if let Some([x, y, z]) = triple(table.get("scale")) {
            transform.scale = Vec3::new(x, y, z);
        }
    }
    let Ok(mut appearance) = world.get::<&mut Appearance>(entity) else {
        return;
    };
    if let Some(on) = table.get("visible").and_then(toml::Value::as_bool) {
        appearance.visible = on;
    }
    if let Some(z) = table.get("z_index").and_then(toml::Value::as_integer) {
        appearance.z_index = z as i32;
    }
    if let Some(on) = table.get("z_relative").and_then(toml::Value::as_bool) {
        appearance.z_relative = on;
    }
    drop(appearance);
    if let Some(list) = table.get("tags").and_then(toml::Value::as_array) {
        let mut tags = Tags::default();
        for tag in list.iter().filter_map(toml::Value::as_str) {
            tags.add(tag);
        }
        drop(world);
        let _ = eng.world_mut().insert_one(entity, tags);
    }
}

fn triple(value: Option<&toml::Value>) -> Option<[f32; 3]> {
    let items = value?.as_array()?;
    if items.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = crate::components::as_f64(item)? as f32;
    }
    Some(out)
}

/// A node's `parent`: the id of an earlier node, or a `/`-separated path of
/// names down from the scene's own root.
///
/// Ids are tried first, so a scene written before paths existed keeps its
/// meaning even where a node's name happens to match some id.
fn resolve_parent(
    eng: &Engine,
    node: &SceneNode,
    root: Entity,
    by_id: &DetHashMap<&str, Entity>,
) -> Result<Entity> {
    if node.parent.is_empty() {
        return Ok(root);
    }
    if let Some(&entity) = by_id.get(node.parent.as_str()) {
        return Ok(entity);
    }
    // `find_node` walks `..`, which here would reparent a prefab's node into
    // the scene that instanced it.
    if node.parent.split('/').any(|segment| segment == "..") {
        bail!(
            "node '{}' names parent '{}': a parent path may not leave its scene with '..'",
            node.name,
            node.parent
        );
    }
    scene::find_node(&eng.world(), root, &node.parent).ok_or_else(|| {
        anyhow!(
            "node '{}' names parent '{}', which no earlier node declares as an id \
             or a path of names",
            node.name,
            node.parent
        )
    })
}

/// Give every node a unique id, repairing what the file got wrong.
///
/// A scene should load even when hand-edited, so a missing or duplicated id is
/// fixed rather than fatal. Generation is by document order and node name, so
/// the same file always yields the same ids — an id that changed between runs
/// would defeat the point of having one. Repairs are logged: the fix is in
/// memory only, and the file still needs saving to make it permanent.
///
/// Missing ids are counted into one line rather than warned about node by
/// node: a scene that parents by path names no ids at all, and a warning per
/// node would bury everything else the load has to say.
fn repair_ids(nodes: &[SceneNode]) -> Vec<String> {
    let mut taken: std::collections::BTreeSet<String> = nodes
        .iter()
        .filter(|n| !n.id.is_empty())
        .map(|n| n.id.clone())
        .collect();
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(nodes.len());
    let mut generated: Vec<&str> = Vec::new();

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
        if reason == "duplicate" {
            tracing::warn!(
                node = %node.name,
                id = %candidate,
                "two scene nodes share an id; the second was given a fresh one"
            );
        } else {
            generated.push(node.name.as_str());
        }
        out.push(candidate);
    }
    if !generated.is_empty() {
        tracing::info!(
            nodes = %generated.join(", "),
            "generated {} scene node id(s); save the scene to keep them",
            generated.len()
        );
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
