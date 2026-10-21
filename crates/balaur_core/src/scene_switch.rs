//! `scene.switch(path)`: replace the root's children with another scene.
//!
//! Requested during a tick and applied at the end of it, so a script asking
//! for a switch inside `update` is not freeing the tree it is running in.
//! Reset is a switch to the same file, which is why there is no second verb.

use anyhow::Result;

use crate::Engine;

/// The switch asked for, waiting for the end of the tick, and the crossover
/// in flight: with `fade`, the old scene's roots fade to the clear colour
/// over the first half, the switch happens, and the new scene's roots fade
/// in over the second.
#[derive(Default)]
pub struct Pending {
    /// The scene to build, or `None` when nothing asked.
    pub scene: Option<String>,
    /// Seconds the renderer fades over; zero cuts.
    pub fade: f32,
    crossing: Option<Crossing>,
}

/// A fade in progress: the scene still to switch to while the old roots
/// fade out (`None` once switched), half the fade in seconds, and what is
/// left of the half in progress.
struct Crossing {
    scene: Option<String>,
    half: f32,
    left: f32,
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

enum Step {
    Switch(String),
    Alpha(f32),
}

/// At the end of every tick: a cut switches now; a fade moves the roots'
/// tint alpha each tick, switching halfway, and propagates the tree so the
/// draw after this tick sees it.
pub fn apply_system(eng: &Engine, dt: f32) {
    let asked = {
        let pending = eng.resource::<Pending>();
        let mut pending = pending.borrow_mut();
        pending.scene.take().map(|scene| (scene, pending.fade))
    };
    if let Some((scene, fade)) = asked {
        if fade > 0.0 {
            let half = fade / 2.0;
            eng.resource::<Pending>().borrow_mut().crossing = Some(Crossing {
                scene: Some(scene),
                half,
                left: half,
            });
        } else {
            eng.resource::<Pending>().borrow_mut().crossing = None;
            if let Err(why) = switch_now(eng, &scene) {
                tracing::error!("scene.switch to '{scene}': {why:#}");
            }
            return;
        }
    }
    let step = {
        let pending = eng.resource::<Pending>();
        let mut pending = pending.borrow_mut();
        let Some(crossing) = pending.crossing.as_mut() else {
            return;
        };
        crossing.left -= dt;
        let share = (crossing.left / crossing.half).clamp(0.0, 1.0);
        match crossing.scene.take() {
            Some(scene) if crossing.left <= 0.0 => {
                crossing.left = crossing.half;
                Step::Switch(scene)
            }
            Some(scene) => {
                crossing.scene = Some(scene);
                Step::Alpha(share)
            }
            None if crossing.left <= 0.0 => {
                pending.crossing = None;
                Step::Alpha(1.0)
            }
            None => Step::Alpha(1.0 - share),
        }
    };
    match step {
        Step::Switch(scene) => {
            if let Err(why) = switch_now(eng, &scene) {
                tracing::error!("scene.switch to '{scene}': {why:#}");
                eng.resource::<Pending>().borrow_mut().crossing = None;
                roots_alpha(eng, 1.0);
            } else {
                roots_alpha(eng, 0.0);
            }
        }
        Step::Alpha(alpha) => roots_alpha(eng, alpha),
    }
}

/// Every root's tint alpha, then the tree propagated so this tick's draw
/// reads it.
fn roots_alpha(eng: &Engine, alpha: f32) {
    let root = eng.root();
    {
        let world = eng.world();
        let children: Vec<crate::hecs::Entity> = world
            .get::<&crate::scene::Children>(root)
            .map(|c| c.0.clone())
            .unwrap_or_default();
        for child in children {
            if let Ok(mut appearance) = world.get::<&mut crate::scene::Appearance>(child) {
                appearance.tint.w = alpha;
            }
        }
    }
    crate::scene::propagate_transforms(&mut eng.world_mut(), root);
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
    crate::project::instantiate_scene_file(eng, scene, root, true)?;
    // The draw after this tick reads the new tree's inherited appearance,
    // and the pass that propagates it has already run: compose it now, or
    // every node authored hidden shows for one frame.
    crate::scene::propagate_transforms(&mut eng.world_mut(), root);
    tracing::info!("switched to {scene}");
    Ok(())
}
