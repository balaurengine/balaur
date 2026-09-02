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
//!   type = "float" | "bool" | "string" | "enum" | "vec2" | "vec3" | "color"
//!          | "asset" | "flags" | "node"
//!   default = ...          (required, and of the declared type)
//!   options = [...]        (enum and flags only, and required there)
//!   asset = "clip_type"    (asset only, and required there)
//!   min/max/step/decimals  (float, optional)
//!   shorthand/readonly     (bool, optional)
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

use anyhow::{anyhow, Context, Result};
use hecs::Entity;

use crate::engine::Engine;

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

pub struct ComponentDef {
    /// TOML table of property specs (see module docs).
    pub schema: toml::Value,
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
    pub get: GetFn,
}

/// The datatypes a schema property may declare (rule N6). Closed: a plugin
/// that wants another one adds it here, so the editor's inspector and the
/// scene format learn about it at the same moment.
pub const PROPERTY_TYPES: [&str; 10] = [
    "float", "bool", "string", "enum", "vec2", "vec3", "color", "asset", "flags", "node",
];

impl ComponentDef {
    /// Parse and validate a schema from TOML text.
    ///
    /// Panics naming the component, the property and the key at fault. Schemas
    /// are compile-time constants written by plugin authors, so a bad one is a
    /// bug in the plugin rather than bad user input, and failing at
    /// registration beats an inspector row that silently never appears.
    pub fn parse_schema(component: &str, text: &str) -> toml::Value {
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
        schema
    }
}

/// The closed set as prose, for a panic message.
fn type_list() -> String {
    PROPERTY_TYPES
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One property spec against the vocabulary in the module docs. The `Err` is
/// the reason alone; the caller prefixes the component and property.
fn validate_property(spec: &toml::Value) -> Result<(), String> {
    let spec = spec.as_table().ok_or_else(|| {
        format!(
            "spec is {}, not a table like {{ type = \"float\", default = 0.0 }}",
            spec.type_str()
        )
    })?;
    let declared = match spec.get("type").map(toml::Value::as_str) {
        Some(Some(declared)) => declared,
        Some(None) => return Err(format!("`type` is not a string; expected {}", type_list())),
        None => return Err(format!("no `type` key; expected one of {}", type_list())),
    };
    if !PROPERTY_TYPES.contains(&declared) {
        return Err(format!(
            "`type = \"{declared}\"` is not one of {}",
            type_list()
        ));
    }
    let options = spec.get("options");
    let takes_options = declared == "enum" || declared == "flags";
    match (takes_options, options) {
        (true, None) => return Err(format!("`type = \"{declared}\"` needs an `options` list")),
        (false, Some(_)) => {
            return Err(format!(
                "`options` belongs to `type = \"enum\"` and `type = \"flags\"`, not `type = \
                 \"{declared}\"`"
            ))
        }
        _ => {}
    }
    // `type` is the datatype key, so the asset's own type name needs a second
    // key rather than reusing it (N6): `{ type = "asset", asset = "clip" }`.
    match (declared == "asset", spec.get("asset")) {
        (true, None) => {
            return Err(
                "`type = \"asset\"` needs an `asset` key naming the asset type it takes".into(),
            )
        }
        (true, Some(name)) if name.as_str().is_none() => {
            return Err(format!("`asset` is {}, not a type name", name.type_str()))
        }
        (false, Some(_)) => {
            return Err(format!(
                "`asset` belongs to `type = \"asset\"`, not `type = \"{declared}\"`"
            ))
        }
        _ => {}
    }
    if let Some(description) = spec.get("description") {
        if description.as_str().is_none() {
            return Err(format!(
                "`description` is {}, not a string",
                description.type_str()
            ));
        }
    }
    let default = spec
        .get("default")
        .ok_or_else(|| format!("no `default`; every property needs one, of type `{declared}`"))?;
    check_default(declared, default, options)
}

/// `default` against its declared type.
fn check_default(
    declared: &str,
    default: &toml::Value,
    options: Option<&toml::Value>,
) -> Result<(), String> {
    let (ok, wanted) = match declared {
        "float" => (as_f64(default).is_some(), "a number"),
        "bool" => (default.as_bool().is_some(), "true or false"),
        // An asset default is a reference, and a reference is a path string.
        // A node default is a scene-relative path, and a path is a string.
        "string" | "asset" | "node" => (default.as_str().is_some(), "a string"),
        "enum" => return check_enum_default(default, options),
        "flags" => return check_flags_default(default, options),
        "vec2" => return check_numbers(declared, default, &[2]),
        "vec3" => return check_numbers(declared, default, &[3]),
        "color" => return check_color_default(default),
        _ => (true, ""),
    };
    if ok {
        return Ok(());
    }
    Err(format!(
        "`default` is {}, but `type = \"{declared}\"` wants {wanted}",
        default.type_str()
    ))
}

/// An enum's `default` must be a string, and one the `options` list offers —
/// otherwise the editor opens a dropdown that cannot show its own value.
fn check_enum_default(default: &toml::Value, options: Option<&toml::Value>) -> Result<(), String> {
    let choices = options
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "`options` is not a list of strings".to_string())?;
    let choices: Vec<&str> = choices.iter().filter_map(toml::Value::as_str).collect();
    if choices.is_empty() {
        return Err("`options` is empty, so no `default` can be legal".into());
    }
    let default = default.as_str().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"enum\"` wants one of the `options` strings",
            default.type_str()
        )
    })?;
    if choices.contains(&default) {
        return Ok(());
    }
    Err(format!(
        "`default = \"{default}\"` is not in `options` {choices:?}"
    ))
}

