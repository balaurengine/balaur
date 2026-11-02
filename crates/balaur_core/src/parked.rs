//! Scenes kept built after a switch: every `scene.switch` parks the scene it
//! leaves, and a switch back returns it.
//!
//! Parking hands a scene's roots from the tree's root to a holder of their
//! own: a node outside the tree, hidden, with `process = disabled`. Whatever
//! walks from the root no longer reaches them, whatever asks the process mode
//! stops, and whatever draws reads them as hidden. The script host takes their
//! instances out of every pass and call, and a plugin holding state of its own
//! beside a node hears through [`on_change`]. Returning hands the roots back:
//! no node is built and no `init` runs.
//!
//! A parked tree is world state, so a digest and a snapshot cover it. Each
//! labels a parked node `<path>|<id>` ([`crate::ids::key_of`]), since two
//! screens' files commonly hold one id. A save holds what a game puts in it,
//! so nothing here reaches one. A park or a return is not an edit, so neither
//! enters the change log, as a switch does not.
//!
//! Three things free a parked scene: going over [`MAX_PARKED_NODES`], a system
//! memory warning, and [`crate::preload::unload`], the inverse of the preload
//! that warmed it. A scene freed is built again the next time it is asked for,
//! its `init` running, as if it had never been visited.

use anyhow::{Result, anyhow};
use hecs::{Entity, World};

use crate::engine::Engine;
use crate::project::FileStamps;
use crate::scene::{Children, NodeScript, Parent};

/// How many nodes stay parked, the scene left longest ago freed first. The
/// same on every machine: a parked tree is in the digest, and a budget read off
/// the memory a machine has would part two runs of one recording.
pub const MAX_PARKED_NODES: usize = 20_000;

/// The component on a holder: which scene hangs from it, and the root of the
/// tree it left, which carries the id counter.
pub struct Held {
    pub path: String,
    pub root: Entity,
}

/// What a plugin does as `nodes` leave the tree (`true`) or come back. Told
/// on a snapshot's restore as well, before the plugin's own source runs.
pub type ParkFn = fn(&Engine, &[Entity], bool);

struct ParkedScene {
    path: String,
    holder: Entity,
    nodes: usize,
    /// When its files last changed as it was built. A file edited since makes
    /// a return a fresh build, or an edit would never show on a revisit.
    built_from: FileStamps,
}

/// The scene under the root, and every scene parked, oldest first.
#[derive(Default)]
pub struct Parked {
    /// The file the root's children were built from; `None` for a tree built
    /// by hand, which has no path to come back by.
    current: Option<String>,
    built_from: FileStamps,
    scenes: Vec<ParkedScene>,
    hooks: Vec<ParkFn>,
}

/// A scene file's name as the parked list keeps it.
fn key(path: &str) -> &str {
    path.trim_start_matches("./")
}

/// Tell `hook` whenever nodes are parked or come back.
pub fn on_change(eng: &Engine, hook: ParkFn) {
    if let Some(parked) = eng.try_resource::<Parked>() {
        parked.borrow_mut().hooks.push(hook);
    }
}

/// Say which file the tree under the root was built from.
pub fn set_current(eng: &Engine, path: Option<&str>) {
    let built_from = path.map_or_else(FileStamps::default, |path| {
        crate::project::file_stamps(eng, path)
    });
    if let Some(parked) = eng.try_resource::<Parked>() {
        let mut parked = parked.borrow_mut();
        parked.current = path.map(|path| key(path).to_string());
        parked.built_from = built_from;
    }
}

/// The file the tree under the root was built from.
#[must_use]
pub fn current(eng: &Engine) -> Option<String> {
    eng.try_resource::<Parked>()?.borrow().current.clone()
}

/// Whether `path` is the file the tree under the root was built from.
#[must_use]
pub fn is_current(eng: &Engine, path: &str) -> bool {
    eng.try_resource::<Parked>()
        .is_some_and(|parked| parked.borrow().current.as_deref() == Some(key(path)))
}

/// Every parked scene's path, oldest first.
#[must_use]
pub fn scenes(eng: &Engine) -> Vec<String> {
    eng.try_resource::<Parked>()
        .map_or_else(Vec::new, |parked| {
            let parked = parked.borrow();
            parked.scenes.iter().map(|at| at.path.clone()).collect()
        })
}

/// Every parked scene with its holder, oldest first.
#[must_use]
pub fn holders(eng: &Engine) -> Vec<(String, Entity)> {
    eng.try_resource::<Parked>()
        .map_or_else(Vec::new, |parked| {
            let parked = parked.borrow();
            let row = |at: &ParkedScene| (at.path.clone(), at.holder);
            parked.scenes.iter().map(row).collect()
        })
}

/// The parked scene `entity` hangs in, read from the holder over it.
#[must_use]
pub fn path_of(world: &World, mut entity: Entity) -> Option<String> {
    while let Ok(parent) = world.get::<&Parent>(entity) {
        entity = parent.0;
    }
    world
        .get::<&Held>(entity)
        .ok()
        .map(|held| held.path.clone())
}

