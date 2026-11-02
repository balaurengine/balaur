//! The walks over a scene file and its prefabs that build nothing: what a
//! preload warms, what an unload lets go of, and what a parked scene is held to.

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

/// When a scene's file and every prefab under it last changed, which a parked
/// scene is held to: an edit since it was built makes a return a fresh build.
#[derive(Default)]
pub struct FileStamps(Vec<(String, Option<f64>)>);

impl FileStamps {
    /// Whether nothing moved. Stamps nothing recorded (a scene a snapshot put
    /// back) have nothing to say.
    #[must_use]
    pub fn matches(&self, now: &Self) -> bool {
        self.0.is_empty() || self.0 == now.0
    }

    /// These stamps, leaving none.
    #[must_use]
    pub fn take(&mut self) -> Self {
        Self(std::mem::take(&mut self.0))
    }
}

/// [`FileStamps`] of the scene at `path`. A stat per file; a file that no
/// longer reads ends the walk, and the build that follows reports it.
pub(crate) fn file_stamps(eng: &Engine, path: &str) -> FileStamps {
    let mut stamps = Vec::new();
    stamp_walk(eng, path, &mut stamps);
    FileStamps(stamps)
}

fn stamp_walk(eng: &Engine, path: &str, stamps: &mut Vec<(String, Option<f64>)>) {
    if stamps.iter().any(|(done, _)| done == path) {
        return;
    }
    let Ok((_, doc)) = scene_doc(eng, path) else {
        return;
    };
    stamps.push((
        path.to_string(),
        path_of(eng, path).ok().and_then(|key| stamp_of(eng, &key)),
    ));
    for node in &doc.nodes {
        if let Some(prefab) = &node.instance {
            stamp_walk(eng, prefab, stamps);
        }
    }
}

/// The project's first scene: its text read by the caller, from a pack or a
/// file, and kept under `path` as a read here would keep it, so a switch back
/// and the stamps a park holds it to read it from the cache.
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
    instantiate_doc(eng, &doc, source, base, true)?;
    crate::parked::set_current(eng, Some(path));
    Ok(())
}
