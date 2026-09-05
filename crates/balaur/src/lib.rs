//! Batteries-included entry points for Balaur games and tools.
//!
//! A shipped game binary is three lines:
//!
//! ```ignore
//! fn main() -> anyhow::Result<()> {
//!     balaur::boot_pack(include_bytes!(concat!(env!("OUT_DIR"), "/game.bpak")))
//! }
//! ```

pub use balaur_anim::AnimationPlugin;
pub use balaur_core::*;
pub use balaur_input::InputPlugin;
pub use balaur_physics::PhysicsPlugin;
pub use balaur_platform::PlatformPlugin;
pub use balaur_render::RenderPlugin;
pub use balaur_ui::UiPlugin;

pub use balaur_anim as animation;
pub use balaur_input as input;
pub use balaur_physics as physics;
pub use balaur_platform as platform;
pub use balaur_render as render;
pub use balaur_script_rune as rune;
pub use balaur_ui as ui;

// A `Transport` a project names at run time, not a plugin: nothing to load.
#[cfg(feature = "webtransport")]
pub use balaur_webtransport as webtransport;

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};

/// Every optional module: the name it goes by, the cargo feature that
/// switches it on, and the plugin it registers.
///
/// One line each. A module used to be the same fact written four times, and
/// the three restatements are what went stale.
macro_rules! modules {
    ($($alias:ident = $feature:literal => $krate:ident::$plugin:ident),* $(,)?) => {
        $(
            #[cfg(feature = $feature)]
            pub use $krate::$plugin;
            #[cfg(feature = $feature)]
            pub use $krate as $alias;
        )*

        /// The optional modules this build linked in.
        // Pushed rather than `vec![]` because `#[cfg]` does not apply to an
        // element inside it; a build with no optional module pushes nothing.
        #[allow(unused_mut, clippy::vec_init_then_push, reason = "see above")]
        fn optional_modules() -> Vec<Box<dyn balaur_plugin::Plugin>> {
            let mut found: Vec<Box<dyn balaur_plugin::Plugin>> = Vec::new();
            $(
                #[cfg(feature = $feature)]
                found.push(Box::new($krate::$plugin::default()));
            )*
            found
        }
    };
}

modules! {
    apple = "apple" => balaur_apple::ApplePlugin,
    audio = "audio" => balaur_audio::AudioPlugin,
    gamend = "gamend" => balaur_gamend::GamendPlugin,
    http = "http" => balaur_http::HttpPlugin,
    web = "web" => balaur_web::WebPlugin,
    websocket = "websocket" => balaur_websocket::WebsocketPlugin,
}

/// The project's manifest, read the way `standard_app` needs it: before the
/// app exists, so it cannot come from `App::manifest`.
///
/// A manifest that will not parse reads as absent. `App::load_project` is
/// where that becomes the error a person can act on.
fn manifest_of(config: &AppConfig) -> Option<balaur_core::project::ProjectManifest> {
    let text = match &config.pack {
        Some(pack) => pack.manifest.clone(),
        None => std::fs::read_to_string(config.project_root.join("project.toml")).ok()?,
    };
    balaur_core::project::ProjectManifest::parse(&text).ok()
}

/// The script backend a project asks for in its `project.toml`. Rune is the
/// one language this build ships; the field stays so a project states it.
fn backend_for(config: &AppConfig) -> Result<balaur_core::ScriptHostFactory> {
    let language = manifest_of(config).map_or_else(|| "rune".to_string(), |m| m.language);
    match language.as_str() {
        "rune" => Ok(balaur_script_rune::factory()),
        other => Err(anyhow::anyhow!(
            "project.toml asks for language \"{other}\"; this build has rune"
        )),
    }
}

/// Build an export pack with the script backend `standard_app` installs.
///
/// Rune resolves `input::…` while compiling, so an export has to run against
/// the same modules the game will have: boot the app the game would boot, and
/// compile through its host. Exporting through a bare context instead rejects
/// every script that touches the engine.
pub fn build_pack(project_root: &std::path::Path) -> Result<Pack> {
    build_pack_with(project_root, false)
}

/// [`build_pack`], keeping script sources in the pack when `keep_sources`
/// — see [`Pack::build_with`] for when a runtime needs that.
pub fn build_pack_with(project_root: &std::path::Path, keep_sources: bool) -> Result<Pack> {
    let app = standard_app(AppConfig::export(project_root))?;
    let host = app
        .engine
        .script_host()
        .context("no script backend for the project")?;
    let host = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .context("expected the rune backend")?;
    Pack::build_with(project_root, host, keep_sources)
}

