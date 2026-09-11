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
use hecs::Entity;
use serde::Deserialize;

use crate::engine::Engine;
use crate::scene::{self, Appearance, Tags};

pub use crate::project_files::{AssetSource, ProjectFiles, path_of};

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
    /// Which plugins this project wants. Every module the build linked in
    /// loads unless it is named `false` here.
    pub plugins: BTreeMap<String, PluginChoice>,
    /// `[check]`: how hard `balaur check` is on this project.
    pub check: CheckSettings,
}

/// What `[check]` sets.
///
/// A project that means to stay clean says so once, here, rather than every
/// caller — CI, a hook, a person at a prompt — having to remember a flag.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CheckSettings {
    /// Treat a warning as an error, so the next one to arrive fails the run.
    #[serde(default)]
    pub strict: bool,
}

/// Which way up a phone may hold the game.
///
/// A device decides this before the game runs, so it is written into the
/// export's own manifest rather than read at startup like the rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Any,
    Portrait,
    Landscape,
}

impl Orientation {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name {
            "portrait" => Self::Portrait,
            "landscape" => Self::Landscape,
            _ => Self::Any,
        }
    }
}

/// How a window opens, and what `render.set_window_mode` switches between.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    /// Borderless over the whole screen, at the desktop's own resolution.
    Fullscreen,
    /// The monitor's largest video mode. The web has none, so it is
    /// borderless there.
    Exclusive,
}

impl WindowMode {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "windowed" => Some(Self::Windowed),
            "maximized" => Some(Self::Maximized),
            "fullscreen" => Some(Self::Fullscreen),
            "exclusive" => Some(Self::Exclusive),
            _ => None,
        }
    }
}

/// `[window]`: the window a windowed build opens, and how it is drawn.
///
/// A headless run holds these and opens nothing, so a project states them
/// once and still ticks identically in CI. Read through the settings
/// registry, so `[override.android.window]` answers on a phone.
#[derive(Clone)]
pub struct WindowSettings {
    /// Logical width. The backing store is this times the display's scale,
    /// which is what the render targets are sized from.
    pub width: u32,
    pub height: u32,
    /// Samples per pixel. `1`, the default, is off; `4` is the only other
    /// count the renderer offers, and it costs two render targets of four
    /// samples each — over a hundred megabytes at a retina backing store,
    /// which is why a game asks for it rather than pays for it unasked.
    pub msaa: u32,
    /// Present in step with the display.
    pub vsync: bool,
    /// How it opens. Scripts switch it later through the same state this
    /// seeds, so a game that starts fullscreen and a game that switches into
    /// it take one path.
    pub mode: WindowMode,
    pub orientation: Orientation,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 1000,
            msaa: 1,
            vsync: true,
            mode: WindowMode::Windowed,
            orientation: Orientation::Any,
        }
    }
}

impl WindowSettings {
    /// `[window]` as this run resolves it, overrides and all.
    #[must_use]
    pub fn from_settings(eng: &Engine) -> Self {
        let fallback = Self::default();
        Self {
            width: setting_u32(eng, "window/width", fallback.width),
            height: setting_u32(eng, "window/height", fallback.height),
            msaa: setting_u32(eng, "window/msaa", fallback.msaa),
            vsync: setting_bool(eng, "window/vsync", fallback.vsync),
            mode: WindowMode::parse(&setting_string(eng, "window/mode")).unwrap_or(fallback.mode),
            orientation: Orientation::parse(&setting_string(eng, "window/orientation")),
        }
    }
}

/// The settings registry answers in `toml::Value`; these are the three shapes
/// a manifest key comes back as, each falling back to the schema's own.
fn setting_u32(eng: &Engine, path: &str, fallback: u32) -> u32 {
    crate::settings::get(eng, path)
        .as_ref()
        .and_then(crate::components::as_f64)
        .filter(|n| *n >= 0.0)
        .map_or(fallback, |n| n as u32)
}

fn setting_bool(eng: &Engine, path: &str, fallback: bool) -> bool {
    crate::settings::get(eng, path)
        .as_ref()
        .and_then(toml::Value::as_bool)
        .unwrap_or(fallback)
}

