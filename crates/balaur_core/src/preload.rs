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
//! Nothing here touches the world: a preloaded scene is warm caches, not
//! nodes, so a replay that never preloaded builds the same world.

use anyhow::Result;
use smol_str::SmolStr;

use crate::collections::DetHashMap;
use crate::engine::Engine;

/// What a plugin does with one component's values in a scene being
/// preloaded: the table a node, or an override inside an instance, writes.
pub type PreloadFn = fn(&Engine, &toml::Value);

/// Each component's preloaders, by the scene key it is written under.
#[derive(Default)]
pub struct Preloaders(DetHashMap<SmolStr, Vec<PreloadFn>>);

/// Hand `component`'s values in every preloaded scene to `preload`.
pub fn on_component(eng: &Engine, component: &str, preload: PreloadFn) {
    if eng.try_resource::<Preloaders>().is_none() {
        eng.insert_resource(Preloaders::default());
    }
    eng.resource::<Preloaders>()
        .borrow_mut()
        .0
        .entry(component.into())
        .or_default()
        .push(preload);
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

/// One scene key's value, to whichever preloaders its component has.
pub(crate) fn feed(eng: &Engine, key: &str, value: &toml::Value) {
    let Some(preloaders) = eng.try_resource::<Preloaders>() else {
        return;
    };
    let found = preloaders.borrow().0.get(key).cloned();
    for preload in found.into_iter().flatten() {
        preload(eng, value);
    }
}
