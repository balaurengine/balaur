//! The named-component registry: how plugins make their data editable.
//!
//! A plugin registers a component under a name with a *schema* (property
//! names, types, defaults — declared as TOML) plus apply/get/remove hooks.
//! Registration buys three things at once:
//!
//! 1. a scene-file key (`body = { kind = "dynamic" }`) applied at
//!    instantiation, in registration order;
//! 2. a runtime script API on every node (`node:set_component`,
//!    `get_component`, `has_component`, `remove_component`,
//!    `node:component_names()`), plus `scene.component_types()` /
//!    `scene.component_schema(name)` for enumeration;
//! 3. editor support for free: the balaur editor builds its "Add component"
//!    list and its property inspectors from the schemas, so third-party
//!    plugin components are addable and editable without editor changes.
//!
//! Property specs (`schema` is a TOML table of `name = { ... }`):
//!   type = "float" | "int" | "bool" | "string" | "enum" | "vec2" | "vec3" | "vec4"
//!          | "color" | "asset" | "flags" | "node" | "strings"
//!   default = ...          (required, and of the declared type)
//!   options = [...]        (enum and flags only, and required there)
//!   asset = "clip_type"    (asset only, and required there)
//!   min/max/step/decimals  (float and int, optional)
//!   readonly               (bool, optional)
//!   description = "..."    (optional, one line, for the reference and the
//!                           inspector row's tooltip)
//!   unit = "degrees"       (optional; what an editor draws the property in,
//!                           and the unit `min`, `max` and `step` are written
//!                           in. Nothing stores it: the file and a script both
//!                           read what the property declares)
//!   group = "damping"      (optional; the fold an editor files the property
//!                           under. A property with no group is one the
//!                           inspector always shows, so grouping one is the
//!                           decision to put it away by default)
//!
//! `type` declares a property's datatype; `kind` is a property *name*, the one
//! reserved for a tagged union's discriminant (`shape.kind = "ball"`), so a
//! discriminant reads `kind = { type = "enum", options = [...] }`.
//! `ComponentDef::parse_schema` enforces all of that and panics on a schema
//! that departs from it.
//!
//! A `color` property may be written either as `[r, g, b]` / `[r, g, b, a]`
//! floats or as a `#rrggbb` / `#rrggbbaa` string; the string form is expanded
//! to the array form before any `apply` hook sees it, so hooks read one shape.
//!
//! An `asset` property obeys the asset layer's one rule — *a string is a
//! reference, a table is a definition* (`crate::assets`). [`properties`]
//! applies it to every asset-typed property before any `apply` hook runs, so
//! a hook always receives a reference string and reaches the object with one
//! `assets::load_typed` call, and every asset type registered from now on
//! inherits the rule without writing a line for it.
//!
//! A `flags` property is a *set* drawn from its `options`, written as an array
//! of those strings (`lock_rotation = ["x", "z"]`). It is the answer to a
//! property that is neither one choice from a list (`enum`) nor a fixed-length
//! vector: an empty array is a legal, and usually the default, value.
//!
//! A `node` property holds a scene-relative path to another node
//! (`body = "../Cart"`), resolved with [`crate::scene::find_node`] by the hook
//! that reads it — a joint's other end, a camera's target. The empty string
//! means "no node", so a component carrying one is still addable before its
//! partner exists.
//!
//! Two ways in: [`add`] describes a component whole, starting from the schema
//! defaults, which is what a scene file means; [`patch`] writes over what the
//! component currently reports, which is what anything driving one property
//! over time means.

use std::rc::Rc;

use anyhow::{Context, Result, anyhow, bail};
use hecs::Entity;
use smol_str::SmolStr;

use crate::engine::Engine;

mod schema;

pub use schema::{PROPERTY_TYPES, UNITS, validate_property};
use schema::hex_rgba;

