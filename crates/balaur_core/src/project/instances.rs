//! A prefab instanced into a scene: its nodes built under the instance's
//! root, the instance's own keys applied, and its overrides laid over them.

use super::problems::{Level, checking, keep, property_of};
#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;
use crate::vocabulary::keys;
use crate::vocabulary::words;

/// Build the prefab `node` names into `entity`, which is its root.
pub(super) fn instance(
    eng: &Engine,
    node: &SceneNode,
    id: &str,
    entity: Entity,
    build: &mut Build,
) -> Result<()> {
    build.merge_into = Some(entity);
    let prefab = node.instance.as_deref().unwrap_or_default();
    let outcome = build_instance(eng, node, id, entity, build)
        .with_context(|| format!("instance '{prefab}' on node '{}'", node.name));
    build.merge_into = None;
    outcome
}

/// A node's own keys: what every node has, its tags, its components and its
/// script. A script already pending on the node, a merged prefab root's, is
/// replaced by the node's own.
pub(super) fn apply_own_keys(
    eng: &Engine,
    (index, node): (usize, &SceneNode),
    entity: Entity,
    handlers: &[(String, SceneKeyHandler)],
    build: &mut Build,
) -> Result<()> {
    {
        let world = eng.world();
        // spawn_node inserts an Appearance on every node it creates.
        let mut appearance = world.get::<&mut Appearance>(entity).unwrap();
        if let Some(on) = node.visible {
            appearance.visible = on;
        }
        if let Some(colour) = node.tint.as_ref().and_then(crate::components::rgba) {
            appearance.tint = glamx::Vec4::from(colour);
        }
        if let Some(z) = node.z_index {
            appearance.z_index = z;
        }
        if let Some(on) = node.z_as_relative {
            appearance.z_as_relative = on;
        }
    }
    apply_process(eng, entity, node.process.as_deref(), &node.name)?;
    if !node.tags.is_empty() {
        let mut tags = eng
            .world()
            .get::<&Tags>(entity)
            .map(|t| (*t).clone())
            .unwrap_or_default();
        for tag in &node.tags {
            tags.add(tag);
        }
        eng.world_mut().insert_one(entity, tags)?;
    }
    for (key, handler) in handlers {
        let Some(value) = node.extra.get(key) else {
            continue;
        };
        if let Err(why) = handler(eng, entity, value) {
            // A component it refuses leaves the node without it, and says why
            // on the node, rather than failing the whole scene over one value.
            if crate::components::is_registered(eng, key) {
                let property = property_of(eng, key, value, &why);
                let path: Vec<&str> = std::iter::once(key.as_str())
                    .chain(property.as_deref())
                    .collect();
                let said = format!("node '{}': {why:#}", node.name);
                if !keep(eng, build, index, &path, Level::Error, &said) {
                    tracing::error!(node = %node.name, component = %key, "{why:#}");
                }
                continue;
            }
            return Err(why.context(format!("scene key '{key}' on node '{}'", node.name)));
        }
    }
    // Kept to be written back: a plugin this build lacks still owns the key.
    let mut unhandled = toml::Table::new();
    for (key, value) in &node.extra {
        if !handlers.iter().any(|(k, _)| k == key) {
            let said = format!("scene key '{key}' on node '{}'", node.name);
            report_unhandled(eng, build, index, &[key.as_str()], &said);
            unhandled.insert(key.clone(), value.clone());
        }
    }
    if !unhandled.is_empty() {
        eng.world_mut()
            .insert_one(entity, crate::scene_toml::Unhandled(unhandled))?;
    }
    if let Some(script) = &node.script {
        if script.source().is_empty() {
            bail!("node '{}' has a script key with no source", node.name);
        }
        let props = script
            .props()
            .with_context(|| format!("script properties on node '{}'", node.name))?;
        // A checker holds the scripts, so it hears here what attaching refuses.
        if checking(eng)
            && let Some(why) = plugin_attached(eng, script.source())
        {
            keep(eng, build, index, &[keys::SCRIPT], Level::Error, &why);
        }
        build.pending.retain(|(e, _, _)| *e != entity);
        build
            .pending
            .push((entity, script.source().to_string(), props));
    }
    Ok(())
}

/// Report a key nothing handles: a typo or a component that is gone, unless
/// this host lacks a module that may own it. The scene loads either way.
fn report_unhandled(eng: &Engine, build: &Build, index: usize, path: &[&str], key: &str) {
    let said = format!("{key} has no registered handler");
    if eng.try_resource::<LacksKeyOwner>().is_some() {
        if !keep(eng, build, index, path, Level::Warning, &said) {
            tracing::warn!("{said}");
        }
    } else {
        refuse(eng, build, index, path, &said);
    }
}