/// Whether `entity` is in a parked scene. One compare while nothing is parked.
#[must_use]
pub fn is_parked(eng: &Engine, entity: Entity) -> bool {
    let any = eng
        .try_resource::<Parked>()
        .is_some_and(|parked| !parked.borrow().scenes.is_empty());
    any && path_of(&eng.world(), entity).is_some()
}

/// The holder of the parked scene `path`, read from the world.
#[must_use]
pub fn holder_named(world: &World, path: &str) -> Option<Entity> {
    world
        .query::<(Entity, &Held)>()
        .iter()
        .find(|(_, held)| held.path == path)
        .map(|(holder, _)| holder)
}

/// A refusal for a script reaching into a parked scene to change its shape: a
/// parked tree takes no node and no script until it returns.
///
/// # Errors
/// When `entity` is in a parked scene.
pub fn refuse_if_parked(eng: &Engine, entity: Entity, doing: &str) -> Result<()> {
    if is_parked(eng, entity) {
        return Err(anyhow!("{doing}: the node is in a parked scene"));
    }
    Ok(())
}

/// Every node of the world's scenes, the one under the root and then each
/// parked, holders left out: what a snapshot saves and restores.
#[must_use]
pub fn every_node(eng: &Engine, world: &World) -> Vec<Entity> {
    let mut nodes = crate::scene::collect_subtree(world, eng.root());
    for (_, holder) in holders(eng) {
        for at in crate::scene::collect_subtree(world, holder) {
            if at != holder {
                nodes.push(at);
            }
        }
    }
    nodes
}

fn children_of(world: &World, parent: Entity) -> Vec<Entity> {
    world
        .get::<&Children>(parent)
        .map(|c| c.0.clone())
        .unwrap_or_default()
}

/// `roots` and everything under them.
fn subtrees(world: &World, roots: &[Entity]) -> Vec<Entity> {
    let mut nodes = Vec::new();
    for &at in roots {
        crate::scene::collect_subtree_into(world, at, &mut nodes);
    }
    nodes
}

/// Hand `children` to `parent`, in their order. Their local poses stand: the
/// tree's root and a holder both sit at the origin, so a park or a return has
/// nothing to work out again, unlike [`crate::scene::reparent`].
fn hand_over(world: &mut World, children: &[Entity], parent: Entity) {
    for &child in children {
        if let Ok(was) = world.get::<&Parent>(child).map(|p| p.0) {
            crate::scene::detach(world, was, child);
        }
        let name = world
            .get::<&crate::scene::Name>(child)
            .map(|n| n.0.clone())
            .unwrap_or_default();
        if world.insert_one(child, Parent(parent)).is_ok() {
            crate::scene::attach(world, parent, &name, child);
        }
    }
}

/// Each script's `on_parked_changed`, on the nodes that name one.
fn tell_scripts(eng: &Engine, nodes: &[Entity], parked: bool) {
    let Some(host) = eng.script_host() else {
        return;
    };
    let scripted: Vec<Entity> = {
        let world = eng.world();
        let named = |node: &&Entity| world.get::<&NodeScript>(**node).is_ok();
        nodes.iter().filter(named).copied().collect()
    };
    let told = [balaur_script::Value::Bool(parked)];
    for node in scripted {
        host.call_on(
            crate::node_id_of(node),
            crate::hooks::ON_PARKED_CHANGED,
            &told,
        );
    }
}

fn tell_host(eng: &Engine, nodes: &[Entity], parked: bool) {
    if let Some(host) = eng.script_host() {
        let ids: Vec<_> = nodes.iter().map(|&node| crate::node_id_of(node)).collect();
        host.set_parked(&ids, parked);
    }
}

fn tell_plugins(eng: &Engine, nodes: &[Entity], parked: bool) {
    let hooks = eng
        .try_resource::<Parked>()
        .map_or_else(Vec::new, |state| state.borrow().hooks.clone());
    for hook in hooks {
        hook(eng, nodes, parked);
    }
}

/// Park the tree under the root under the file it was built from. A tree with
/// no file to come back by, or past the budget alone, is freed.
pub(crate) fn park_current(eng: &Engine) {
    let root = eng.root();
    let roots = children_of(&eng.world(), root);
    if roots.is_empty() {
        return;
    }
    let Some(path) = current(eng) else {
        crate::scene::free_nodes(eng, &roots);
        return;
    };
    let nodes = subtrees(&eng.world(), &roots);
    if nodes.len() > MAX_PARKED_NODES {
        crate::scene::free_nodes(eng, &roots);
        return;
    }
    // While it is all still in the tree, so a script can read what it leaves.
    tell_scripts(eng, &nodes, true);
    tell_host(eng, &nodes, true);
    tell_plugins(eng, &nodes, true);
    let built_from = eng.resource::<Parked>().borrow_mut().built_from.take();
    let holder = hold(eng, &path, nodes.len(), built_from);
    {
        let mut world = eng.world_mut();
        hand_over(&mut world, &roots, holder);
        crate::scene::propagate_transforms(&mut world, holder);
    }
    free_past_budget(eng);
}

