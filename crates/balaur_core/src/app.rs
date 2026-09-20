//! The application shell: plugins, staged scheduler, main loop.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::time::Instant;

use anyhow::{Context, Result, bail};
use serde::Deserialize as _;

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
/// up on, at [`TICK_HZ`]. Without it a stalled frame spends its recovery in a
/// spiral of catch-up steps.
pub const MAX_SUBSTEPS: u32 = 4;

thread_local! {
    /// The rate this run ticks at: `[time] tick_hz`, or [`TICK_HZ`] until a
    /// project says otherwise. Per thread rather than global because a test
    /// binary runs many apps at once, each with its own project.
    static TICK: std::cell::Cell<(u32, f32)> =
        const { std::cell::Cell::new((TICK_HZ, FIXED_DT)) };
}

/// How many fixed steps a second this run takes.
#[must_use]
pub fn tick_hz() -> u32 {
    TICK.with(|t| t.get().0)
}

/// The fixed step this run takes, in seconds. What every subsystem counting
/// simulation time reads instead of [`FIXED_DT`], which is only the default.
#[must_use]
pub fn fixed_dt() -> f32 {
    TICK.with(|t| t.get().1)
}

/// [`MAX_SUBSTEPS`] at this run's rate: the same wall clock of catch-up
/// whatever the tick, so a faster tick does not cap a frame sooner.
#[must_use]
pub fn max_substeps() -> u32 {
    (MAX_SUBSTEPS * tick_hz() / TICK_HZ).max(1)
}

/// Set the rate this run ticks at. For [`App`] reading `[time] tick_hz` and
/// for [`crate::replay`] restoring the rate a recording was made at; anything
/// else moving it mid-run changes the simulation under everything holding a
/// step count.
pub fn set_tick_hz(hz: u32) {
    let hz = hz.max(1);
    TICK.with(|t| t.set((hz, 1.0 / hz as f32)));
}

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
    /// Where extension libraries load from; `None` is the project's own
    /// `extensions/`. Only a build with the `extensions` feature reads it.
    pub extensions: Option<PathBuf>,
}

