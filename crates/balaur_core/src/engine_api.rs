//! The engine-level script modules, declared once for every language.
//!
//! `engine` is the clock, argv and quit; `scene` is the tree, spawning and
//! instancing. Same shape as `node_api`: a list of function pointers a backend
//! registers, so a new language inherits them.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::{anyhow, Result};
use balaur_script::{Bindings as _, Value};

use crate::engine::Engine;
use crate::file_api::{
    fs_exists, fs_list, fs_mkdir, fs_mtime, fs_read, fs_remove, fs_rename, fs_write, json_encode,
    json_parse, toml_encode, toml_parse,
};
use crate::rng::Pcg32;
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
        module: "engine",
        name: "timings",
        call: timings,
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
        name: "mtime",
        call: fs_mtime,
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
/// Takes `&Engine` rather than a `Bindings` — unlike every other
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
    Ok(())
}

/// The reference text for one module, added as that module is created.
///
/// One table spans nine modules, so this cannot sit beside a single
/// `install_*` the way every other subsystem's documentation does.
fn document(module: &str, m: &mut dyn balaur_script::Bindings<Engine>) {
    match module {
        "engine" => document_engine(m),
        "scene" => document_scene(m),
        "skeleton" => document_skeleton(m),
        "assets" => document_assets(m),
        "log" => document_log(m),
        "save" => document_save(m),
        "strings" => document_strings(m),
        "rng" => document_rng(m),
        "fs" => document_fs(m),
        "toml" => document_toml(m),
        "json" => document_json(m),
        _ => {}
    }
}

fn document_engine(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The running app itself: the clock a frame reads, the command line it \
         was started with, the directory it may write to, and the way out.",
    );
    m.describe(&[
        ("time", &[], "()", "Seconds of engine time since the app started, accumulated as a float."),
        ("timings", &[], "()", "What the last frame cost, in seconds: `{ frame, fixed_steps, stages, spans }`. Presentation only — branching a `fixed_update` on wall time desyncs, and nothing records it."),
        ("delta", &[], "()", "Seconds the frame in progress covers, the same number a system is handed."),
        ("tick", &[], "()", "Which frame this is, counted whole — what simulation code branches on instead of `time`."),
        ("quit", &[], "()", "Ask the app to shut down; the frame in flight still finishes."),
        ("args", &[], "()", "The command-line arguments the app was started with, empty when it was given none."),
        ("reload_script", &[], "(key: string)", "Recompile one script by its project-relative key, for a tool editing files outside the watched root."),
        ("user_data_dir", &[], "()", "A writable per-user directory for saves and settings, created on first call and named after the project."),
    ]);
}

fn document_scene(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The node tree: its root, lookup by path, spawning and instancing. \
         Also the component and preset vocabulary an editor builds its \
         palette from.",
    );
    m.describe(&[
        ("root", &[], "()", "The tree's root node."),
        ("get_node", &[], "(path: string)", "The node at an `A/B/C` path from the root, where `..` climbs to the parent; nil when nothing matches."),
        ("spawn", &[], "(name: string, parent: node?)", "Create one empty named node under the given parent, or under the root when none is given."),
        ("instantiate", &[], "(source: string, parent: node?, opts: any?)", "Build a scene document — TOML text, not a path — under a parent; `{ scripts: false }` leaves scripts unattached."),
        ("source", &[], "(path: string)", "A scene file's raw TOML text, project-relative and found inside the pack in a packed run; nil when missing."),
        ("component_types", &[], "()", "The names of every registered component type, not the components on any node."),
        ("component_tags", &[], "(name: string)", "The facets a component type is filed under, for filtering a palette; nil for a name nothing registered."),
        ("component_schema", &[], "(name: string)", "A component type's property schema as a table; nil for a name nothing registered."),
        ("component_properties", &[], "(name: string, params: any)", "What a component's `apply` would receive for `params`: the schema's defaults with a shorthand or a partial table merged over them. This is how a tool compares two spellings of the same component."),
        ("presets", &[], "()", "The names of every registered preset."),
        ("preset_info", &[], "(name: string)", "A preset's description, tags and the components it adds; nil for a name nothing registered."),
        ("apply_preset", &[], "(node: node, name: string)", "Add every component a preset names to the node; a part that fails leaves the parts before it in place."),
        ("unmet_expectations", &[], "(node: node)", "Components on the node whose expectations nothing satisfies, as `{ component, expects }`; advisory only."),
    ]);
}

