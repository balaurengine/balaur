//! A scene written back from its nodes (PLAN-editor-as-scene §10.1): what
//! `scene.to_toml` answers, and what an editor saves and plays from.
//!
//! Each node is written as it was asked for rather than read back: a
//! component is the table every write asked of it ([`Authored`]), a node key
//! what the node holds where that is not the default, the script what it
//! names. A prefab's nodes are not written: the node that instanced it
//! carries what they changed, read against what the prefab file asks.
//!
//! [`Authored`]: crate::components::Authored

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use hecs::Entity;

use crate::collections::DetHashMap;
use crate::engine::Engine;
use crate::process::ProcessMode;
use crate::scene::{Appearance, Children, Name, NodeScript, Tags};

/// The prefab a node instances, as its scene named it (absolute when the
/// scene came from another project's root), and the overrides it carried
/// that named nothing inside it, kept to be written back as they were.
pub struct Instanced {
    pub source: String,
    pub unmatched: toml::Table,
}

/// A node's scene keys nothing handled, kept to be written back as they were.
pub struct Unhandled(pub toml::Table);

/// The `id://` reference each path read from another project's scene was
/// resolved from, so a save writes the reference rather than the file. By
/// path: a file one node names by id and another by path is written by id
/// for both, which names the same file.
#[derive(Default)]
pub struct IdSpellings(pub DetHashMap<String, String>);

/// The path an override names the instancing node itself by.
const ITSELF: &str = ".";

/// The keys a node table holds that are not what the node is asked for.
const STRUCTURE: [&str; 5] = ["id", "name", "parent", "instance", "overrides"];

/// What each node under `root` asks, `root` the document's root, as a scene
/// file's table. `owner` is the project the paths are written for: every path
/// under it is written as that project spells it.
///
/// # Errors
/// If a prefab a node instances does not read.
pub fn to_toml(eng: &Engine, root: Entity, owner: Option<&Path>) -> Result<toml::Table> {
    let written = written_ids(eng, root);
    let mut nodes = Vec::new();
    write_node(eng, root, None, &written, &mut nodes)?;
    let mut document = toml::Table::new();
    document.insert("nodes".into(), toml::Value::Array(nodes));
    let mut document = toml::Value::Table(document);
    if let Some(owner) = owner {
        crate::document_paths::relative_in(eng, owner, &mut document);
    }
    let toml::Value::Table(document) = document else {
        unreachable!("a table stays a table");
    };
    Ok(document)
}

fn stable_id(eng: &Engine, entity: Entity) -> String {
    crate::ids::of(&eng.world(), entity).unwrap_or_default()
}

/// The id each node under `root` is written with: its own, but for one the
/// engine minted at a spawn (`local:7`), which a file must not hold: a game
/// loading it mints the same ids again. That one is named for the node, the
/// way a scene that gave none is.
fn written_ids(eng: &Engine, root: Entity) -> DetHashMap<Entity, String> {
    let subtree = crate::scene::collect_subtree(&eng.world(), root);
    let ids: Vec<(Entity, String)> = subtree
        .into_iter()
        .map(|entity| (entity, stable_id(eng, entity)))
        .collect();
    let mut taken: crate::collections::DetHashSet<String> = ids
        .iter()
        .filter(|(_, id)| !crate::ids::is_minted(id))
        .map(|(_, id)| id.clone())
        .collect();
    ids.into_iter()
        .map(|(entity, id)| {
            if !crate::ids::is_minted(&id) {
                return (entity, id);
            }
            let base = crate::project::slug(&name_of(eng, entity));
            let mut candidate = base.clone();
            let mut n = 2;
            while !taken.insert(candidate.clone()) {
                candidate = format!("{base}_{n}");
                n += 1;
            }
            (entity, candidate)
        })
        .collect()
}

fn name_of(eng: &Engine, entity: Entity) -> String {
    eng.world()
        .get::<&Name>(entity)
        .map(|name| name.0.clone())
        .unwrap_or_default()
}

fn children_of(eng: &Engine, entity: Entity) -> Vec<Entity> {
    eng.world()
        .get::<&Children>(entity)
        .map(|children| children.0.clone())
        .unwrap_or_default()
}