/// Say what the build refused and went on without: to the checker building
/// the scene, or to the log of a run.
fn refuse(eng: &Engine, build: &Build, index: usize, path: &[&str], said: &str) {
    if !keep(eng, build, index, path, Level::Error, said) {
        tracing::error!("{said}");
    }
}

/// Build a prefab under the node that names it, with every id inside it
/// prefixed by this node's own.
fn build_instance(
    eng: &Engine,
    node: &SceneNode,
    id: &str,
    entity: Entity,
    build: &mut Build,
) -> Result<()> {
    let prefab = node.instance.as_deref().unwrap_or_default();
    if build.open.iter().any(|open| open == prefab) {
        let mut chain = build.open.clone();
        chain.push(prefab.to_string());
        bail!("a prefab cannot contain itself: {}", chain.join(" -> "));
    }
    let (source, doc) = scene_doc(eng, prefab)?;
    let inner = format!("{}{}/", build.prefix, id);
    let outer = std::mem::replace(&mut build.prefix, inner);
    build.open.push(prefab.to_string());
    let outcome = build_doc(eng, &doc, &source, entity, build);
    build.open.pop();
    build.prefix = outer;
    outcome
}

/// Apply this node's `overrides` to the instance it just built, answering
/// the ones whose path names nothing inside it.
///
/// A path that no longer names anything is an error and is kept: the prefab
/// may have moved a node, so the edit applies to nothing until the path is
/// fixed, and dropping it would lose work the file still holds. Everything
/// else is dispatched exactly as a node's own keys are, so an override reaches
/// components and the transform both.
pub(super) fn apply_overrides(
    eng: &Engine,
    (index, node): (usize, &SceneNode),
    entity: Entity,
    handlers: &[(String, SceneKeyHandler)],
    build: &mut Build,
) -> toml::Table {
    let mut unmatched = toml::Table::new();
    for (path, table) in &node.overrides {
        let over = [keys::OVERRIDES, path.as_str()];
        let Some(target) = scene::find_node(&eng.world(), entity, path) else {
            let said = format!(
                "override '{path}' on node '{}' names nothing inside the instance",
                node.name
            );
            refuse(eng, build, index, &over, &said);
            unmatched.insert(path.clone(), table.clone());
            continue;
        };
        let Some(table) = table.as_table() else {
            let said = format!(
                "override '{path}' on node '{}' is not a table of scene keys",
                node.name
            );
            refuse(eng, build, index, &over, &said);
            continue;
        };
        apply_node_keys(eng, target, table);
        if let Some(script) = table.get(keys::SCRIPT)
            && let Err(err) = override_script(build, target, script)
        {
            let said = format!("override '{path}.script' on node '{}': {err:#}", node.name);
            refuse(eng, build, index, &[over[0], over[1], keys::SCRIPT], &said);
        }
        for (key, handler) in handlers {
            let Some(value) = table.get(key) else {
                continue;
            };
            // An override *patches* one property of a component the prefab
            // already described. The scene key handler would rebuild it from
            // the schema defaults, resetting every property it does not name.
            let applied = if crate::components::is_registered(eng, key) {
                crate::components::patch(eng, target, key, value)
            } else {
                handler(eng, target, value)
            };
            if let Err(err) = applied {
                let property = property_of(eng, key, value, &err);
                let at: Vec<&str> = over
                    .into_iter()
                    .chain([key.as_str()])
                    .chain(property.as_deref())
                    .collect();
                let said = format!("override '{path}.{key}' on node '{}': {err:#}", node.name);
                refuse(eng, build, index, &at, &said);
            }
        }
        for key in table.keys() {
            if key != words::SCRIPT
                && !NODE_KEYS.contains(&key.as_str())
                && !handlers.iter().any(|(k, _)| k == key)
            {
                let said = format!("override '{path}.{key}' on node '{}'", node.name);
                report_unhandled(eng, build, index, &[over[0], over[1], key.as_str()], &said);
            }
        }
    }
    unmatched
}

/// Retune a prefab node's script from the instance that named it: the
/// override's `props` are merged over the ones the prefab set, and its
/// `source` replaces the file.
///
/// This edits the pending attachment rather than the node, because a script
/// is attached once the whole tree exists — which has not happened yet. An
/// override on a node the prefab gave no script to is one nothing can act on,
/// so it says so.
fn override_script(build: &mut Build, target: Entity, value: &toml::Value) -> Result<()> {
    let over: ScriptRef = value.clone().try_into().context("reading the script key")?;
    let props = over.props()?;
    let Some(pending) = build.pending.iter_mut().find(|(e, _, _)| *e == target) else {
        bail!("the node it names has no script to retune");
    };
    if !over.source().is_empty() {
        pending.1 = over.source().to_string();
    }
    for (name, value) in props {
        match pending.2.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => pending.2.push((name, value)),
        }
    }
    Ok(())
}
