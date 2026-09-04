//! `settings.*`: read, write and define settings by path.
//!
//! Two audiences. The editor lists everything, renders each setting from its
//! spec and writes the two files back. A game reads a setting it cares about,
//! changes one at run time, or defines its own — which is the same call the
//! engine's own plugins make, so a game's setting is not a lesser kind.

use balaur_script::{Bindings, BindingsExt, Value};

use crate::engine::Engine;
use crate::node_api::{from_toml, to_toml};
use crate::settings::{self, Scope, SettingDef};

fn scope_of(name: &str) -> Scope {
    if name == "editor" {
        Scope::Editor
    } else {
        Scope::Project
    }
}

const fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "project",
        Scope::Editor => "editor",
    }
}

pub fn install_settings_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Every setting the engine, its plugins and this game declare, \
         addressed by path: `physics/solver_iterations`, `netcode/faults`, \
         `editor/appearance/theme`. The first segment is the category, the \
         last is the key, and the path is also where the value is stored — \
         `physics/solver_iterations` is `[physics] solver_iterations` in \
         project.toml. A project setting ships with the game; an editor one \
         stays on the machine that set it. Define your own with `define` and \
         it appears in the settings screen beside the engine's.",
    );
    m.describe(&[
        ("all", &[], "()", "Every defined setting as `{ path, scope, spec }`, in definition order."),
        ("get", &[], "(path: string)", "One setting's value: what was set, else what its definition defaults to, else nil."),
        ("set", &[], "(path: string, value: any)", "Change one setting, in memory. Whether it takes effect now or on the next run is the setting's own business; `all` reports it as `applies`."),
        ("define", &[], "(path: string, spec: table)", "Declare a setting of your own: `type`, `default`, and optionally `min`, `max`, `options`, `help`, `order` and `applies`. A path starting `editor/` is kept on this machine; anything else ships with the game."),
        ("load", &[], "(text: string)", "Read values out of a TOML text, folding them onto what is already loaded."),
        ("to_toml", &[], "(scope: string, existing: string)", "The text one scope would write, starting from `existing` so anything no setting describes survives. Scope is \"project\" or \"editor\"."),
    ]);
    m.function("all", |eng: &Engine, (): ()| {
        let registry = settings::all(eng);
        let registry = registry.borrow();
        let out: Vec<Value> = registry
            .0
            .iter()
            .map(|def| {
                Value::Map(vec![
                    ("path".into(), Value::Str(def.path.clone())),
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
    m.function("get", |eng: &Engine, path: String| {
        Ok(settings::get(eng, &path)
            .and_then(|v| from_toml(&v).ok())
            .unwrap_or(Value::Nil))
    });
    m.function("set", |eng: &Engine, (path, value): (String, Value)| {
        settings::set(eng, &path, to_toml(&value)?);
        Ok(Value::Nil)
    });
    m.function("define", |eng: &Engine, (path, spec): (String, Value)| {
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
    m.function("load", |eng: &Engine, text: String| {
        settings::load(eng, &text)?;
        Ok(Value::Nil)
    });
    m.function(
        "to_toml",
        |eng: &Engine, (scope, existing): (String, String)| {
            Ok(Value::Str(settings::to_toml(
                eng,
                scope_of(&scope),
                &existing,
            )?))
        },
    );
}