/// One node and every node under it the document owns, in tree order.
fn write_node(
    eng: &Engine,
    entity: Entity,
    parent: Option<&str>,
    written: &DetHashMap<Entity, String>,
    out: &mut Vec<toml::Value>,
) -> Result<()> {
    let id = stable_id(eng, entity);
    let own_id = written.get(&entity).cloned().unwrap_or_else(|| id.clone());
    let mut table = toml::Table::new();
    table.insert("id".into(), toml::Value::String(own_id.clone()));
    table.insert("name".into(), toml::Value::String(name_of(eng, entity)));
    if let Some(parent) = parent {
        table.insert("parent".into(), toml::Value::String(parent.to_string()));
    }
    let instanced = eng
        .world()
        .get::<&Instanced>(entity)
        .ok()
        .map(|instanced| (instanced.source.clone(), instanced.unmatched.clone()));
    // What the prefab built is the prefab's: its ids carry this node's as
    // their prefix, which a node added under the instance's does not.
    let built = format!("{id}/");
    match &instanced {
        Some((source, unmatched)) => {
            let asks = prefab_asks(eng, source, &mut Vec::new())
                .with_context(|| format!("instance '{source}' on node '{id}'"))?;
            table.insert("instance".into(), toml::Value::String(source.clone()));
            write_asked(eng, entity, asks.get(ITSELF), &mut table);
            let mut overrides = unmatched.clone();
            for child in children_of(eng, entity) {
                if stable_id(eng, child).starts_with(&built) {
                    write_overrides(
                        eng,
                        child,
                        &name_of(eng, child),
                        &built,
                        &asks,
                        &mut overrides,
                    );
                }
            }
            if !overrides.is_empty() {
                table.insert("overrides".into(), toml::Value::Table(overrides));
            }
        }
        None => write_asked(eng, entity, None, &mut table),
    }
    if let Ok(unhandled) = eng.world().get::<&Unhandled>(entity) {
        for (key, value) in &unhandled.0 {
            table.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    out.push(toml::Value::Table(table));
    for child in children_of(eng, entity) {
        if instanced.is_some() && stable_id(eng, child).starts_with(&built) {
            continue;
        }
        write_node(eng, child, Some(&own_id), written, out)?;
    }
    Ok(())
}

/// What a node asks over `base`, what its prefab already asked of it: every
/// node key and component that differs is written whole, and the script.
fn write_asked(eng: &Engine, entity: Entity, base: Option<&toml::Table>, out: &mut toml::Table) {
    let empty = toml::Table::new();
    let base = base.unwrap_or(&empty);
    NodeKeys::of_node(eng, entity).write_over(&NodeKeys::of_table(base), out);
    if let Some(script) = script_of(eng, entity)
        && base.get("script").is_none_or(|was| !same(&script, was))
    {
        out.insert("script".into(), script);
    }
    for (name, asked) in crate::components::asked_of(eng, entity) {
        if base.get(&name).is_none_or(|was| !same(&asked, was)) {
            out.insert(name, asked);
        }
    }
}

/// What a prefab's node changed from what the prefab asks of it, property by
/// property, under the path from the instancing node; then its children's.
fn write_overrides(
    eng: &Engine,
    entity: Entity,
    path: &str,
    built: &str,
    asks: &BTreeMap<String, toml::Table>,
    out: &mut toml::Table,
) {
    let empty = toml::Table::new();
    let base = asks.get(path).unwrap_or(&empty);
    let mut entry = toml::Table::new();
    NodeKeys::of_node(eng, entity).write_over(&NodeKeys::of_table(base), &mut entry);
    if let Some(toml::Value::Table(script)) = script_of(eng, entity) {
        let changed = match base.get("script").and_then(toml::Value::as_table) {
            Some(was) => script_changes(&script, was),
            None => Some(script),
        };
        if let Some(changed) = changed {
            entry.insert("script".into(), toml::Value::Table(changed));
        }
    }
    for (name, asked) in crate::components::asked_of(eng, entity) {
        let defaults = defaults_of(eng, &name);
        let changed = match (asked, base.get(&name)) {
            (toml::Value::Table(asked), Some(toml::Value::Table(was))) => {
                // A property the prefab leaves out is its default, so one
                // written back to that is no override either.
                let differing: toml::Table = asked
                    .into_iter()
                    .filter(|(key, value)| {
                        was.get(key)
                            .or_else(|| defaults.get(key))
                            .is_none_or(|old| !same(value, old))
                    })
                    .collect();
                (!differing.is_empty()).then_some(toml::Value::Table(differing))
            }
            (asked, Some(was)) if same(&asked, was) => None,
            (asked, _) => Some(asked),
        };
        if let Some(changed) = changed {
            entry.insert(name, changed);
        }
    }
    if !entry.is_empty() {
        out.insert(path.to_string(), toml::Value::Table(entry));
    }
    for child in children_of(eng, entity) {
        if stable_id(eng, child).starts_with(built) {
            let inner = format!("{path}/{}", name_of(eng, child));
            write_overrides(eng, child, &inner, built, asks, out);
        }
    }
}

/// What a component holds for each property nobody asked of it.
fn defaults_of(eng: &Engine, component: &str) -> toml::Table {
    let Some(registry) = eng.try_resource::<crate::components::ComponentRegistry>() else {
        return toml::Table::new();
    };
    let registry = registry.borrow();
    registry
        .index_of(component)
        .and_then(|index| registry.facts(index))
        .map(|facts| (*facts.defaults).clone())
        .unwrap_or_default()
}

/// A script override against the one the prefab names: the whole script when
/// it names another file, else the properties it sets differently.
fn script_changes(script: &toml::Table, was: &toml::Table) -> Option<toml::Table> {
    if script.get("source") != was.get("source") {
        return Some(script.clone());
    }
    let empty = toml::Table::new();
    let props = script
        .get("props")
        .and_then(toml::Value::as_table)
        .unwrap_or(&empty);
    let before = was
        .get("props")
        .and_then(toml::Value::as_table)
        .unwrap_or(&empty);
    let differing: toml::Table = props
        .iter()
        .filter(|(key, value)| before.get(*key).is_none_or(|old| !same(value, old)))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    (!differing.is_empty()).then(|| {
        let mut out = toml::Table::new();
        out.insert("props".into(), toml::Value::Table(differing));
        out
    })
}

/// The node's `script` key: the file it names and what it sets.
fn script_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let script = world.get::<&NodeScript>(entity).ok()?;
    let mut table = toml::Table::new();
    table.insert("source".into(), toml::Value::String(script.path.clone()));
    let props: toml::Table = script
        .props
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), crate::node_api::to_toml(value).ok()?)))
        .collect();
    if !props.is_empty() {
        table.insert("props".into(), toml::Value::Table(props));
    }
    Some(toml::Value::Table(table))
}

