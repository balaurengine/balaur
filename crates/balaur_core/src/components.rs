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
//!          | "color" | "asset" | "flags" | "node" | "list" | "map" | "record"
//!   default = ...          (required, and of the declared type)
//!   options = [...]        (enum and flags only, and required there)
//!   asset = "clip_type"    (asset only, and required there)
//!   of = { ... }           (list and map only, and required there: the spec
//!                           of what it holds)
//!   fields = { name = { ... } }
//!                          (record only, and required there: one spec a
//!                           field)
//!   key = "string" | "int" (map only, optional; "string" by default)
//!   class = "Wave"         (record only, optional: the script struct whose
//!                           shape it is, handed to a script as that class)
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
//! reserved for a tagged union's discriminant (`shape.kind = "sphere"`), so a
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
//! A `list`, a `map` and a `record` hold other properties: `of` is the spec a
//! `list`'s entries and a `map`'s values take, `fields` is one spec per
//! `record` field, and a `map`'s `key` says whether its keys are text or
//! whole numbers. A nested spec may leave its `default` out, and
//! [`complete_property`] writes the type's zero in at registration, so every
//! reader finds one at every depth. A `color` or an `asset` inside one is
//! expanded and resolved like any other. A `record` may name the script
//! `class` whose shape it is; the file still holds a table, and the script
//! host is what hands a script its own type back.
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

mod attached;
mod authored;
mod property;
mod schema;

use attached::mark;
pub(crate) use attached::mark_present;
pub use attached::{Attached, MAX_COMPONENTS, TRANSFORM_BIT, attached_of};
pub use authored::Authored;
use authored::{asked_for_at, forget, record_at, record_one};
pub(crate) use property::resolve_property_hooks;
pub use property::{
    PropertyReaders, PropertyWriteFn, PropertyWriters, answers_alone, answers_property, property,
    property_at, set_property, set_property_at, writes_property,
};
use schema::hex_rgba;
pub use schema::{
    PROPERTY_TYPES, UNITS, complete_property, inner_specs, validate_property, validate_value,
    zero_of,
};

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
/// What a component says is off about it on this node, beyond whether its
/// last write was accepted (see [`crate::warnings`]).
pub type WarningsFn = Option<Box<dyn Fn(&Engine, Entity) -> Vec<crate::warnings::Warning>>>;
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
    /// The events the component announces from its node, as `(name,
    /// payload)`: what an `emitted:<name>` row, a subscriber and the node's
    /// own `on_<name>` hear. Read by the Events view and the reference.
    pub events: &'static [(&'static str, &'static str)],
    /// Insert-or-update the component from a full property table.
    pub apply: ApplyFn,
    pub remove: RemoveFn,
    /// Current property table, or None when the entity lacks the component.
    ///
    /// Every property the component holds, because [`patch`] rebuilds from this
    /// and defaults whatever it omits. Values the component derives are the
    /// exception, and must be left out for the same reason.
    pub get: GetFn,
    /// What is off about the component on a node, for the editor to show.
    pub warnings: WarningsFn,
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
        let mut schema: toml::Value = toml::from_str(text)
            .unwrap_or_else(|e| panic!("component '{component}': schema is not valid TOML: {e}"));
        let table = schema.as_table_mut().unwrap_or_else(|| {
            panic!("component '{component}': schema is not a table of property specs")
        });
        for (prop, spec) in table {
            if let Err(why) = validate_property(spec) {
                panic!("component '{component}', property '{prop}': {why}");
            }
            complete_property(spec);
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
    /// Whether any property is `type = "record"`, so a value naming only some
    /// of its fields has the rest filled in before `apply` sees it.
    pub has_record: bool,
}

impl Facts {
    fn of(schema: &toml::Value) -> Self {
        let mut found = [false; 3];
        if let Some(table) = schema.as_table() {
            for spec in table.values() {
                scan_for_passes(spec, &mut found);
            }
        }
        let [has_color, has_asset, has_record] = found;
        Self {
            defaults: Rc::new(defaults_of(schema)),
            has_color,
            has_asset,
            has_record,
        }
    }
}

