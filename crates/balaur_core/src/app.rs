//! The application shell: plugins, staged scheduler, main loop.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::time::Instant;

use anyhow::{Context, Result};

use crate::engine::{Command, Engine};
use crate::pack::Pack;
use crate::plugins::{PluginInfo, PluginRegistry};
use crate::project::{self, ProjectManifest, ProjectRoot, SceneKeyRegistry};
use crate::scene;

/// Frame stages, run in order every tick.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Input, OS events.
    First,
    /// Core: hot reload pump.
    PreUpdate,
    /// Core: script `update(dt)`. Presentation and gameplay systems that
    /// tolerate a variable step.
    Update,
    /// The simulation: script `fixed_update(dt)`, then the physics step.
    ///
    /// Runs zero or more times per frame, always at [`FIXED_DT`], driven by
    /// one accumulator the app owns. A system here must never read the
    /// frame's measured time — that is what makes the tick reproducible.
    FixedUpdate,
    /// Reacting to the simulation (audio sync).
    PostUpdate,
    /// Core: global transform propagation.
    SceneSync,
    /// Rendering.
    Render,
    /// Core: deferred commands (node destruction).
    Last,
}

const STAGE_COUNT: usize = 8;

/// The simulation's tick rate. One declaration, because two independently
/// written 60ths of a second are equal today and a desync the day one moves.
pub const TICK_HZ: u32 = 60;

/// The fixed simulation step. Physics, animation and `App::set_fixed_dt` all
/// take their step from here.
pub const FIXED_DT: f32 = 1.0 / TICK_HZ as f32;

/// How far behind a frame may fall before time is dropped rather than caught
/// up on. Without it a stalled frame spends its recovery in a spiral of
/// catch-up steps.
pub const MAX_SUBSTEPS: u32 = 4;

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
        Self::bare(project_root)
    }

    /// An engine and nothing else: no pack, no watcher, no script backend.
    /// What a test loads one plugin into.
    pub fn bare(project_root: impl Into<PathBuf>) -> Self {
        Self {
            watch: false,
            ..Self::dev(project_root)
        }
    }

    pub fn packed(pack: Pack) -> Self {
        Self {
            pack: Some(pack),
            ..Self::bare(".")
        }
    }
}

pub struct App {
    pub engine: Engine,
    systems: Vec<Vec<SystemFn>>,
    pack: Option<Pack>,
    project_root: PathBuf,
    manifest: Option<ProjectManifest>,
    fixed_dt: Option<f32>,
    accumulator: f32,
}

/// The typemap entries every app has, whatever plugins it goes on to add.
///
/// Its own function because `App::new` is otherwise a list of twenty-odd
/// inserts with the interesting work at the bottom.
fn insert_core_resources(eng: &Engine, config: &AppConfig) {
    eng.insert_resource(SceneKeyRegistry::default());
    eng.insert_resource(crate::components::ComponentRegistry::default());
    eng.insert_resource(crate::plugins::PluginRegistry::default());
    eng.insert_resource(crate::presets::PresetRegistry::default());
    eng.insert_resource(crate::assets::AssetTypeRegistry::default());
    eng.insert_resource(crate::assets::AssetState::default());
    eng.insert_resource(ProjectRoot(config.project_root.clone()));
    // A packed game serves its textures, sounds and fonts from the pack;
    // a dev run serves them from the source tree.
    eng.insert_resource(
        match config.pack.as_ref() {
            Some(pack) => crate::project::ProjectFiles::packed(
                config.project_root.clone(),
                pack.assets.clone(),
                crate::project::ProjectManifest::parse(&pack.manifest)
                    .map(|m| m.assets)
                    .unwrap_or_default(),
            ),
            None => crate::project::ProjectFiles::directory(config.project_root.clone()),
        }
        .on(crate::files::backend(eng)),
    );
    eng.insert_resource(ScriptArgs(config.script_args.clone()));
    eng.insert_resource(crate::rng::RngState::default());
    eng.insert_resource(crate::ids::IdAllocator::default());
    eng.insert_resource(crate::settings::SettingsRegistry::default());
    eng.insert_resource(crate::settings::SettingsValues::default());
    eng.insert_resource(crate::netsession::PeerTraffic::default());
    eng.insert_resource(crate::netsession::SessionStats::default());
    eng.insert_resource(crate::rollback::TickInputs::default());
    eng.insert_resource(crate::rollback::Resimulating::default());
    eng.insert_resource(crate::rollback::Clock::default());
    eng.insert_resource(crate::events::EventState::default());
    eng.insert_resource(crate::digest::DigestRegistry::default());
    eng.insert_resource(crate::replay::ReplayRegistry::default());
    eng.insert_resource(crate::replay::ReplaySetupRegistry::default());
    eng.insert_resource(crate::timings::Timings::default());
    eng.insert_resource(crate::strings::Strings::default());
    eng.insert_resource(crate::replay::ReplayFeed::default());
    eng.insert_resource(crate::replay::ReplayMode::default());
    eng.insert_resource(crate::replay::Recording::default());
    eng.insert_resource(crate::replay::ReplayPlayer::default());
    eng.insert_resource(crate::replay::EventLog::default());
    eng.insert_resource(crate::snapshot::SnapshotRegistry::default());
}