/// What a prefab file asks of each node it builds, by path of names from its
/// root, the root itself [`ITSELF`]: its own keys, a nested prefab's under
/// them, and its overrides over both. Read the way a build reads it, so a
/// value compares equal to what the build asked.
fn prefab_asks(
    eng: &Engine,
    source: &str,
    open: &mut Vec<String>,
) -> Result<BTreeMap<String, toml::Table>> {
    if open.iter().any(|held| held == source) {
        bail!(
            "a prefab cannot contain itself: {} -> {source}",
            open.join(" -> ")
        );
    }
    let (text, root) = crate::project::scene_from_file(eng, source)?;
    let mut document: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing scene '{source}'"))?;
    if let Some(root) = &root {
        crate::document_paths::absolute_in(
            eng,
            root,
            &mut document,
            crate::document_paths::Ids::Resolve,
        );
    }
    let nodes: Vec<toml::Table> = document
        .get("nodes")
        .and_then(toml::Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| node.as_table().cloned())
                .collect()
        })
        .unwrap_or_default();
    let text_of = |node: &toml::Table, key: &str| -> String {
        node.get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let root_name = nodes
        .iter()
        .find(|node| text_of(node, "parent").is_empty())
        .map(|node| text_of(node, "name"))
        .unwrap_or_default();
    let mut path_of_id: DetHashMap<String, String> = DetHashMap::default();
    let mut asks: BTreeMap<String, toml::Table> = BTreeMap::new();
    open.push(source.to_string());
    for node in &nodes {
        let parent = text_of(node, "parent");
        let path = if parent.is_empty() {
            ITSELF.to_string()
        } else {
            let above = path_of_id.get(&parent).cloned().unwrap_or_else(|| {
                // A path of names from the root, which may start at the root.
                let named = parent
                    .strip_prefix(&format!("{root_name}/"))
                    .unwrap_or(&parent);
                if named == root_name {
                    ITSELF.to_string()
                } else {
                    named.to_string()
                }
            });
            joined(&above, &text_of(node, "name"))
        };
        path_of_id.insert(text_of(node, "id"), path.clone());
        let own: toml::Table = node
            .iter()
            .filter(|(key, _)| !STRUCTURE.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let instance = text_of(node, "instance");
        if instance.is_empty() {
            asks.insert(path.clone(), own);
        } else {
            for (inner, mut asked) in prefab_asks(eng, &instance, open)? {
                if inner == ITSELF {
                    asked.extend(own.clone());
                }
                asks.insert(joined(&path, &inner), asked);
            }
        }
        if let Some(overrides) = node.get("overrides").and_then(toml::Value::as_table) {
            for (inner, over) in overrides {
                let Some(over) = over.as_table() else {
                    continue;
                };
                patch(asks.entry(joined(&path, inner)).or_default(), over);
            }
        }
    }
    open.pop();
    Ok(asks)
}

/// `inner` under `path`, either of them [`ITSELF`].
fn joined(path: &str, inner: &str) -> String {
    match (path, inner) {
        (ITSELF, inner) => inner.to_string(),
        (path, ITSELF) => path.to_string(),
        (path, inner) => format!("{path}/{inner}"),
    }
}

/// An override laid over what a node asks, as a build applies one: a
/// component's properties patched, a script's properties merged and its file
/// replaced, anything else replaced.
fn patch(asked: &mut toml::Table, over: &toml::Table) {
    for (key, value) in over {
        match (asked.get_mut(key), value) {
            (Some(toml::Value::Table(held)), toml::Value::Table(changes)) => {
                for (inner, changed) in changes {
                    match (held.get_mut(inner), changed) {
                        (Some(toml::Value::Table(props)), toml::Value::Table(set))
                            if key == "script" && inner == "props" =>
                        {
                            props.extend(set.clone());
                        }
                        _ => {
                            held.insert(inner.clone(), changed.clone());
                        }
                    }
                }
            }
            _ => {
                asked.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Two asked values the same, numbers to the f32 a component holds them in:
/// the file's 0.8 and the 0.800000011920929 a script wrote back are one.
fn same(a: &toml::Value, b: &toml::Value) -> bool {
    match (a, b) {
        (toml::Value::Array(a), toml::Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (toml::Value::Table(a), toml::Value::Table(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same(value, other)))
        }
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-5 * x.abs().max(1.0),
            _ => a == b,
        },
    }
}

fn number(value: &toml::Value) -> Option<f64> {
    match value {
        toml::Value::Integer(n) => Some(*n as f64),
        toml::Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// The keys every node has, as one value: read off a node, or out of a table
/// the way a build applies them, so the two compare.
#[derive(Clone, PartialEq)]
struct NodeKeys {
    visible: bool,
    tint: [f32; 4],
    z_index: i64,
    z_as_relative: bool,
    process: ProcessMode,
    interpolate: Option<bool>,
    tags: Vec<String>,
}

impl Default for NodeKeys {
    fn default() -> Self {
        Self {
            visible: true,
            tint: [1.0; 4],
            z_index: 0,
            z_as_relative: true,
            process: ProcessMode::Inherit,
            interpolate: None,
            tags: Vec::new(),
        }
    }
}

impl NodeKeys {
    fn of_node(eng: &Engine, entity: Entity) -> Self {
        let mut keys = Self::default();
        let world = eng.world();
        if let Ok(appearance) = world.get::<&Appearance>(entity) {
            keys.visible = appearance.visible;
            keys.tint = appearance.tint.to_array();
            keys.z_index = i64::from(appearance.z_index);
            keys.z_as_relative = appearance.z_as_relative;
        }
        keys.process = crate::process::own(&world, entity);
        if let Ok(tags) = world.get::<&Tags>(entity) {
            keys.tags.clone_from(&tags.0);
        }
        drop(world);
        keys.interpolate = crate::interpolate::own(eng, entity);
        keys
    }

    fn of_table(table: &toml::Table) -> Self {
        let mut keys = Self::default();
        if let Some(on) = table.get("visible").and_then(toml::Value::as_bool) {
            keys.visible = on;
        }
        if let Some(colour) = table.get("tint").and_then(crate::components::rgba) {
            keys.tint = colour;
        }
        if let Some(z) = table.get("z_index").and_then(toml::Value::as_integer) {
            keys.z_index = z;
        }
        if let Some(on) = table.get("z_as_relative").and_then(toml::Value::as_bool) {
            keys.z_as_relative = on;
        }
        if let Some(mode) = table
            .get(crate::process::KEY)
            .and_then(toml::Value::as_str)
            .and_then(ProcessMode::parse)
        {
            keys.process = mode;
        }
        keys.interpolate = table
            .get(crate::interpolate::KEY)
            .and_then(toml::Value::as_bool);
        if let Some(list) = table.get("tags").and_then(toml::Value::as_array) {
            let mut tags = Tags::default();
            for tag in list.iter().filter_map(toml::Value::as_str) {
                tags.add(tag);
            }
            keys.tags = tags.0;
        }
        keys
    }

    /// Every key that differs from `base`, written into `out`.
    fn write_over(&self, base: &Self, out: &mut toml::Table) {
        if self.visible != base.visible {
            out.insert("visible".into(), toml::Value::Boolean(self.visible));
        }
        let tinted = self
            .tint
            .iter()
            .zip(base.tint)
            .any(|(a, b)| (a - b).abs() > 1e-5);
        if tinted {
            out.insert(
                "tint".into(),
                toml::Value::Array(self.tint.iter().map(|&c| short_float(c)).collect()),
            );
        }
        if self.z_index != base.z_index {
            out.insert("z_index".into(), toml::Value::Integer(self.z_index));
        }
        if self.z_as_relative != base.z_as_relative {
            out.insert(
                "z_as_relative".into(),
                toml::Value::Boolean(self.z_as_relative),
            );
        }
        if self.process != base.process {
            out.insert(
                crate::process::KEY.into(),
                toml::Value::String(self.process.name().into()),
            );
        }
        if self.interpolate != base.interpolate
            && let Some(on) = self.interpolate
        {
            out.insert(crate::interpolate::KEY.into(), toml::Value::Boolean(on));
        }
        if self.tags != base.tags {
            out.insert(
                "tags".into(),
                toml::Value::Array(
                    self.tags
                        .iter()
                        .map(|tag| toml::Value::String(tag.clone()))
                        .collect(),
                ),
            );
        }
    }
}

/// Every one of a node's own keys as it holds them, defaults included, as a
/// script table: what a kept change to one of them replaced.
pub(crate) fn node_keys_value(eng: &Engine, entity: Entity) -> balaur_script::Value {
    use balaur_script::Value;
    let keys = NodeKeys::of_node(eng, entity);
    let float = |f: f32| Value::Num(f64::from(f).to_string().parse().unwrap_or(f64::from(f)));
    Value::Map(vec![
        ("visible".into(), Value::Bool(keys.visible)),
        (
            "tint".into(),
            Value::List(keys.tint.iter().map(|&c| float(c)).collect()),
        ),
        ("z_index".into(), Value::Int(keys.z_index)),
        ("z_as_relative".into(), Value::Bool(keys.z_as_relative)),
        (crate::process::KEY.into(), Value::text(keys.process.name())),
        (
            crate::interpolate::KEY.into(),
            keys.interpolate.map_or(Value::Nil, Value::Bool),
        ),
        (
            "tags".into(),
            Value::List(keys.tags.iter().map(Value::text).collect()),
        ),
    ])
}

/// An f32 as the shortest float that reads back as it: 0.8, not the
/// 0.800000011920929 widening it gives.
fn short_float(value: f32) -> toml::Value {
    toml::Value::Float(value.to_string().parse().unwrap_or(f64::from(value)))
}
