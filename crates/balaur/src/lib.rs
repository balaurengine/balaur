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
#[cfg(feature = "audio")]
pub use balaur_audio::AudioPlugin;
pub use balaur_core::*;
#[cfg(feature = "gamend")]
pub use balaur_gamend::GamendPlugin;
pub use balaur_input::InputPlugin;
#[cfg(feature = "net")]
pub use balaur_net::NetPlugin;
pub use balaur_physics::PhysicsPlugin;
pub use balaur_render::RenderPlugin;
pub use balaur_ui::UiPlugin;

pub use balaur_anim as animation;
#[cfg(feature = "audio")]
pub use balaur_audio as audio;
#[cfg(feature = "gamend")]
pub use balaur_gamend as gamend;
pub use balaur_input as input;
#[cfg(feature = "net")]
pub use balaur_net as net;
pub use balaur_physics as physics;
pub use balaur_render as render;
pub use balaur_script_rune as rune;
pub use balaur_ui as ui;

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
    #[cfg(feature = "net")]
    balaur_plugin::load(&mut app, &mut NetPlugin::default())?;
    #[cfg(feature = "gamend")]
    balaur_plugin::load(&mut app, &mut GamendPlugin::default())?;
    app.add_plugin(UiPlugin)?;
    #[cfg(feature = "extensions")]
    load_project_extensions(&mut app)?;
    Ok(app)
}

/// Load every extension in the project's `extensions/` directory.
///
/// # Errors
/// If a library fails to load, disagrees about the build, or requires
/// something absent.
#[cfg(feature = "extensions")]
fn load_project_extensions(app: &mut App) -> Result<()> {
    let dir = app.project_root().join("extensions");
    // Safety: opening a library runs its initialisers, and the fingerprint
    // check inside refuses a build that cannot share this process.
    let mut loaded = unsafe { balaur_plugin::load_extensions_in(&dir) }?;
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
