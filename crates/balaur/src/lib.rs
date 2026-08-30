//! Batteries-included entry points for Balaur games and tools.
//!
//! A shipped game binary is three lines:
//!
//! ```ignore
//! fn main() -> anyhow::Result<()> {
//!     balaur::boot_pack(include_bytes!(concat!(env!("OUT_DIR"), "/game.bpak")))
//! }
//! ```

#[cfg(feature = "audio")]
pub use balaur_audio::AudioPlugin;
pub use balaur_core::*;
pub use balaur_input::InputPlugin;
pub use balaur_physics::PhysicsPlugin;
pub use balaur_render::RenderPlugin;
pub use balaur_ui::UiPlugin;

#[cfg(feature = "audio")]
pub use balaur_audio as audio;
pub use balaur_input as input;
pub use balaur_physics as physics;
pub use balaur_render as render;
pub use balaur_script_luau as luau;
pub use balaur_script_rune as rune;
pub use balaur_ui as ui;

use anyhow::{Context, Result};

/// Build an [`App`] with the standard plugin set (input, physics, render,
/// audio).
/// The script backend a project asks for in its `project.toml`.
///
/// One project, one language. Two backends can run side by side in a process,
/// but a callback minted by one is meaningless to the other, so mixing them in
/// a single project needs an id space they share.
fn backend_for(config: &AppConfig) -> Result<balaur_core::ScriptHostFactory> {
    let manifest = match &config.pack {
        Some(pack) => Some(pack.manifest.clone()),
        None => std::fs::read_to_string(config.project_root.join("project.toml")).ok(),
    };
    let language = manifest
        .as_deref()
        .and_then(|m| balaur_core::project::ProjectManifest::parse(m).ok())
        .map_or_else(|| "luau".to_string(), |m| m.language);
    match language.as_str() {
        "luau" | "lua" => Ok(balaur_script_luau::factory()),
        "rune" => Ok(balaur_script_rune::factory()),
        other => Err(anyhow::anyhow!(
            "project.toml asks for language \"{other}\"; this build has luau and rune"
        )),
    }
}

/// Build an export pack with the script backend `standard_app` installs.
///
/// Luau compiles a file on its own — names are resolved at run time, so
/// bytecode does not depend on which modules exist. Rune resolves `input::…`
/// while compiling, so its export has to run against the same modules the
/// game will have: boot the app the game would boot, and compile through its
/// host. Exporting through a bare context instead rejects every script that
/// touches the engine.
pub fn build_pack(project_root: &std::path::Path) -> Result<Pack> {
    let manifest = std::fs::read_to_string(project_root.join("project.toml"))?;
    match balaur_core::project::ProjectManifest::parse(&manifest)?
        .language
        .as_str()
    {
        "rune" => {
            let app = standard_app(AppConfig::export(project_root))?;
            let host = app
                .engine
                .script_host()
                .context("no script backend for a rune project")?;
            let host = host
                .as_any()
                .downcast_ref::<balaur_script_rune::RuneHost>()
                .context("expected the rune backend for a rune project")?;
            Pack::build(project_root, host)
        }
        _ => Pack::build(project_root, &balaur_script_luau::Compiler),
    }
}

pub fn standard_app(mut config: AppConfig) -> Result<App> {
    if config.script_backend.is_none() {
        config.script_backend = Some(backend_for(&config)?);
    }
    let mut app = App::new(config)?;
    app.add_plugin(InputPlugin)?;
    app.add_plugin(PhysicsPlugin)?;
    app.add_plugin(RenderPlugin)?;
    #[cfg(feature = "audio")]
    balaur_plugin::load(&mut app, &mut AudioPlugin::default())?;
    app.add_plugin(UiPlugin)?;
    Ok(app)
}

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
        Ok(())
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
