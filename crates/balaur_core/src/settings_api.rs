//! `settings.*`: read, write and define settings by path.
//!
//! Two audiences. The editor lists everything, renders each setting from its
//! spec and writes the two files back. A game reads a setting it cares about,
//! changes one at run time, or defines its own: which is the same call the
//! engine's own plugins make, so a game's setting is not a lesser kind.

use balaur_script::{Module, Value};

use crate::engine::Engine;
use crate::node_api::{from_toml, to_toml};
use crate::settings::{self, Scope, SettingDef};
use crate::vocabulary::keys;
use crate::vocabulary::words;

const ALL: &str = "all";
const BASE: &str = "base";
const DEFINE: &str = "define";
const GET: &str = "get";
const KNOWN_TAGS: &str = "known_tags";
const MERGE_TOML: &str = "merge_toml";
const REVISION: &str = "revision";
const SET: &str = "set";

fn scope_of(name: &str) -> Scope {
    if name == words::EDITOR {
        Scope::Editor
    } else {
        Scope::Project
    }
}

const fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => words::dependencies::PROJECT,
        Scope::Editor => words::EDITOR,
    }
}

pub fn install_settings_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Every setting the engine, its plugins and the game declare, by path: `physics/solver_iterations` is `[physics] solver_iterations` in `project.toml`. `define` adds your own; `[override.<platform>]` tables answer per platform.",
    );
    m.records(&[(
        "Setting",
        "{ path: string, category: string, label: string, scope: string, applies_now: bool, spec: map }",
        "A defined setting: where it lives, how a settings screen files and labels it, which file it is written to and its spec.",
    )]);
    m.describe(&[
        (ALL, &[], "() -> [Setting]", "Every defined setting as `{ path, scope, spec }`, in definition order."),
        (GET, &[], "(path: string) -> any", "One setting's value here: the narrowest `[override.<tag>]` this platform answers to, else what was set, else what its definition defaults to, else nil."),
        (BASE, &[], "(path: string) -> any", "What the file says, ignoring every override: what a settings screen edits."),
        (words::settings_api::TAGS, &[], "() -> [string]", "The names this run answers to, broad to narrow: the kind of machine, the operating system, the architecture, the build."),
        (KNOWN_TAGS, &[], "() -> [string]", "Every tag a project may write an override for: the engine's, then the project's own from `[export] tags`."),
        (keys::OVERRIDES, &[], "(path: string) -> [string]", "The tags this project holds an override for at `path`, in tag order."),
        (words::functions::CLEAR, &[], "(path: string) -> ()", "Forget one value, so the next write drops the key: how an override is removed."),
        (REVISION, &[], "() -> int", "A count that moves whenever a setting is defined, set, cleared or loaded: a screen listing settings rebuilds its rows when it moves, not every frame."),
        (SET, &[], "(path: string, value: any) -> ()", "Change one setting, in memory. Whether it takes effect now or on the next run is the setting's own business; `all` reports it as `applies`."),
        (DEFINE, &[], "(path: string, spec: table)", "Declare a setting of your own: `type`, `default`, and optionally `min`, `max`, `options`, `help`, `order` and `applies`. A path starting `editor/` is kept on this machine; anything else ships with the game."),
        (MERGE_TOML, &[], "(text: string) -> ()", "Read values out of a TOML text, folding them onto what is already loaded."),
        (words::functions::TO_TOML, &[], "(scope: string, existing: string) -> string", "The text one scope would write, starting from `existing` so anything no setting describes survives. Scope is \"project\" or \"editor\"."),
    ]);
    m.function(ALL, |eng: &Engine, (): ()| {
        let registry = settings::all(eng);
        let registry = registry.borrow();
        let out: Vec<Value> = registry
            .0
            .iter()
            .map(|def| {
                Value::Map(vec![
                    (keys::PATH.into(), Value::Str(def.path.clone())),
                    ("category".into(), Value::Str(def.category().to_string())),
                    ("label".into(), Value::Str(def.label().to_string())),
                    ("scope".into(), Value::Str(scope_name(def.scope).into())),
                    ("applies_now".into(), Value::Bool(def.applies_now())),
                    ("spec".into(), from_toml(&def.spec).unwrap_or(Value::Nil)),
                ])
            })
            .collect();
        Ok(Value::List(out))
    });
    m.function(GET, |eng: &Engine, path: String| {
        Ok(settings::get(eng, &path)
            .and_then(|v| from_toml(&v).ok())
            .unwrap_or(Value::Nil))
    });
    install_override_api(m);
    m.function(SET, |eng: &Engine, (path, value): (String, Value)| {
        settings::set(eng, &path, to_toml(&value)?);
        notice_change(eng, path, value);
        Ok(Value::Nil)
    });
    m.function(DEFINE, |eng: &Engine, (path, spec): (String, Value)| {
        // A path's own first segment decides the scope, so a game cannot
        // define something under `editor/` and have it ship by accident.
        let scope = scope_of(path.split('/').next().unwrap_or(""));
        settings::define(
            eng,
            SettingDef {
                path,
                scope,
                spec: to_toml(&spec)?,
            },
        );
        Ok(Value::Nil)
    });
    m.function(MERGE_TOML, |eng: &Engine, text: String| {
        settings::load(eng, &text)?;
        Ok(Value::Nil)
    });
    m.function(
        words::functions::TO_TOML,
        |eng: &Engine, (scope, existing): (String, String)| {
            Ok(Value::Str(settings::to_toml(
                eng,
                scope_of(&scope),
                &existing,
            )?))
        },
    );
}

/// Tell every script that a setting changed, as `on_setting_changed`.
fn notice_change(eng: &Engine, path: String, value: Value) {
    let change = Value::Map(vec![
        (keys::PATH.into(), Value::text(path)),
        (keys::VALUE.into(), value),
    ]);
    crate::facts::notice(eng, crate::hooks::ON_SETTING_CHANGED, change);
}

/// The calls the settings screen needs for a key with more than one answer:
/// the file's own value, the tags in play, and the overrides a key carries.
fn install_override_api(m: &mut Module<Engine>) {
    m.function(BASE, |eng: &Engine, path: String| {
        Ok(settings::base(eng, &path)
            .and_then(|v| from_toml(&v).ok())
            .unwrap_or(Value::Nil))
    });
    m.function(words::settings_api::TAGS, |eng: &Engine, (): ()| {
        let tags = eng.resource::<crate::tags::Tags>();
        let tags = tags.borrow();
        Ok(Value::List(
            tags.0.iter().cloned().map(Value::text).collect(),
        ))
    });
    m.function(KNOWN_TAGS, |eng: &Engine, (): ()| {
        Ok(Value::List(
            settings::known_tags(eng)
                .into_iter()
                .map(Value::text)
                .collect(),
        ))
    });
    m.function(keys::OVERRIDES, |eng: &Engine, path: String| {
        Ok(Value::List(
            settings::overrides(eng, &path)
                .into_iter()
                .map(Value::text)
                .collect(),
        ))
    });
    m.function(words::functions::CLEAR, |eng: &Engine, path: String| {
        settings::clear(eng, &path);
        Ok(Value::Nil)
    });
    m.function(REVISION, |eng: &Engine, (): ()| {
        Ok(Value::Int(
            i64::try_from(settings::revision(eng)).unwrap_or(i64::MAX),
        ))
    });
}