fn document_skeleton(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Bones under a rig node: the rest pose a rig returns to, and the tree \
         order a skin numbers its joints in. A bone is any node carrying \
         `bone2d` or `bone3d`; there is no skeleton component.",
    );
    m.describe(&[
        ("apply_rest", &["bone2d", "bone3d"], "(node: node)", "Move every bone under the node back to its rest transform."),
        ("overwrite_rest", &["bone2d", "bone3d"], "(node: node)", "Record every bone's current transform under the node as its new rest pose."),
        ("bones", &["bone2d", "bone3d"], "(node: node)", "The bones under the node in tree order, the order a skin numbers them in, the node itself first when it is one."),
    ]);
}

fn document_assets(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Asset definitions by reference: a project-relative file path, \
         `file#entry` for one entry inside it, or `#id` for a block the scene \
         declares. A script gets the definition table, not the parsed object \
         the owning plugin builds from it.",
    );
    m.describe(&[
        ("load", &[], "(reference: string)", "The definition table behind a reference, from the cache; an error when the reference resolves to nothing."),
        ("duplicate", &[], "(reference: string)", "A private copy of a definition, read past the cache, so editing it disturbs no other holder of that reference."),
        ("exists", &[], "(reference: string)", "Whether a reference resolves to a definition that is really there; false rather than an error when it does not."),
        ("reload", &[], "(reference: string)", "Forget a reference so the next load re-reads its file, along with every entry cut from that same file."),
        ("invalidate", &[], "()", "Declare everything derived from project files stale — a shader a material links, say — so it is rebuilt from disk; for a file the watcher does not cover."),
        ("save", &[], "(reference: string, definition: any)", "Write a definition table to the project-relative file a reference names; an error unless it names a whole file."),
        ("directory", &[], "(type_name: string)", "The project-relative directory files of an asset type belong in; empty when the type is unknown or declared none."),
    ]);
}

fn document_strings(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Localization: one `strings/<locale>.toml` per language, keys to \
         strings. `[locale]` in `project.toml` sets the locale a run starts \
         in and the one a missing key falls back to. A key neither has comes \
         back as itself — visible in the game, which is how a missing string \
         gets noticed rather than showing as a blank label.",
    );
    m.describe(&[
        ("tr", &[], "(key: string, args: table?)", "The string for a key in the current locale. `{name}` in it is replaced by the argument called `name`, and an `n` argument also picks the plural form the locale's language calls for."),
        ("locale", &[], "()", "The locale in force."),
        ("set_locale", &[], "(locale: string)", "Switch locale; the next `tr` answers in it, which for a widget showing a key is the next frame."),
        ("locales", &[], "()", "Every locale the project ships a `strings/<locale>.toml` for, in name order."),
        ("set_root", &[], "(root: string)", "Read the catalogues from this directory instead of the project root, forgetting the ones already read; an empty string puts it back. For a host running a project other than its own — the editor, whose own root has no `strings/`, so without this every `text_key` in a played scene draws as its key."),
    ]);
}

fn document_save(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Save games: a table in, a table out, stored per user rather than in \
         the project. Nothing here is engine state — a save is whatever the \
         game puts in it — so what the engine decides is only where it lives, \
         that a half-written file cannot replace a good one, and what version \
         it was written at. `[save] version` in `project.toml` sets that \
         version and `[save] migrate` names the script whose \
         `migrate_save(version, data)` brings an older file forward, one \
         version per call.",
    );
    m.describe(&[
        ("write", &[], "(slot: string, data: any)", "Write a table to a named slot, stamped with the project's save version. Written beside the target and renamed over it, so a crash mid-save cannot destroy the last one."),
        ("read", &[], "(slot: string)", "The table in a slot, brought forward to this build's version; nil when the slot was never written. An error when the file was written by a newer build, or when it needs a migration the project declares no script for."),
        ("slots", &[], "()", "Every slot that has been written, in name order."),
        ("remove", &[], "(slot: string)", "Delete a slot. Not an error when it was not there."),
        ("version", &[], "()", "The save version this build writes, from `[save] version`."),
    ]);
}

fn document_log(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The three levels a script writes at, and the buffer behind them. \
         Scripted lines go through the engine's own `tracing` stream, so they \
         land beside engine ones.",
    );
    m.describe(&[
        ("info", &[], "(message: string)", "Write a line at info level, tagged as coming from a script."),
        ("warn", &[], "(message: string)", "Write a line at warning level, tagged as coming from a script."),
        ("error", &[], "(message: string)", "Write a line at error level, tagged as coming from a script."),
        ("recent", &[], "(n: int?)", "The last n buffered entries, 100 by default, each `{ time, level, tag, message, fields }`."),
        ("clear", &[], "()", "Empty the buffer, so a console reading it starts again from nothing."),
    ]);
}

