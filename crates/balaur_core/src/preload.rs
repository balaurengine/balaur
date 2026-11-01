//! Readying a scene before anything switches to it.
//!
//! A switch reads the scene file, every prefab it instances, and whatever
//! their components name -- the pictures a screen draws, say -- on the frame
//! it happens. [`scene`] does the reading and parsing a frame ahead, or at
//! boot for every screen an app has, and hands each node's component values
//! to the plugins that asked ([`on_component`]), so a plugin can start its
//! own slow part off the frame: the UI decodes a widget's picture on a worker
//! thread and uploads it in a later pass.
//!
//! A packed game warms every scene it ships, one a frame, once its first
//! frame is out (`warm_system`). A dev run reads the sources as it goes.
//!
//! Nothing here touches the world: a preloaded scene is warm caches, not
//! nodes, so a replay that never preloaded builds the same world. [`unload`]
//! is its inverse, and also lets go of the built scene a switch parked
//! ([`crate::parked`]), which is world state.

use anyhow::Result;
use smol_str::SmolStr;

use crate::collections::DetHashMap;
use crate::engine::Engine;

/// What a plugin does with one component's values in a scene being preloaded
/// or unloaded: the table a node, or an override inside an instance, writes.
pub type PreloadFn = fn(&Engine, &toml::Value);

/// A plugin's pair for one component: start what a preload readies, and let
/// it go.
#[derive(Clone, Copy)]
pub struct Warming {
    pub preload: PreloadFn,
    pub unload: PreloadFn,
}

/// Each component's preloaders, by the scene key it is written under.
#[derive(Default)]
pub struct Preloaders(DetHashMap<SmolStr, Vec<Warming>>);

/// Hand `component`'s values in every scene preloaded or unloaded to `warming`.
pub fn on_component(eng: &Engine, component: &str, warming: Warming) {
    if eng.try_resource::<Preloaders>().is_none() {
        eng.insert_resource(Preloaders::default());
    }
    eng.resource::<Preloaders>()
        .borrow_mut()
        .0
        .entry(component.into())
        .or_default()
        .push(warming);
}

/// Read and parse the scene at `path` and every prefab under it, and hand
/// each component value to its preloaders.
///
/// # Errors
/// If a file does not read or a document does not parse: what a switch to it
/// would have said, said now.
pub fn scene(eng: &Engine, path: &str) -> Result<()> {
    crate::project::preload_scene(eng, path)
}

/// Let go of what [`scene`] warmed and a switch kept: the parked scene, if it
/// is parked, and whatever its file and prefabs have cached.
pub fn unload(eng: &Engine, path: &str) {
    crate::parked::free(eng, path);
    crate::project::unload_scene(eng, path);
}

/// The scenes a pack ships that are still to warm.
#[derive(Default)]
pub struct ToWarm(std::collections::VecDeque<String>);

/// Queue every scene of `pack` but the one already built.
pub(crate) fn queue_pack(eng: &Engine, pack: Option<&crate::pack::Pack>, main_scene: &str) {
    let Some(pack) = pack else {
        return;
    };
    let scenes = pack
        .scenes
        .iter()
        .filter(|(path, text)| path.as_str() != main_scene && text.contains("[[nodes]]"))
        .map(|(path, _)| path.clone())
        .collect();
    eng.insert_resource(ToWarm(scenes));
}

/// One scene a frame, each once the frame is drawn: a memory warning ends it,
/// the way it frees what a switch parked. A scene that does not read is left
/// for the switch to it to report.
pub(crate) fn warm_system(eng: &Engine, _dt: f32) {
    let Some(waiting) = eng.try_resource::<ToWarm>() else {
        return;
    };
    if waiting.borrow().0.is_empty() {
        return;
    }
    if crate::facts::device(eng).memory_warnings > 0 {
        waiting.borrow_mut().0.clear();
        return;
    }
    let next = waiting.borrow_mut().0.pop_front();
    if let Some(path) = next
        && let Err(why) = scene(eng, &path)
    {
        tracing::debug!("warming {path}: {why:#}");
    }
}

/// One scene key's value, to whichever preloaders its component has.
pub(crate) fn feed(eng: &Engine, key: &str, value: &toml::Value) {
    for warming in warmings(eng, key) {
        (warming.preload)(eng, value);
    }
}

/// One scene key's value, to whichever unloaders its component has.
pub(crate) fn drop_value(eng: &Engine, key: &str, value: &toml::Value) {
    for warming in warmings(eng, key) {
        (warming.unload)(eng, value);
    }
}

fn warmings(eng: &Engine, key: &str) -> Vec<Warming> {
    eng.try_resource::<Preloaders>()
        .and_then(|preloaders| preloaders.borrow().0.get(key).cloned())
        .unwrap_or_default()
}