/// The facts and timers every app carries: recorded as replay sources so a
/// replay answers as the original run did, and timers in the snapshot.
fn register_facts(app: &mut App) {
    app.engine.insert_resource(crate::facts::Facts::default());
    app.engine
        .insert_resource(crate::facts::WallClock::default());
    app.engine.insert_resource(crate::timers::Timers::default());
    app.engine.insert_resource(crate::facts::Device::default());
    app.add_replay_source(
        "device",
        |eng| serde_json::to_value(crate::facts::device(eng)).unwrap_or_default(),
        |eng, value| {
            if let Ok(facts) = serde_json::from_value::<crate::facts::DeviceFacts>(value.clone()) {
                eng.resource::<crate::facts::Device>().borrow_mut().now = facts;
            }
        },
    );
    app.add_replay_source(
        "wall_clock",
        |eng| {
            serde_json::to_value(*eng.resource::<crate::facts::WallClock>().borrow())
                .unwrap_or_default()
        },
        |eng, value| {
            if let Ok(clock) = serde_json::from_value::<crate::facts::WallClock>(value.clone()) {
                *eng.resource::<crate::facts::WallClock>().borrow_mut() = clock;
            }
        },
    );
    app.add_replay_setup(
        "platform",
        |eng| serde_json::to_value(crate::facts::platform(eng)).unwrap_or_default(),
        |eng, value| {
            if let Ok(facts) = serde_json::from_value::<crate::facts::PlatformFacts>(value.clone())
            {
                eng.resource::<crate::facts::Facts>().borrow_mut().0 = Some(facts);
            }
        },
    );
    app.add_snapshot_source(
        "timers",
        crate::timers::save_timers,
        crate::timers::load_timers,
    );
}