fn document_rng(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The engine's one deterministic PCG32 stream: the same seed draws the \
         same numbers on every platform, and a replay reproduces every draw a \
         recorded session made.",
    );
    m.describe(&[
        ("seed", &[], "(seed: int)", "Restart the deterministic engine stream at the given seed, so every draw after it repeats."),
        ("random", &[], "()", "A float from the deterministic engine stream, uniform in `[0, 1)`."),
        ("range", &[], "(low: float, high: float)", "A float from the deterministic engine stream, uniform in `[low, high)` — the two arguments."),
        ("int", &[], "(low: int, high: int)", "A whole number from the deterministic engine stream, uniform in `[low, high]`, both ends included."),
    ]);
}

fn document_fs(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Files on disk, project-relative unless the path is absolute, so a \
         script cannot wander the filesystem by accident. This is the disk \
         itself: a packed build's contents are reached through `assets` and \
         `scene.source`.",
    );
    m.describe(&[
        ("read", &[], "(path: string)", "A whole file as text, project-relative unless absolute; nil when it cannot be read."),
        ("write", &[], "(path: string, text: string)", "Write text to a project-relative file, creating the directory it goes in."),
        ("exists", &[], "(path: string)", "Whether a project-relative path has anything at it, file or directory."),
        ("list", &[], "(path: string)", "A directory's entries as `{ name, is_dir }`, sorted, dotfiles skipped; empty for a directory that is not there."),
        ("remove", &[], "(path: string)", "Delete a project-relative file, or a directory and everything under it; false when there was nothing there."),
        ("mkdir", &[], "(path: string)", "Create a project-relative directory and every parent it needs."),
        ("rename", &[], "(from: string, to: string)", "Move a project-relative file or directory, creating the destination's parent first."),
        ("mtime", &[], "(path: string)", "When a file last changed, in seconds since the Unix epoch; nil for one that is not there."),
    ]);
}

fn document_toml(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "TOML text to and from script tables: the format scene files, asset \
         definitions and component properties are all written in.",
    );
    m.describe(&[
        ("parse", &[], "(text: string)", "The table a TOML document describes; an error on text that does not parse."),
        ("encode", &[], "(value: any)", "A table written back out as TOML text; a node or callback in it is not data and is an error."),
    ]);
}

fn document_json(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "JSON text to and from script values, for talking to anything outside \
         the engine. Unlike TOML it has null, so nil survives a round trip.",
    );
    m.describe(&[
        ("parse", &[], "(text: string)", "The value a JSON document describes; an error on text that does not parse."),
        ("encode", &[], "(value: any)", "A value written back out as JSON text; NaN, infinity, a node or a callback has no JSON form and is an error."),
    ]);
}

fn time(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(eng.time()))
}

fn delta(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(f64::from(eng.delta())))
}

/// Which frame this is. What simulation code should branch on instead of
/// wall-clock: an exact integer, where `time` is an accumulated float.
fn tick(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(eng.tick() as f64))
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
/// call: `<platform data dir>/balaur/<project name>` — Application Support on
/// macOS and iOS, AppData on Windows, XDG data on Linux. Platforms with no
/// such notion (Android today) fall back to `user_data/` inside the project
/// root so a game always has somewhere to write. The project directory itself
/// is deliberately not the default: a shipped game may live somewhere
/// read-only.
fn user_data_dir(eng: &Engine, _: &[Value]) -> Result<Value> {
    let dir = user_data_dir_of(eng);
    std::fs::create_dir_all(&dir)?;
    Ok(Value::Str(dir.to_string_lossy().into_owned()))
}

/// The same directory, for a plugin that keeps a file there — input
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

