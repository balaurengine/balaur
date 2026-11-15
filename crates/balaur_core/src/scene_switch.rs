//! `scene.switch(path)`: free the scene the last switch built under a node,
//! and build another there.
//!
//! Requested during a tick and applied at the end of it, so a script asking
//! for a switch inside `update` is not freeing the tree it is running in.
//! Reset is a switch to the same file, which is why there is no second verb.
//! Only the scene's own root goes: a node beside it under the same parent,
//! such as one a script made under the tree's root, stays.

use anyhow::{Result, anyhow};
use hecs::Entity;

use crate::Engine;
use crate::scene::{Appearance, Children};

/// One switch: the scene file, the node it goes under (`None` for the tree's
/// root), and the seconds of crossover it fades through; zero cuts.
#[derive(Clone, Debug)]
pub struct Request {
    pub scene: String,
    pub under: Option<Entity>,
    pub fade: f32,
}

/// The switches asked for this tick, the last per node, and the fades in
/// flight: the old scene's root fades to the clear colour over the first half,
/// the switch happens, and the new one fades in over the second.
#[derive(Default)]
pub struct Pending {
    asked: Vec<Request>,
    crossings: Vec<Crossing>,
}

/// A fade in progress under `under`: the scene still to switch to while the
/// old root fades out (`None` once switched), half the fade in seconds, and
/// what is left of the half in progress.
struct Crossing {
    under: Entity,
    scene: Option<String>,
    half: f32,
    left: f32,
}

/// What a switch built: the node it went under, the file, and the root it
/// made there, which the next switch under that node frees.
#[derive(Clone, Debug)]
pub struct Built {
    pub under: Entity,
    pub path: String,
    pub root: Entity,
}

/// Every scene a switch, or the run's start, built and that still stands.
#[derive(Default)]
pub struct Scenes(Vec<Built>);

/// Ask for a cut to `scene` under the tree's root.
pub fn request(eng: &Engine, scene: &str) {
    ask(
        eng,
        Request {
            scene: scene.to_string(),
            under: None,
            fade: 0.0,
        },
    );
}

/// Ask for a switch. The last request for a node in a tick is the one that
/// happens: two scripts switching one place is a bug, and building both
/// scenes would be worse.
pub fn ask(eng: &Engine, mut asked: Request) {
    asked.fade = asked.fade.max(0.0);
    let pending = eng.resource::<Pending>();
    let mut pending = pending.borrow_mut();
    pending.asked.retain(|was| was.under != asked.under);
    pending.asked.push(asked);
}

/// The scene the last switch built under `under`, the tree's root for
/// `None`: its file and its root, while that root stands.
#[must_use]
pub fn current(eng: &Engine, under: Option<Entity>) -> Option<Built> {
    let under = under.unwrap_or_else(|| eng.root());
    let scenes = eng.try_resource::<Scenes>()?;
    let world = eng.world();
    scenes
        .borrow()
        .0
        .iter()
        .find(|built| built.under == under && world.contains(built.root))
        .cloned()
}

/// Every scene standing, in the order they were built.
#[must_use]
pub fn all(eng: &Engine) -> Vec<Built> {
    eng.try_resource::<Scenes>()
        .map(|scenes| scenes.borrow().0.clone())
        .unwrap_or_default()
}

/// Say what stands, as the run's start and a snapshot's restore know it.
pub fn set_all(eng: &Engine, built: Vec<Built>) {
    if eng.try_resource::<Scenes>().is_none() {
        eng.insert_resource(Scenes::default());
    }
    eng.resource::<Scenes>().borrow_mut().0 = built;
}

/// Note a scene built under `under` outside a switch: the main scene.
pub(crate) fn note_built(eng: &Engine, under: Entity, path: &str, root: Entity) {
    let mut built = all(eng);
    built.retain(|was| was.under != under);
    built.push(Built {
        under,
        path: path.to_string(),
        root,
    });
    set_all(eng, built);
}

enum Step {
    Switch(Entity, String),
    Alpha(Entity, f32),
    Done(Entity),
}