impl App {
    pub fn new(mut config: AppConfig) -> Result<Self> {
        let engine = Engine::new();
        insert_core_resources(&engine, &config);
        match config.script_backend.take() {
            Some(make) => {
                engine.set_script_host(make(ScriptSetup {
                    engine: &engine,
                    project_root: &config.project_root,
                    pack: config.pack.clone(),
                    watch: config.watch,
                })?);
                crate::engine_api::install_engine_api(&engine)?;
            }
            _ => {
                tracing::info!("no script backend configured; scripting is off");
            }
        }
        let mut app = Self {
            engine,
            systems: (0..STAGE_COUNT).map(|_| Vec::new()).collect(),
            pack: config.pack,
            project_root: config.project_root,
            manifest: None,
            fixed_dt: None,
            accumulator: 0.0,
        };
        // Geometry is core content, not a rendering concern: physics reads
        // the same asset for its trimesh and convex-hull colliders, and it
        // does not depend on the render crate.
        crate::mesh::register_mesh_asset(&mut app);
        crate::heightfield::register_heightfield_asset(&mut app);
        crate::voxels::register_voxels_asset(&mut app);
        // A bone is scene-tree data the same way: rendering skins with it,
        // and nothing about a rest pose belongs to the renderer.
        crate::skeleton::register_bone2d_component(&mut app);
        crate::skeleton::register_bone3d_component(&mut app);
        crate::snapshot::build_core_sources(&mut app);
        crate::netsession::build_session_source(&mut app);
        register_facts(&mut app);
        crate::settings::build_core_settings(&app.engine);
        // Before every plugin's First work, so a subsystem that dispatches
        // incoming traffic there sees the recording rather than the network.
        app.add_system(Stage::First, |eng, _| {
            let feed = eng.resource::<crate::replay::ReplayFeed>();
            let frame = feed.borrow().0.clone();
            if let Some(frame) = frame {
                crate::replay::restore(eng, &frame);
            }
        });
        app.add_system(Stage::First, crate::facts::read_clock_system);
        app.add_system(Stage::First, crate::facts::announce_device_system);
        app.add_system(Stage::FixedUpdate, crate::timers::step_timers_system);
        app.add_system(Stage::PreUpdate, |eng, _| {
            if let Some(host) = eng.script_host() {
                crate::timings::measure(eng, "scripts/reload", || host.pump_reloads());
            }
        });
        // Before the script tick, so an event emitted last frame reaches its
        // handler at a point where nothing is mid-iteration.
        app.add_system(Stage::Update, crate::events::pump_system);
        app.add_system(Stage::Update, |eng, dt| {
            if let Some(host) = eng.script_host() {
                crate::timings::measure(eng, "scripts/update", || host.update(dt));
            }
        });
        // First in the stage, so a force a script applies lands on the step
        // that physics is about to take.
        app.add_system(Stage::FixedUpdate, |eng, dt| {
            if let Some(host) = eng.script_host() {
                crate::timings::measure(eng, "scripts/fixed_update", || host.fixed_update(dt));
            }
        });
        app.add_system(Stage::SceneSync, |eng, _| {
            crate::timings::measure(eng, "scene/transforms", || {
                let root = eng.root();
                scene::propagate_transforms(&mut eng.world_mut(), root);
            });
        });
        app.add_system(Stage::Last, |eng, _| {
            // Every free of the frame in one batch: see `scene::free_nodes`.
            let mut freed = Vec::new();
            for cmd in eng.take_commands() {
                match cmd {
                    Command::Free(entity) => {
                        let label = crate::digest::node_label(&eng.world(), entity);
                        crate::replay::event(
                            eng,
                            "scene.free",
                            format!("freed {label}"),
                            Some(serde_json::json!({ "node": label })),
                        );
                        freed.push(entity);
                    }
                }
            }
            if !freed.is_empty() {
                scene::free_nodes(eng, &freed);
            }
        });
        // After deferred destruction, so a recorded digest describes the
        // world the next tick starts from, and after every plugin's Last work
        // so nothing this frame produced is left out of the events.
        app.add_system(Stage::Last, crate::replay::record_frame_system);
        Ok(app)
    }

    /// Note that a plugin finished registering.
    ///
    /// Called once the plugin's own work succeeded, so a plugin that failed
    /// is not one another can claim to require.
    pub fn record_plugin(&mut self, info: PluginInfo) {
        self.engine
            .resource::<PluginRegistry>()
            .borrow_mut()
            .0
            .push(info);
    }

    /// Every plugin that registered, in load order.
    #[must_use]
    pub fn plugins(&self) -> Vec<PluginInfo> {
        crate::plugins::loaded(&self.engine)
    }

    /// Run the simulation at a fixed step, ignoring wall-clock jitter.
    ///
    /// The deterministic mode: a slow frame makes the simulation fall behind
    /// real time rather than take a bigger step, because a bigger step is a
    /// different simulation. Off by default — a variable step is smoother
    /// for a single-player game that never records or networks anything.
    /// The step one [`App::tick`] is worth: `set_fixed_dt` where a game set
    /// one, [`FIXED_DT`] otherwise.
    ///
    /// A rollback session drives at exactly this, so the substep accumulator
    /// is back at zero every time it captures.
    #[must_use]
    pub fn fixed_step(&self) -> f32 {
        self.fixed_dt.unwrap_or(FIXED_DT)
    }

    pub fn set_fixed_dt(&mut self, dt: Option<f32>) -> &mut Self {
        self.fixed_dt = dt;
        self
    }