/// Every finding in a project: each script a scene attaches, compiled through
/// a booted host, plus the files those scenes name.
///
/// The unit a check compiles is the root a scene names, not every `.rn` in
/// the tree: a `mod` submodule compiled on its own fails on its own imports,
/// and its diagnostics arrive through the root that reaches it anyway.
///
/// # Errors
/// If the project will not boot.
pub fn check_project(project_root: &std::path::Path) -> Result<Vec<balaur_script_rune::Finding>> {
    let app = standard_app(AppConfig::export(project_root))?;
    let host = app
        .engine
        .script_host()
        .context("no script backend for the project")?;
    let host = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .context("expected the rune backend")?;
    let mut found = Vec::new();
    for rel in scene_scripts(project_root) {
        let path = project_root.join(&rel);
        let Ok(source) = std::fs::read_to_string(&path) else {
            found.push(balaur_script_rune::Finding {
                file: rel.clone(),
                line: 0,
                column: 0,
                end_line: 0,
                end_column: 0,
                severity: "error",
                message: format!("script {rel} does not exist"),
            });
            continue;
        };
        found.extend(host.check_source(&rel, &source)?);
    }
    Ok(found)
}

/// Every script path the project's scene files attach, deduplicated and in a
/// stable order. A scene that will not parse is skipped: it is the scene
/// loader's error to report, not the checker's.
///
/// This is what a checker means by a root: the unit the compiler starts from.
/// A `mod` submodule is not one — compiled on its own it fails on its own
/// imports — and its diagnostics arrive through the root that imports it.
#[must_use]
pub fn scene_scripts(project_root: &std::path::Path) -> Vec<String> {
    let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut dirs = vec![project_root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(document) = text.parse::<toml::Table>() else {
                continue;
            };
            let Some(nodes) = document.get("nodes").and_then(toml::Value::as_array) else {
                continue;
            };
            for node in nodes {
                // `script` is a path, or a table whose `source` is one.
                let script = match node.get("script") {
                    Some(toml::Value::String(path)) => Some(path.clone()),
                    Some(toml::Value::Table(table)) => table
                        .get("source")
                        .and_then(toml::Value::as_str)
                        .map(str::to_string),
                    _ => None,
                };
                if let Some(script) = script {
                    out.insert(script);
                }
            }
        }
    }
    out.into_iter().collect()
}

pub fn standard_app(mut config: AppConfig) -> Result<App> {
    if config.script_backend.is_none() {
        config.script_backend = Some(backend_for(&config)?);
    }
    let asked = manifest_of(&config).map(|m| m.plugins).unwrap_or_default();
    let mut app = App::new(config)?;
    app.engine.insert_resource(configs_from(&asked));
    balaur_plugin::load_all(&mut app, &mut standard_plugins(&asked)?)?;
    drive_ui_focus(&mut app);
    #[cfg(feature = "extensions")]
    load_project_extensions(&mut app, &asked)?;
    refuse_absent(&app, &asked)?;
    Ok(app)
}

/// What a project asked of `[plugins]`: a name, and what it said about it.
type Selection = BTreeMap<String, balaur_core::PluginChoice>;

/// The tables `[plugins]` handed each plugin, for `Registry::config` to
/// answer from once the plugin is registering.
fn configs_from(asked: &Selection) -> balaur_core::PluginConfigs {
    balaur_core::PluginConfigs(
        asked
            .iter()
            .filter_map(|(name, choice)| Some((name.clone(), choice.config()?.clone())))
            .collect(),
    )
}

/// Every plugin a standard app registers, for `load_all` to order.
///
/// One set rather than two sequences is what lets `apple` say it requires
/// `platform` and be believed.
fn standard_plugins(asked: &Selection) -> Result<Vec<Box<dyn balaur_plugin::Plugin>>> {
    let always: Vec<Box<dyn balaur_plugin::Plugin>> = vec![
        Box::new(AnimationPlugin::default()),
        Box::new(InputPlugin::default()),
        Box::new(PhysicsPlugin::default()),
        Box::new(RenderPlugin::default()),
        Box::new(UiPlugin::default()),
        Box::new(PlatformPlugin::default()),
    ];
    for plugin in &always {
        let name = &plugin.manifest().name;
        if asked.get(name).is_some_and(|choice| !choice.wanted()) {
            bail!("project.toml turns off `{name}`, which every build has");
        }
    }
    let mut all = always;
    all.extend(optional_modules().into_iter().filter(|module| {
        asked
            .get(&module.manifest().name)
            .is_none_or(balaur_core::PluginChoice::wanted)
    }));
    Ok(all)
}

