//! Every setting the engine, its plugins and a game declare, addressed by
//! path.
//!
//! A setting is named the way Godot names one: `physics/solver_iterations`,
//! `netcode/faults`, `editor/appearance/theme`. The first segment is the
//! category the editor groups under, the last is the key, and everything
//! between nests. That is the whole addressing scheme — there is no second
//! way to refer to a setting, and no registry of tables to keep in step with
//! a registry of names.
//!
//! The path is also the storage: `physics/solver_iterations` is
//! `[physics] solver_iterations`, and `editor/appearance/theme` is
//! `[editor.appearance] theme`. What you read in the file is what you write
//! in code.
//!
//! **Two scopes, and the difference matters.** A [`Scope::Project`] setting
//! is the game's: it lives in `project.toml`, ships with the build and
//! belongs in version control. A [`Scope::Editor`] setting is the person's:
//! it lives in the editor's own data directory and never touches the project,
//! so one developer turning on packet loss cannot ship that to anyone.
//!
//! **Anyone may define one.** A plugin declares its settings from `build`; a
//! game declares its own from a script with `settings.define`. Nothing
//! distinguishes them afterwards, which is what makes the screen a complete
//! list rather than a curated one.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{Context, Result};

use crate::engine::Engine;

/// Whose setting this is, and therefore where it is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The game's. Written to `project.toml`, shipped, version-controlled.
    Project,
    /// The person's. Written to the editor's data directory, never shipped.
    Editor,
}

/// One setting: where it lives, whose it is, and what it accepts.
#[derive(Clone)]
pub struct SettingDef {
    /// `physics/solver_iterations`. Never empty, and never starts or ends
    /// with a slash.
    pub path: String,
    pub scope: Scope,
    /// A component-style property spec: `type`, `default`, `min`, `max`,
    /// `options`, `help`, and `order` for where it sits on its page.
    pub spec: toml::Value,
}

impl SettingDef {
    /// The category the editor groups this under: the path's first segment.
    #[must_use]
    pub fn category(&self) -> &str {
        self.path.split('/').next().unwrap_or(&self.path)
    }

    /// What the row is labelled: everything after the category.
    #[must_use]
    pub fn label(&self) -> &str {
        self.path
            .split_once('/')
            .map_or(self.path.as_str(), |(_, rest)| rest)
    }

    /// Whether changing this takes effect straight away. A setting the engine
    /// only reads while starting says so, so the editor can too.
    #[must_use]
    pub fn applies_now(&self) -> bool {
        self.spec
            .get("applies")
            .and_then(toml::Value::as_str)
            .is_none_or(|when| when == "now")
    }
}

/// Every setting, in definition order.
#[derive(Default)]
pub struct SettingsRegistry(pub Vec<SettingDef>);

/// The values settings currently hold, as the nested tables they are stored
/// in.
#[derive(Default)]
pub struct SettingsValues(pub toml::value::Table);

/// Define one setting.
pub fn define(eng: &Engine, def: SettingDef) {
    if let Some(registry) = eng.try_resource::<SettingsRegistry>() {
        let mut registry = registry.borrow_mut();
        // Redefining replaces, so a game may override a default the engine
        // shipped without two rows appearing.
        if let Some(at) = registry.0.iter().position(|d| d.path == def.path) {
            registry.0[at] = def;
        } else {
            registry.0.push(def);
        }
    }
}

/// Define a group at once: every key in `schema` becomes `<prefix>/<key>`.
///
/// A key may itself contain slashes, so one block can declare
/// `appearance/theme` and `sessions/keep` under the same prefix.
pub fn define_group(eng: &Engine, prefix: &str, scope: Scope, schema: &toml::Value) {
    let Some(table) = schema.as_table() else {
        return;
    };
    for (key, spec) in table {
        define(
            eng,
            SettingDef {
                path: format!("{prefix}/{key}"),
                scope,
                spec: spec.clone(),
            },
        );
    }
}

/// Every defined setting, for the editor or a listing.
#[must_use]
pub fn all(eng: &Engine) -> Rc<RefCell<SettingsRegistry>> {
    eng.resource::<SettingsRegistry>()
}