    /// Declare what this plugin receives from outside the simulation.
    ///
    /// Everything registered here is written to a recording each tick and fed
    /// back on replay. A subsystem that takes input from the OS, a socket or
    /// a player and does not register is a hole a replay cannot fill.
    pub fn add_replay_source(
        &mut self,
        name: &str,
        capture: impl Fn(&Engine) -> serde_json::Value + 'static,
        restore: impl Fn(&Engine, &serde_json::Value) + 'static,
    ) -> &mut Self {
        let sources = self.engine.resource::<crate::replay::ReplayRegistry>();
        sources
            .borrow_mut()
            .0
            .push((name.to_string(), Box::new(capture), Box::new(restore)));
        self
    }

    /// Declare state this plugin *loads* rather than simulates, so a replay
    /// derives from what the recording had rather than what this machine has.
    ///
    /// Captured once into the recording's header and restored before its
    /// first tick — input bindings are the case that made it exist: a player
    /// who rebinds jump after recording a session would otherwise replay it
    /// with a different action firing, and nothing would say so.
    pub fn add_replay_setup(
        &mut self,
        name: &str,
        capture: impl Fn(&Engine) -> serde_json::Value + 'static,
        restore: impl Fn(&Engine, &serde_json::Value) + 'static,
    ) -> &mut Self {
        let registry = self.engine.resource::<crate::replay::ReplaySetupRegistry>();
        registry
            .borrow_mut()
            .0
            .push((name.to_string(), Box::new(capture), Box::new(restore)));
        self
    }

    /// Declare simulation state this plugin owns, so rollback can put it
    /// back.
    ///
    /// A plugin that keeps simulation state outside the scene tree — physics
    /// does — must register here for the same reason it registers a digest
    /// source: core cannot see it, and what core cannot see cannot be
    /// restored.
    pub fn add_snapshot_source(
        &mut self,
        name: &str,
        save: impl Fn(&Engine) -> serde_json::Value + 'static,
        load: impl Fn(&Engine, &serde_json::Value) + 'static,
    ) -> &mut Self {
        let sources = self.engine.resource::<crate::snapshot::SnapshotRegistry>();
        sources
            .borrow_mut()
            .0
            .push((name.to_string(), Box::new(save), Box::new(load)));
        self
    }

    /// Register a whole resource as a replay source, for passive state.
    ///
    /// The common case: something the OS fills in and nothing here sends
    /// back, so capture and restore are just serialize and replace. A
    /// subsystem with an outbound side wants
    /// [`replay::ExternalIo`](crate::replay::ExternalIo) instead.
    pub fn add_replay_resource<T>(&mut self, name: &str) -> &mut Self
    where
        T: serde::Serialize + serde::de::DeserializeOwned + 'static,
    {
        self.add_replay_source(
            name,
            |eng| {
                serde_json::to_value(&*eng.resource::<T>().borrow())
                    .unwrap_or(serde_json::Value::Null)
            },
            |eng, value| match serde_json::from_value::<T>(value.clone()) {
                Ok(restored) => *eng.resource::<T>().borrow_mut() = restored,
                Err(e) => tracing::error!(error = %e, "replaying a resource"),
            },
        )
    }

