//! `scene.switch(path)`: replace the root's children with another scene.
//!
//! Requested during a tick and applied at the end of it, so a script asking
//! for a switch inside `update` is not freeing the tree it is running in.
//! Reset is a switch to the same file, which is why there is no second verb.

use anyhow::Result;

use crate::Engine;

/// The switch asked for, waiting for the end of the tick.
#[derive(Default)]
pub struct Pending {
    /// The scene to build, or `None` when nothing asked.
    pub scene: Option<String>,
    /// Seconds the renderer fades over; zero cuts.
    pub fade: f32,
}

/// Ask for a switch. The last request in a tick is the one that happens: two
/// scripts both switching is a bug, and building both scenes would be worse.
pub fn request(eng: &Engine, scene: &str) {
    request_with_fade(eng, scene, 0.0);
}

pub fn request_with_fade(eng: &Engine, scene: &str, fade: f32) {
    let pending = eng.resource::<Pending>();
    let mut pending = pending.borrow_mut();
    pending.scene = Some(scene.to_string());
    pending.fade = fade.max(0.0);
}

/// How long the current switch fades for, for a renderer drawing one.
#[must_use]
pub fn fade(eng: &Engine) -> f32 {
    eng.resource::<Pending>().borrow().fade
}

/// Build what was asked for, if anything was.
pub fn apply_system(eng: &Engine, _dt: f32) {
    let asked = {
        let pending = eng.resource::<Pending>();
        let mut pending = pending.borrow_mut();
        pending.scene.take()
    };
    let Some(scene) = asked else {
        return;
    };
    if let Err(why) = switch_now(eng, &scene) {
        tracing::error!("scene.switch to '{scene}': {why:#}");
    }
}

fn switch_now(eng: &Engine, scene: &str) -> Result<()> {
    let root = eng.root();
    let children: Vec<crate::hecs::Entity> = {
        let world = eng.world();
        world
            .get::<&crate::scene::Children>(root)
            .map(|c| c.0.clone())
            .unwrap_or_default()
    };
    crate::scene::free_nodes(eng, &children);
    // Freeing is deferred to the end of the frame; the new tree is built
    // beside the old one and the old one goes, which is what keeps a node
    // being ticked from being freed under itself.
    let source = crate::project::scene_text(eng, scene)?;
    crate::project::instantiate_scene(eng, &source, root, true)?;
    tracing::info!("switched to {scene}");
    Ok(())
}