/// Whether a colour, an asset or a record sits anywhere in one property's
/// spec, a composite's contents included: `[color, asset, record]`.
fn scan_for_passes(spec: &toml::Value, found: &mut [bool; 3]) {
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("color") => found[0] = true,
        Some("asset") => found[1] = true,
        Some("record") => found[2] = true,
        _ => {}
    }
    for inner in inner_specs(spec) {
        scan_for_passes(inner, found);
    }
}

/// Every declared field of a `record` value, so a scene naming one of two
/// fields still hands `apply` both. A key no field answers to is dropped:
/// the record's shape is the schema's.
fn fill_records(schema: &toml::Value, out: &mut toml::map::Map<String, toml::Value>) {
    let Some(table) = schema.as_table() else {
        return;
    };
    for (prop, spec) in table {
        if let Some(value) = out.get_mut(prop) {
            fill_record_value(spec, value);
        }
    }
}

fn fill_record_value(spec: &toml::Value, value: &mut toml::Value) {
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("record") => {
            let Some(fields) = spec.get("fields").and_then(toml::Value::as_table) else {
                return;
            };
            let held = value.as_table().cloned().unwrap_or_default();
            let mut whole = toml::map::Map::new();
            for (name, field) in fields {
                let mut inner = held
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| schema::zero_of(field));
                fill_record_value(field, &mut inner);
                whole.insert(name.clone(), inner);
            }
            *value = toml::Value::Table(whole);
        }
        Some("list" | "map") => {
            if let Some(of) = spec.get("of") {
                for inner in held_mut(value) {
                    fill_record_value(of, inner);
                }
            }
        }
        _ => {}
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
        let first = self.defs.is_empty();
        assert!(
            self.defs.insert(SmolStr::new(name), def).is_none(),
            "component '{name}' is registered twice; a name belongs to one definition"
        );
        assert!(
            !first || name == crate::transform::COMPONENT,
            "'{name}' registered before '{}', which owns TRANSFORM_BIT and is what \
             the node bundle marks",
            crate::transform::COMPONENT
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

/// A registered component's position in the registry, which is its key above.
/// Where `name` sits in registration order, which is the number every other
/// `_at` entry point takes.
///
/// A backend that dispatches on a component name resolves it once, when it
/// builds its handles, and passes the number afterwards: that is what keeps a
/// property read off the hash table and out of the allocator.
#[must_use]
pub fn index_of(eng: &Engine, name: &str) -> Option<usize> {
    let registry = eng.try_resource::<ComponentRegistry>()?;
    let at = registry.borrow().index_of(name);
    drop(registry);
    at
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
        if let Some(value) = out.get_mut(prop) {
            expand_color_value(prop, spec, value);
        }
    }
}

/// One value's colours, wherever the spec puts them: a colour held in a list
/// or a record is written the same way as one held by the property itself.
fn expand_color_value(prop: &str, spec: &toml::Value, value: &mut toml::Value) {
    // `type` is the spec's datatype key (see the module docs).
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("color") => {
            let Some(text) = value.as_str() else {
                return;
            };
            if let Some(rgba) = hex_rgba(text) {
                *value = toml::Value::Array(rgba.iter().copied().map(toml::Value::Float).collect());
            } else {
                tracing::warn!(
                    property = prop,
                    value = text,
                    "not a colour; expected #rrggbb, #rrggbbaa or [r, g, b, a]"
                );
            }
        }
        Some("list" | "map") => {
            if let Some(of) = spec.get("of") {
                for inner in held_mut(value) {
                    expand_color_value(prop, of, inner);
                }
            }
        }
        Some("record") => {
            for (field, inner) in fields_mut(spec, value) {
                expand_color_value(prop, field, inner);
            }
        }
        _ => {}
    }
}

/// What a `list` or a `map` value holds, for a pass that rewrites entries.
fn held_mut(value: &mut toml::Value) -> Vec<&mut toml::Value> {
    match value {
        toml::Value::Array(items) => items.iter_mut().collect(),
        toml::Value::Table(table) => table.iter_mut().map(|(_, inner)| inner).collect(),
        _ => Vec::new(),
    }
}

