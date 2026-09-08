//! The engine-level script modules, declared once for every language.
//!
//! `engine` is the clock, argv and quit; `scene` is the tree, spawning and
//! instancing. Same shape as `node_api`: a list of function pointers a backend
//! registers, so a new language inherits them.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::{Result, anyhow};
use balaur_script::{Bindings as _, Value};

use crate::batteries_api::{
    assets_assign_id, assets_directory, assets_duplicate, assets_exists, assets_id,
    assets_invalidate, assets_load, assets_path, assets_reload, assets_rename, assets_save,
    dark_mode, device_id, encoding_base64, encoding_from_base64, focused, hash_sha256,
    hash_sha256_text, log_clear, log_error, log_info, log_recent, log_warn, platform, rng_int,
    rng_random, rng_range, rng_seed, rng_uuid, scene_tagged, strings_system_locale, unix_time,
};
use crate::engine::Engine;
use crate::file_api::{
    fs_copy, fs_exists, fs_list, fs_mkdir, fs_mtime, fs_read, fs_remove, fs_rename, fs_write,
    json_encode, json_parse, toml_encode, toml_parse, toml_patch,
};
use crate::scene;

// Callers reach these through `engine_api` because that is where they were
// declared; the code lives in `file_api`.
pub(crate) use crate::file_api::resolve;
pub use crate::file_api::{from_json, to_json};

/// One engine operation, tagged with the module it belongs to.
pub struct EngineOp {
    pub module: &'static str,
    pub name: &'static str,
    pub call: fn(&Engine, &[Value]) -> Result<Value>,
}

