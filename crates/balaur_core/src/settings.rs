//! Every setting the engine and its plugins have, in one place.
//!
//! Settings were scattered before this: some in `project.toml` tables that
//! only their own crate knew about, some in an editor flag that vanished on
//! restart, and some — fault injection, most obviously — reachable from a
//! Rust test and nowhere else. A game author had no list of what was
//! adjustable, and no plugin could add to one.
//!
//! A page is declared with the same schema a component uses, so a setting is
//! described exactly the way a component property is: a type, a default, a
//! range or a set of options, and a line of help. The editor renders both
//! from the same code, and a plugin adds a page the way it adds a component.
//!
//! **Two scopes, and the difference matters.** A [`Scope::Project`] setting
//! is the game's: it lives in `project.toml`, ships with the build and
//! belongs in version control, so changing it changes what every player gets.
//! A [`Scope::Editor`] setting is the person's: it lives in the editor's own
//! data directory and never touches the project, so one developer turning on
//! packet loss cannot ship that to anyone. Putting a debug toggle in the
//! manifest would be a bug, not a convenience.

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

/// One page of settings, as the editor shows it.
pub struct SettingsPage {
    /// What the page is called, and the order it sorts under.
    pub category: String,
    /// The `project.toml` table these live in, or the prefs key for an editor
    /// page. `""` puts them at the manifest's top level.
    pub table: String,
    pub scope: Scope,
    /// A component-style schema: one entry per setting, with its type,
    /// default and help.
    pub schema: toml::Value,
}

/// Every page, in registration order.
#[derive(Default)]
pub struct SettingsRegistry(pub Vec<SettingsPage>);

/// The values a page's settings currently hold, by table then key.
///
/// Kept apart from the registry because the registry is what a plugin
/// declares once and this is what changes.
#[derive(Default)]
pub struct SettingsValues(pub toml::value::Table);

/// Declare a page. Called from a plugin's `declare`, or by core for its own.
pub fn register(eng: &Engine, page: SettingsPage) {
    if let Some(registry) = eng.try_resource::<SettingsRegistry>() {
        registry.borrow_mut().0.push(page);
    }
}

/// Every declared page, for an editor or a `--settings` listing.
#[must_use]
pub fn pages(eng: &Engine) -> Rc<RefCell<SettingsRegistry>> {
    eng.resource::<SettingsRegistry>()
}

/// One setting's current value: what was set, else what the schema defaults
/// to, else nil.
#[must_use]
pub fn get(eng: &Engine, table: &str, key: &str) -> Option<toml::Value> {
    if let Some(values) = eng.try_resource::<SettingsValues>() {
        let values = values.borrow();
        let found = if table.is_empty() {
            values.0.get(key).cloned()
        } else {
            values
                .0
                .get(table)
                .and_then(|t| t.as_table())
                .and_then(|t| t.get(key))
                .cloned()
        };
        if found.is_some() {
            return found;
        }
    }
    default_of(eng, table, key)
}