/// A `record` value's entries beside the spec each field declares. A key no
/// field answers to is skipped; `validate_property` already refused it.
fn fields_mut<'a>(
    spec: &'a toml::Value,
    value: &'a mut toml::Value,
) -> Vec<(&'a toml::Value, &'a mut toml::Value)> {
    let (Some(fields), Some(table)) = (
        spec.get("fields").and_then(toml::Value::as_table),
        value.as_table_mut(),
    ) else {
        return Vec::new();
    };
    table
        .iter_mut()
        .filter_map(|(name, inner)| Some((fields.get(name)?, inner)))
        .collect()
}

pub fn merge_defaults(schema: &toml::Value, params: Option<&toml::Value>) -> Result<toml::Value> {
    let mut out = defaults_of(schema);
    overlay(schema, &mut out, params)?;
    fill_records(schema, &mut out);
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
        if let Some(value) = out.get_mut(prop) {
            resolve_asset_value(eng, prop, spec, value)?;
        }
    }
    Ok(())
}

/// One value's assets, wherever the spec puts them.
fn resolve_asset_value(
    eng: &Engine,
    prop: &str,
    spec: &toml::Value,
    value: &mut toml::Value,
) -> Result<()> {
    match spec.get("type").and_then(toml::Value::as_str) {
        Some("asset") => {
            let type_name = spec
                .get("asset")
                .and_then(toml::Value::as_str)
                .unwrap_or("");
            if let Some(reference) = asset_reference(eng, prop, type_name, value)? {
                *value = toml::Value::String(reference);
            }
        }
        Some("list" | "map") => {
            if let Some(of) = spec.get("of") {
                for inner in held_mut(value) {
                    resolve_asset_value(eng, prop, of, inner)?;
                }
            }
        }
        Some("record") => {
            for (field, inner) in fields_mut(spec, value) {
                resolve_asset_value(eng, prop, field, inner)?;
            }
        }
        _ => {}
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
    let Resolved { index, schema, .. } = resolve(eng, name)?;
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
    let Resolved {
        index,
        schema,
        defaults,
        has_color,
        has_asset,
        has_record,
    } = resolve(eng, name)?;
    // A write of what the component already holds changes nothing, and a
    // script setting a value every frame would otherwise rebuild it every frame.
    // The component's own reader answers that without building its table.
    if let Some(asked) = params.as_table()
        && attached_of(eng, entity).has(index)
        && property::holds_already(eng, entity, index, asked)
    {
        unchanged(eng, entity, index, name, params);
        return Ok(());
    }
    let current = get_at(eng, entity, index);
    if let (Some(toml::Value::Table(held)), Some(asked)) = (&current, params.as_table())
        && asked
            .iter()
            .all(|(key, value)| held.get(key) == Some(value))
    {
        unchanged(eng, entity, index, name, params);
        return Ok(());
    }
    // The component's own table is the base, taken rather than copied: what
    // `get` leaves out is filled from the request, then from the defaults.
    let mut out = match current {
        Some(toml::Value::Table(table)) => table,
        Some(other) => {
            let mut table = toml::map::Map::new();
            overlay(&schema, &mut table, Some(&other))?;
            table
        }
        None => toml::map::Map::new(),
    };
    // A `get` reporting every property the schema declares, which is its
    // contract, leaves the two fills below nothing to find and a table to clone.
    if !defaults.keys().all(|key| out.contains_key(key)) {
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
    }
    overlay(&schema, &mut out, Some(params))?;
    if has_record {
        fill_records(&schema, &mut out);
    }
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

/// What a [`patch`] that changed nothing still owes: the refusal it answers
/// cleared, and the request filed for a save.
fn unchanged(eng: &Engine, entity: Entity, index: usize, name: &str, params: &toml::Value) {
    crate::warnings::accepted(eng, entity, name);
    record_at(eng, entity, index, Some(params), false);
}

/// Whether a name is a registered component, as opposed to some other scene
/// key a plugin declared.
#[must_use]
pub fn is_registered(eng: &Engine, name: &str) -> bool {
    eng.try_resource::<ComponentRegistry>()
        .is_some_and(|registry| registry.borrow().def(name).is_some())
}

/// Everything a write needs from the registry, taken in one borrow.
struct Resolved {
    /// Where the component sits, which is its bit in [`Attached`].
    index: usize,
    schema: Rc<toml::Value>,
    /// [`Facts::defaults`], shared rather than rebuilt.
    defaults: Rc<toml::map::Map<String, toml::Value>>,
    has_color: bool,
    has_asset: bool,
    has_record: bool,
}

/// Resolve `name` once.
///
/// A write used to look the name up four times -- for the schema, for what was
/// asked before, to apply, and to record -- and each one re-entered the
/// resource map and the registry.
fn resolve(eng: &Engine, name: &str) -> Result<Resolved> {
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
    Ok(Resolved {
        index,
        schema,
        defaults: facts.defaults.clone(),
        has_color: facts.has_color,
        has_asset: facts.has_asset,
        has_record: facts.has_record,
    })
}

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
        if let Err(why) = (def.apply)(eng, entity, full) {
            let held = (def.get)(eng, entity);
            crate::warnings::refused(eng, entity, name, held.as_ref(), full, &why);
            return Err(why.context(format!("applying component '{name}'")));
        }
    }
    crate::warnings::accepted(eng, entity, name);
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
    crate::warnings::accepted(eng, entity, name);
    Ok(())
}