/// Everything the engine itself exposes to scripts.
pub const ENGINE_OPS: &[EngineOp] = &[
    EngineOp {
        module: "engine",
        name: "time",
        call: time,
    },
    EngineOp {
        module: "engine",
        name: "delta",
        call: delta,
    },
    EngineOp {
        module: "engine",
        name: "tick",
        call: tick,
    },
    EngineOp {
        module: "engine",
        name: "quit",
        call: quit,
    },
    EngineOp {
        module: "engine",
        name: "args",
        call: args,
    },
    EngineOp {
        module: "engine",
        name: "reload_script",
        call: reload_script,
    },
    EngineOp {
        module: "engine",
        name: "user_data_dir",
        call: user_data_dir,
    },
    EngineOp {
        module: "engine",
        name: "open_url",
        call: crate::desktop_api::open_url,
    },
    EngineOp {
        module: "engine",
        name: "reveal",
        call: crate::desktop_api::reveal,
    },
    EngineOp {
        module: "engine",
        name: "platform",
        call: platform,
    },
    EngineOp {
        module: "engine",
        name: "device_id",
        call: device_id,
    },
    EngineOp {
        module: "engine",
        name: "unix_time",
        call: unix_time,
    },
    EngineOp {
        module: "engine",
        name: "focused",
        call: focused,
    },
    EngineOp {
        module: "engine",
        name: "dark_mode",
        call: dark_mode,
    },
    EngineOp {
        module: "scene",
        name: "root",
        call: root,
    },
    EngineOp {
        module: "scene",
        name: "get_node",
        call: get_node,
    },
    EngineOp {
        module: "scene",
        name: "node_by_id",
        call: node_by_id,
    },
    EngineOp {
        module: "scene",
        name: "with_component",
        call: with_component,
    },
    EngineOp {
        module: "scene",
        name: "spawn",
        call: spawn,
    },
    EngineOp {
        module: "scene",
        name: "instantiate",
        call: instantiate,
    },
    EngineOp {
        module: "scene",
        name: "source",
        call: source,
    },
    EngineOp {
        module: "scene",
        name: "component_types",
        call: component_types,
    },
    EngineOp {
        module: "scene",
        name: "component_tags",
        call: component_tags,
    },
    EngineOp {
        module: "scene",
        name: "component_expects",
        call: component_expects,
    },
    EngineOp {
        module: "scene",
        name: "presets",
        call: presets,
    },
    EngineOp {
        module: "scene",
        name: "preset_info",
        call: preset_info,
    },
    EngineOp {
        module: "scene",
        name: "apply_preset",
        call: apply_preset,
    },
    EngineOp {
        module: "scene",
        name: "unmet_expectations",
        call: unmet_expectations,
    },
    EngineOp {
        module: "scene",
        name: "component_schema",
        call: component_schema,
    },
    EngineOp {
        module: "scene",
        name: "component_properties",
        call: component_properties,
    },
    EngineOp {
        module: "scene",
        name: "switch",
        call: crate::scene_api::scene_switch,
    },
    EngineOp {
        module: "scene",
        name: "bindable_events",
        call: crate::scene_api::bindable_events,
    },
    EngineOp {
        module: "scene",
        name: "binding_actions",
        call: crate::scene_api::binding_actions,
    },
    EngineOp {
        module: "scene",
        name: "variable",
        call: crate::scene_api::scene_variable,
    },
    EngineOp {
        module: "scene",
        name: "set_variable",
        call: crate::scene_api::scene_set_variable,
    },
    EngineOp {
        module: "scene",
        name: "variables",
        call: crate::scene_api::scene_variables,
    },
    EngineOp {
        module: "engine",
        name: "timings",
        call: timings,
    },
    EngineOp {
        module: "engine",
        name: "profile_scripts",
        call: profile_scripts,
    },
    EngineOp {
        module: "engine",
        name: "script_costs",
        call: script_costs,
    },
    EngineOp {
        module: "save",
        name: "write",
        call: save_write,
    },
    EngineOp {
        module: "save",
        name: "read",
        call: save_read,
    },
    EngineOp {
        module: "save",
        name: "slots",
        call: save_slots,
    },
    EngineOp {
        module: "save",
        name: "remove",
        call: save_remove,
    },
    EngineOp {
        module: "save",
        name: "version",
        call: save_version,
    },
    EngineOp {
        module: "strings",
        name: "tr",
        call: strings_tr,
    },
    EngineOp {
        module: "strings",
        name: "locale",
        call: strings_locale,
    },
    EngineOp {
        module: "strings",
        name: "set_locale",
        call: strings_set_locale,
    },
    EngineOp {
        module: "strings",
        name: "locales",
        call: strings_locales,
    },
    EngineOp {
        module: "strings",
        name: "set_root",
        call: strings_set_root,
    },
    EngineOp {
        module: "strings",
        name: "system_locale",
        call: strings_system_locale,
    },
    EngineOp {
        module: "scene",
        name: "tagged",
        call: scene_tagged,
    },
    EngineOp {
        module: "skeleton",
        name: "apply_rest",
        call: crate::skeleton::apply_rest_op,
    },
    EngineOp {
        module: "skeleton",
        name: "overwrite_rest",
        call: crate::skeleton::overwrite_rest_op,
    },
    EngineOp {
        module: "skeleton",
        name: "bones",
        call: crate::skeleton::bones_op,
    },
    EngineOp {
        module: "assets",
        name: "load",
        call: assets_load,
    },
    EngineOp {
        module: "assets",
        name: "duplicate",
        call: assets_duplicate,
    },
    EngineOp {
        module: "assets",
        name: "exists",
        call: assets_exists,
    },
    EngineOp {
        module: "assets",
        name: "reload",
        call: assets_reload,
    },
    EngineOp {
        module: "assets",
        name: "invalidate",
        call: assets_invalidate,
    },
    EngineOp {
        module: "assets",
        name: "save",
        call: assets_save,
    },
    EngineOp {
        module: "assets",
        name: "directory",
        call: assets_directory,
    },
    EngineOp {
        module: "assets",
        name: "rename",
        call: assets_rename,
    },
    EngineOp {
        module: "assets",
        name: "id",
        call: assets_id,
    },
    EngineOp {
        module: "assets",
        name: "assign_id",
        call: assets_assign_id,
    },
    EngineOp {
        module: "assets",
        name: "path",
        call: assets_path,
    },
    EngineOp {
        module: "log",
        name: "info",
        call: log_info,
    },
    EngineOp {
        module: "log",
        name: "warn",
        call: log_warn,
    },
    EngineOp {
        module: "log",
        name: "error",
        call: log_error,
    },
    EngineOp {
        module: "log",
        name: "recent",
        call: log_recent,
    },
    EngineOp {
        module: "log",
        name: "clear",
        call: log_clear,
    },
    EngineOp {
        module: "rng",
        name: "seed",
        call: rng_seed,
    },
    EngineOp {
        module: "rng",
        name: "random",
        call: rng_random,
    },
    EngineOp {
        module: "rng",
        name: "range",
        call: rng_range,
    },
    EngineOp {
        module: "rng",
        name: "int",
        call: rng_int,
    },
    EngineOp {
        module: "rng",
        name: "uuid",
        call: rng_uuid,
    },
    EngineOp {
        module: "hash",
        name: "sha256",
        call: hash_sha256,
    },
    EngineOp {
        module: "hash",
        name: "sha256_text",
        call: hash_sha256_text,
    },
    EngineOp {
        module: "encoding",
        name: "base64",
        call: encoding_base64,
    },
    EngineOp {
        module: "encoding",
        name: "from_base64",
        call: encoding_from_base64,
    },
    EngineOp {
        module: "fs",
        name: "read",
        call: fs_read,
    },
    EngineOp {
        module: "fs",
        name: "write",
        call: fs_write,
    },
    EngineOp {
        module: "fs",
        name: "exists",
        call: fs_exists,
    },
    EngineOp {
        module: "fs",
        name: "list",
        call: fs_list,
    },
    EngineOp {
        module: "fs",
        name: "remove",
        call: fs_remove,
    },
    EngineOp {
        module: "fs",
        name: "mkdir",
        call: fs_mkdir,
    },
    EngineOp {
        module: "fs",
        name: "rename",
        call: fs_rename,
    },
    EngineOp {
        module: "fs",
        name: "copy",
        call: fs_copy,
    },
    EngineOp {
        module: "fs",
        name: "mtime",
        call: fs_mtime,
    },
    EngineOp {
        module: "engine",
        name: "plugins",
        call: loaded_plugins,
    },
    EngineOp {
        module: "engine",
        name: "has_plugin",
        call: has_plugin,
    },
    EngineOp {
        module: "engine",
        name: "plugin_version",
        call: plugin_version,
    },
    EngineOp {
        module: "toml",
        name: "parse",
        call: toml_parse,
    },
    EngineOp {
        module: "toml",
        name: "encode",
        call: toml_encode,
    },
    EngineOp {
        module: "toml",
        name: "patch",
        call: toml_patch,
    },
    EngineOp {
        module: "json",
        name: "parse",
        call: json_parse,
    },
    EngineOp {
        module: "json",
        name: "encode",
        call: json_encode,
    },
];