/// Free the scenes left longest ago until what stays fits the budget.
fn free_past_budget(eng: &Engine) {
    let over = |parked: &Parked| {
        let held: usize = parked.scenes.iter().map(|at| at.nodes).sum();
        (held > MAX_PARKED_NODES).then(|| parked.scenes[0].path.clone())
    };
    while let Some(oldest) = eng.try_resource::<Parked>().and_then(|p| over(&p.borrow())) {
        free(eng, &oldest);
    }
}

/// The holder a scene hangs from, and its place in the list.
fn hold(eng: &Engine, path: &str, nodes: usize, built_from: FileStamps) -> Entity {
    let held = Held {
        path: path.to_string(),
        root: eng.root(),
    };
    let holder = crate::scene::spawn_holder(&mut eng.world_mut(), held);
    if let Some(parked) = eng.try_resource::<Parked>() {
        parked.borrow_mut().scenes.push(ParkedScene {
            path: path.to_string(),
            holder,
            nodes,
            built_from,
        });
    }
    holder
}

/// Take a scene off the list.
fn unlist(eng: &Engine, path: &str) -> Option<ParkedScene> {
    let parked = eng.try_resource::<Parked>()?;
    let mut parked = parked.borrow_mut();
    let at = parked.scenes.iter().position(|at| at.path == path)?;
    Some(parked.scenes.remove(at))
}

/// Bring a parked scene back under the root, answering whether one came. A
/// scene whose files changed since it was built is freed instead.
pub(crate) fn unpark(eng: &Engine, path: &str) -> bool {
    let path = key(path);
    let Some(scene) = unlist(eng, path) else {
        return false;
    };
    let now = crate::project::file_stamps(eng, path);
    if !scene.built_from.matches(&now) {
        crate::scene::free_nodes(eng, &[scene.holder]);
        return false;
    }
    let root = eng.root();
    let (roots, nodes) = {
        let world = eng.world();
        let roots = children_of(&world, scene.holder);
        let nodes = subtrees(&world, &roots);
        (roots, nodes)
    };
    {
        let mut world = eng.world_mut();
        hand_over(&mut world, &roots, root);
        let _ = world.despawn(scene.holder);
        // A script told it is back reads where it stands.
        crate::scene::propagate_transforms(&mut world, root);
    }
    if let Some(parked) = eng.try_resource::<Parked>() {
        let mut parked = parked.borrow_mut();
        parked.current = Some(path.to_string());
        parked.built_from = now;
    }
    tell_host(eng, &nodes, false);
    tell_plugins(eng, &nodes, false);
    tell_scripts(eng, &nodes, false);
    true
}

/// Free one parked scene, answering whether one was there. Each script's
/// `on_free` runs, as for any node freed.
pub fn free(eng: &Engine, path: &str) -> bool {
    let Some(scene) = unlist(eng, key(path)) else {
        return false;
    };
    crate::scene::free_nodes(eng, &[scene.holder]);
    true
}

/// Free every parked scene.
pub fn free_all(eng: &Engine) {
    for path in scenes(eng) {
        free(eng, &path);
    }
}

/// Make sure the parked list holds these scenes before the restore puts their
/// nodes back. What it holds besides them goes in [`settle_restored`].
pub(crate) fn hold_restored(eng: &Engine, wanted: &[String]) {
    let held = scenes(eng);
    for path in wanted {
        if !held.contains(path) {
            hold(eng, path, 0, FileStamps::default());
        }
    }
}

/// End a restore: scenes the snapshot did not hold are gone, the file is the
/// snapshot's, and the host and the plugins agree with where each node is now.
pub(crate) fn settle_restored(
    eng: &Engine,
    was: &[String],
    current: Option<&str>,
    wanted: &[String],
) {
    set_current(eng, current);
    for path in was {
        if !wanted.contains(path) {
            free(eng, path);
        }
    }
    if was.is_empty() && wanted.is_empty() {
        return;
    }
    // In the frame's order: a digest walks them in this one.
    if let Some(parked) = eng.try_resource::<Parked>() {
        parked
            .borrow_mut()
            .scenes
            .sort_by_key(|at| wanted.iter().position(|held| *held == at.path));
    }
    let live = crate::scene::collect_subtree(&eng.world(), eng.root());
    tell_host(eng, &live, false);
    tell_plugins(eng, &live, false);
    for (path, holder) in holders(eng) {
        let nodes = {
            let world = eng.world();
            let roots = children_of(&world, holder);
            subtrees(&world, &roots)
        };
        tell_host(eng, &nodes, true);
        tell_plugins(eng, &nodes, true);
        crate::scene::propagate_transforms(&mut eng.world_mut(), holder);
        if let Some(parked) = eng.try_resource::<Parked>()
            && let Some(at) = parked
                .borrow_mut()
                .scenes
                .iter_mut()
                .find(|at| at.path == path)
        {
            at.nodes = nodes.len();
        }
    }
}