/// One setting's definition.
#[must_use]
pub fn def(eng: &Engine, path: &str) -> Option<SettingDef> {
    let registry = eng.try_resource::<SettingsRegistry>()?;
    let found = registry.borrow().0.iter().find(|d| d.path == path).cloned();
    found
}

/// A path split into the tables it nests through and the key it ends at.
fn split(path: &str) -> Option<(Vec<&str>, &str)> {
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let key = parts.pop()?;
    Some((parts, key))
}

/// One setting's value: what was set, else what its definition defaults to.
#[must_use]
pub fn get(eng: &Engine, path: &str) -> Option<toml::Value> {
    if let Some(values) = eng.try_resource::<SettingsValues>() {
        if let Some((tables, key)) = split(path) {
            let values = values.borrow();
            let mut at: &toml::value::Table = &values.0;
            let mut reached = true;
            for table in tables {
                if let Some(next) = at.get(table).and_then(toml::Value::as_table) {
                    at = next;
                } else {
                    reached = false;
                    break;
                }
            }
            if reached {
                if let Some(found) = at.get(key) {
                    return Some(found.clone());
                }
            }
        }
    }
    def(eng, path).and_then(|d| d.spec.get("default").cloned())
}

/// Change one setting. Not written to disk until a caller asks for the text.
pub fn set(eng: &Engine, path: &str, value: toml::Value) {
    let Some(values) = eng.try_resource::<SettingsValues>() else {
        return;
    };
    let Some((tables, key)) = split(path) else {
        return;
    };
    let mut values = values.borrow_mut();
    let mut at: &mut toml::value::Table = &mut values.0;
    for table in tables {
        let entry = at
            .entry(table.to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if !entry.is_table() {
            *entry = toml::Value::Table(toml::value::Table::new());
        }
        // Just replaced with a table if it was not one.
        at = entry.as_table_mut().expect("made a table above");
    }
    at.insert(key.to_string(), value);
}

/// Read every value out of a manifest's text.
///
/// # Errors
/// When the text is not valid TOML.
pub fn load(eng: &Engine, text: &str) -> Result<()> {
    let parsed: toml::value::Table = toml::from_str(text).context("parsing settings")?;
    if let Some(values) = eng.try_resource::<SettingsValues>() {
        merge(&mut values.borrow_mut().0, parsed);
    }
    Ok(())
}

/// Fold one table into another, table by table rather than wholesale, so
/// loading the editor's file after the project's does not drop the project's.
fn merge(into: &mut toml::value::Table, from: toml::value::Table) {
    for (key, value) in from {
        match (into.get_mut(&key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(incoming)) => {
                merge(existing, incoming);
            }
            (_, value) => {
                into.insert(key, value);
            }
        }
    }
}

/// The text one scope's settings would write, starting from `existing` so
/// anything no setting describes survives.
///
/// Only the paths that scope defines are touched, which is what lets a
/// manifest keep its comments, its ordering and its unrelated tables.
///
/// # Errors
/// When `existing` is not valid TOML, or the result cannot be written.
pub fn to_toml(eng: &Engine, scope: Scope, existing: &str) -> Result<String> {
    let mut doc: toml::value::Table = if existing.trim().is_empty() {
        toml::value::Table::new()
    } else {
        toml::from_str(existing).context("parsing the file being written")?
    };
    let Some(registry) = eng.try_resource::<SettingsRegistry>() else {
        return toml::to_string_pretty(&doc).context("writing settings");
    };
    let paths: Vec<String> = registry
        .borrow()
        .0
        .iter()
        .filter(|d| d.scope == scope)
        .map(|d| d.path.clone())
        .collect();
    for path in paths {
        let Some(value) = get(eng, &path) else {
            continue;
        };
        let Some((tables, key)) = split(&path) else {
            continue;
        };
        let mut at: &mut toml::value::Table = &mut doc;
        for table in tables {
            let entry = at
                .entry(table.to_string())
                .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::value::Table::new());
            }
            at = entry.as_table_mut().expect("made a table above");
        }
        at.insert(key.to_string(), value);
    }
    toml::to_string_pretty(&doc).context("writing settings")
}