fn setting_string(eng: &Engine, path: &str) -> String {
    crate::settings::get(eng, path)
        .as_ref()
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// `[ui]`: what the UI layer loads before it draws. Read through the settings
/// registry, so a platform may answer differently.
#[derive(Clone)]
pub struct UiSettings {
    /// Append the operating system's own faces to every font chain, so text
    /// in a script balaur does not vendor draws instead of tofu.
    ///
    /// They are the largest files on the machine — one CJK collection runs
    /// to tens of megabytes, and macOS ships three — so a game that only
    /// ever draws the faces it vendors can turn them off and not pay for
    /// them.
    pub system_fonts: bool,
    /// The `widget_theme` every root widget starts from; empty is the
    /// built-in look.
    pub theme: String,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            system_fonts: true,
            theme: String::new(),
        }
    }
}

impl UiSettings {
    /// `[ui]` as this run resolves it.
    #[must_use]
    pub fn from_settings(eng: &Engine) -> Self {
        let fallback = Self::default();
        Self {
            system_fonts: setting_bool(eng, "ui/system_fonts", fallback.system_fonts),
            theme: setting_string(eng, "ui/theme"),
        }
    }
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
    #[serde(default)]
    check: CheckSettings,
}

#[derive(Deserialize)]
struct Application {
    name: String,
    main_scene: String,
    #[serde(default = "default_language")]
    language: String,
}

