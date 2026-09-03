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
#[cfg(feature = "apple")]
pub use balaur_apple::ApplePlugin;
#[cfg(feature = "audio")]
pub use balaur_audio::AudioPlugin;
pub use balaur_core::*;
#[cfg(feature = "gamend")]
pub use balaur_gamend::GamendPlugin;
#[cfg(feature = "http")]
pub use balaur_http::HttpPlugin;
pub use balaur_input::InputPlugin;
pub use balaur_physics::PhysicsPlugin;
pub use balaur_platform::PlatformPlugin;
pub use balaur_render::RenderPlugin;
pub use balaur_ui::UiPlugin;
#[cfg(feature = "websocket")]
pub use balaur_websocket::WebsocketPlugin;

pub use balaur_anim as animation;
#[cfg(feature = "apple")]
pub use balaur_apple as apple;
#[cfg(feature = "audio")]
pub use balaur_audio as audio;
#[cfg(feature = "gamend")]
pub use balaur_gamend as gamend;
#[cfg(feature = "http")]
pub use balaur_http as http;
pub use balaur_input as input;
pub use balaur_physics as physics;
pub use balaur_platform as platform;
pub use balaur_render as render;
pub use balaur_script_rune as rune;
pub use balaur_ui as ui;
#[cfg(feature = "websocket")]
pub use balaur_websocket as websocket;
#[cfg(feature = "webtransport")]
pub use balaur_webtransport as webtransport;

use anyhow::{Context, Result};

/// The script backend a project asks for in its `project.toml`. Rune is the
/// one language this build ships; the field stays so a project states it.
fn backend_for(config: &AppConfig) -> Result<balaur_core::ScriptHostFactory> {
    let manifest = match &config.pack {
        Some(pack) => Some(pack.manifest.clone()),
        None => std::fs::read_to_string(config.project_root.join("project.toml")).ok(),
    };
    let language = manifest
        .as_deref()
        .and_then(|m| balaur_core::project::ProjectManifest::parse(m).ok())
        .map_or_else(|| "rune".to_string(), |m| m.language);
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
    let app = standard_app(AppConfig::export(project_root))?;
    let host = app
        .engine
        .script_host()
        .context("no script backend for the project")?;
    let host = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .context("expected the rune backend")?;
    Pack::build(project_root, host)
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
    let mut app = App::new(config)?;
    app.add_plugin(InputPlugin)?;
    app.add_plugin(PhysicsPlugin)?;
    app.add_plugin(AnimationPlugin)?;
    app.add_plugin(RenderPlugin)?;
    #[cfg(feature = "audio")]
    balaur_plugin::load(&mut app, &mut AudioPlugin::default())?;
    #[cfg(feature = "http")]
    balaur_plugin::load(&mut app, &mut HttpPlugin::default())?;
    #[cfg(feature = "websocket")]
    balaur_plugin::load(&mut app, &mut WebsocketPlugin::default())?;
    #[cfg(feature = "gamend")]
    balaur_plugin::load(&mut app, &mut GamendPlugin::default())?;
    // Before any store's plugin: a backend registers into what this inserts.
    balaur_plugin::load(&mut app, &mut PlatformPlugin::default())?;
    #[cfg(feature = "apple")]
    balaur_plugin::load(&mut app, &mut ApplePlugin::default())?;
    app.add_plugin(UiPlugin)?;
    drive_ui_focus(&mut app);
    #[cfg(feature = "extensions")]
    load_project_extensions(&mut app)?;
    Ok(app)
}

/// Let a pad walk a menu, by mapping three actions onto the focus verbs.
///
/// Here rather than in either plugin: `balaur_ui` reads its input from egui,
/// which has keys but no pads, and `balaur_input` knows nothing about
/// widgets. This crate is the one that knows about both, which is what
/// assembling them means. A project that declares none of these three
/// actions gets a keyboard-only menu and no system worth the name.
fn drive_ui_focus(app: &mut App) {
    use balaur_ui::{Move, UiFocus};
    app.add_system(balaur_core::Stage::First, |eng: &balaur_core::Engine, _| {
        let Some(actions) = eng.try_resource::<balaur_input::InputActions>() else {
            return;
        };
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
    });
}

/// Load every extension in the project's `extensions/` directory.
///
/// # Errors
/// If a library fails to load, disagrees about the build, or requires
/// something absent.
#[cfg(feature = "extensions")]
fn load_project_extensions(app: &mut App) -> Result<()> {
    let dir = app.project_root().join("extensions");
    let modules = balaur_core::plugins::names(&app.engine);
    // Safety: opening a library runs its initialisers, and the fingerprint
    // check inside refuses a build that cannot share this process.
    let mut loaded = unsafe { balaur_plugin::load_extensions_in(&dir, &modules) }?;
    for extension in &mut loaded {
        let name = extension.manifest().name.clone();
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