/// A `flags` default: an array, each entry one of the `options` strings. The
/// empty array is legal and is what most flag properties default to.
fn check_flags_default(default: &toml::Value, options: Option<&toml::Value>) -> Result<(), String> {
    let choices = options
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "`options` is not a list of strings".to_string())?;
    let choices: Vec<&str> = choices.iter().filter_map(toml::Value::as_str).collect();
    if choices.is_empty() {
        return Err("`options` is empty, so the property can hold nothing".into());
    }
    let default = default.as_array().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"flags\"` wants an array of `options` strings",
            default.type_str()
        )
    })?;
    for entry in default {
        let Some(name) = entry.as_str() else {
            return Err(format!(
                "`default` holds {}, but `type = \"flags\"` wants `options` strings",
                entry.type_str()
            ));
        };
        if !choices.contains(&name) {
            return Err(format!(
                "`default` holds \"{name}\", which is not in `options` {choices:?}"
            ));
        }
    }
    Ok(())
}

/// A `vec2`/`vec3` default: an array of numbers of exactly the right length.
fn check_numbers(declared: &str, default: &toml::Value, lengths: &[usize]) -> Result<(), String> {
    let wanted = || {
        lengths
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(" or ")
    };
    let array = default.as_array().ok_or_else(|| {
        format!(
            "`default` is {}, but `type = \"{declared}\"` wants an array of {} numbers",
            default.type_str(),
            wanted()
        )
    })?;
    if !lengths.contains(&array.len()) {
        return Err(format!(
            "`default` has {} entries, but `type = \"{declared}\"` wants {}",
            array.len(),
            wanted()
        ));
    }
    if array.iter().all(|v| as_f64(v).is_some()) {
        return Ok(());
    }
    Err(format!(
        "`default` holds something that is not a number, but `type = \"{declared}\"` wants numbers"
    ))
}

/// A `color` default, in either spelling the value form accepts.
fn check_color_default(default: &toml::Value) -> Result<(), String> {
    if let Some(text) = default.as_str() {
        return if hex_rgba(text).is_some() {
            Ok(())
        } else {
            Err(format!(
                "`default = \"{text}\"` is not #rrggbb or #rrggbbaa"
            ))
        };
    }
    check_numbers("color", default, &[3, 4])
}

/// Ordered by registration: scene keys and editor sections follow it.
#[derive(Default)]
pub struct ComponentRegistry(pub Vec<(String, ComponentDef)>);

impl ComponentRegistry {
    pub fn def(&self, name: &str) -> Option<&ComponentDef> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, def)| def)
    }
}

