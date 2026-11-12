//! Project manifest and declarative scene files.
//!
//! A project is a directory (Godot-style):
//!
//! ```text
//! project.toml          # name + main scene
//! scenes/main.toml      # node tree
//! scripts/*.bs          # node scripts
//! ```
//!
//! Scene files declare the node tree; behavior lives in scripts. Keys the
//! core does not know are dispatched to plugin-registered handlers, so a
//! plugin can teach scenes new keys (e.g. `shape = "sphere"`).

use std::collections::{BTreeMap, HashMap};

use crate::assets::SceneAsset;
use crate::collections::DetHashMap;
use crate::document_paths::Ids;
use anyhow::{Context, Result, anyhow, bail};
use balaur_script::Value;
use hecs::Entity;
use serde::Deserialize;

use crate::engine::Engine;
use crate::scene::{self, Appearance, Tags};

pub use crate::project_files::{AssetSource, ProjectFiles, path_of};
mod instances;
mod scene_walks;

use crate::vocabulary::keys;
use crate::vocabulary::words;
use instances::{apply_overrides, apply_own_keys, instance};
pub use scene_walks::FileStamps;
pub(crate) use scene_walks::{file_stamps, instantiate_main_scene, preload_scene, unload_scene};

/// The file that makes a directory a project.
pub const MANIFEST: &str = "project.toml";

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
            Self::Portrait => words::window_modes::PORTRAIT,
            Self::Landscape => words::window_modes::LANDSCAPE,
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name {
            words::window_modes::PORTRAIT => Self::Portrait,
            words::window_modes::LANDSCAPE => Self::Landscape,
            _ => Self::Any,
        }
    }
}

/// How a window opens, and what `window.set_window_mode` switches between.
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
            words::window_modes::WINDOWED => Some(Self::Windowed),
            words::window_modes::MAXIMIZED => Some(Self::Maximized),
            words::window_modes::FULLSCREEN => Some(Self::Fullscreen),
            words::window_modes::EXCLUSIVE => Some(Self::Exclusive),
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
    /// Run a frame only when something asks for one, and sleep between.
    pub low_processor: bool,
    /// Open at the size the window last closed at, kept per person.
    pub remember_size: bool,
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
            low_processor: false,
            remember_size: false,
        }
    }
}

impl WindowSettings {
    /// `[window]` as this run resolves it, overrides and all, worked out once
    /// per change of a setting.
    #[must_use]
    pub fn of(eng: &Engine) -> std::rc::Rc<Self> {
        crate::settings::kept(eng, Self::from_settings)
    }

    fn from_settings(eng: &Engine) -> Self {
        let fallback = Self::default();
        Self {
            width: setting_u32(eng, "window/width", fallback.width),
            height: setting_u32(eng, "window/height", fallback.height),
            msaa: setting_u32(eng, "window/msaa", fallback.msaa),
            vsync: setting_bool(eng, "window/vsync", fallback.vsync),
            mode: WindowMode::parse(&setting_string(eng, "window/mode")).unwrap_or(fallback.mode),
            orientation: Orientation::parse(&setting_string(eng, "window/orientation")),
            low_processor: setting_bool(eng, "window/low_processor", fallback.low_processor),
            remember_size: setting_bool(eng, "window/remember_size", fallback.remember_size),
        }
    }
}

/// `[window] max_fps`: the shortest a frame may be, as frames per second.
/// Zero is no cap, and the loop paces itself against the tick instead.
#[must_use]
pub fn max_fps(eng: &Engine) -> u32 {
    setting_u32(eng, "window/max_fps", 0)
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

fn setting_f32(eng: &Engine, path: &str, fallback: f32) -> f32 {
    crate::settings::get(eng, path)
        .as_ref()
        .and_then(toml::Value::as_float)
        .map_or(fallback, |n| n as f32)
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
    /// That theme's form for a dark system; empty keeps `theme`.
    pub theme_dark: String,
    /// How large the game's own UI is drawn, as egui's zoom factor.
    pub scale: f32,
    /// Whether the reader's own preferred text size multiplies that scale.
    pub system_text_size: bool,
    /// Where this project puts the lines between the screen classes.
    pub classes: crate::facts::ClassLines,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            system_fonts: true,
            theme: String::new(),
            theme_dark: String::new(),
            scale: 1.0,
            system_text_size: true,
            classes: crate::facts::ClassLines::default(),
        }
    }
}

