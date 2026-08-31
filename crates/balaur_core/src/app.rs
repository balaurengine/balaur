//! The application shell: plugins, staged scheduler, main loop.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::engine::{Command, Engine};
use crate::pack::Pack;
use crate::project::{self, ProjectManifest, ProjectRoot, SceneKeyRegistry};
use crate::scene;

/// Frame stages, run in order every tick.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Input, OS events.
    First,
    /// Core: hot reload pump.
    PreUpdate,
    /// Core: script `update(dt)`. Gameplay systems.
    Update,
    /// Simulation reacting to gameplay (physics step, audio sync).
    PostUpdate,
    /// Core: global transform propagation.
    SceneSync,
    /// Rendering.
    Render,
    /// Core: deferred commands (node destruction).
    Last,
}

const STAGE_COUNT: usize = 7;

pub type SystemFn = Box<dyn FnMut(&Engine, f32)>;

/// CLI arguments exposed to scripts through `engine.args()`.
pub struct ScriptArgs(pub Vec<String>);

/// What a script backend needs to start.
pub struct ScriptSetup<'a> {
    pub engine: &'a Engine,
    pub project_root: &'a Path,
    pub pack: Option<Pack>,
    pub watch: bool,
}

/// Builds the script host for an app.
///
/// Core names no language. The crate assembling the app picks a backend and
/// puts its factory here; `balaur::standard_app` reads `language` from
/// project.toml and installs the matching backend. An app with no
/// factory runs without scripting: binding registrations are discarded and the
/// per-frame script systems do nothing.
pub type ScriptHostFactory =
    Box<dyn FnOnce(ScriptSetup<'_>) -> Result<Rc<dyn balaur_script::ScriptHost<Engine>>>>;

pub struct AppConfig {
    /// Project directory (scripts and scenes are resolved against it). For
    /// packed runs this is only used as a working directory hint.
    pub project_root: PathBuf,
    /// Run from a precompiled pack instead of source files.
    pub pack: Option<Pack>,
    /// Watch the project directory and hot reload scripts automatically.
    pub watch: bool,
    /// Extra arguments handed to scripts via `engine.args()` (e.g. the
    /// project a tool operates on).
    pub script_args: Vec<String>,
    /// Which script backend to run. `None` means no scripting.
    pub script_backend: Option<ScriptHostFactory>,
}

impl AppConfig {
    pub fn dev(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            pack: None,
            watch: true,
            script_args: Vec::new(),
            script_backend: None,
        }
    }

    /// Load a project the way a build tool does: no watcher, no hot reload.
    ///
    /// Export needs a booted app (a statically-resolved language can only
    /// validate a script against the modules its plugins registered), and a
    /// build tool that leaves a file watcher running is a build tool that
    /// never exits.
    pub fn export(project_root: impl Into<PathBuf>) -> Self {
        Self {
            watch: false,
            ..Self::dev(project_root)
        }
    }

    pub fn packed(pack: Pack) -> Self {
        Self {
            project_root: PathBuf::from("."),
            pack: Some(pack),
            watch: false,
            script_args: Vec::new(),
            script_backend: None,
        }
    }
}

/// A plugin wires a subsystem into the app: resources, per-frame systems,
/// script modules, scene keys.
pub trait Plugin {
    fn name(&self) -> &str;
    fn build(&mut self, app: &mut App) -> Result<()>;
}

pub struct App {
    pub engine: Engine,
    systems: Vec<Vec<SystemFn>>,
    plugins: Vec<String>,
    pack: Option<Pack>,
    project_root: PathBuf,
    manifest: Option<ProjectManifest>,
}

impl App {
    pub fn new(mut config: AppConfig) -> Result<Self> {
        let engine = Engine::new();
        engine.insert_resource(SceneKeyRegistry::default());
        engine.insert_resource(crate::components::ComponentRegistry::default());
        engine.insert_resource(crate::assets::AssetTypeRegistry::default());
        engine.insert_resource(crate::assets::AssetState::default());
        engine.insert_resource(ProjectRoot(config.project_root.clone()));
        engine.insert_resource(ScriptArgs(config.script_args.clone()));
        engine.insert_resource(crate::rng::RngState::default());
        if let Some(make) = config.script_backend.take() {
            engine.set_script_host(make(ScriptSetup {
                engine: &engine,
                project_root: &config.project_root,
                pack: config.pack.clone(),
                watch: config.watch,
            })?);
            crate::engine_api::install_engine_api(&engine)?;
        } else {
            tracing::info!("no script backend configured; scripting is off");
        }
        let mut app = Self {
            engine,
            systems: (0..STAGE_COUNT).map(|_| Vec::new()).collect(),
            plugins: Vec::new(),
            pack: config.pack,
            project_root: config.project_root,
            manifest: None,
        };
        app.add_system(Stage::PreUpdate, |eng, _| {
            if let Some(host) = eng.script_host() {
                host.pump_reloads();
            }
        });
        app.add_system(Stage::Update, |eng, dt| {
            if let Some(host) = eng.script_host() {
                host.update(dt);
            }
        });
        app.add_system(Stage::SceneSync, |eng, _| {
            let root = eng.root();
            scene::propagate_transforms(&mut eng.world_mut(), root);
        });
        app.add_system(Stage::Last, |eng, _| {
            for cmd in eng.take_commands() {
                match cmd {
                    Command::Free(entity) => {
                        let subtree = scene::collect_subtree(&eng.world(), entity);
                        if let Some(host) = eng.script_host() {
                            for &e in &subtree {
                                host.detach(crate::node_id_of(e));
                            }
                        }
                        scene::free_subtree(&mut eng.world_mut(), entity);
                    }
                }
            }
        });
        Ok(app)
    }