/// Merge `params` over the schema's defaults, producing the full property
/// table `apply` hooks receive. Unknown keys pass through untouched (schemas
/// evolve; scenes may carry newer keys).
/// `#rrggbb` or `#rrggbbaa` as `[r, g, b, a]` in 0..=1.
fn hex_rgba(text: &str) -> Option<[f64; 4]> {
    let hex = text.strip_prefix('#')?;
    let channel = |i: usize| {
        u8::from_str_radix(hex.get(i..i + 2)?, 16)
            .ok()
            .map(|b| f64::from(b) / 255.0)
    };
    match hex.len() {
        6 => Some([channel(0)?, channel(2)?, channel(4)?, 1.0]),
        8 => Some([channel(0)?, channel(2)?, channel(4)?, channel(6)?]),
        _ => None,
    }
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

pub fn merge_defaults(schema: &toml::Value, params: Option<&toml::Value>) -> toml::Value {
    let mut out = defaults_of(schema);
    overlay(schema, &mut out, params);
    expand_colors(schema, &mut out);
    toml::Value::Table(out)
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
) {
    match params {
        Some(toml::Value::Table(params)) => {
            for (k, v) in params {
                out.insert(k.clone(), v.clone());
            }
        }
        // Scalar/array shorthand (`body = "static"`, `color = [1, 0, 0]`)
        // lands on the prop marked `shorthand = true` in the schema.
        Some(other) => {
            if let Some(table) = schema.as_table() {
                for (prop, spec) in table {
                    if spec.get("shorthand").and_then(toml::Value::as_bool) == Some(true) {
                        out.insert(prop.clone(), other.clone());
                        break;
                    }
                }
            }
        }
        None => {}
    }
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
    resolved(eng, schema, merge_defaults(schema, params))
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
        toml::Value::Table(_) => Ok(Some(
            crate::assets::define_inline(eng, type_name, value.clone())?.to_string(),
        )),
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
    let schema = schema_of(eng, name)?;
    let full = properties(eng, &schema, params)?;
    apply_full(eng, entity, name, &full)
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
pub fn patch(eng: &Engine, entity: Entity, name: &str, params: &toml::Value) -> Result<()> {
    let schema = schema_of(eng, name)?;
    let current = get(eng, entity, name);
    let mut out = defaults_of(&schema);
    overlay(&schema, &mut out, current.as_ref());
    overlay(&schema, &mut out, Some(params));
    expand_colors(&schema, &mut out);
    let full = resolved(eng, &schema, toml::Value::Table(out))?;
    apply_full(eng, entity, name, &full)
}

/// A registered component's schema, cloned so the registry borrow ends here.
fn schema_of(eng: &Engine, name: &str) -> Result<toml::Value> {
    let registry = eng
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let registry = registry.borrow();
    Ok(registry
        .def(name)
        .ok_or_else(|| anyhow!("unknown component '{name}'"))?
        .schema
        .clone())
}

/// Hand a finished property table to the component's `apply` hook.
fn apply_full(eng: &Engine, entity: Entity, name: &str, full: &toml::Value) -> Result<()> {
    let registry = eng
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let registry = registry.borrow();
    let def = registry
        .def(name)
        .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    (def.apply)(eng, entity, full).with_context(|| format!("applying component '{name}'"))
}

pub fn remove(eng: &Engine, entity: Entity, name: &str) -> Result<()> {
    let registry = eng
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let registry = registry.borrow();
    let def = registry
        .def(name)
        .ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    (def.remove)(eng, entity)
}

pub fn get(eng: &Engine, entity: Entity, name: &str) -> Option<toml::Value> {
    let registry = eng.try_resource::<ComponentRegistry>()?;
    let registry = registry.borrow();
    registry.def(name).and_then(|def| (def.get)(eng, entity))
}

pub fn names(eng: &Engine) -> Vec<String> {
    eng.try_resource::<ComponentRegistry>()
        .map(|r| r.borrow().0.iter().map(|(n, _)| n.clone()).collect())
        .unwrap_or_default()
}

/// Every registered component's name and schema, for tooling and docs.
pub fn schemas(eng: &Engine) -> Vec<(String, toml::Value)> {
    eng.try_resource::<ComponentRegistry>()
        .map(|r| {
            r.borrow()
                .0
                .iter()
                .map(|(n, d)| (n.clone(), d.schema.clone()))
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
        .0
        .iter()
        .filter(|(_, def)| (def.get)(eng, entity).is_some())
        .map(|(n, _)| n.clone())
        .collect()
}

/// A node's stable identity from the scene file.
///
/// Present only on nodes that carry an `id`. It is what `parent` refers to and
/// what a future save path writes back, so it must survive rename, reparent
/// and reload.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StableId(pub String);