impl From<RawManifest> for ProjectManifest {
    fn from(raw: RawManifest) -> Self {
        Self {
            name: raw.application.name,
            main_scene: raw.application.main_scene,
            language: raw.application.language,
            plugins: raw.plugins,
            check: raw.check,
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
    /// The manifest as this machine resolves it: its own tags, and any a pack
    /// was built with. What a host reads before an engine exists, so
    /// `[override.ios.plugins] http = false` decides what loads.
    pub fn parse(source: &str) -> Result<Self> {
        let doc: toml::value::Table = toml::from_str(source).context("parsing project.toml")?;
        let mut tags = crate::tags::Tags::current();
        for name in crate::tags::built_in(&doc) {
            tags.push(&name);
        }
        Self::parse_for(source, &tags)
    }

    /// The manifest as `tags` resolve it: `App::load_project` passes the
    /// engine's, so a demo build's `[override.demo.application] main_scene`
    /// is the scene it opens.
    pub fn parse_for(source: &str, tags: &crate::tags::Tags) -> Result<Self> {
        let resolved = crate::settings::resolve(source, tags).context("parsing project.toml")?;
        toml::Value::Table(resolved)
            .try_into()
            .context("parsing project.toml")
    }
}

#[derive(Deserialize)]
struct SceneDoc {
    /// Assets this scene owns, addressable as `#id` from any node in it —
    /// Godot's `[sub_resource]`. See `crate::assets`.
    #[serde(default)]
    assets: Vec<SceneAsset>,
    /// Typed values the scene owns: what a binding's `when` reads and what a
    /// page embedding the game sets. See `crate::variables`.
    #[serde(default)]
    variables: toml::Table,
    #[serde(default)]
    nodes: Vec<SceneNode>,
}

#[derive(Deserialize)]
pub(crate) struct SceneNode {
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
    /// Hides the node and everything under it. Physics is unaffected.
    visible: Option<bool>,
    /// A colour multiplied into what this node and its descendants draw,
    /// as `[r, g, b, a]` or `#rrggbb` / `#rrggbbaa`.
    tint: Option<toml::Value>,
    z_index: Option<i32>,
    /// False makes `z_index` absolute rather than added to the parent's.
    z_relative: Option<bool>,
    /// Names the node is filed under, for `scene.tagged`.
    #[serde(default)]
    tags: Vec<String>,
    pub(crate) script: Option<ScriptRef>,
    /// A prefab: another scene file, built as this node's children.
    ///
    /// The node keeps its own name, transform and components — they are the
    /// instance's, not the prefab's — and the prefab's roots become its
    /// children, which is what `scene::instantiate` does from a script.
    instance: Option<String>,
    /// The node *is* the prefab's one root, as a Godot instance is: the
    /// root's keys, components and script land on this node, under this
    /// node's own, and its children are this node's. Overrides then name
    /// paths from this node, `.` for the node itself.
    #[serde(default)]
    instance_root: bool,
    /// Per-node edits inside the instance, keyed by path from this node:
    /// `[nodes.overrides."Body/Arm"]`. Each holds scene keys, applied after
    /// the prefab is built, in key order.
    #[serde(default)]
    overrides: toml::Table,
    /// Plugin-owned keys, dispatched to scene key handlers.
    #[serde(flatten)]
    pub(crate) extra: HashMap<String, toml::Value>,
}

/// A node's `script`: a path, or a path with the properties this node sets.
///
/// `props` holds only what differs from the script's exported defaults, so a
/// changed default reaches every node that did not override it.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ScriptRef {
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
    pub(crate) fn source(&self) -> &str {
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
        merge_into: None,
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
    /// The node the prefab being built becomes the root of, for an
    /// `instance_root` instance; taken by that prefab's root.
    merge_into: Option<Entity>,
}

/// Parse and build one scene document under `base`.
fn build_scene(eng: &Engine, source: &str, base: Entity, build: &mut Build) -> Result<()> {
    let doc: SceneDoc = toml::from_str(source).context("parsing scene")?;
    // A scene's `[[assets]]` are in scope only while it is being built, so
    // `#id` never resolves against a sibling scene; a prefab's own blocks nest
    // inside that rather than accumulating.
    crate::variables::declare_from_toml(eng, &doc.variables)?;
    let previous = crate::assets::enter_scene_scope(eng, source, &doc.assets)?;
    let outcome = instantiate_nodes(eng, &doc, base, build);
    crate::assets::leave_scene_scope(eng, previous);
    outcome
}

/// A scene file's text: from the pack in a packed run, from disk otherwise —
/// the same resolution an asset document gets.
pub fn scene_text(eng: &Engine, path: &str) -> Result<String> {
    let path = &path_of(eng, path)?;
    if let Some(source) = eng.script_host().and_then(|host| host.scene_source(path)) {
        return Ok(source);
    }
    // The project's own root, then any a host added. `balaur edit <game>`
    // runs with the editor as the project root, so a scene the game names
    // relative to itself is only found under the game's.
    let backend = crate::files::backend(eng);
    let mut roots = crate::file_api::project_roots(eng);
    if roots.is_empty() {
        roots.push(std::path::PathBuf::new());
    }
    let mut last = None;
    for root in &roots {
        match backend.read(&root.join(path)) {
            Ok(bytes) => {
                return String::from_utf8(bytes)
                    .with_context(|| format!("scene file '{path}' is not UTF-8"));
            }
            Err(why) => last = Some(why),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("no project root to read it from")))
        .with_context(|| format!("reading scene file '{path}'"))
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
    // Taken once, by this document's root: a nested prefab asks afresh.
    let mut merge_into = build.merge_into.take();
    let into_node = merge_into.is_some();
    // The document's first root, which a parent path may start from by name
    // even once it has been merged into the node that instanced it.
    let mut scene_root: Option<(&str, Entity)> = None;
    for (index, node) in doc.nodes.iter().enumerate() {
        let parent = resolve_parent(eng, node, root, scene_root, &by_id)?;
        let merged = if node.parent.is_empty() { merge_into.take() } else { None };
        if into_node && merged.is_none() && node.parent.is_empty() {
            bail!(
                "instanced as its root, the prefab has to have one root; '{}' is another",
                node.name
            );
        }
        let entity = match merged {
            Some(entity) => entity,
            // The transform is a component, so a node that names none has
            // none. Chosen at the spawn rather than inserted after, which
            // would move every node in the file to another archetype.
            None if node.extra.contains_key(crate::transform::COMPONENT) => {
                scene::spawn_node(&mut eng.world_mut(), &node.name, parent)
            }
            None => scene::spawn_node_bare(&mut eng.world_mut(), &node.name, parent),
        };
        by_id.insert(ids[index].as_str(), entity);
        if node.parent.is_empty() && scene_root.is_none() {
            scene_root = Some((node.name.as_str(), entity));
        }
        if merged.is_none() {
            eng.world_mut()
                .insert_one(entity, StableId(format!("{}{}", build.prefix, ids[index])))?;
        }
        // A prefab instanced as this node's root lands first, so the node's
        // own keys win over the root's, as a Godot instance's do.
        if node.instance_root && node.instance.is_some() {
            instance(eng, node, &ids[index], entity, build, true)?;
            apply_own_keys(eng, node, entity, handlers, build)?;
            apply_overrides(eng, node, entity, handlers, build);
        } else {
            apply_own_keys(eng, node, entity, handlers, build)?;
            if node.instance.is_some() {
                instance(eng, node, &ids[index], entity, build, false)?;
                apply_overrides(eng, node, entity, handlers, build);
            }
        }
    }
    Ok(())
}

/// Build the prefab `node` names under `entity`, or, `as_root`, into it.
fn instance(
    eng: &Engine,
    node: &SceneNode,
    id: &str,
    entity: Entity,
    build: &mut Build,
    as_root: bool,
) -> Result<()> {
    if as_root {
        build.merge_into = Some(entity);
    }
    let prefab = node.instance.as_deref().unwrap_or_default();
    let outcome = build_instance(eng, node, id, entity, build)
        .with_context(|| format!("instance '{prefab}' on node '{}'", node.name));
    build.merge_into = None;
    outcome
}

/// A node's own keys: what every node has, its tags, its components and its
/// script. A script already pending on the node, a merged prefab root's, is
/// replaced by the node's own.
fn apply_own_keys(
    eng: &Engine,
    node: &SceneNode,
    entity: Entity,
    handlers: &[(String, SceneKeyHandler)],
    build: &mut Build,
) -> Result<()> {
    {
        let world = eng.world();
        // spawn_node inserts an Appearance on every node it creates.
        let mut appearance = world.get::<&mut Appearance>(entity).unwrap();
        if let Some(on) = node.visible {
            appearance.visible = on;
        }
        if let Some(colour) = node.tint.as_ref().and_then(crate::components::rgba) {
            appearance.tint = glamx::Vec4::from(colour);
        }
        if let Some(z) = node.z_index {
            appearance.z_index = z;
        }
        if let Some(on) = node.z_relative {
            appearance.z_relative = on;
        }
    }
    if !node.tags.is_empty() {
        let mut tags = eng
            .world()
            .get::<&Tags>(entity)
            .map(|t| (*t).clone())
            .unwrap_or_default();
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
        build.pending.retain(|(e, _, _)| *e != entity);
        build
            .pending
            .push((entity, script.source().to_string(), props));
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
const NODE_KEYS: [&str; 5] = ["visible", "tint", "z_index", "z_relative", "tags"];

fn apply_node_keys(eng: &Engine, entity: Entity, table: &toml::Table) {
    let world = eng.world();
    let Ok(mut appearance) = world.get::<&mut Appearance>(entity) else {
        return;
    };
    if let Some(on) = table.get("visible").and_then(toml::Value::as_bool) {
        appearance.visible = on;
    }
    if let Some(colour) = table.get("tint").and_then(crate::components::rgba) {
        appearance.tint = glamx::Vec4::from(colour);
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

/// A node's `parent`: the id of an earlier node, or a `/`-separated path of
/// names down from the scene's own root.
///
/// Ids are tried first, so a scene written before paths existed keeps its
/// meaning even where a node's name happens to match some id.
fn resolve_parent(
    eng: &Engine,
    node: &SceneNode,
    root: Entity,
    scene_root: Option<(&str, Entity)>,
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
    let world = eng.world();
    let from_root = scene_root.and_then(|(name, entity)| {
        let rest = node.parent.strip_prefix(name)?;
        (rest.is_empty() || rest.starts_with('/'))
            .then(|| scene::find_node(&world, entity, rest))
            .flatten()
    });
    from_root.or_else(|| scene::find_node(&world, root, &node.parent)).ok_or_else(|| {
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