/// Register every engine module into the host, plus the node API under `node`.
///
/// Called once when an app gains a script backend. A backend that gives its
/// node handle method syntax still walks `node_api::NODE_OPS` for the
/// sugar; this is what makes the operations reachable at all.
///
/// Takes `&Engine` rather than a `Bindings`: unlike every other
/// `install_*`, it creates the modules on the host itself instead of filling
/// one it was handed, because the operations it registers span several
/// modules.
pub fn install_engine_api(eng: &Engine) -> Result<()> {
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    let mut current: Option<(&str, Box<dyn balaur_script::Bindings<Engine>>)> = None;
    for d in ENGINE_OPS {
        let m = match &mut current {
            Some((name, m)) if *name == d.module => m,
            _ => {
                let mut fresh = host.module(d.module)?;
                document(d.module, &mut *fresh);
                current = Some((d.module, fresh));
                &mut current.as_mut().expect("just assigned").1
            }
        };
        m.function_raw(d.name, Box::new(d.call));
    }
    let mut node = host.module("node")?;
    crate::node_api::install_node_api(&mut *node);
    let mut debugger = host.module("debugger")?;
    crate::debugger_api::install_debugger_api(&mut *debugger);
    let mut replay = host.module("replay")?;
    crate::replay_api::install_replay_api(&mut *replay);
    let mut math = host.module("math")?;
    crate::math_api::install_math_api(&mut *math);
    let mut rollback = host.module("rollback")?;
    crate::rollback_api::install_rollback_api(&mut *rollback);
    let mut settings = host.module("settings")?;
    crate::settings_api::install_settings_api(&mut *settings);
    let mut events = host.module("events")?;
    crate::events::install_events_api(&mut *events);
    let mut geometry2d = host.module("geometry2d")?;
    crate::geometry2d::install_geometry2d_api(&mut *geometry2d);
    Ok(())
}

