//! The walks over a scene file and its prefabs that build nothing: what a
//! preload warms and what an unload lets go of.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

/// [`crate::preload::scene`]: the scene at `path` and every prefab under it
/// parsed into [`SceneFiles`], each component value handed to its
/// preloaders. A prefab instanced twice is walked once.
pub(crate) fn preload_scene(eng: &Engine, path: &str) -> Result<()> {
    let mut seen = Vec::new();
    preload_walk(eng, path, &mut seen)
}

fn preload_walk(eng: &Engine, path: &str, seen: &mut Vec<String>) -> Result<()> {
    if seen.iter().any(|done| done == path) {
        return Ok(());
    }
    seen.push(path.to_string());
    let (_, doc) = scene_doc(eng, path)?;
    for node in &doc.nodes {
        for (key, value) in &node.extra {
            crate::preload::feed(eng, key, value);
        }
        for keys in node.overrides.values().filter_map(toml::Value::as_table) {
            for (key, value) in keys {
                crate::preload::feed(eng, key, value);
            }
        }
        if let Some(prefab) = &node.instance {
            preload_walk(eng, prefab, seen)?;
        }
    }
    Ok(())
}

/// Whether the scene at `path` and every prefab under it have each component
/// value ready ([`crate::preload::ready`]). Reads a file not read yet.
pub(crate) fn scene_ready(eng: &Engine, path: &str) -> Result<bool> {
    let mut seen = Vec::new();
    ready_walk(eng, path, &mut seen)
}

fn ready_walk(eng: &Engine, path: &str, seen: &mut Vec<String>) -> Result<bool> {
    if seen.iter().any(|done| done == path) {
        return Ok(true);
    }
    seen.push(path.to_string());
    let (_, doc) = scene_doc(eng, path)?;
    for node in &doc.nodes {
        let own = node.extra.iter();
        let overridden = node
            .overrides
            .values()
            .filter_map(toml::Value::as_table)
            .flat_map(|keys| keys.iter());
        for (key, value) in own.chain(overridden) {
            if !crate::preload::value_ready(eng, key, value) {
                return Ok(false);
            }
        }
        if let Some(prefab) = &node.instance
            && !ready_walk(eng, prefab, seen)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Let go of the parsed text of the scene at `path` and every prefab under it
/// that is cached, each component value handed to its unloaders. A file no one
/// has read has nothing to let go of.
pub(crate) fn unload_scene(eng: &Engine, path: &str) {
    let mut seen = Vec::new();
    unload_walk(eng, path, &mut seen);
}

fn unload_walk(eng: &Engine, path: &str, seen: &mut Vec<String>) {
    if seen.iter().any(|done| done == path) {
        return;
    }
    seen.push(path.to_string());
    let Ok(key) = path_of(eng, path) else {
        return;
    };
    let Some(file) = eng
        .try_resource::<SceneFiles>()
        .and_then(|files| files.borrow_mut().0.shift_remove(&key))
    else {
        return;
    };
    for node in &file.doc.nodes {
        for (key, value) in &node.extra {
            crate::preload::drop_value(eng, key, value);
        }
        for keys in node.overrides.values().filter_map(toml::Value::as_table) {
            for (key, value) in keys {
                crate::preload::drop_value(eng, key, value);
            }
        }
        if let Some(prefab) = &node.instance {
            unload_walk(eng, prefab, seen);
        }
    }
}

/// The project's first scene: its text read by the caller, from a pack or a
/// file, and kept under `path` as a read here would keep it, so a switch back
/// reads it from the cache.
pub(crate) fn instantiate_main_scene(
    eng: &Engine,
    path: &str,
    source: &str,
    base: Entity,
) -> Result<()> {
    let doc = std::rc::Rc::new(parse_scene(eng, source, None)?);
    // A harness's own scene may sit outside the project: read, but not kept.
    if let Ok(key) = path_of(eng, path) {
        let stamp = stamp_of(eng, &key);
        let files = eng.try_resource::<SceneFiles>().unwrap_or_else(|| {
            eng.insert_resource(SceneFiles::default());
            eng.resource::<SceneFiles>()
        });
        files.borrow_mut().0.insert(
            key,
            SceneFile {
                stamp,
                source: source.into(),
                doc: doc.clone(),
            },
        );
    }
    let before = child_list(eng, base);
    instantiate_doc(eng, &doc, source, base, true, String::new())?;
    if let Some(made) = child_list(eng, base)
        .into_iter()
        .find(|child| !before.contains(child))
    {
        crate::scene_switch::note_built(eng, base, path, made);
    }
    Ok(())
}

fn child_list(eng: &Engine, parent: Entity) -> Vec<Entity> {
    eng.world()
        .get::<&crate::scene::Children>(parent)
        .map(|c| c.0.clone())
        .unwrap_or_default()
}