/// Change one setting. The value is not written to disk until [`save`].
pub fn set(eng: &Engine, table: &str, key: &str, value: toml::Value) {
    let Some(values) = eng.try_resource::<SettingsValues>() else {
        return;
    };
    let mut values = values.borrow_mut();
    if table.is_empty() {
        values.0.insert(key.to_string(), value);
        return;
    }
    let entry = values
        .0
        .entry(table.to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    if let Some(entry) = entry.as_table_mut() {
        entry.insert(key.to_string(), value);
    }
}

/// What the schema says a setting starts at.
fn default_of(eng: &Engine, table: &str, key: &str) -> Option<toml::Value> {
    let registry = eng.try_resource::<SettingsRegistry>()?;
    let registry = registry.borrow();
    let page = registry.0.iter().find(|page| page.table == table)?;
    page.schema
        .get(key)
        .and_then(|spec| spec.get("default"))
        .cloned()
}

/// Read every value out of a manifest's text, so the editor starts from what
/// the project actually says rather than from the defaults.
///
/// # Errors
/// When the text is not valid TOML.
pub fn load_project(eng: &Engine, manifest: &str) -> Result<()> {
    let parsed: toml::value::Table =
        toml::from_str(manifest).context("parsing project.toml for settings")?;
    if let Some(values) = eng.try_resource::<SettingsValues>() {
        values.borrow_mut().0 = parsed;
    }
    Ok(())
}

/// The manifest text these settings would write, starting from `manifest` so
/// anything the settings do not describe survives.
///
/// Rewriting the parsed table rather than the text loses comments and order,
/// which a hand-edited manifest is entitled to keep. Only the keys a page
/// declares are touched.
///
/// # Errors
/// When the existing text is not valid TOML.
pub fn project_toml(eng: &Engine, manifest: &str) -> Result<String> {
    let mut doc: toml::value::Table = if manifest.trim().is_empty() {
        toml::value::Table::new()
    } else {
        toml::from_str(manifest).context("parsing project.toml")?
    };
    let Some(registry) = eng.try_resource::<SettingsRegistry>() else {
        return toml::to_string_pretty(&doc).context("writing project.toml");
    };
    let registry = registry.borrow();
    for page in registry.0.iter().filter(|p| p.scope == Scope::Project) {
        let Some(schema) = page.schema.as_table() else {
            continue;
        };
        for key in schema.keys() {
            let Some(value) = get(eng, &page.table, key) else {
                continue;
            };
            if page.table.is_empty() {
                doc.insert(key.clone(), value);
                continue;
            }
            let entry = doc
                .entry(page.table.clone())
                .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
            if let Some(entry) = entry.as_table_mut() {
                entry.insert(key.clone(), value);
            }
        }
    }
    toml::to_string_pretty(&doc).context("writing project.toml")
}

/// The editor's own settings as TOML, for its preferences file.
///
/// # Errors
/// When the values cannot be serialized.
pub fn editor_toml(eng: &Engine) -> Result<String> {
    let mut doc = toml::value::Table::new();
    let Some(registry) = eng.try_resource::<SettingsRegistry>() else {
        return Ok(String::new());
    };
    let registry = registry.borrow();
    for page in registry.0.iter().filter(|p| p.scope == Scope::Editor) {
        let Some(schema) = page.schema.as_table() else {
            continue;
        };
        let mut table = toml::value::Table::new();
        for key in schema.keys() {
            if let Some(value) = get(eng, &page.table, key) {
                table.insert(key.clone(), value);
            }
        }
        doc.insert(page.table.clone(), toml::Value::Table(table));
    }
    toml::to_string_pretty(&doc).context("writing the editor's settings")
}

/// Core's own pages: what a project is, how it saves, and what language it
/// speaks. Plugins add theirs from their own `declare`.
pub(crate) fn build_core_pages(eng: &Engine) {
    register(
        eng,
        SettingsPage {
            category: String::from("General"),
            table: String::new(),
            scope: Scope::Project,
            schema: crate::components::ComponentDef::parse_schema(
                "settings.general",
                r#"
name = { type = "string", default = "", help = "The game's name, used for its window title and its data directory." }
main_scene = { type = "string", default = "", help = "The scene a run opens with." }
language = { type = "enum", default = "rune", options = ["rune"], help = "Which scripting language this project is written in." }
"#,
            ),
        },
    );
    register(
        eng,
        SettingsPage {
            category: String::from("Saves"),
            table: String::from("save"),
            scope: Scope::Project,
            schema: crate::components::ComponentDef::parse_schema(
                "settings.save",
                r#"
version = { type = "float", default = 1.0, min = 1.0, max = 9999.0, help = "The save version this build writes. A lower file is migrated; a higher one is refused." }
migrate = { type = "string", default = "", help = "A script whose migrate_save(version, data) brings a file forward one version per call." }
"#,
            ),
        },
    );
    register(
        eng,
        SettingsPage {
            category: String::from("Language"),
            table: String::from("locale"),
            scope: Scope::Project,
            schema: crate::components::ComponentDef::parse_schema(
                "settings.locale",
                r#"
default = { type = "string", default = "en", help = "The locale a fresh run starts in." }
fallback = { type = "string", default = "en", help = "Where a key missing from the current locale is looked for next." }
"#,
            ),
        },
    );
    register(
        eng,
        SettingsPage {
            category: String::from("Netcode"),
            table: String::from("netcode"),
            scope: Scope::Editor,
            schema: crate::components::ComponentDef::parse_schema(
                "settings.netcode",
                r#"
faults = { type = "bool", default = false, help = "Put delay, jitter and packet loss on every session link, to test rollback against a link that misbehaves." }
delay = { type = "float", default = 9.0, min = 0.0, max = 60.0, help = "Ticks every payload waits before delivery. Nine is about 150 ms at 60 Hz." }
jitter = { type = "float", default = 3.0, min = 0.0, max = 30.0, help = "Extra ticks drawn per payload. Jitter is what reorders a stream." }
loss = { type = "float", default = 0.05, min = 0.0, max = 1.0, help = "The fraction of datagrams dropped. Datagrams only: losing a reliable payload would break the transport's contract." }
"#,
            ),
        },
    );
}

/// The faults the Netcode page currently asks for, or `None` when it is off.
///
/// Read by whoever builds a session's transports, so turning the toggle on
/// and starting a play session is all a developer has to do.
#[must_use]
pub fn faults(eng: &Engine) -> Option<crate::transport::Faults> {
    let on = get(eng, "netcode", "faults")?.as_bool()?;
    if !on {
        return None;
    }
    let number = |key: &str| get(eng, "netcode", key).and_then(|v| v.as_float());
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a tick count from a bounded setting"
    )]
    Some(crate::transport::Faults {
        delay: number("delay").unwrap_or(0.0) as u32,
        jitter: number("jitter").unwrap_or(0.0) as u32,
        loss: number("loss").unwrap_or(0.0) as f32,
    })
}