impl AppConfig {
    pub fn dev(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            pack: None,
            watch: true,
            script_args: Vec::new(),
            script_backend: None,
            extensions: None,
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

    /// A shipped game. Its extensions sit beside its executable, never in the
    /// working directory, which is `/` for a `.app` opened from Finder.
    pub fn packed(pack: Pack) -> Self {
        Self {
            pack: Some(pack),
            extensions: std::env::current_exe()
                .ok()
                .map(|exe| crate::standalone::extensions_beside(&exe)),
            ..Self::bare(".")
        }
    }
}

/// Takes a live frame instead of [`App::tick`]: a networked match, which
/// decides each tick's inputs and may run several ticks or none. Answers
/// whether it took the frame.
pub type DriveFn = Box<dyn FnMut(&mut App, f32) -> bool>;

/// The one [`DriveFn`] a run has, when a plugin installed one.
#[derive(Default)]
pub struct FrameDriver(pub Option<DriveFn>);

pub struct App {
    pub engine: Engine,
    systems: Vec<Vec<SystemFn>>,
    pack: Option<Pack>,
    project_root: PathBuf,
    manifest: Option<ProjectManifest>,
    /// A scene to open instead of the manifest's `main_scene`.
    main_scene: Option<String>,
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
    eng.insert_resource(crate::components::Attached::default());
    eng.insert_resource(crate::components::Authored::default());
    eng.insert_resource(crate::plugins::PluginRegistry::default());
    eng.insert_resource(crate::presets::PresetRegistry::default());
    eng.insert_resource(crate::assets::AssetTypeRegistry::default());
    eng.insert_resource(crate::assets::AssetState::default());
    eng.insert_resource(ProjectRoot(config.project_root.clone()));
    eng.insert_resource(crate::settings::SettingsRegistry::default());
    eng.insert_resource(crate::settings::SettingsValues::default());
    // Before any setting is read, since the tags in force decide which
    // `[override.<tag>]` a read answers from.
    eng.insert_resource(crate::tags::Tags::current());
    // A pack's manifest is here already, and `application/assets` decides
    // how its files are served, so it is read before the files exist.
    if let Some(pack) = config.pack.as_ref()
        && crate::settings::load(eng, &pack.manifest).is_ok()
    {
        crate::settings::answer_to_built_tags(eng);
    }
    // A packed game serves its textures, sounds and fonts from the pack;
    // a dev run serves them from the source tree.
    eng.insert_resource(
        match config.pack.as_ref() {
            Some(pack) => crate::project::ProjectFiles::packed(
                config.project_root.clone(),
                pack.assets.clone(),
                crate::settings::stated(eng, "application/assets")
                    .and_then(|v| v.try_into().ok())
                    .unwrap_or_default(),
            )
            .with_index(pack.scenes.get(crate::assets::INDEX_PATH).cloned()),
            None => crate::project::ProjectFiles::directory(config.project_root.clone()),
        }
        .on(crate::files::backend(eng)),
    );
    eng.insert_resource(ScriptArgs(config.script_args.clone()));
    eng.insert_resource(crate::rng::RngState::default());
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
            if let Ok(facts) = crate::facts::DeviceFacts::deserialize(value) {
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
            if let Ok(clock) = crate::facts::WallClock::deserialize(value) {
                *eng.resource::<crate::facts::WallClock>().borrow_mut() = clock;
            }
        },
    );
    app.add_replay_setup(
        "platform",
        |eng| serde_json::to_value(crate::facts::platform(eng)).unwrap_or_default(),
        |eng, value| {
            if let Ok(facts) = crate::facts::PlatformFacts::deserialize(value) {
                // The input class is a tag, so a session recorded on a phone
                // resolves the phone's overrides when it replays on a desktop.
                eng.resource::<crate::tags::Tags>()
                    .borrow_mut()
                    .set_input_class(facts.touchscreen);
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

/// The assets and components core owns, and the resources behind them.
///
/// Geometry is core content, not a rendering concern: physics reads the same
/// asset for its trimesh colliders. A bone is scene-tree data the same way.
/// Interactivity is core because a binding's actions are calls core already
/// has, and because the digest carries what they change.
fn register_core_content(app: &mut App) {
    crate::mesh::register_mesh_asset(app);
    crate::texture_asset::register_texture_asset(app);
    crate::path::register_path_assets(app);
    crate::heightfield::register_heightfield_asset(app);
    crate::voxels::register_voxels_asset(app);
    crate::transform::register_transform_component(app);
    crate::skeleton::register_bone2d_component(app);
    crate::skeleton::register_bone3d_component(app);
    crate::states::register_states_component(app);
    crate::timer::register_timer_component(app);
    crate::bindings::register_bindings_component(app);
    crate::node_meta::register_meta_component(app);
    app.engine
        .insert_resource(crate::variables::Variables::default());
    app.engine
        .insert_resource(crate::bindings::Runners::default());
    app.engine
        .insert_resource(crate::scene_switch::Pending::default());
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
            main_scene: None,
            fixed_dt: None,
            accumulator: 0.0,
        };
        // A test binary builds many apps on one thread, and the rate is that
        // thread's: each starts at the default until its project moves it.
        set_tick_hz(TICK_HZ);
        register_core_content(&mut app);
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
        // A browser has no threads, so a stepped job is advanced here; a
        // desktop parks nothing and this is an empty borrow.
        app.add_system(Stage::First, crate::task::advance_parked_system);
        app.add_system(Stage::First, crate::facts::read_clock_system);
        app.add_system(Stage::First, crate::facts::announce_device_system);
        app.add_system(Stage::First, crate::process::announce_pause_system);
        app.add_system(Stage::FixedUpdate, crate::timers::step_timers_system);
        app.add_system(Stage::FixedUpdate, crate::timer::step_system);
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
                // 1.0 is the tick's own pose for every node, and the pass
                // skips the per-node lookup that a blend would cost.
                let alpha = if crate::interpolate::on(eng) {
                    eng.frame_alpha()
                } else {
                    1.0
                };
                scene::propagate_transforms_at(&mut eng.world_mut(), root, alpha);
            });
        });
        app.add_system(Stage::Last, |eng, _| {
            // Every free of the frame in one batch: see `scene::free_nodes`.
            let mut freed = Vec::new();
            for cmd in eng.take_commands() {
                match cmd {
                    Command::Free(entity) => {
                        // Most ticks record nothing; the label is only built
                        // for one that does.
                        if crate::replay::wants_events(eng) {
                            let label = crate::digest::node_label(&eng.world(), entity);
                            crate::replay::event(
                                eng,
                                "scene.free",
                                format!("freed {label}"),
                                Some(serde_json::json!({ "node": label })),
                            );
                        }
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
    /// The shortest a frame may be, from `[window] max_fps`; `None` when the
    /// project set no cap and the loop paces itself against the tick.
    ///
    /// A cap is what stops a menu screen from drawing four hundred frames a
    /// second on a machine with vsync off, which on a laptop is audible.
    #[must_use]
    pub fn frame_budget(&self) -> Option<Duration> {
        let fps = crate::project::max_fps(&self.engine);
        (fps > 0).then(|| Duration::from_secs_f32(1.0 / fps as f32))
    }

    #[must_use]
    pub fn fixed_step(&self) -> f32 {
        self.fixed_dt.unwrap_or_else(fixed_dt)
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
            |eng, value| match T::deserialize(value) {
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
            let mut registry = registry.borrow_mut();
            assert!(
                registry.len() < crate::components::MAX_COMPONENTS,
                "registering '{name}': a build may have at most {} components",
                crate::components::MAX_COMPONENTS
            );
            registry.insert(name, def);
        }
        let component = name.to_string();
        self.scene_key_handler(name, move |eng, entity, value| {
            let full = crate::components::properties(eng, &schema, Some(value))
                .with_context(|| format!("component `{component}`"))?;
            crate::components::apply_full(eng, entity, &component, &full)
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
    /// Open `scene` (project-relative) when the project loads, instead of the
    /// manifest's `main_scene`: a test harness booting its own scene around
    /// the game's.
    pub fn set_main_scene(&mut self, scene: impl Into<String>) -> &mut Self {
        self.main_scene = Some(scene.into());
        self
    }

    pub fn load_project(&mut self) -> Result<&mut Self> {
        let fs = crate::files::backend(&self.engine);
        let manifest_src = if let Some(pack) = &self.pack {
            pack.manifest.clone()
        } else {
            let path = self.project_root.join("project.toml");
            fs.read(&path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .with_context(|| format!("no project.toml in {}", self.project_root.display()))?
        };
        // Every table in the file, as values the settings registry answers
        // from: one reader for what a project declares, whoever declared it.
        // First, because the tags it adds decide what the manifest says.
        crate::settings::load(&self.engine, &manifest_src)?;
        crate::settings::answer_to_built_tags(&self.engine);
        let unknown = crate::settings::unknown(&self.engine, &manifest_src);
        if !unknown.is_empty() {
            bail!(
                "project.toml has {} nothing declares. A misspelled key is a \
                 setting that silently does not apply; a table of your own \
                 (`[mygame] url`) is not checked.",
                unknown.join(", ")
            );
        }
        let tags = self.engine.resource::<crate::tags::Tags>().borrow().clone();
        // Before the scene: a node's `interpolate` key is read as it is built,
        // and its step is what the first tick takes.
        self.apply_time_settings();
        let manifest = ProjectManifest::parse_for(&manifest_src, &tags)?;
        let main_scene = self.main_scene.as_ref().unwrap_or(&manifest.main_scene);
        let scene_src = if let Some(pack) = &self.pack {
            pack.scenes
                .get(main_scene)
                .cloned()
                .with_context(|| format!("scene {main_scene} missing from pack"))?
        } else {
            let scene_path = self.project_root.join(main_scene);
            fs.read(&scene_path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .with_context(|| format!("reading {}", scene_path.display()))?
        };
        self.manifest = Some(manifest);
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

    /// Put `[time]` into effect: the rate this run ticks at and whether it
    /// draws between steps.
    fn apply_time_settings(&self) {
        let hz = crate::settings::get(&self.engine, "time/tick_hz")
            .as_ref()
            .and_then(crate::components::as_f64)
            .filter(|n| *n >= 1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a rate from a setting bounded at 1..=480"
        )]
        set_tick_hz(hz.map_or(TICK_HZ, |n| n as u32));
        crate::interpolate::apply_setting(&self.engine);
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
            self.engine.restart_steps();
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
                if self.drive(measured_dt) {
                    return;
                }
                // The scale is a wall-clock matter: a run driving at a fixed
                // step is reproducing a tick sequence, and scaling that would
                // change the simulation rather than how fast it is watched.
                self.tick(
                    self.fixed_dt
                        .unwrap_or(measured_dt * self.engine.time_scale()),
                );
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
            self.engine.restart_steps();
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

    /// Hand a live frame to the [`FrameDriver`], if one is installed and
    /// takes it. The driver is out of the engine while it runs, so a script
    /// it ticks can reach every other resource.
    fn drive(&mut self, measured_dt: f32) -> bool {
        let Some(slot) = self.engine.try_resource::<FrameDriver>() else {
            return false;
        };
        let Some(mut driver) = slot.borrow_mut().0.take() else {
            return false;
        };
        let taken = driver(self, measured_dt);
        let mut slot = slot.borrow_mut();
        if slot.0.is_none() {
            slot.0 = Some(driver);
        }
        taken
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
        // A re-run tick is simulation only: nothing draws a world about to be
        // stepped again.
        let resimulating = crate::rollback::is_resimulating(&self.engine);
        for (stage, elapsed) in stages.iter_mut().enumerate() {
            let started = Instant::now();
            if resimulating && stage == Stage::Render as usize {
                continue;
            }
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
        crate::logbuf::flush_file();
    }

    /// Drain the accumulator into whole [`FIXED_DT`] steps.
    ///
    /// One accumulator for the whole simulation, so scripts and physics take
    /// the same number of steps in the same order every frame. Time past
    /// [`MAX_SUBSTEPS`] is dropped rather than caught up on.
    ///
    /// A game's own pause does not stop this: the step still runs, and each
    /// subsystem skips the nodes the pause holds, so an `always` subtree
    /// keeps ticking. A debugger's freeze stops the whole stage.
    fn run_fixed_steps(&mut self, dt: f32) -> u32 {
        // A debugger pause holds the simulation: the time is dropped, not owed.
        if self.engine.frozen_root().is_some() {
            self.accumulator = 0.0;
            // A held frame draws the tick's own pose, so what the inspector
            // reports and what the viewport shows are the same place.
            self.engine.set_frame_alpha(1.0);
            return 0;
        }
        let step = fixed_dt();
        let mut steps = 0;
        // Fast forward is owed more steps per frame than real time is, or the
        // cap would silently undo the scale.
        let budget = step * max_substeps() as f32 * self.engine.time_scale().max(1.0);
        self.accumulator = (self.accumulator + dt).min(budget);
        while self.accumulator >= step {
            for system in &mut self.systems[Stage::FixedUpdate as usize] {
                system(&self.engine, step);
            }
            // After the step, so the pair kept is the pose this step left and
            // the one before it: what a frame between the two blends.
            crate::interpolate::capture(&self.engine);
            self.accumulator -= step;
            steps += 1;
        }
        self.engine.set_frame_alpha(self.accumulator / step);
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
        let target = self
            .frame_budget()
            .unwrap_or_else(|| Duration::from_secs_f32(self.fixed_step()));
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