/// The reference text for one module, added as that module is created.
///
/// One table spans nine modules, so this cannot sit beside a single
/// `install_*` the way every other subsystem's documentation does.
fn document(module: &str, m: &mut dyn balaur_script::Bindings<Engine>) {
    match module {
        "engine" => crate::engine_docs::document_engine(m),
        "scene" => crate::engine_docs::document_scene(m),
        "skeleton" => crate::engine_docs::document_skeleton(m),
        "assets" => crate::engine_docs::document_assets(m),
        "log" => crate::engine_docs::document_log(m),
        "save" => crate::engine_docs::document_save(m),
        "strings" => crate::engine_docs::document_strings(m),
        "rng" => crate::engine_docs::document_rng(m),
        "fs" => crate::engine_docs::document_fs(m),
        "toml" => crate::engine_docs::document_toml(m),
        "json" => crate::engine_docs::document_json(m),
        "hash" => crate::engine_docs::document_hash(m),
        "encoding" => crate::engine_docs::document_encoding(m),
        _ => {}
    }
}

/// Which plugins this build loaded, in load order.
///
/// A game shipped without `http` can say so rather than call into a module
/// that is not there.
fn loaded_plugins(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::plugins::names(eng)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

fn plugin_version(eng: &Engine, args: &[Value]) -> Result<Value> {
    let name = text(args, 0)?;
    Ok(crate::plugins::loaded(eng)
        .into_iter()
        .find(|p| p.name == name)
        .map_or(Value::Nil, |p| Value::Str(p.version)))
}

fn has_plugin(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Bool(crate::plugins::is_loaded(eng, text(args, 0)?)))
}

fn time(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(eng.time()))
}

fn delta(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(f64::from(eng.delta())))
}

/// Which frame this is. What simulation code should branch on instead of
/// wall-clock: an exact integer, where `time` is an accumulated float.
///
/// An integer, so `tick() % 60` is a whole number against a whole number:
/// Rune never mixes the two, and a float here made every such test an error.
fn tick(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Int(i64::try_from(eng.tick()).unwrap_or(i64::MAX)))
}

fn quit(eng: &Engine, _: &[Value]) -> Result<Value> {
    eng.request_quit();
    Ok(Value::Nil)
}

fn args(eng: &Engine, _: &[Value]) -> Result<Value> {
    let list = eng
        .try_resource::<crate::app::ScriptArgs>()
        .map(|a| a.borrow().0.clone())
        .unwrap_or_default();
    Ok(Value::List(list.into_iter().map(Value::Str).collect()))
}

/// A writable per-user directory for saves and settings, created on first
/// call: `<platform data dir>/balaur/<project name>`, Application Support on
/// macOS and iOS, AppData on Windows, XDG data on Linux. Platforms with no
/// such notion (Android today) fall back to `user_data/` inside the project
/// root so a game always has somewhere to write. The project directory itself
/// is deliberately not the default: a shipped game may live somewhere
/// read-only.
fn user_data_dir(eng: &Engine, _: &[Value]) -> Result<Value> {
    let dir = user_data_dir_of(eng);
    crate::files::backend(eng).mkdir(&dir)?;
    Ok(Value::Str(dir.to_string_lossy().into_owned()))
}