/// Run the `remove` hook of every component the node carries, in
/// registration order, which is what a node's destruction owes its plugins.
///
/// Reads [`Attached`] rather than asking every definition, so a node that
/// was never given a component costs one lookup.
pub fn remove_present(eng: &Engine, entity: Entity) {
    crate::warnings::forget(eng, entity);
    let owed = attached_of(eng, entity);
    let bits = owed.hooked;
    #[cfg(debug_assertions)]
    let bits = bits | untracked(eng, entity, owed.present);
    // Cleared in place: removing the component moves the node to another
    // archetype, a copy of everything it holds just before it is despawned.
    if let Ok(mut held) = eng.world().get::<&mut Attached>(entity) {
        *held = Attached::default();
    }
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
        tracing::warn!(component = %name, "attached behind the component registry; a release build would not run its remove hook");
        extra |= 1u128 << i;
    }
    extra
}

/// Whether `entity` carries `name`, without building the component's table.
///
/// [`Attached`] is the whole answer: every path that gives a node a component
/// marks a bit, `add` through the registry and the node bundle's own
/// `Transform` alike. A definition is never asked, so this costs one
/// archetype lookup whatever the component holds.
#[must_use]
pub fn has(eng: &Engine, entity: Entity, name: &str) -> bool {
    let Some(index) = index_of(eng, name) else {
        return false;
    };
    attached_of(eng, entity).has(index)
}

/// [`patch`] with the definition already resolved.
///
/// # Errors
/// What [`patch`] errors on.
pub fn patch_at(eng: &Engine, entity: Entity, index: usize, params: &toml::Value) -> Result<()> {
    let name = eng
        .try_resource::<ComponentRegistry>()
        .and_then(|r| r.borrow().at(index).map(|(n, _)| n.clone()))
        .ok_or_else(|| anyhow!("no component at {index}"))?;
    patch(eng, entity, &name, params)
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

/// What the components on a node announce, by event name, in the order the
/// registry holds the components.
pub fn events_on(eng: &Engine, entity: Entity) -> Vec<&'static str> {
    let Some(registry) = eng.try_resource::<ComponentRegistry>() else {
        return Vec::new();
    };
    let registry = registry.borrow();
    let bits = attached_of(eng, entity);
    registry
        .iter()
        .enumerate()
        .filter(|(i, _)| bits.has(*i))
        .flat_map(|(_, (_, def))| def.events.iter().map(|(name, _)| *name))
        .collect()
}

pub fn present_on(eng: &Engine, entity: Entity) -> Vec<String> {
    let Some(registry) = eng.try_resource::<ComponentRegistry>() else {
        return Vec::new();
    };
    let registry = registry.borrow();
    let bits = attached_of(eng, entity);
    registry
        .iter()
        .enumerate()
        .filter(|(i, _)| bits.has(*i))
        .map(|(_, (n, _))| n.to_string())
        .collect()
}

/// A node's stable identity from the scene file.
///
/// Present only on nodes that carry an `id`. It is what `parent` refers to and
/// what a future save path writes back, so it must survive rename, reparent
/// and reload.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StableId(pub String);