/// Read a numeric TOML value as f64, integers included: schemas say
/// "float" but scene authors naturally write `14`, which TOML parses as an
/// integer (`Value::as_float` alone would reject it).
pub const fn as_f64(value: &toml::Value) -> Option<f64> {
    match value {
        toml::Value::Float(f) => Some(*f),
        toml::Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// A property table with everything the schema marks `readonly` dropped.
///
/// A readonly property is the host writing back what it computed — a widget's
/// `clicked` comes from the event pump — so no recording carries it and
/// nothing that has to reproduce a tick may read it.
#[must_use]
pub fn authored(schema: &toml::Value, value: &toml::Value) -> toml::Value {
    let (Some(specs), Some(table)) = (schema.as_table(), value.as_table()) else {
        return value.clone();
    };
    let kept: toml::map::Map<String, toml::Value> = table
        .iter()
        .filter(|(prop, _)| !is_readonly(specs.get(prop.as_str())))
        .map(|(prop, v)| (prop.clone(), v.clone()))
        .collect();
    toml::Value::Table(kept)
}

fn is_readonly(spec: Option<&toml::Value>) -> bool {
    spec.and_then(|s| s.get("readonly"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

/// The readers an `apply` uses on the table it was handed.
///
/// Every declared property carries a `default` — [`validate_property`] refuses
/// one without — and [`add`] and [`patch`] merge those in before `apply` runs,
/// so the key is always there and holds its declared type. These take no
/// fallback of their own on purpose: a second default written beside the
/// reader is a copy of the schema's that nothing would notice going stale.
pub fn prop_f32(params: &toml::Value, key: &str) -> f32 {
    prop_f64(params, key) as f32
}

/// [`prop_f32`] at full width, for a property compared against `f64` data.
pub fn prop_f64(params: &toml::Value, key: &str) -> f64 {
    params.get(key).and_then(as_f64).unwrap_or_default()
}

/// The two numbers a `vec2`-typed property holds.
pub fn prop_vec2(params: &toml::Value, key: &str) -> [f32; 2] {
    let [x, y, _] = prop_vec3(params, key);
    [x, y]
}

/// The three numbers a `vec3`-typed property holds.
pub fn prop_vec3(params: &toml::Value, key: &str) -> [f32; 3] {
    let axis = |i: usize| {
        params
            .get(key)
            .and_then(toml::Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(as_f64)
            .map(|v| v as f32)
            .unwrap_or_default()
    };
    [axis(0), axis(1), axis(2)]
}

/// The whole number an `int`-typed property holds.
pub fn prop_i64(params: &toml::Value, key: &str) -> i64 {
    params
        .get(key)
        .and_then(toml::Value::as_integer)
        .unwrap_or_default()
}

/// Whether a `bool`-typed property is set.
pub fn prop_bool(params: &toml::Value, key: &str) -> bool {
    params
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or_default()
}

/// The text a `string`, `enum`, `asset` or `node` property holds.
pub fn prop_str<'a>(params: &'a toml::Value, key: &str) -> &'a str {
    params
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
}

/// The names a `flags`-typed property holds, in the order they were written.
///
/// Anything that is not a string is dropped rather than refused: a schema
/// validated its `default`, and a scene may carry a name a newer version of
/// the component added.
pub fn as_flags(value: Option<&toml::Value>) -> Vec<&str> {
    value
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default()
}

/// Whether a `flags`-typed property holds `name`.
pub fn has_flag(value: Option<&toml::Value>, name: &str) -> bool {
    as_flags(value).contains(&name)
}

/// The node a `node`-typed property names, resolved relative to the node that
/// carries the component (Godot's NodePath rules, via [`crate::scene::find_node`]).
///
/// `None` for the empty default and for a path that resolves to nothing — a
/// component with an unset or dangling partner is inert, not an error, because
/// the partner may be spawned a tick later.
pub fn as_node(eng: &Engine, from: Entity, value: Option<&toml::Value>) -> Option<Entity> {
    let path = value.and_then(toml::Value::as_str)?;
    if path.trim().is_empty() {
        return None;
    }
    // A leading `/` walks from the scene root, as it does in Godot; the
    // editor's node picker writes that form because it is the one spelling
    // that does not change when the node carrying it moves.
    let (from, path) = match path.strip_prefix('/') {
        Some(rest) => (eng.root(), rest),
        None => (from, path),
    };
    crate::scene::find_node(&eng.world(), from, path)
}

/// Insert-or-update a component from a full property table.
pub type ApplyFn = Box<dyn Fn(&Engine, Entity, &toml::Value) -> Result<()>>;
/// Remove a component from an entity.
pub type RemoveFn = Box<dyn Fn(&Engine, Entity) -> Result<()>>;
/// Read a component's property table, or `None` when the entity lacks it.
pub type GetFn = Box<dyn Fn(&Engine, Entity) -> Option<toml::Value>>;
/// Read one of a component's properties, for a component that can answer
/// without building its whole table. `None` means "ask the whole table",
/// which is also the answer for a property the component does not hold.
pub type PropertyFn = Box<dyn Fn(&Engine, Entity, &str) -> Option<toml::Value>>;

pub struct ComponentDef {
    /// TOML table of property specs (see module docs). Shared, because a
    /// patch reads it every time a property is written and a copy per write
    /// is the whole table.
    pub schema: Rc<toml::Value>,
    /// What the component gives a node, in one or two sentences, for the
    /// generated reference. `scripts/api_lints.py` fails an empty one.
    pub doc: &'static str,
    /// Facets this component belongs to, for browsing: `2d`, `3d`, `physics`,
    /// `render`, `audio`, `ui`. Several apply at once on purpose -- a
    /// `collider2d` is both `2d` and `physics`, and a single category path
    /// would bury one of those.
    pub tags: &'static [&'static str],
    /// Components this one needs *something* from, any one of which will do.
    ///
    /// Not a requirement: a script may add the missing piece on a later tick,
    /// and nothing here blocks or reorders anything -- the editor warns, the
    /// runtime does not care.
    ///
    /// No built-in component declares one today, and that is a finding rather
    /// than an oversight: every candidate turned out to be either already an
    /// error (`color` refuses a node with nothing to tint) or perfectly valid
    /// (a `collider2d` with no `body2d` is standalone static geometry). It is
    /// here for plugins, and for the case where an error would be too strict.
    pub expects: &'static [&'static str],
    /// Insert-or-update the component from a full property table.
    pub apply: ApplyFn,
    pub remove: RemoveFn,
    /// Current property table, or None when the entity lacks the component.
    ///
    /// Every property the component holds, because [`patch`] rebuilds from this
    /// and defaults whatever it omits. Values the component derives are the
    /// exception, and must be left out for the same reason.
    pub get: GetFn,
}

pub mod tag {
    pub const DIM_3D: &str = "3d";
    pub const DIM_2D: &str = "2d";
    pub const PHYSICS: &str = "physics";
    pub const RENDER: &str = "render";
    pub const UI: &str = "ui";
    pub const AUDIO: &str = "audio";
    pub const ANIMATION: &str = "animation";
}

impl ComponentDef {
    /// Schema text from `(key, spec)` lines, so a key is spelled once, by a
    /// constant the reader uses too, and the spec stays the TOML table the
    /// inspector reads.
    pub fn schema(lines: &[(&str, &str)]) -> String {
        lines
            .iter()
            .map(|(key, spec)| format!("{key} = {spec}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The words an enum or flags property offers, as its `options` list.
    pub fn options(words: &[&str]) -> String {
        words
            .iter()
            .map(|word| format!("\"{word}\""))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Parse and validate a schema from TOML text.
    ///
    /// Panics naming the component, the property and the key at fault. Schemas
    /// are compile-time constants written by plugin authors, so a bad one is a
    /// bug in the plugin rather than bad user input, and failing at
    /// registration beats an inspector row that silently never appears.
    pub fn parse_schema(component: &str, text: &str) -> Rc<toml::Value> {
        let schema: toml::Value = toml::from_str(text)
            .unwrap_or_else(|e| panic!("component '{component}': schema is not valid TOML: {e}"));
        let table = schema.as_table().unwrap_or_else(|| {
            panic!("component '{component}': schema is not a table of property specs")
        });
        for (prop, spec) in table {
            if let Err(why) = validate_property(spec) {
                panic!("component '{component}', property '{prop}': {why}");
            }
        }
        Rc::new(schema)
    }
}

/// What a schema says once, so no write has to read it again.
///
/// Built at registration and held beside the definition. Everything here is a
/// question [`patch`] would otherwise ask the schema on every call: the
/// defaults it starts from, and whether either of the two passes over a
/// finished table has anything to do. A component with no colour and no asset
/// property -- which is most of them -- skips both.
pub struct Facts {
    /// Every declared property at its default, ready to clone.
    pub defaults: Rc<toml::map::Map<String, toml::Value>>,
    /// Whether any property is `type = "color"`, so a `#rrggbb` string has to
    /// be expanded before `apply` sees it.
    pub has_color: bool,
    /// Whether any property is `type = "asset"`, so an inline definition has
    /// to be cached and rewritten to the reference naming it.
    pub has_asset: bool,
}

impl Facts {
    fn of(schema: &toml::Value) -> Self {
        let mut has_color = false;
        let mut has_asset = false;
        if let Some(table) = schema.as_table() {
            for spec in table.values() {
                match spec.get("type").and_then(toml::Value::as_str) {
                    Some("color") => has_color = true,
                    Some("asset") => has_asset = true,
                    _ => {}
                }
            }
        }
        Self {
            defaults: Rc::new(defaults_of(schema)),
            has_color,
            has_asset,
        }
    }
}

/// Every registered component, in registration order, addressable by name.
///
/// An [`crate::collections::DetHashMap`] rather than a `Vec`: a name resolves
/// in one lookup where it used to be a scan, and the map iterates in insertion
/// order, so registration order -- which is the scene key order, the editor's
/// section order, and a component's bit in [`Attached`] -- is unchanged.
/// [`Facts`] sits parallel to it, indexed the same way.
#[derive(Default)]
pub struct ComponentRegistry {
    defs: crate::collections::DetHashMap<SmolStr, ComponentDef>,
    facts: Vec<Facts>,
}

impl ComponentRegistry {
    pub fn def(&self, name: &str) -> Option<&ComponentDef> {
        self.defs.get(name)
    }

    /// Where `name` sits in registration order, which is its bit in
    /// [`Attached`].
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.defs.get_index_of(name)
    }

    /// The name and definition registered at `index`.
    #[must_use]
    pub fn at(&self, index: usize) -> Option<(&SmolStr, &ComponentDef)> {
        self.defs.get_index(index)
    }

    /// What the schema at `index` says, worked out once at registration.
    #[must_use]
    pub fn facts(&self, index: usize) -> Option<&Facts> {
        self.facts.get(index)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Name and definition in registration order.
    pub fn iter(&self) -> impl Iterator<Item = (&SmolStr, &ComponentDef)> {
        self.defs.iter()
    }

    /// Take a definition under `name`.
    ///
    /// # Panics
    /// When `name` is already registered. Two definitions under one name is
    /// not a merge and not a replacement: every lookup would answer with the
    /// first, so the second's `apply` would never run and its inspector rows
    /// would never draw. A plugin colliding with a built-in has to be told.
    pub fn insert(&mut self, name: &str, def: ComponentDef) {
        let facts = Facts::of(&def.schema);
        assert!(
            self.defs.insert(SmolStr::new(name), def).is_none(),
            "component '{name}' is registered twice; a name belongs to one definition"
        );
        self.facts.push(facts);
    }
}

impl<'a> IntoIterator for &'a ComponentRegistry {
    type Item = (&'a SmolStr, &'a ComponentDef);
    type IntoIter = indexmap::map::Iter<'a, SmolStr, ComponentDef>;

    fn into_iter(self) -> Self::IntoIter {
        self.defs.iter()
    }
}

/// The most components one build may register: a bit each in [`Attached`].
pub const MAX_COMPONENTS: usize = 128;

/// Which registered components each node was given through the registry,
/// one bit per definition in registration order.
///
/// Set by `apply`, cleared by `remove`, dropped when the node is freed. A
/// node with no entry was never given one, so freeing fifty thousand bare
/// nodes asks no plugin anything. A component attached behind the registry's
/// back is not in here: a debug build still finds it on free and warns, a
/// release build skips its hook.
#[derive(Default)]
pub struct Attached(pub crate::collections::DetHashMap<Entity, u128>);

/// What a scene or a script asked of each component, by node and definition.
///
/// A component's live state is its own Rust struct, and `get` is the only way
/// back to a table. [`patch`] builds on this rather than on `get` alone, so a
/// `get` that does not mention a property cannot have it reset.
#[derive(Default)]
pub struct Authored(pub crate::collections::DetHashMap<(Entity, usize), toml::Value>);

/// Merge what is being asked for into what was asked before, for the
/// definition at `index`.
fn record_at(eng: &Engine, entity: Entity, index: usize, params: Option<&toml::Value>, over: bool) {
    let Some(authored) = eng.try_resource::<Authored>() else {
        return;
    };
    let mut authored = authored.borrow_mut();
    let slot = authored
        .0
        .entry((entity, index))
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if over {
        *slot = toml::Value::Table(toml::map::Map::new());
    }
    let (Some(slot), Some(asked)) = (slot.as_table_mut(), params.and_then(toml::Value::as_table))
    else {
        return;
    };
    for (key, value) in asked {
        slot.insert(key.clone(), value.clone());
    }
}

/// What was asked of this component before now, if anything.
fn asked_for_at(eng: &Engine, entity: Entity, index: usize) -> Option<toml::Value> {
    let authored = eng.try_resource::<Authored>()?;
    let asked = authored.borrow().0.get(&(entity, index)).cloned();
    drop(authored);
    asked
}

/// Forget what was asked of one component on one node.
fn forget(eng: &Engine, entity: Entity, name: &str) {
    let (Some(authored), Some(index)) = (eng.try_resource::<Authored>(), index_of(eng, name))
    else {
        return;
    };
    authored.borrow_mut().0.swap_remove(&(entity, index));
}

/// A registered component's position in the registry, which is its key above.
fn index_of(eng: &Engine, name: &str) -> Option<usize> {
    let registry = eng.try_resource::<ComponentRegistry>()?;
    let at = registry.borrow().index_of(name);
    drop(registry);
    at
}

/// Set or clear one node's bit for the definition at `index`.
fn mark(eng: &Engine, entity: Entity, index: usize, on: bool) {
    let Some(attached) = eng.try_resource::<Attached>() else {
        return;
    };
    let mut attached = attached.borrow_mut();
    let bit = 1u128 << index;
    if on {
        *attached.0.entry(entity).or_insert(0) |= bit;
    } else if let Some(bits) = attached.0.get_mut(&entity) {
        *bits &= !bit;
        if *bits == 0 {
            attached.0.swap_remove(&entity);
        }
    }
}

/// A colour written either way: `[r, g, b, a]` floats, or `#rrggbb` /
/// `#rrggbbaa`. A missing alpha is opaque.
///
/// Public because the node-level keys are colours too and are read outside
/// the schema path, which is where `expand_colors` does this.
#[must_use]
pub fn rgba(value: &toml::Value) -> Option<[f32; 4]> {
    if let Some(text) = value.as_str() {
        return hex_rgba(text).map(|c| c.map(|v| v as f32));
    }
    let array = value.as_array()?;
    let channel =
        |i: usize, default: f32| array.get(i).and_then(as_f64).map_or(default, |v| v as f32);
    Some([
        channel(0, 1.0),
        channel(1, 1.0),
        channel(2, 1.0),
        channel(3, 1.0),
    ])
}

/// Expand hex strings on `color`-typed properties into the float array every
/// `apply` hook reads.
///
/// Done once here rather than in each hook, so a hex value works on every
/// colour property that exists or is added later. Before this, `color =
/// "#ff0000"` on a node reached an `as_array()` that returned `None` and fell
/// through to the default grey without a word.
fn expand_colors(schema: &toml::Value, out: &mut toml::map::Map<String, toml::Value>) {
    let Some(table) = schema.as_table() else {
        return;
    };
    for (prop, spec) in table {
        // `type` is the spec's datatype key (see the module docs).
        if spec.get("type").and_then(toml::Value::as_str) != Some("color") {
            continue;
        }
        let Some(text) = out.get(prop).and_then(toml::Value::as_str) else {
            continue;
        };
        if let Some(rgba) = hex_rgba(text) {
            let array = rgba.iter().copied().map(toml::Value::Float).collect();
            out.insert(prop.clone(), toml::Value::Array(array));
        } else {
            tracing::warn!(
                property = prop.as_str(),
                value = text,
                "not a colour; expected #rrggbb, #rrggbbaa or [r, g, b, a]"
            );
        }
    }
}

pub fn merge_defaults(schema: &toml::Value, params: Option<&toml::Value>) -> Result<toml::Value> {
    let mut out = defaults_of(schema);
    overlay(schema, &mut out, params)?;
    expand_colors(schema, &mut out);
    Ok(toml::Value::Table(out))
}

/// Every property the schema declares, at its declared default.
fn defaults_of(schema: &toml::Value) -> toml::map::Map<String, toml::Value> {
    let mut out = toml::map::Map::new();
    if let Some(table) = schema.as_table() {
        for (prop, spec) in table {
            if let Some(default) = spec.get("default") {
                out.insert(prop.clone(), default.clone());
            }
        }
    }
    out
}

/// Write `params` over whatever `out` already holds, leaving every property
/// `params` does not mention alone.
fn overlay(
    schema: &toml::Value,
    out: &mut toml::map::Map<String, toml::Value>,
    params: Option<&toml::Value>,
) -> Result<()> {
    match params {
        Some(toml::Value::Table(params)) => {
            for (k, v) in params {
                out.insert(k.clone(), v.clone());
            }
        }
        // A component is written as a table and nothing else, so a bare
        // value is an error rather than a default the file did not ask for.
        Some(other) => {
            let shown = if other.is_array() {
                "[...]".to_string()
            } else {
                other.to_string()
            };
            let hint = match schema.as_table() {
                Some(t) if other.is_str() && t.contains_key("kind") => {
                    format!("{{ kind = {shown} }}")
                }
                Some(t) if t.len() == 1 => match t.keys().next() {
                    Some(only) => format!("{{ {only} = {shown} }}"),
                    None => "{ property = value }".to_string(),
                },
                _ => "{ property = value }".to_string(),
            };
            bail!("expected a table of properties, got {shown}; write `{hint}`");
        }
        None => {}
    }
    Ok(())
}

/// The full property table an `apply` hook receives: schema defaults, the
/// scene's or script's own values merged over them, and every asset-typed
/// property resolved to a reference.
///
/// [`merge_defaults`] is the half that needs no engine; this is the whole
/// thing, and it is what both entry points into `apply` call.
pub fn properties(
    eng: &Engine,
    schema: &toml::Value,
    params: Option<&toml::Value>,
) -> Result<toml::Value> {
    resolved(eng, schema, merge_defaults(schema, params)?)
}

/// Every asset-typed property of an already-merged table turned into a
/// reference, which is the last thing that happens before `apply` sees it.
fn resolved(eng: &Engine, schema: &toml::Value, mut full: toml::Value) -> Result<toml::Value> {
    if let Some(table) = full.as_table_mut() {
        resolve_assets(eng, schema, table)?;
    }
    Ok(full)
}

/// A string is a reference; a table is a definition.
///
/// Done here once, so every asset type any plugin ever registers accepts both
/// spellings with no code of its own. An inline table is recorded in the asset
/// cache and replaced by the reference that now names it, which leaves the
/// property table pure TOML and every `apply` hook reading one shape.
fn resolve_assets(
    eng: &Engine,
    schema: &toml::Value,
    out: &mut toml::map::Map<String, toml::Value>,
) -> Result<()> {
    let Some(table) = schema.as_table() else {
        return Ok(());
    };
    for (prop, spec) in table {
        if spec.get("type").and_then(toml::Value::as_str) != Some("asset") {
            continue;
        }
        let Some(value) = out.get(prop).cloned() else {
            continue;
        };
        let type_name = spec
            .get("asset")
            .and_then(toml::Value::as_str)
            .unwrap_or("");
        if let Some(reference) = asset_reference(eng, prop, type_name, &value)? {
            out.insert(prop.clone(), toml::Value::String(reference));
        }
    }
    Ok(())
}

/// The reference an asset property should carry, or `None` when what it
/// already carries is one.
fn asset_reference(
    eng: &Engine,
    prop: &str,
    type_name: &str,
    value: &toml::Value,
) -> Result<Option<String>> {
    match value {
        // The empty default means "no asset", and resolving it would be an
        // error about a reference the author never wrote.
        toml::Value::String(text) if text.trim().is_empty() => Ok(None),
        // Checked so a typo is reported where it was written, but only warned:
        // a tool must be able to open a scene whose files it cannot reach.
        toml::Value::String(text) => {
            if let Err(why) = crate::assets::definition(eng, text)
                .with_context(|| format!("asset property '{prop}'"))
            {
                tracing::warn!("{why:#}");
            }
            Ok(None)
        }
        // A table's own `type` wins over the schema's: `shape2d.mesh` takes a
        // `mesh` or a `path2d`, and the schema can only name one of them.
        toml::Value::Table(table) => {
            let declared = table
                .get("type")
                .and_then(toml::Value::as_str)
                .unwrap_or(type_name);
            Ok(Some(
                crate::assets::define_inline(eng, declared, value.clone())?.to_string(),
            ))
        }
        other => Err(anyhow!(
            "property '{prop}' is {}; an asset property takes either a reference string or a \
             definition table",
            other.type_str()
        )),
    }
}

pub fn add(eng: &Engine, entity: Entity, name: &str, params: Option<&toml::Value>) -> Result<()> {
    // Resolving assets can read files and reach the asset cache, so the
    // schema is cloned and the registry borrow dropped first: a parser is free
    // to look things up.
    let (index, schema, _, _, _) = resolve(eng, name)?;
    let full = properties(eng, &schema, params)?;
    apply_at(eng, entity, index, name, &full)?;
    // Describing the component whole replaces what was asked of it before.
    record_at(eng, entity, index, params, true);
    Ok(())
}

/// Write `params` over what the component currently holds, rather than over
/// the schema defaults.
///
/// [`add`] describes a component *whole*: it starts from the defaults, so
/// setting one property puts every other one back where the schema says. That
/// is right for a scene file and wrong for anything driving a single property
/// over time — animating `shape/radius` through [`add`] would reset
/// `half_extents` sixty times a second. `patch` starts from the component's
/// own `get` instead, so the properties it does not mention survive.
///
/// On a node that does not have the component yet there is nothing to read
/// back, so the schema defaults are its current value and `patch` adds it.
///
/// A component's live state is its own Rust struct; `get` is the only way back
/// to a table. So a `get` that leaves out a property it holds has that property
/// reset to the schema default by the next patch of any other one. Reporting a
/// derived value is the other half of that, and as wrong: it freezes what the
/// component would otherwise work out again. Report what the component holds
/// and nothing else.
pub fn patch(eng: &Engine, entity: Entity, name: &str, params: &toml::Value) -> Result<()> {
    let (index, schema, defaults, has_color, has_asset) = resolve(eng, name)?;
    let current = get_at(eng, entity, index);
    // The component's own table is the base, taken rather than copied: it
    // already holds every property the component has, so starting from the
    // defaults and writing over them twice was two tables built to be thrown
    // away. What `get` leaves out is filled from what was asked before, then
    // from the defaults -- the same order of precedence, without the copies.
    let mut out = match current {
        Some(toml::Value::Table(table)) => table,
        Some(other) => {
            let mut table = toml::map::Map::new();
            overlay(&schema, &mut table, Some(&other))?;
            table
        }
        None => toml::map::Map::new(),
    };
    if let Some(toml::Value::Table(asked)) = asked_for_at(eng, entity, index) {
        for (key, value) in asked {
            out.entry(key).or_insert(value);
        }
    }
    for (key, value) in defaults.iter() {
        if !out.contains_key(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    overlay(&schema, &mut out, Some(params))?;
    if has_color {
        expand_colors(&schema, &mut out);
    }
    let full = toml::Value::Table(out);
    let full = if has_asset {
        resolved(eng, &schema, full)?
    } else {
        full
    };
    apply_at(eng, entity, index, name, &full)?;
    record_at(eng, entity, index, Some(params), false);
    Ok(())
}

/// Whether a name is a registered component, as opposed to some other scene
/// key a plugin declared.
#[must_use]
pub fn is_registered(eng: &Engine, name: &str) -> bool {
    eng.try_resource::<ComponentRegistry>()
        .is_some_and(|registry| registry.borrow().def(name).is_some())
}

/// Everything a write needs from the registry, taken in one borrow: where the
/// component sits, its schema, and what its schema says.
///
/// Resolving the name once is the point. A write used to look it up four
/// times -- for the schema, for what was asked before, to apply, and to record
/// -- and each one re-entered the resource map and the registry.
fn resolve(eng: &Engine, name: &str) -> Result<(usize, Rc<toml::Value>, Rc<toml::map::Map<String, toml::Value>>, bool, bool)> {
    let registry = eng
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let registry = registry.borrow();
    let index = registry
        .index_of(name)
        .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    let (_, def) = registry
        .at(index)
        .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    let schema = def.schema.clone();
    let facts = registry
        .facts(index)
        .ok_or_else(|| anyhow!("component '{name}' has no schema facts"))?;
    Ok((
        index,
        schema,
        facts.defaults.clone(),
        facts.has_color,
        facts.has_asset,
    ))
}

/// Hand a finished property table to the component's `apply` hook, and note
/// in [`Attached`] that the node now carries it.
pub(crate) fn apply_full(
    eng: &Engine,
    entity: Entity,
    name: &str,
    full: &toml::Value,
) -> Result<()> {
    let index = index_of(eng, name).ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    apply_at(eng, entity, index, name, full)
}

/// [`apply_full`] with the definition already resolved. `name` is carried for
/// the error alone.
fn apply_at(
    eng: &Engine,
    entity: Entity,
    index: usize,
    name: &str,
    full: &toml::Value,
) -> Result<()> {
    let registry = eng
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    {
        let registry = registry.borrow();
        let (_, def) = registry
            .at(index)
            .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
        (def.apply)(eng, entity, full)
            .with_context(|| format!("applying component '{name}'"))?;
    }
    mark(eng, entity, index, true);
    Ok(())
}

/// [`get`] with the definition already resolved.
fn get_at(eng: &Engine, entity: Entity, index: usize) -> Option<toml::Value> {
    let registry = eng.try_resource::<ComponentRegistry>()?;
    let registry = registry.borrow();
    let (_, def) = registry.at(index)?;
    (def.get)(eng, entity)
}

pub fn remove(eng: &Engine, entity: Entity, name: &str) -> Result<()> {
    let registry = eng
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let index = {
        let registry = registry.borrow();
        let index = registry
            .index_of(name)
            .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
        let (_, def) = registry
            .at(index)
            .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
        (def.remove)(eng, entity)?;
        index
    };
    mark(eng, entity, index, false);
    forget(eng, entity, name);
    Ok(())
}

/// Run the `remove` hook of every component the node carries, in
/// registration order, which is what a node's destruction owes its plugins.
///
/// Reads [`Attached`] rather than asking every definition, so a node that
/// was never given a component costs one lookup.
pub fn remove_present(eng: &Engine, entity: Entity) {
    let bits = eng
        .try_resource::<Attached>()
        .and_then(|attached| attached.borrow_mut().0.swap_remove(&entity))
        .unwrap_or(0);
    #[cfg(debug_assertions)]
    let bits = bits | untracked(eng, entity, bits);
    if bits == 0 {
        return;
    }
    let Some(registry) = eng.try_resource::<ComponentRegistry>() else {
        return;
    };
    let names: Vec<SmolStr> = registry
        .borrow()
        .iter()
        .enumerate()
        .filter(|(i, _)| bits & (1u128 << i) != 0)
        .map(|(_, (name, _))| name.clone())
        .collect();
    for name in names {
        if let Err(why) = remove(eng, entity, &name) {
            tracing::error!(error = %why, component = %name, "removing a component from a freed node");
        }
    }
}

/// A debug build's safety net for [`remove_present`]: the bits of components
/// a definition's `get` reports on the node and [`Attached`] does not, each
/// with a warning naming what attached it behind the registry's back.
#[cfg(debug_assertions)]
fn untracked(eng: &Engine, entity: Entity, bits: u128) -> u128 {
    let Some(registry) = eng.try_resource::<ComponentRegistry>() else {
        return 0;
    };
    let registry = registry.borrow();
    let mut extra = 0u128;
    for (i, (name, def)) in registry.iter().enumerate() {
        if bits & (1u128 << i) != 0 || (def.get)(eng, entity).is_none() {
            continue;
        }
        // The node bundle attaches a `Transform` in the one spawn, so a node
        // that never went through `add` carries one; freeing takes it off.
        if name != crate::transform::COMPONENT {
            tracing::warn!(component = %name, "attached behind the component registry; a release build would not run its remove hook");
        }
        extra |= 1u128 << i;
    }
    extra
}

/// The components that can answer one property on their own, by name.
///
/// A resource rather than a field on [`ComponentDef`]: every component builds
/// its whole table today, and this is the fast path for the one or two that a
/// UI pass reads a single property of, hundreds of times a frame.
#[derive(Default)]
pub struct PropertyReaders(std::collections::HashMap<String, PropertyFn>);

/// Say that `name` can answer a single property, and how.
pub fn answers_property(eng: &Engine, name: &str, read: PropertyFn) {
    if eng.try_resource::<PropertyReaders>().is_none() {
        eng.insert_resource(PropertyReaders::default());
    }
    if let Some(readers) = eng.try_resource::<PropertyReaders>() {
        readers.borrow_mut().0.insert(name.to_string(), read);
    }
}

/// One property of a component, without building the rest where the component
/// knows how to answer: `get` and index is what happens otherwise.
pub fn property(eng: &Engine, entity: Entity, name: &str, key: &str) -> Option<toml::Value> {
    if let Some(readers) = eng.try_resource::<PropertyReaders>() {
        let readers = readers.borrow();
        if let Some(read) = readers.0.get(name)
            && let Some(found) = read(eng, entity, key)
        {
            return Some(found);
        }
    }
    get(eng, entity, name)?.get(key).cloned()
}

/// Whether `entity` carries `name`, without building the component's table.
///
/// [`Attached`] answers on its own for anything the registry attached. A
/// component put on a node by another path has no bit, and `transform` is the
/// one built-in that does -- the node bundle carries it -- so a clear bit
/// falls back to asking the definition.
#[must_use]
pub fn has(eng: &Engine, entity: Entity, name: &str) -> bool {
    let Some(index) = index_of(eng, name) else {
        return false;
    };
    let bits = eng
        .try_resource::<Attached>()
        .and_then(|attached| attached.borrow().0.get(&entity).copied())
        .unwrap_or(0);
    if bits & (1u128 << index) != 0 {
        return true;
    }
    get_at(eng, entity, index).is_some()
}

pub fn get(eng: &Engine, entity: Entity, name: &str) -> Option<toml::Value> {
    let registry = eng.try_resource::<ComponentRegistry>()?;
    let registry = registry.borrow();
    registry.def(name).and_then(|def| (def.get)(eng, entity))
}

pub fn names(eng: &Engine) -> Vec<String> {
    eng.try_resource::<ComponentRegistry>()
        .map(|r| r.borrow().iter().map(|(n, _)| n.to_string()).collect())
        .unwrap_or_default()
}

/// Every registered component's name and schema, for tooling and docs.
pub fn schemas(eng: &Engine) -> Vec<(String, Rc<toml::Value>)> {
    eng.try_resource::<ComponentRegistry>()
        .map(|r| {
            r.borrow()
                .iter()
                .map(|(n, d)| (n.to_string(), d.schema.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn present_on(eng: &Engine, entity: Entity) -> Vec<String> {
    let Some(registry) = eng.try_resource::<ComponentRegistry>() else {
        return Vec::new();
    };
    let registry = registry.borrow();
    registry
        .iter()
        .filter(|(_, def)| (def.get)(eng, entity).is_some())
        .map(|(n, _)| n.to_string())
        .collect()
}

/// A node's stable identity from the scene file.
///
/// Present only on nodes that carry an `id`. It is what `parent` refers to and
/// what a future save path writes back, so it must survive rename, reparent
/// and reload.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StableId(pub String);
