//! Batteries-included entry points for Balaur games and tools.
//!
//! A shipped game binary is three lines:
//!
//! ```ignore
//! fn main() -> anyhow::Result<()> {
//!     balaur::boot_pack(include_bytes!(concat!(env!("OUT_DIR"), "/game.bpak")))
//! }
//! ```

pub use balaur_audio::AudioPlugin;
pub use balaur_core::*;
pub use balaur_input::InputPlugin;
pub use balaur_physics::PhysicsPlugin;
pub use balaur_render::RenderPlugin;
pub use balaur_ui::UiPlugin;

pub use balaur_audio as audio;
pub use balaur_input as input;
pub use balaur_physics as physics;
pub use balaur_render as render;
pub use balaur_ui as ui;

use anyhow::Result;

/// Build an [`App`] with the standard plugin set (input, physics, render,
/// audio).
pub fn standard_app(config: AppConfig) -> Result<App> {
    let mut app = App::new(config)?;
    app.add_plugin(InputPlugin)?;
    app.add_plugin(PhysicsPlugin)?;
    app.add_plugin(RenderPlugin)?;
    app.add_plugin(AudioPlugin)?;
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
    #[allow(unreachable_code)]
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
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "balaur".to_string());
    run(app, &title)
}

/// Shipping mode: run a precompiled pack (e.g. embedded via `include_bytes!`).
pub fn boot_pack(bytes: &[u8]) -> Result<()> {
    let pack = Pack::decode(bytes)?;
    let mut app = standard_app(AppConfig::packed(pack))?;
    app.load_project()?;
    let title = app
        .manifest()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "balaur".to_string());
    run(app, &title)
}