/// The same directory, for a plugin that keeps a file there: input
/// rebindings, say. The script binding creates it; this only names it, so a
/// reader does not make a directory just by asking where one would be.
pub fn user_data_dir_of(eng: &Engine) -> std::path::PathBuf {
    let name = eng
        .try_resource::<crate::project::ProjectManifest>()
        .map(|m| m.borrow().name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "project".to_string());
    // A manifest name is free text; keep only what every filesystem accepts.
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = dirs::data_dir().map_or_else(
        || {
            eng.resource::<crate::project::ProjectRoot>()
                .borrow()
                .0
                .join("user_data")
        },
        |dir| dir.join("balaur"),
    );
    base.join(name)
}

fn reload_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    host.reload(text(args, 0)?)?;
    Ok(Value::Nil)
}

fn root(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Node(crate::node_id_of(eng.root()).0))
}

fn get_node(eng: &Engine, args: &[Value]) -> Result<Value> {
    let world = eng.world();
    Ok(scene::find_node(&world, eng.root(), text(args, 0)?)
        .map_or(Value::Nil, |e| Value::Node(crate::node_id_of(e).0)))
}

/// The node carrying a stable id: what a scene file declared, or what
/// `ids::mint` gave one a script spawned.
///
/// The second argument bounds the search, which is what a tool holding two
/// trees at once needs: the editor's own nodes carry ids too, and a game's
/// scene must not resolve against them.
fn node_by_id(eng: &Engine, args: &[Value]) -> Result<Value> {
    let under = optional_node(args, 1)?.unwrap_or_else(|| eng.root());
    let world = eng.world();
    Ok(crate::ids::find(&world, under, text(args, 0)?)
        .map_or(Value::Nil, |e| Value::Node(crate::node_id_of(e).0)))
}

/// Every node carrying a component, in tree order.
///
/// Asked of the registry's own `get` hook rather than of the ECS, so it
/// answers for a plugin's component the day it is registered, with no type
/// this crate would have to know.
fn with_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let name = text(args, 0)?;
    let world = eng.world();
    let mut out = Vec::new();
    let mut stack = vec![eng.root()];
    while let Some(at) = stack.pop() {
        if crate::components::get(eng, at, name).is_some() {
            out.push(Value::Node(crate::node_id_of(at).0));
        }
        if let Ok(children) = world.get::<&crate::scene::Children>(at) {
            // Pushed in reverse so the stack pops them in declaration order.
            for child in children.0.iter().rev() {
                stack.push(*child);
            }
        }
    }
    Ok(Value::List(out))
}

fn spawn(eng: &Engine, args: &[Value]) -> Result<Value> {
    let parent = optional_node(args, 1)?.unwrap_or_else(|| eng.root());
    let name = text(args, 0)?.to_string();
    let id = crate::ids::mint(eng);
    let entity = {
        let mut world = eng.world_mut();
        if id.is_empty() {
            scene::spawn_node(&mut world, &name, parent)
        } else {
            scene::spawn_node_with_id(&mut world, &name, parent, id)
        }
    };
    // A game that spawns is a game whose world changed, which is what a
    // session timeline is for. Scene loading does not come through here.
    crate::replay::event(
        eng,
        "scene.spawn",
        format!("spawned {name}"),
        Some(serde_json::json!({ "name": name })),
    );
    Ok(Value::Node(crate::node_id_of(entity).0))
}

fn instantiate(eng: &Engine, args: &[Value]) -> Result<Value> {
    let base = optional_node(args, 1)?.unwrap_or_else(|| eng.root());
    let attach = match args.get(2) {
        Some(Value::Map(pairs)) => !pairs
            .iter()
            .any(|(k, v)| k == "scripts" && matches!(v, Value::Bool(false))),
        _ => true,
    };
    crate::project::instantiate_scene(eng, text(args, 0)?, base, attach)?;
    Ok(Value::Nil)
}

/// The scene file's raw TOML text, or nil. Not a load: nothing is parsed or
/// spawned. Unlike `fs.read` it goes through the script host, so it finds the
/// file inside the pack in a packed run.
fn source(eng: &Engine, args: &[Value]) -> Result<Value> {
    let rel = text(args, 0)?;
    // The host's own copy first, which is what a packed game carries, then the
    // file, the way `project::scene_text` reads one: the editor's session
    // replay registers none of the game's scenes, and nil is not an answer.
    Ok(eng
        .script_host()
        .and_then(|host| host.scene_source(rel))
        .or_else(|| crate::project::scene_text(eng, rel).ok())
        .map_or(Value::Nil, Value::Str))
}