/// Refuse a project that asked for a plugin nothing registered.
///
/// Only what it asked *for*: a project turning off something this build has
/// not got already has what it wanted, and saying so would be noise.
fn refuse_absent(app: &App, asked: &Selection) -> Result<()> {
    let loaded = balaur_core::plugins::names(&app.engine);
    let missing: Vec<&str> = asked
        .iter()
        .filter(|(_, choice)| choice.asked_for())
        .map(|(name, _)| name.as_str())
        .filter(|name| !loaded.iter().any(|n| n == name))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "project.toml asks for `{}`, which nothing registered: this build has          no such module (try --features {}) and no extension declares it",
        missing.join("`, `"),
        missing.join(","),
    )
}

/// Let a pad walk a menu, by mapping three actions onto the focus verbs.
///
/// Here rather than in either plugin: `balaur_ui` reads its input from egui,
/// which has keys but no pads, and `balaur_input` knows nothing about
/// widgets. This crate is the one that knows about both, which is what
/// assembling them means. A project that declares none of these three
/// actions gets no menu system and no keyboard focus either — the keys stay
/// the game's, which is what a game that moves with the arrows needs.
fn drive_ui_focus(app: &mut App) {
    use balaur_ui::{Move, UiFocus};
    // Turned on once, the first frame the actions are there: a project's
    // table is loaded lazily, and the editor declares a played game's later
    // still. Only ever on, so `ui.set_keyboard_focus(false)` is not undone.
    let mut armed = false;
    app.add_system(
        balaur_core::Stage::First,
        move |eng: &balaur_core::Engine, _| {
            let Some(actions) = eng.try_resource::<balaur_input::InputActions>() else {
                return;
            };
            if !armed
                && ["ui_accept", "ui_next", "ui_previous"]
                    .iter()
                    .any(|name| actions.borrow().is_declared(name))
            {
                armed = true;
                eng.resource::<balaur_ui::WidgetLayerConfig>()
                    .borrow_mut()
                    .keyboard = true;
            }
            let asked = {
                let actions = actions.borrow();
                // Accept first: a frame that both moved and accepted meant the
                // accept, and one press cannot sensibly do two things.
                if actions.just_pressed("ui_accept") {
                    Some(Move::Accept)
                } else if actions.just_pressed("ui_next") {
                    Some(Move::Next)
                } else if actions.just_pressed("ui_previous") {
                    Some(Move::Previous)
                } else {
                    None
                }
            };
            if let Some(asked) = asked {
                eng.resource::<UiFocus>().borrow_mut().pending = Some(asked);
            }
        },
    );
}

/// Load every extension in the project's `extensions/` directory.
///
/// # Errors
/// If a library fails to load, disagrees about the build, or requires
/// something absent.
#[cfg(feature = "extensions")]
fn load_project_extensions(app: &mut App, asked: &Selection) -> Result<()> {
    let dir = app.project_root().join("extensions");
    let modules = balaur_core::plugins::names(&app.engine);
    // Safety: opening a library runs its initialisers, and the fingerprint
    // check inside refuses a build that cannot share this process.
    let mut loaded = unsafe { balaur_plugin::load_extensions_in(&dir, &modules) }?;
    for extension in &mut loaded {
        let name = extension.manifest().name.clone();
        if asked.get(&name).is_some_and(|choice| !choice.wanted()) {
            tracing::info!(extension = %name, "off in project.toml");
            continue;
        }
        balaur_plugin::load(app, extension.plugin_mut())
            .with_context(|| format!("extension `{name}`"))?;
        tracing::info!(extension = %name, "loaded");
    }
    // The libraries have to outlive every plugin they produced.
    app.engine.insert_resource(LoadedExtensions(loaded));
    Ok(())
}

/// Keeps the shared libraries mapped for as long as the app runs. Unloading
/// one while its code is still reachable would leave a dangling vtable.
#[cfg(feature = "extensions")]
struct LoadedExtensions(
    #[allow(dead_code, reason = "held only to keep the libraries mapped")]
    Vec<balaur_plugin::Extension>,
);