    pub fn add_plugin(&mut self, mut plugin: impl Plugin) -> Result<&mut Self> {
        plugin
            .build(self)
            .with_context(|| format!("building plugin {}", plugin.name()))?;
        self.plugins.push(plugin.name().to_string());
        Ok(self)
    }

    pub fn add_system(
        &mut self,
        stage: Stage,
        system: impl FnMut(&Engine, f32) + 'static,
    ) -> &mut Self {
        self.systems[stage as usize].push(Box::new(system));
        self
    }

    /// The binding group a plugin registers into, creating it if needed.
    /// With no script backend the registrations go nowhere, since nothing can
    /// call them.
    pub fn script_module(
        &mut self,
        name: &str,
    ) -> Result<Box<dyn balaur_script::Bindings<Engine>>> {
        match self.engine.script_host() {
            Some(host) => host.module(name),
            None => Ok(Box::new(balaur_script::NoBindings)),
        }
    }

    /// Register a named, schema-described component (see
    /// `balaur_core::components`). Also registers the matching scene-file
    /// key, so `name = { ... }` in a scene applies the component.
    pub fn register_component(
        &mut self,
        name: &str,
        def: crate::components::ComponentDef,
    ) -> &mut Self {
        let schema = def.schema.clone();
        {
            let registry = self
                .engine
                .resource::<crate::components::ComponentRegistry>();
            registry.borrow_mut().0.push((name.to_string(), def));
        }
        let component = name.to_string();
        self.scene_key_handler(name, move |eng, entity, value| {
            let full = crate::components::properties(eng, &schema, Some(value))?;
            let registry = eng.resource::<crate::components::ComponentRegistry>();
            let registry = registry.borrow();
            let def = registry
                .def(&component)
                .ok_or_else(|| anyhow::anyhow!("unknown component '{component}'"))?;
            (def.apply)(eng, entity, &full)
        });
        self
    }

    /// Register a parser for one asset type (see `balaur_core::assets`).
    ///
    /// The mirror of [`Self::register_component`]: core learns the name and
    /// nothing else, the parser returns an object only its plugin
    /// understands, and `assets::load_typed` hands it back downcast.
    pub fn register_asset_type(
        &mut self,
        name: &str,
        parse: impl Fn(&toml::Value) -> Result<std::rc::Rc<dyn std::any::Any>> + 'static,
    ) -> &mut Self {
        let registry = self.engine.resource::<crate::assets::AssetTypeRegistry>();
        registry
            .borrow_mut()
            .0
            .push((name.to_string(), Box::new(parse)));
        self
    }

    /// Teach scene files a new key handled by the calling plugin.
    pub fn scene_key_handler(
        &mut self,
        key: &str,
        handler: impl Fn(&Engine, hecs::Entity, &toml::Value) -> Result<()> + 'static,
    ) -> &mut Self {
        let keys = self.engine.resource::<SceneKeyRegistry>();
        keys.borrow_mut()
            .0
            .push((key.to_string(), Box::new(handler)));
        self
    }

    /// Load `project.toml` and instantiate the main scene. Call after all
    /// plugins are added so their scene keys are known.
    pub fn load_project(&mut self) -> Result<&mut Self> {
        let (manifest_src, scene_src);
        if let Some(pack) = &self.pack {
            manifest_src = pack.manifest.clone();
            let manifest = ProjectManifest::parse(&manifest_src)?;
            scene_src = pack
                .scenes
                .get(&manifest.main_scene)
                .cloned()
                .with_context(|| format!("scene {} missing from pack", manifest.main_scene))?;
            self.manifest = Some(manifest);
        } else {
            let path = self.project_root.join("project.toml");
            manifest_src = std::fs::read_to_string(&path)
                .with_context(|| format!("no project.toml in {}", self.project_root.display()))?;
            let manifest = ProjectManifest::parse(&manifest_src)?;
            let scene_path = self.project_root.join(&manifest.main_scene);
            scene_src = std::fs::read_to_string(&scene_path)
                .with_context(|| format!("reading {}", scene_path.display()))?;
            self.manifest = Some(manifest);
        }
        // A resource too, so subsystems can read the project's language and
        // name without reaching back through App.
        if let Some(manifest) = &self.manifest {
            self.engine.insert_resource(manifest.clone());
        }
        let root = self.engine.root();
        project::instantiate_scene(&self.engine, &scene_src, root, true)?;
        Ok(self)
    }

    pub const fn manifest(&self) -> Option<&ProjectManifest> {
        self.manifest.as_ref()
    }

    pub fn project_root(&self) -> &std::path::Path {
        &self.project_root
    }

    /// Run one frame.
    pub fn tick(&mut self, dt: f32) {
        self.engine.advance_time(dt);
        for stage in 0..STAGE_COUNT {
            for system in &mut self.systems[stage] {
                system(&self.engine, dt);
            }
        }
    }

    /// Fixed-cadence main loop (60 Hz), until quit is requested. Rendering
    /// backends may block on vsync inside their Render-stage system; the
    /// sleep below only tops up whatever time is left.
    pub fn run(&mut self) {
        const TARGET: Duration = Duration::from_micros(16_667);
        let mut last = Instant::now();
        while !self.engine.quit_requested() {
            let now = Instant::now();
            let dt = (now - last).as_secs_f32().min(0.1);
            last = now;
            self.tick(dt);
            let elapsed = last.elapsed();
            if elapsed < TARGET {
                // elapsed < TARGET was just checked, so the subtraction cannot underflow.
                std::thread::sleep(TARGET.checked_sub(elapsed).unwrap());
            }
        }
    }
}