fn spawn(eng: &Engine, args: &[Value]) -> Result<Value> {
    let parent = optional_node(args, 1)?.unwrap_or_else(|| eng.root());
    let name = text(args, 0)?.to_string();
    let entity = {
        let mut world = eng.world_mut();
        let entity = scene::spawn_node(&mut world, &name, parent);
        crate::ids::assign(eng, &mut world, entity);
        entity
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
    Ok(eng
        .script_host()
        .and_then(|host| host.scene_source(rel))
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

/// `engine.timings()` — what the last frame cost.
///
/// Presentation, like `engine.time()`: reading it from `fixed_update` would
/// branch the simulation on wall time, which no two machines agree about.
fn timings(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(crate::timings::table(eng))
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
fn assets_load(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::assets::definition(eng, text(args, 0)?)?;
    crate::node_api::from_toml(&definition)
}

/// A private copy: read past the cache, so editing it cannot disturb what
/// every other holder of that reference sees.
fn assets_duplicate(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::assets::duplicate_definition(eng, text(args, 0)?)?;
    crate::node_api::from_toml(&definition)
}

fn assets_exists(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Bool(crate::assets::exists(eng, text(args, 0)?)))
}

/// Forget a reference, so the next load re-reads its source. What the editor
/// calls after writing an asset file.
fn assets_reload(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::assets::reload(eng, text(args, 0)?)?;
    Ok(Value::Nil)
}

fn assets_invalidate(eng: &Engine, _args: &[Value]) -> Result<Value> {
    crate::assets::invalidate(eng);
    Ok(Value::Nil)
}

/// Write a definition table back to the file a reference names, and forget the
/// cached copy so the next load reads what was written.
fn assets_save(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::node_api::to_toml(
        args.get(1)
            .ok_or_else(|| anyhow!("assets.save needs the table to write"))?,
    )?;
    crate::assets::save(eng, text(args, 0)?, &definition)?;
    Ok(Value::Nil)
}

/// Where files of an asset type belong, as its plugin declared it.
///
/// The editor promotes an inline definition to a file and has to put it
/// somewhere; only the type knows where. Empty when the type is unknown or
/// declared no directory, which a caller reads as "cannot promote".
fn assets_directory(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Str(crate::assets::directory(eng, text(args, 0)?)))
}

/// The three writers a script has. They emit through `tracing`, so a scripted
/// line lands in the same stream, and the same `logbuf`, as an engine one --
/// which is what makes `log.recent` able to show both.
fn log_info(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::info!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

fn log_warn(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::warn!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

fn log_error(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::error!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

fn log_recent(_: &Engine, args: &[Value]) -> Result<Value> {
    let n = match args.first() {
        Some(Value::Int(n)) => usize::try_from(*n).unwrap_or(100),
        Some(Value::Num(n)) => *n as usize,
        _ => 100,
    };
    Ok(Value::List(
        crate::logbuf::recent(n)
            .into_iter()
            .map(|e| {
                // The structured fields ride along: a viewer that drops them
                // shows a message the event deliberately did not put there.
                let fields = e
                    .fields
                    .iter()
                    .map(|(name, value)| {
                        Value::Map(vec![
                            ("name".into(), Value::Str(name.clone())),
                            ("value".into(), Value::Str(value.clone())),
                        ])
                    })
                    .collect();
                Value::Map(vec![
                    ("time".into(), Value::Num(e.time)),
                    ("level".into(), Value::Str(e.level.clone())),
                    ("tag".into(), Value::Str(e.tag.clone())),
                    ("message".into(), Value::Str(e.message.clone())),
                    ("fields".into(), Value::List(fields)),
                ])
            })
            .collect(),
    ))
}

fn log_clear(_: &Engine, _: &[Value]) -> Result<Value> {
    crate::logbuf::clear();
    Ok(Value::Nil)
}

fn rng_seed(eng: &Engine, args: &[Value]) -> Result<Value> {
    let seed = match args.first() {
        Some(Value::Int(n)) => *n,
        Some(Value::Num(n)) => *n as i64,
        other => return Err(anyhow!("seed should be a number, got {other:?}")),
    };
    crate::rng::with_rng(eng, |rng| *rng = Pcg32::new(seed as u64));
    Ok(Value::Nil)
}

fn rng_random(eng: &Engine, _: &[Value]) -> Result<Value> {
    let v = crate::rng::with_rng(eng, Pcg32::next_f64);
    Ok(Value::Num(v))
}

fn rng_range(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (number(args, 0)?, number(args, 1)?);
    let v = crate::rng::with_rng(eng, Pcg32::next_f64);
    Ok(Value::Num(v.mul_add(hi - lo, lo)))
}

fn rng_int(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (integer(args, 0)?, integer(args, 1)?);
    let v = crate::rng::with_rng(eng, |rng| rng.next_range_i64(lo, hi));
    Ok(Value::Int(v))
}

/// Project-relative unless absolute, so a script cannot wander the disk by
/// accident.

pub(crate) fn number(args: &[Value], i: usize) -> Result<f64> {
    match args.get(i) {
        Some(Value::Num(n)) => Ok(*n),
        Some(Value::Int(n)) => Ok(*n as f64),
        other => Err(anyhow!("argument {i} should be a number, got {other:?}")),
    }
}

fn integer(args: &[Value], i: usize) -> Result<i64> {
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