/// Run the app with the best available frontend: a kiss3d window when the
/// `window` feature is enabled, the headless fixed-rate loop otherwise.
#[allow(unused_mut)] // `mut` is only needed by the headless fallback path.
pub fn run(mut app: App, title: &str) -> Result<()> {
    #[cfg(feature = "window")]
    {
        return balaur_render::kiss3d_backend::run_windowed(app, title);
    }
    #[allow(unreachable_code)] // The windowed path above returns when the feature is on.
    {
        let _ = title;
        app.run();
        balaur_render::warn_if_unserved(&app.engine);
        Ok(())
    }
}

/// Render to a hidden window: a real GPU, no OS window, a fixed frame step.
///
/// The mode an automation client or a visual CI job wants — screenshots that
/// match what a player sees, on a machine with no display. Distinct from
/// headless, which runs no renderer at all and is what keeps tests fast and
/// portable to machines with no adapter.
///
/// # Errors
/// If this build has no renderer (the `window` feature is off), or no GPU
/// adapter is available.
#[allow(unused_variables, unused_mut)] // Both are used only by the windowed build.
pub fn run_offscreen(mut app: App, title: &str, width: u32, height: u32) -> Result<()> {
    #[cfg(feature = "window")]
    {
        return balaur_render::kiss3d_backend::run_offscreen(app, title, width, height);
    }
    #[allow(unreachable_code)] // The windowed path above returns when the feature is on.
    {
        Err(anyhow::anyhow!(
            "this build has no renderer, so it cannot render offscreen; \
             build with --features window"
        ))
    }
}

/// Dev mode: load a project directory with hot reload enabled and run it.
pub fn boot_project(project_root: &str) -> Result<()> {
    let mut app = standard_app(AppConfig::dev(project_root))?;
    app.load_project()?;
    let title = app
        .manifest()
        .map_or_else(|| "balaur".to_string(), |m| m.name.clone());
    run(app, &title)
}

/// A pack booted onto an HTML canvas: the browser's `boot_pack`. Nothing in a
/// page may block, so this is a future the entry point spawns; the loop it
/// runs is the windowed one, drawing on the `<canvas>` named by `canvas_id`.
#[cfg(feature = "window")]
pub async fn boot_pack_on_canvas(bytes: &[u8], canvas_id: &str) -> Result<()> {
    let pack = Pack::decode(bytes)?;
    let mut app = standard_app(AppConfig::packed(pack))?;
    app.load_project()?;
    let title = app
        .manifest()
        .map_or_else(|| "balaur".to_string(), |m| m.name.clone());
    balaur_render::kiss3d_backend::run_windowed_async(app, &title, Some(canvas_id)).await
}

/// The editor, booted on an HTML canvas over a project that is already in
/// memory.
///
/// The editor is a Balaur project like any other, so this is `boot_pack_on_canvas`
/// with two differences: the game it edits is handed to its scripts as
/// `engine.args()[0]`, the way `balaur edit <game>` does on a desktop, and
/// that directory is declared as a second `fs` root so the editor may write
/// into it. Seeding `game_root` is the caller's job — see
/// `balaur_core::files::MemoryFs`.
#[cfg(feature = "window")]
pub async fn boot_editor_on_canvas(
    editor_pack: &[u8],
    editor_root: &str,
    game_root: &str,
    canvas_id: &str,
) -> Result<()> {
    let pack = Pack::decode(editor_pack)?;
    let mut config = AppConfig::packed(pack);
    // The editor's scripts and scenes come from the pack, but it reads its own
    // themes and layouts through `fs` like any project would, so its root has
    // to be a directory that exists — in a browser, one the caller seeded.
    config.project_root = std::path::PathBuf::from(editor_root);
    config.script_args = vec![game_root.to_string()];
    let mut app = standard_app(config)?;
    file_api::add_root(&app.engine, game_root);
    app.load_project()?;
    balaur_render::kiss3d_backend::run_windowed_async(app, "balaur editor", Some(canvas_id)).await
}

/// Shipping mode: run a precompiled pack (e.g. embedded via `include_bytes!`).
pub fn boot_pack(bytes: &[u8]) -> Result<()> {
    let pack = Pack::decode(bytes)?;
    let mut app = standard_app(AppConfig::packed(pack))?;
    app.load_project()?;
    let title = app
        .manifest()
        .map_or_else(|| "balaur".to_string(), |m| m.name.clone());
    run(app, &title)
}