impl UiSettings {
    /// `[ui]` as this run resolves it, worked out once per change of a setting.
    #[must_use]
    pub fn of(eng: &Engine) -> std::rc::Rc<Self> {
        crate::settings::kept(eng, Self::from_settings)
    }

    fn from_settings(eng: &Engine) -> Self {
        let fallback = Self::default();
        Self {
            system_fonts: setting_bool(eng, "ui/system_fonts", fallback.system_fonts),
            theme: setting_string(eng, "ui/theme"),
            theme_dark: setting_string(eng, "ui/theme_dark"),
            scale: setting_f32(eng, "ui/scale", fallback.scale),
            system_text_size: setting_bool(eng, "ui/system_text_size", fallback.system_text_size),
            classes: crate::facts::ClassLines {
                narrow_below: setting_f32(
                    eng,
                    "ui/narrow_below_pixels",
                    fallback.classes.narrow_below,
                ),
                wide_from: setting_f32(eng, "ui/wide_from_pixels", fallback.classes.wide_from),
                short_below: setting_f32(
                    eng,
                    "ui/short_below_pixels",
                    fallback.classes.short_below,
                ),
            },
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
}

impl From<RawManifest> for ProjectManifest {
    fn from(raw: RawManifest) -> Self {
        Self {
            name: raw.application.name,
            main_scene: raw.application.main_scene,
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
    z_as_relative: Option<bool>,
    /// When this node and its subtree tick: `pausable`, `always`, `disabled`,
    /// or `inherit` to take the parent's.
    process: Option<String>,
    /// Names the node is filed under, for `scene.tagged`.
    #[serde(default)]
    tags: Vec<String>,
    pub(crate) script: Option<ScriptRef>,
    /// A prefab: another scene file, and this node *is* its root.
    ///
    /// The prefab root's keys, components and script land on this node, under
    /// this node's own, and the root's children are this node's children.
    /// Overrides then name paths from this node, `.` for the node itself.
    instance: Option<String>,
    /// Per-node edits inside the instance, keyed by path from this node:
    /// `[nodes.overrides."Body/Arm"]`. Each holds scene keys, applied after
    /// the prefab is built, in key order.
    #[serde(default)]
    overrides: toml::Table,
    /// Plugin-owned keys, dispatched to scene key handlers.
    #[serde(flatten)]
    pub(crate) extra: HashMap<String, toml::Value>,
}

/// A node's `script`: `{ source, props }`, the path and the properties this
/// node sets.
///
/// `props` holds only what differs from the script's exported defaults, so a
/// changed default reaches every node that did not override it.
#[derive(Deserialize)]
pub(crate) struct ScriptRef {
    /// Absent in an override, which retunes the script the prefab already
    /// gave the node rather than replacing it. A node's own `script` must
    /// name one, and is told so if it does not.
    #[serde(default)]
    source: String,
    #[serde(default)]
    props: toml::Table,
}

impl ScriptRef {
    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    /// The node's overrides, as the host takes them. Order is the table's,
    /// which `toml` keeps sorted, so two runs write the same instance.
    fn props(&self) -> Result<Vec<(String, Value)>> {
        self.props
            .iter()
            .map(|(k, v)| Ok((k.as_str().into(), crate::node_api::from_toml(v)?)))
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

/// Inserted by a host missing a module that owns scene keys: it cannot tell
/// one of those keys from a typo, so a key nothing handles only warns there.
pub struct LacksKeyOwner;

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

/// The project whose content this engine plays when that is not its own: the
/// game the editor edits. Its string catalogues and its fonts are read from
/// here, and the subtree [`Engine::debug_scope`] names draws in them. Each
/// reader keeps the root it read under and reads again when this moves.
pub struct ContentRoot(pub std::path::PathBuf);

/// Play the content of the project at `root`; an empty `root` plays the
/// project's own again.
pub fn set_content_root(eng: &Engine, root: &str) {
    if root.is_empty() {
        eng.remove_resource::<ContentRoot>();
    } else {
        eng.insert_resource(ContentRoot(std::path::PathBuf::from(root)));
    }
}

/// The root of the project whose content this engine plays, when it is not
/// the engine's own.
#[must_use]
pub fn content_root(eng: &Engine) -> Option<std::path::PathBuf> {
    eng.try_resource::<ContentRoot>()
        .map(|root| root.borrow().0.clone())
}

/// The subtree playing the content of another project, when one is played:
/// the editor's mirror of the game.
#[must_use]
pub fn content_scope(eng: &Engine) -> Option<Entity> {
    eng.try_resource::<ContentRoot>()
        .and_then(|_| eng.debug_scope())
}

/// Whether `entity` is content of the project the engine plays rather than
/// its own: it sits in [`content_scope`].
#[must_use]
pub fn in_content(eng: &Engine, entity: Entity) -> bool {
    content_scope(eng).is_some_and(|scope| crate::scene::is_within(&eng.world(), entity, scope))
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
    instantiate_from(eng, source, base, attach_scripts, None)
}

/// Instantiate a scene document whose files are named from another project's
/// `root`, the way the editor builds the game it edits: every path is made
/// absolute against that root, and [`crate::scene_toml::to_toml`] given the
/// same root writes them back as it spells them.
///
/// # Errors
/// If the scene does not build.
pub fn instantiate_scene_from_root(
    eng: &Engine,
    source: &str,
    base: Entity,
    attach_scripts: bool,
    root: &std::path::Path,
) -> Result<()> {
    instantiate_from(eng, source, base, attach_scripts, Some(root))
}

/// Instantiate the scene file at `path`, read the way a prefab is. Use this
/// rather than [`scene_text`] and [`instantiate_scene`]: a scene from a root
/// other than the project's names its files from that root, and only the read
/// knows which root that was.
///
/// # Errors
/// If the file does not read, or the scene does not build.
pub fn instantiate_scene_file(
    eng: &Engine,
    path: &str,
    base: Entity,
    attach_scripts: bool,
) -> Result<()> {
    let (source, doc) = scene_doc(eng, path)?;
    instantiate_doc(eng, &doc, &source, base, attach_scripts)
}

fn instantiate_from(
    eng: &Engine,
    source: &str,
    base: Entity,
    attach_scripts: bool,
    foreign: Option<&std::path::Path>,
) -> Result<()> {
    instantiate_doc(
        eng,
        &parse_scene(eng, source, foreign)?,
        source,
        base,
        attach_scripts,
    )
}

fn instantiate_doc(
    eng: &Engine,
    doc: &SceneDoc,
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
    // A document's paths are the game's spelling by now, whichever script
    // asked for the scene; each script it attaches runs as its own file's.
    let _game = crate::dependencies::as_game(eng);
    build_doc(eng, doc, source, base, &mut build)?;
    children_first(eng, base, &mut build.pending);
    attach_pending(eng, &build)
}

/// Reorder scripts waiting to attach so every child's comes before its
/// parent's and siblings keep their order.
///
/// A child's `init` can only reach a parent that exists, which it does before
/// any script runs; a parent's `init` reading a child's script state is the
/// one that would find nothing, so children go first.
fn children_first(eng: &Engine, base: Entity, pending: &mut [PendingScript]) {
    fn walk(world: &hecs::World, node: Entity, order: &mut DetHashMap<Entity, usize>) {
        if let Ok(children) = world.get::<&scene::Children>(node) {
            for &child in &children.0 {
                walk(world, child, order);
            }
        }
        let next = order.len();
        order.insert(node, next);
    }
    let mut order = DetHashMap::default();
    walk(&eng.world(), base, &mut order);
    pending.sort_by_key(|(entity, _, _)| order.get(entity).copied().unwrap_or(usize::MAX));
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
    /// The node the prefab being built becomes the root of; taken by that
    /// prefab's root.
    merge_into: Option<Entity>,
}

/// Parse and build one scene document under `base`. `foreign` is the root the
/// document came from when that is not the project's, and every path it names
/// is made absolute against it before anything reads one.
/// A scene's text as a document, its paths made the project's where it
/// came from another root.
fn parse_scene(eng: &Engine, source: &str, foreign: Option<&std::path::Path>) -> Result<SceneDoc> {
    Ok(match foreign {
        Some(root) => {
            let mut value: toml::Value = toml::from_str(source).context("parsing scene")?;
            // A scene is never written back through the engine, so an `id://`
            // may be resolved here: the id spelling is lost, not translated.
            crate::document_paths::absolute_in(eng, root, &mut value, Ids::Resolve);
            value.try_into().context("parsing scene")?
        }
        None => toml::from_str(source).context("parsing scene")?,
    })
}

/// Every scene file read and parsed so far, kept while its file is
/// unchanged: a screen of 54 cards built from one prefab reads and parses it
/// once, not 54 times. Checked against the file's modified time on every
/// use, so a save from anywhere -- the editor, a script, another program --
/// is read again; a pack, which never changes, has none and is kept.
#[derive(Default)]
struct SceneFiles(DetHashMap<String, SceneFile>);

struct SceneFile {
    stamp: Option<f64>,
    source: std::rc::Rc<str>,
    doc: std::rc::Rc<SceneDoc>,
}

/// A scene file's text and document, parsed once while the file is unchanged.
fn scene_doc(eng: &Engine, path: &str) -> Result<(std::rc::Rc<str>, std::rc::Rc<SceneDoc>)> {
    let key = path_of(eng, path)?;
    let stamp = stamp_of(eng, &key);
    let files = eng.try_resource::<SceneFiles>().unwrap_or_else(|| {
        eng.insert_resource(SceneFiles::default());
        eng.resource::<SceneFiles>()
    });
    if let Some(file) = files.borrow().0.get(&key)
        && file.stamp == stamp
    {
        return Ok((file.source.clone(), file.doc.clone()));
    }
    let (text, root) = scene_from_file(eng, path)?;
    let doc = std::rc::Rc::new(parse_scene(eng, &text, root.as_deref())?);
    let source: std::rc::Rc<str> = text.into();
    files.borrow_mut().0.insert(
        key,
        SceneFile {
            stamp,
            source: source.clone(),
            doc: doc.clone(),
        },
    );
    Ok((source, doc))
}

/// When the file a scene path reads from last changed, under the first root
/// that has it, as [`scene_from_file`] looks; `None` where none does.
fn stamp_of(eng: &Engine, path: &str) -> Option<f64> {
    let backend = crate::files::backend(eng);
    let roots = crate::file_api::project_roots(eng);
    if roots.is_empty() {
        return backend.mtime(std::path::Path::new(path));
    }
    roots
        .iter()
        .find_map(|root| backend.mtime(&root.join(path)))
}

/// Build a parsed scene under `base`; `source` is its text, which names its
/// `[[assets]]` scope.
fn build_doc(
    eng: &Engine,
    doc: &SceneDoc,
    source: &str,
    base: Entity,
    build: &mut Build,
) -> Result<()> {
    // A scene's `[[assets]]` are in scope only while it is being built, so
    // `#id` never resolves against a sibling scene; a prefab's own blocks nest
    // inside that rather than accumulating.
    crate::variables::declare_from_toml(eng, &doc.variables)?;
    let previous = crate::assets::enter_scene_scope(eng, source, &doc.assets)?;
    let outcome = instantiate_nodes(eng, doc, base, build);
    crate::assets::leave_scene_scope(eng, previous);
    outcome
}

/// A scene file's text: from the pack in a packed run, from disk otherwise —
/// the same resolution an asset document gets.
///
/// # Errors
/// If no root has the file, or it is not UTF-8.
pub fn scene_text(eng: &Engine, path: &str) -> Result<String> {
    Ok(scene_from_file(eng, path)?.0)
}

/// A scene file's text and the root that owns the paths inside it, or `None`
/// for the project's own. Reading is what knows: by the time a node's `mesh`
/// is drawn the scene it came from is long built.
pub(crate) fn scene_from_file(
    eng: &Engine,
    path: &str,
) -> Result<(String, Option<std::path::PathBuf>)> {
    let path = &path_of(eng, path)?;
    // The project's own root, then any a host added. `balaur edit <game>`
    // runs with the editor as the project root, so a scene the game names
    // relative to itself is only found under the game's.
    let backend = crate::files::backend(eng);
    let mut roots = crate::file_api::project_roots(eng);
    if roots.is_empty() {
        roots.push(std::path::PathBuf::new());
    }
    // The script host reads under the project's root, so a relative path it
    // answers is the project's; an absolute one still names the root it is in.
    if let Some(source) = eng.script_host().and_then(|host| host.scene_source(path)) {
        let project = roots.first().cloned().unwrap_or_default();
        return Ok((
            source,
            crate::document_paths::foreign_root(eng, path, &project),
        ));
    }
    let mut last = None;
    for root in &roots {
        match backend.read(&root.join(path)) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes)
                    .with_context(|| format!("scene file '{path}' is not UTF-8"))?;
                return Ok((text, crate::document_paths::foreign_root(eng, path, root)));
            }
            Err(why) => last = Some(why),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("no project root to read it from")))
        .with_context(|| format!("reading scene file '{path}'"))
}

fn attach_pending(eng: &Engine, build: &Build) -> Result<()> {
    // Named either way: a node built with its scripts held still says which
    // script it has, and a save writes it back.
    for (entity, script, props) in &build.pending {
        scene::name_script(eng, *entity, Some((script, props)));
    }
    if !build.attach_scripts {
        return Ok(());
    }
    attach_named(eng, &build.pending)
}

/// Run the scripts `pending` names, every `init` held until all of them are
/// attached.
fn attach_named(eng: &Engine, pending: &[PendingScript]) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("the scene attaches scripts but no script backend is running"))?;
    host.hold_inits();
    let attached = pending.iter().try_for_each(|(entity, script, props)| {
        host.attach_with_props(crate::node_id_of(*entity), script, props)?;
        crate::interpolate::follow_script(eng, *entity, host.as_ref());
        Ok(())
    });
    host.release_inits();
    attached
}

/// Run every script a node under `root` names, the root's included, children
/// first as an instantiate attaches them: what starts a scene that was built
/// with `{ scripts: false }`.
///
/// # Errors
/// If a script does not attach, or no script backend is running.
pub fn attach_scripts(eng: &Engine, root: Entity) -> Result<()> {
    let mut pending: Vec<PendingScript> = {
        let world = eng.world();
        scene::collect_subtree(&world, root)
            .into_iter()
            .filter_map(|entity| {
                let script = world.get::<&scene::NodeScript>(entity).ok()?;
                Some((entity, script.path.clone(), script.props.clone()))
            })
            .collect()
    };
    children_first(eng, root, &mut pending);
    attach_named(eng, &pending)
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
    // The document's root, which a parent path may start from by name even
    // once it has been merged into the node that instanced it.
    let mut scene_root: Option<(&str, Entity)> = None;
    for (index, node) in doc.nodes.iter().enumerate() {
        let parent = resolve_parent(eng, node, root, scene_root, &by_id)?;
        let merged = if node.parent.is_empty() {
            merge_into.take()
        } else {
            None
        };
        // One root, as a Godot scene and a Unity prefab have: it is what a
        // path, a stable id and an instance all address.
        if node.parent.is_empty() && scene_root.is_some() {
            bail!(
                "a scene has one root; '{}' is a second. Put it under the first",
                node.name
            );
        }
        let entity = match merged {
            Some(entity) => entity,
            // The transform is a component, so a node that names none has
            // none. Chosen at the spawn rather than inserted after, which
            // would move every node in the file to another archetype.
            None if node.extra.contains_key(crate::transform::COMPONENT) => {
                scene::spawn_node_with_id(
                    &mut eng.world_mut(),
                    &node.name,
                    parent,
                    format!("{}{}", build.prefix, ids[index]),
                )
            }
            None => scene::spawn_node_bare_with_id(
                &mut eng.world_mut(),
                &node.name,
                parent,
                format!("{}{}", build.prefix, ids[index]),
            ),
        };
        by_id.insert(ids[index].as_str(), entity);
        if node.parent.is_empty() {
            scene_root = Some((node.name.as_str(), entity));
        }
        // The prefab lands first, so the node's own keys win over its root's.
        if let Some(source) = &node.instance {
            instance(eng, node, &ids[index], entity, build)?;
            apply_own_keys(eng, node, entity, handlers, build)?;
            let unmatched = apply_overrides(eng, node, entity, handlers, build);
            eng.world_mut().insert_one(
                entity,
                crate::scene_toml::Instanced {
                    source: source.clone(),
                    unmatched,
                },
            )?;
        } else {
            apply_own_keys(eng, node, entity, handlers, build)?;
        }
    }
    Ok(())
}

/// A node's `process` key, refusing a word that is not a mode rather than
/// silently ticking it the default way.
fn apply_process(eng: &Engine, entity: Entity, named: Option<&str>, node: &str) -> Result<()> {
    let Some(named) = named else {
        return Ok(());
    };
    let mode = crate::process::ProcessMode::parse(named).ok_or_else(|| {
        anyhow::anyhow!("node '{node}' sets process = '{named}', which is not a process mode")
    })?;
    crate::process::set(&mut eng.world_mut(), entity, mode);
    Ok(())
}

/// The keys every node has, which an override may set like any other.
const NODE_KEYS: [&str; 6] = [
    keys::VISIBLE,
    "tint",
    keys::Z_INDEX,
    keys::Z_AS_RELATIVE,
    crate::process::KEY,
    words::settings_api::TAGS,
];

fn apply_node_keys(eng: &Engine, entity: Entity, table: &toml::Table) {
    let world = eng.world();
    let Ok(mut appearance) = world.get::<&mut Appearance>(entity) else {
        return;
    };
    if let Some(on) = table.get(keys::VISIBLE).and_then(toml::Value::as_bool) {
        appearance.visible = on;
    }
    if let Some(colour) = table.get(keys::TINT).and_then(crate::components::rgba) {
        appearance.tint = glamx::Vec4::from(colour);
    }
    if let Some(z) = table.get(keys::Z_INDEX).and_then(toml::Value::as_integer) {
        appearance.z_index = z as i32;
    }
    if let Some(on) = table
        .get(keys::Z_AS_RELATIVE)
        .and_then(toml::Value::as_bool)
    {
        appearance.z_as_relative = on;
    }
    drop(appearance);
    drop(world);
    if let Some(mode) = table
        .get(crate::process::KEY)
        .and_then(toml::Value::as_str)
        .and_then(crate::process::ProcessMode::parse)
    {
        crate::process::set(&mut eng.world_mut(), entity, mode);
    }
    let world = eng.world();
    if let Some(list) = table.get(keys::TAGS).and_then(toml::Value::as_array) {
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
        let below = rest.strip_prefix('/').or(rest.is_empty().then_some(""));
        below.and_then(|below| scene::find_node(&world, entity, below))
    });
    from_root
        .or_else(|| scene::find_node(&world, root, &node.parent))
        .ok_or_else(|| {
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
            Some(words::window_modes::DUPLICATE)
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
        if reason == words::window_modes::DUPLICATE {
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

pub(crate) fn slug(name: &str) -> String {
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