    /// Fold plugin-owned state into the per-tick digest.
    ///
    /// Components report what a scene author set; a step computes more than
    /// that, and what it computes is what diverges first.
    pub fn add_digest_source(
        &mut self,
        name: &str,
        source: impl Fn(&Engine, &mut Vec<crate::digest::Entry>) + 'static,
    ) -> &mut Self {
        let sources = self.engine.resource::<crate::digest::DigestRegistry>();
        sources
            .borrow_mut()
            .0
            .push((name.to_string(), Box::new(source)));
        self
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

    /// Register a preset: a named set of components applied to one node.
    ///
    /// A recipe, not a type -- see `balaur_core::presets`. Plugins register
    /// the presets for the components they own, so the physics plugin ships
    /// the body ones and a headless build offers neither.
    pub fn register_preset(&mut self, name: &str, def: crate::presets::PresetDef) -> &mut Self {
        let registry = self.engine.resource::<crate::presets::PresetRegistry>();
        registry.borrow_mut().0.insert(name.to_string(), def);
        self
    }

    /// Register a parser for one asset type (see `balaur_core::assets`).
    ///
    /// The mirror of [`Self::register_component`]: core learns the name and
    /// nothing else, the parser returns an object only its plugin
    /// understands, and `assets::load_typed` hands it back downcast.
    ///
    /// `directory` is the project-relative folder files of this type belong in
    /// (`animations`). A tool promoting an inline definition to a file needs
    /// somewhere to put it and the schema does not carry that; an empty string
    /// means the type has no home and cannot be promoted.
    ///
    /// `doc` describes the definition table for the generated reference:
    /// markdown, a paragraph and a TOML example.
    pub fn register_asset_type(
        &mut self,
        name: &str,
        directory: &str,
        doc: &'static str,
        parse: impl Fn(&toml::Value) -> Result<std::rc::Rc<dyn std::any::Any>> + 'static,
    ) -> &mut Self {
        let registry = self.engine.resource::<crate::assets::AssetTypeRegistry>();
        registry.borrow_mut().0.push((
            name.to_string(),
            crate::assets::AssetType {
                parse: Box::new(parse),
                directory: directory.to_string(),
                doc,
            },
        ));
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
            let fs = crate::files::backend(&self.engine);
            let path = self.project_root.join("project.toml");
            manifest_src = fs
                .read(&path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .with_context(|| format!("no project.toml in {}", self.project_root.display()))?;
            let manifest = ProjectManifest::parse(&manifest_src)?;
            let scene_path = self.project_root.join(&manifest.main_scene);
            scene_src = fs
                .read(&scene_path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .with_context(|| format!("reading {}", scene_path.display()))?;
            self.manifest = Some(manifest);
        }
        // A resource too, so subsystems can read the project's language and
        // name without reaching back through App.
        if let Some(manifest) = &self.manifest {
            self.engine.insert_resource(manifest.clone());
        }
        self.engine
            .insert_resource(project::ManifestSource(manifest_src.clone()));
        self.load_project_presets()?;
        let root = self.engine.root();
        project::instantiate_scene(&self.engine, &scene_src, root, true)?;
        Ok(self)
    }

    /// Load `presets.toml`, letting a project name its own recipes.
    ///
    /// Read through `project::scene_text`, which is where a pack keeps its
    /// documents: `ProjectFiles` serves the pack's *assets*, and a shipped
    /// game found no presets there at all. Absent is the normal case, not an
    /// error; malformed is an error, because ignoring it hides a typo forever.
    fn load_project_presets(&mut self) -> Result<()> {
        let Ok(text) = project::scene_text(&self.engine, "presets.toml") else {
            return Ok(());
        };
        let table: toml::Value = toml::from_str(&text).context("parsing presets.toml")?;
        let table = table
            .as_table()
            .ok_or_else(|| anyhow::anyhow!("presets.toml should be a table of presets"))?;
        for (name, body) in table {
            let def = crate::presets::from_toml(name, body)?;
            self.register_preset(name, def);
        }
        Ok(())
    }

    pub const fn manifest(&self) -> Option<&ProjectManifest> {
        self.manifest.as_ref()
    }

    pub fn project_root(&self) -> &std::path::Path {
        &self.project_root
    }

    /// Run one frame from a *measured* frame time, applying the fixed-step
    /// policy.
    ///
    /// Backends with their own main loop call this rather than [`App::tick`],
    /// so `set_fixed_dt` reaches every run mode instead of only the one whose
    /// loop lives here.
    pub fn advance(&mut self, measured_dt: f32) {
        // A session records from a frame boundary, and its replay starts from
        // one: both zero the accumulator so they take the same fixed steps.
        if crate::replay::take_record_restart(&self.engine) {
            self.accumulator = 0.0;
        }
        // In its own statement: a match holds its scrutinee's temporaries for
        // every arm, and the arms below borrow the player again.
        let plan = self
            .engine
            .resource::<crate::replay::ReplayPlayer>()
            .borrow()
            .plan();
        match plan {
            crate::replay::Step::Live => {
                self.engine.set_replay_hold(false);
                self.tick(self.fixed_dt.unwrap_or(measured_dt));
            }
            crate::replay::Step::Hold => {
                self.engine.set_replay_hold(true);
                self.tick_held(measured_dt);
            }
            crate::replay::Step::Frames(count) => {
                self.engine.set_replay_hold(false);
                self.replay_frames(count);
            }
        }
    }

    /// Feed and run `count` recorded frames.
    ///
    /// A seek runs many in one call, which is why this is here and not in a
    /// system: only the app can both put a frame in the feed and tick it, and
    /// the fixed-step accumulator it owns has to start where the recording's
    /// did.
    fn replay_frames(&mut self, count: usize) {
        if std::mem::take(
            &mut self
                .engine
                .resource::<crate::replay::ReplayPlayer>()
                .borrow_mut()
                .restart,
        ) {
            self.accumulator = 0.0;
        }
        for _ in 0..count {
            let Some(fed) = crate::replay::feed_next(&self.engine) else {
                break;
            };
            // A frame the debugger held ran no fixed step and no script; the
            // replay has to hold it the same way or it steps a tick the
            // recording never took.
            let debugging = self.engine.is_frozen();
            self.engine.set_frozen(debugging || fed.frozen);
            self.tick(fed.dt);
            self.engine.set_frozen(debugging);
            crate::replay::after_frame(&self.engine);
            // A seek arrives mid-budget, and a script may pause from inside
            // the frame that just ran: either way the rest of the budget is
            // no longer wanted.
            if !crate::replay::is_advancing(&self.engine) {
                break;
            }
        }
    }

    /// Run one frame at exactly `dt`, whatever the fixed-step policy says.
    pub fn tick(&mut self, dt: f32) {
        self.engine.advance_time(dt);
        self.run_stages(dt);
    }

    /// Draw a frame that is not a tick: a paused replay's clock has to stay
    /// on the tick the recording reached, or every later replayed tick runs
    /// at a number no recorded frame ever had.
    fn tick_held(&mut self, dt: f32) {
        self.engine.hold_time(dt);
        self.run_stages(dt);
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "times the frame for the profiler; systems are fed dt, never the clock"
    )]
    fn run_stages(&mut self, dt: f32) {
        let frame_started = Instant::now();
        let mut stages = [std::time::Duration::ZERO; STAGE_COUNT];
        let mut fixed_steps = 0;
        for (stage, elapsed) in stages.iter_mut().enumerate() {
            let started = Instant::now();
            if stage == Stage::FixedUpdate as usize {
                fixed_steps = self.run_fixed_steps(dt);
            } else {
                for system in &mut self.systems[stage] {
                    system(&self.engine, dt);
                }
            }
            *elapsed = started.elapsed();
        }
        crate::timings::publish(&self.engine, frame_started.elapsed(), stages, fixed_steps);
    }

    /// Drain the accumulator into whole [`FIXED_DT`] steps.
    ///
    /// One accumulator for the whole simulation, so scripts and physics take
    /// the same number of steps in the same order every frame. Time past
    /// [`MAX_SUBSTEPS`] is dropped rather than caught up on.
    fn run_fixed_steps(&mut self, dt: f32) -> u32 {
        // A debugger pause holds the simulation: the time is dropped, not owed.
        if self.engine.frozen_root().is_some() {
            self.accumulator = 0.0;
            return 0;
        }
        let mut steps = 0;
        self.accumulator = (self.accumulator + dt).min(FIXED_DT * MAX_SUBSTEPS as f32);
        while self.accumulator >= FIXED_DT {
            for system in &mut self.systems[Stage::FixedUpdate as usize] {
                system(&self.engine, FIXED_DT);
            }
            self.accumulator -= FIXED_DT;
            steps += 1;
        }
        steps
    }

    /// Fixed-cadence main loop (60 Hz), until quit is requested. Rendering
    /// backends may block on vsync inside their Render-stage system; the
    /// sleep below only tops up whatever time is left.
    ///
    /// Under [`App::set_fixed_dt`] the step fed to systems is that constant
    /// rather than the measured frame time, so an interactive run reproduces
    /// a headless one tick for tick — given the same inputs, which is what a
    /// replay file supplies.
    #[allow(
        clippy::disallowed_methods,
        reason = "the loop driver measures a frame; what it feeds systems is fixed_dt"
    )]
    pub fn run(&mut self) {
        let target = Duration::from_secs_f32(self.fixed_dt.unwrap_or(FIXED_DT));
        let mut last = Instant::now();
        while !self.engine.quit_requested() {
            let now = Instant::now();
            let measured = (now - last).as_secs_f32().min(0.1);
            last = now;
            self.advance(measured);
            let elapsed = last.elapsed();
            if elapsed < target {
                // elapsed < target was just checked, so the subtraction cannot underflow.
                std::thread::sleep(target.checked_sub(elapsed).unwrap());
            }
        }
    }
}
