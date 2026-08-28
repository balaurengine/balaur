//! The application shell: plugins, staged scheduler, main loop.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::engine::{Command, Engine};
use crate::pack::Pack;
use crate::project::{self, ProjectManifest, ProjectRoot, SceneVocab};
use crate::scene;
use crate::script::{LuaModule, ScriptHost};

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
}

impl AppConfig {
    pub fn dev(project_root: impl Into<PathBuf>) -> Self {
        AppConfig {
            project_root: project_root.into(),
            pack: None,
            watch: true,
            script_args: Vec::new(),
        }
    }

    pub fn packed(pack: Pack) -> Self {
        AppConfig {
            project_root: PathBuf::from("."),
            pack: Some(pack),
            watch: false,
            script_args: Vec::new(),
        }
    }
}

/// A plugin wires a subsystem into the app: resources, per-frame systems,
/// Lua modules, scene vocabulary.
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
    pub fn new(config: AppConfig) -> Result<Self> {
        let engine = Engine::new();
        engine.insert_resource(SceneVocab::default());
        engine.insert_resource(crate::components::ComponentRegistry::default());
        engine.insert_resource(ProjectRoot(config.project_root.clone()));
        engine.insert_resource(crate::script::ScriptArgs(config.script_args.clone()));
        let host = ScriptHost::new(
            engine.clone(),
            &config.project_root,
            config.pack.clone(),
            config.watch,
        )?;
        engine.set_scripts(host);
        let mut app = App {
            engine,
            systems: (0..STAGE_COUNT).map(|_| Vec::new()).collect(),
            plugins: Vec::new(),
            pack: config.pack,
            project_root: config.project_root,
            manifest: None,
        };
        app.add_system(Stage::PreUpdate, |eng, _| {
            if let Some(host) = eng.scripts() {
                host.pump_reloads();
            }
        });
        app.add_system(Stage::Update, |eng, dt| {
            if let Some(host) = eng.scripts() {
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
                        if let Some(host) = eng.scripts() {
                            for &e in &subtree {
                                host.detach(e);
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

    /// Get the builder for a global Lua module (creating it if needed).
    pub fn lua_module(&mut self, name: &str) -> Result<LuaModule> {
        self.engine
            .scripts()
            .expect("script host always present")
            .module(name)
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
            let full = crate::components::merge_defaults(&schema, Some(value));
            let registry = eng.resource::<crate::components::ComponentRegistry>();
            let registry = registry.borrow();
            let def = registry
                .def(&component)
                .ok_or_else(|| anyhow::anyhow!("unknown component '{component}'"))?;
            (def.apply)(eng, entity, &full)
        });
        self
    }

    /// Teach scene files a new key handled by the calling plugin.
    pub fn scene_key_handler(
        &mut self,
        key: &str,
        handler: impl Fn(&Engine, hecs::Entity, &toml::Value) -> Result<()> + 'static,
    ) -> &mut Self {
        let vocab = self.engine.resource::<SceneVocab>();
        vocab
            .borrow_mut()
            .0
            .push((key.to_string(), Box::new(handler)));
        self
    }

    /// Load `project.toml` and instantiate the main scene. Call after all
    /// plugins are added so their scene keys are known.
    pub fn load_project(&mut self) -> Result<&mut Self> {
        let (manifest_src, scene_src);
        match &self.pack {
            Some(pack) => {
                manifest_src = pack.manifest.clone();
                let manifest = ProjectManifest::parse(&manifest_src)?;
                scene_src = pack
                    .scenes
                    .get(&manifest.main_scene)
                    .cloned()
                    .with_context(|| format!("scene {} missing from pack", manifest.main_scene))?;
                self.manifest = Some(manifest);
            }
            None => {
                let path = self.project_root.join("project.toml");
                manifest_src = std::fs::read_to_string(&path).with_context(|| {
                    format!("no project.toml in {}", self.project_root.display())
                })?;
                let manifest = ProjectManifest::parse(&manifest_src)?;
                let scene_path = self.project_root.join(&manifest.main_scene);
                scene_src = std::fs::read_to_string(&scene_path)
                    .with_context(|| format!("reading {}", scene_path.display()))?;
                self.manifest = Some(manifest);
            }
        }
        let root = self.engine.root();
        project::instantiate_scene(&self.engine, &scene_src, root, true)?;
        Ok(self)
    }

    pub fn manifest(&self) -> Option<&ProjectManifest> {
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
                std::thread::sleep(TARGET - elapsed);
            }
        }
    }
}