/// The names of every registered component TYPE, not the components on any
/// node. Pairs with `scene.component_schema(name)`.
fn component_types(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::components::names(eng)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

/// The facets a component belongs to, for filtering the palette.
fn component_tags(eng: &Engine, args: &[Value]) -> Result<Value> {
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let registry = registry.borrow();
    Ok(registry.def(text(args, 0)?).map_or(Value::Nil, |def| {
        Value::List(
            def.tags
                .iter()
                .map(|t| Value::Str((*t).to_string()))
                .collect(),
        )
    }))
}

/// What a component declares it needs something from, for a tool ordering or
/// grouping its sections. `unmet_expectations` answers the same question about
/// one node; this answers it about the type.
fn component_expects(eng: &Engine, args: &[Value]) -> Result<Value> {
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let registry = registry.borrow();
    Ok(registry.def(text(args, 0)?).map_or(Value::Nil, |def| {
        Value::List(
            def.expects
                .iter()
                .map(|t| Value::Str((*t).to_string()))
                .collect(),
        )
    }))
}

fn presets(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::presets::names(eng)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

/// A preset's description, tags and the components it adds.
fn preset_info(eng: &Engine, args: &[Value]) -> Result<Value> {
    let name = text(args, 0)?;
    let registry = eng.resource::<crate::presets::PresetRegistry>();
    let registry = registry.borrow();
    Ok(registry.0.get(name).map_or(Value::Nil, |def| {
        Value::Map(vec![
            (
                "description".to_string(),
                Value::Str(def.description.clone()),
            ),
            (
                "tags".to_string(),
                Value::List(def.tags.iter().cloned().map(Value::Str).collect()),
            ),
            (
                "components".to_string(),
                Value::List(
                    def.parts
                        .iter()
                        .map(|p| Value::Str(p.component.clone()))
                        .collect(),
                ),
            ),
        ])
    }))
}

fn apply_preset(eng: &Engine, args: &[Value]) -> Result<Value> {
    let entity = optional_node(args, 0)?.ok_or_else(|| anyhow!("apply_preset needs a node"))?;
    crate::presets::apply(eng, entity, text(args, 1)?)?;
    Ok(Value::Nil)
}

/// Components on this node whose expectations nothing satisfies, as a list of
/// `{ component, expects }`. Advisory: the editor warns, nothing blocks.
fn unmet_expectations(eng: &Engine, args: &[Value]) -> Result<Value> {
    let entity =
        optional_node(args, 0)?.ok_or_else(|| anyhow!("unmet_expectations needs a node"))?;
    Ok(Value::List(
        crate::presets::unmet_expectations(eng, entity)
            .into_iter()
            .map(|(component, expects)| {
                Value::Map(vec![
                    ("component".to_string(), Value::Str(component)),
                    (
                        "expects".to_string(),
                        Value::List(expects.into_iter().map(Value::Str).collect()),
                    ),
                ])
            })
            .collect(),
    ))
}

fn component_schema(eng: &Engine, args: &[Value]) -> Result<Value> {
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let registry = registry.borrow();
    registry.def(text(args, 0)?).map_or(Ok(Value::Nil), |def| {
        crate::node_api::from_toml(&def.schema)
    })
}

fn strings_tr(eng: &Engine, args: &[Value]) -> Result<Value> {
    let args_table = match args.get(1) {
        Some(Value::Map(fields)) => fields.clone(),
        _ => Vec::new(),
    };
    Ok(Value::Str(crate::strings::tr(
        eng,
        text(args, 0)?,
        &args_table,
    )))
}

fn strings_locale(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Str(crate::strings::locale(eng)))
}

fn strings_set_locale(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::strings::set_locale(eng, text(args, 0)?);
    Ok(Value::Nil)
}

fn strings_set_root(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::strings::set_root(eng, text(args, 0)?);
    Ok(Value::Nil)
}

fn strings_locales(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::strings::locales(eng)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

fn save_write(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::save::write(eng, text(args, 0)?, args.get(1).unwrap_or(&Value::Nil))?;
    Ok(Value::Nil)
}

fn save_read(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::save::read(eng, text(args, 0)?)
}

fn save_slots(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::save::slots(eng)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

fn save_remove(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::save::remove(eng, text(args, 0)?)?;
    Ok(Value::Nil)
}

fn save_version(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Int(i64::from(
        crate::save::SaveConfig::load(eng).version,
    )))
}

/// `engine.timings()`: what the last frame cost.
///
/// Presentation, like `engine.time()`: reading it from `fixed_update` would
/// branch the simulation on wall time, which no two machines agree about.
fn timings(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(crate::timings::table(eng))
}

/// `engine.profile_scripts(on)`: start or stop counting what each script
/// costs. Turning it on clears the tally.
fn profile_scripts(eng: &Engine, args: &[Value]) -> Result<Value> {
    let on = matches!(args.first(), Some(Value::Bool(true)));
    if let Some(host) = eng.script_host() {
        host.set_profiling(on);
    }
    Ok(Value::Nil)
}

/// `engine.script_costs()`: what each script has cost since profiling
/// started, dearest first.
///
/// Counted in instructions, not seconds: the same run executes the same
/// instructions on every machine, so a number that moved is a real change.
fn script_costs(eng: &Engine, _: &[Value]) -> Result<Value> {
    let rows = eng
        .script_host()
        .map(|h| h.script_costs())
        .unwrap_or_default();
    Ok(Value::List(
        rows.into_iter()
            .map(|(path, calls, instructions)| {
                Value::Map(vec![
                    ("path".to_string(), Value::Str(path)),
                    ("calls".to_string(), Value::Int(calls.cast_signed())),
                    (
                        "instructions".to_string(),
                        Value::Int(instructions.cast_signed()),
                    ),
                ])
            })
            .collect(),
    ))
}

/// The whole property table a scene key's value stands for.
///
/// A scene file may write a component as a shorthand (`body3d = "dynamic"`),
/// as a partial table, or in full, and all three mean the same component. A
/// tool comparing what two files said therefore cannot compare the text: it
/// has to compare what the engine would make of it, which is this.
fn component_properties(eng: &Engine, args: &[Value]) -> Result<Value> {
    let name = text(args, 0)?;
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let schema = match registry.borrow().def(name) {
        Some(def) => def.schema.clone(),
        None => return Ok(Value::Nil),
    };
    let params = match args.get(1) {
        None | Some(Value::Nil) => None,
        Some(value) => Some(crate::node_api::to_toml(value)?),
    };
    let full = crate::components::properties(eng, &schema, params.as_ref())?;
    crate::node_api::from_toml(&full)
}

/// An asset's definition table, from any of the three reference forms.
///
/// A script gets the data, not the engine's parsed object: a table is what a
/// script can read, edit and hand to `toml.encode`. The parsed side belongs to
/// the plugin that registered the type.
pub(crate) fn number(args: &[Value], i: usize) -> Result<f64> {
    match args.get(i) {
        Some(Value::Num(n)) => Ok(*n),
        Some(Value::Int(n)) => Ok(*n as f64),
        other => Err(anyhow!("argument {i} should be a number, got {other:?}")),
    }
}

pub(crate) fn integer(args: &[Value], i: usize) -> Result<i64> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Num(n)) => Ok(*n as i64),
        other => Err(anyhow!("argument {i} should be a number, got {other:?}")),
    }
}

pub(crate) fn text(args: &[Value], i: usize) -> Result<&str> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s),
        other => Err(anyhow!("argument {i} should be a string, got {other:?}")),
    }
}

pub(crate) fn optional_node(args: &[Value], i: usize) -> Result<Option<hecs::Entity>> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Node(id)) => Ok(Some(crate::entity_of(balaur_script::NodeId(*id))?)),
        other => Err(anyhow!("argument {i} should be a node, got {other:?}")),
    }
}