/// Core's own settings. Plugins define theirs from their own `build`.
pub(crate) fn build_core_settings(eng: &Engine) {
    let parse = |name: &str, text: &str| crate::components::ComponentDef::parse_schema(name, text);
    define_group(
        eng,
        "application",
        Scope::Project,
        &parse(
            "settings.application",
            r#"
name = { type = "string", default = "", order = 1, help = "The game's name, used for its window title and its data directory." }
main_scene = { type = "string", default = "", order = 2, help = "The scene a run opens with." }
language = { type = "enum", default = "rune", options = ["rune"], order = 3, applies = "restart", help = "Which scripting language this project is written in." }
"#,
        ),
    );
    define_group(
        eng,
        "save",
        Scope::Project,
        &parse(
            "settings.save",
            r#"
version = { type = "int", default = 1, min = 1, max = 9999, help = "The save version this build writes. A lower file is migrated; a higher one is refused." }
migrate = { type = "string", default = "", help = "A script whose migrate_save(version, data) brings a file forward one version per call." }
"#,
        ),
    );
    define_group(
        eng,
        "locale",
        Scope::Project,
        &parse(
            "settings.locale",
            r#"
default = { type = "string", default = "en", help = "The locale a fresh run starts in." }
fallback = { type = "string", default = "en", help = "Where a key missing from the current locale is looked for next." }
"#,
        ),
    );
    define_group(
        eng,
        "netcode",
        Scope::Editor,
        &parse(
            "settings.netcode",
            r#"
faults = { type = "bool", default = false, order = 1, help = "Put delay, jitter and packet loss on every session link, to test rollback against a link that misbehaves." }
delay = { type = "int", default = 9, min = 0, max = 60, order = 2, help = "Ticks every payload waits before delivery. Nine is about 150 ms at 60 Hz." }
jitter = { type = "int", default = 3, min = 0, max = 30, order = 3, help = "Extra ticks drawn per payload. Jitter is what reorders a stream." }
loss = { type = "float", default = 0.05, min = 0.0, max = 1.0, order = 4, help = "The fraction of datagrams dropped. Datagrams only: losing a reliable payload would break the transport's contract." }
"#,
        ),
    );
    // A prefix may nest, so a subsystem with many settings declares them a
    // group at a time and the editor shows each group under its own heading.
    define_group(
        eng,
        "editor/appearance",
        Scope::Editor,
        &parse(
            "settings.editor.appearance",
            r#"
theme = { type = "enum", default = "dark", options = ["dark", "light"], order = 1, help = "Which chrome the editor wears." }
ui_scale = { type = "float", default = 1.25, min = 0.75, max = 2.5, order = 2, help = "How large the editor's own text and controls are drawn." }
compact = { type = "bool", default = false, order = 3, help = "Drop labels the icon already says, for a narrow window." }
"#,
        ),
    );
    define_group(
        eng,
        "editor/sessions",
        Scope::Editor,
        &parse(
            "settings.editor.sessions",
            r#"
keep = { type = "int", default = 10, min = 1, max = 200, order = 10, help = "How many recorded play sessions are kept per game before the oldest is pruned." }
verify = { type = "bool", default = false, order = 11, help = "Hash the world every tick while recording, so a replay can say where it parted. Costs a walk of every node per frame." }
"#,
        ),
    );
}

/// The faults the `netcode` settings ask for, or `None` when they are off.
#[must_use]
pub fn faults(eng: &Engine) -> Option<crate::transport::Faults> {
    if !get(eng, "netcode/faults")?.as_bool()? {
        return None;
    }
    // `as_f64`, not `as_float`: a tick count is an integer in the file and in
    // anything a hand edit writes.
    let number = |path: &str| get(eng, path).and_then(|v| crate::components::as_f64(&v));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a tick count from a bounded setting"
    )]
    Some(crate::transport::Faults {
        delay: number("netcode/delay").unwrap_or(0.0) as u32,
        jitter: number("netcode/jitter").unwrap_or(0.0) as u32,
        loss: number("netcode/loss").unwrap_or(0.0) as f32,
    })
}