/// At the end of every tick: a cut switches now; a fade moves the root's tint
/// alpha each tick, switching halfway, and propagates the tree so the draw
/// after this tick sees it.
pub fn apply_system(eng: &Engine, dt: f32) {
    let asked: Vec<Request> = {
        let pending = eng.resource::<Pending>();
        let mut pending = pending.borrow_mut();
        // A scene naming what the pack's tail holds waits for the tail.
        let (waiting, ready): (Vec<Request>, Vec<Request>) = std::mem::take(&mut pending.asked)
            .into_iter()
            .partition(|asked| crate::pack_tail::waits(eng, &asked.scene));
        pending.asked = waiting;
        ready
    };
    for request in asked {
        let under = request.under.unwrap_or_else(|| eng.root());
        let pending = eng.resource::<Pending>();
        pending.borrow_mut().crossings.retain(|c| c.under != under);
        if request.fade > 0.0 {
            let half = request.fade / 2.0;
            pending.borrow_mut().crossings.push(Crossing {
                under,
                scene: Some(request.scene),
                half,
                left: half,
            });
        } else if let Err(why) = switch_now(eng, &request.scene, under) {
            tracing::error!("scene.switch to '{}': {why:#}", request.scene);
        }
    }
    for step in fade_steps(eng, dt) {
        match step {
            Step::Switch(under, scene) => {
                if let Err(why) = switch_now(eng, &scene, under) {
                    tracing::error!("scene.switch to '{scene}': {why:#}");
                    let pending = eng.resource::<Pending>();
                    pending.borrow_mut().crossings.retain(|c| c.under != under);
                    root_alpha(eng, under, 1.0);
                } else {
                    root_alpha(eng, under, 0.0);
                }
            }
            Step::Alpha(under, alpha) => root_alpha(eng, under, alpha),
            Step::Done(under) => root_alpha(eng, under, 1.0),
        }
    }
}

/// Each fade's move this tick, the finished ones taken off the list.
fn fade_steps(eng: &Engine, dt: f32) -> Vec<Step> {
    let pending = eng.resource::<Pending>();
    let mut pending = pending.borrow_mut();
    let mut steps = Vec::new();
    pending.crossings.retain_mut(|crossing| {
        crossing.left -= dt;
        let share = (crossing.left / crossing.half).clamp(0.0, 1.0);
        match crossing.scene.take() {
            Some(scene) if crossing.left <= 0.0 => {
                crossing.left = crossing.half;
                steps.push(Step::Switch(crossing.under, scene));
                true
            }
            Some(scene) => {
                crossing.scene = Some(scene);
                steps.push(Step::Alpha(crossing.under, share));
                true
            }
            None if crossing.left <= 0.0 => {
                steps.push(Step::Done(crossing.under));
                false
            }
            None => {
                steps.push(Step::Alpha(crossing.under, 1.0 - share));
                true
            }
        }
    });
    steps
}

/// The tint alpha of the scene standing under `under`, then the tree
/// propagated so this tick's draw reads it.
fn root_alpha(eng: &Engine, under: Entity, alpha: f32) {
    if let Some(built) = current(eng, Some(under)) {
        let world = eng.world();
        if let Ok(mut appearance) = world.get::<&mut Appearance>(built.root) {
            appearance.tint.w = alpha;
        }
    }
    crate::scene::propagate_transforms(&mut eng.world_mut(), eng.root());
}

fn switch_now(eng: &Engine, scene: &str, under: Entity) -> Result<()> {
    crate::timings::measure(eng, "scene/switch", || switch_to(eng, scene, under))
}

fn children_of(eng: &Engine, under: Entity) -> Vec<Entity> {
    eng.world()
        .get::<&Children>(under)
        .map(|c| c.0.clone())
        .unwrap_or_default()
}

fn switch_to(eng: &Engine, scene: &str, under: Entity) -> Result<()> {
    if !eng.world().contains(under) {
        return Err(anyhow!("the node it was to go under is gone"));
    }
    if let Some(left) = current(eng, Some(under)) {
        crate::scene::free_nodes(eng, &[left.root]);
    }
    let mut built = all(eng);
    {
        let world = eng.world();
        built.retain(|was| was.under != under && world.contains(was.root));
    }
    set_all(eng, built);
    let before = children_of(eng, under);
    crate::project::instantiate_scene_file(eng, scene, under, true)?;
    // A file with no nodes builds nothing, and nothing stands for the next to free.
    if let Some(made) = children_of(eng, under)
        .into_iter()
        .find(|child| !before.contains(child))
    {
        note_built(eng, under, scene, made);
    }
    // The draw after this tick reads the new tree's inherited appearance,
    // and the pass that propagates it has already run: compose it now, or
    // every node authored hidden shows for one frame.
    crate::scene::propagate_transforms(&mut eng.world_mut(), eng.root());
    tracing::info!("switched to {scene}");
    Ok(())
}
