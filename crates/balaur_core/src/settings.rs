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
//!
//! **One key may hold more than one answer.** A setting's path under
//! `override/<tag>/` is what that platform reads instead:
//! `[override.android.window] orientation = "portrait"` is `window/orientation`
//! on a phone and nowhere else. The storage rule needs no exception for it, since
//! the override is a path like any other. [`get`] resolves against the tags
//! this run answers to, narrowest first; [`base`] answers what the file says,
//! which is what the editor edits. See [`crate::tags`].

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{Context, Result};

use crate::engine::Engine;
use crate::tags::{OVERRIDE, Tags};

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

    registry.borrow().0.iter().find(|d| d.path == path).cloned()
}

/// A path split into the tables it nests through and the key it ends at.
fn split(path: &str) -> Option<(Vec<&str>, &str)> {
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let key = parts.pop()?;
    Some((parts, key))
}

/// One setting's value here: the narrowest override this run answers to,
/// else what the file set, else what its definition defaults to.
#[must_use]
pub fn get(eng: &Engine, path: &str) -> Option<toml::Value> {
    stated(eng, path).or_else(|| def(eng, path).and_then(|d| d.spec.get("default").cloned()))
}

/// What this run was told, and nothing more: the narrowest override, else what
/// the file set, else `None`.
///
/// The read for a subsystem whose own default is not a number a schema can
/// hold — rapier's solver, say, where absent means "leave it alone" and a
/// declared default would quietly become an instruction.
#[must_use]
pub fn stated(eng: &Engine, path: &str) -> Option<toml::Value> {
    if let Some(tags) = eng.try_resource::<Tags>() {
        let tags = tags.borrow();
        for tag in tags.narrowest_first() {
            if let Some(found) = stored(eng, &format!("{OVERRIDE}/{tag}/{path}")) {
                return Some(found);
            }
        }
    }
    stored(eng, path)
}

/// A whole table as this run resolves it: the file's own, with each tag's
/// override folded on, broad to narrow.
///
/// The read for a table whose keys are the game's rather than the engine's:
/// `[input.actions]` names actions and `[audio.buses]` names buses, so
/// `[override.mobile.input.actions] jump = ["touch:..."]` rebinds one action
/// and leaves the rest as the file has them.
#[must_use]
pub fn table(eng: &Engine, path: &str) -> toml::value::Table {
    let mut out = match stored(eng, path) {
        Some(toml::Value::Table(base)) => base,
        _ => toml::value::Table::new(),
    };
    if let Some(tags) = eng.try_resource::<Tags>() {
        for tag in &tags.borrow().0 {
            if let Some(toml::Value::Table(layer)) =
                stored(eng, &format!("{OVERRIDE}/{tag}/{path}"))
            {
                merge(&mut out, layer);
            }
        }
    }
    out
}

/// What the file says, whatever platform is reading it.
///
/// The editor's own read: a settings screen showing the override its machine
/// happened to match would write that value back onto the base key.
#[must_use]
pub fn base(eng: &Engine, path: &str) -> Option<toml::Value> {
    stored(eng, path).or_else(|| def(eng, path).and_then(|d| d.spec.get("default").cloned()))
}

/// One path's value as loaded, with no definition behind it: what an override
/// has, and what an undeclared table of the game's own has.
#[must_use]
fn stored(eng: &Engine, path: &str) -> Option<toml::Value> {
    if let Some(values) = eng.try_resource::<SettingsValues>()
        && let Some((tables, key)) = split(path)
    {
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
        if reached && let Some(found) = at.get(key) {
            return Some(found.clone());
        }
    }
    None
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
    table_at(&mut values.0, &tables).insert(key.to_string(), value);
}

/// The table `tables` names under `root`, made on the way down; a value
/// sitting where a table belongs is replaced by one.
fn table_at<'a>(root: &'a mut toml::value::Table, tables: &[&str]) -> &'a mut toml::value::Table {
    let mut at = root;
    for table in tables {
        let entry = at
            .entry((*table).to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if !entry.is_table() {
            *entry = toml::Value::Table(toml::value::Table::new());
        }
        at = entry.as_table_mut().expect("made a table above");
    }
    at
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

/// Every tag an override may be written for here: the engine's, then the
/// project's own from `[export] tags`.
#[must_use]
pub fn known_tags(eng: &Engine) -> Vec<String> {
    let mut all: Vec<String> = crate::tags::ALL.iter().map(|t| (*t).to_string()).collect();
    if let Some(values) = eng.try_resource::<SettingsValues>() {
        for own in crate::tags::declared_in(&values.borrow().0) {
            if !all.contains(&own) {
                all.push(own);
            }
        }
    }
    all
}

/// Push the tags an export resolved, from `[build] tags`, onto the ones this
/// run answers to. Called wherever a manifest is loaded, so a shipped demo
/// reads `[override.demo]` from its first setting on.
pub fn answer_to_built_tags(eng: &Engine) {
    let (Some(values), Some(tags)) = (
        eng.try_resource::<SettingsValues>(),
        eng.try_resource::<Tags>(),
    ) else {
        return;
    };
    let built = crate::tags::built_in(&values.borrow().0);
    let mut tags = tags.borrow_mut();
    for name in &built {
        tags.push(name);
    }
}

/// The tags this manifest holds an override for at `path`, in tag order.
///
/// What the settings screen lists under a row: the answers this key has
/// besides the one in front of you.
#[must_use]
pub fn overrides(eng: &Engine, path: &str) -> Vec<String> {
    known_tags(eng)
        .into_iter()
        .filter(|tag| stored(eng, &format!("{OVERRIDE}/{tag}/{path}")).is_some())
        .collect()
}

/// Keys a manifest holds that nothing declares, inside tables that something
/// does.
///
/// The rule a typo has to fall foul of: `[window] fullscren` is an error
/// because `[window]` is a table the engine describes, while `[mygame]
/// local_server_url` is not, because the table is the game's own space. An
/// override is checked against the setting it overrides, so
/// `[override.android.window] fullscren` is caught too.
#[must_use]
pub fn unknown(eng: &Engine, text: &str) -> Vec<String> {
    let Ok(doc) = toml::from_str::<toml::value::Table>(text) else {
        return Vec::new();
    };
    let Some(registry) = eng.try_resource::<SettingsRegistry>() else {
        return Vec::new();
    };
    let declared: Vec<String> = registry.borrow().0.iter().map(|d| d.path.clone()).collect();
    let mut found = Vec::new();
    walk(&doc, "", &mut |path: &str| {
        let named = without_tag(path);
        if declared.iter().any(|d| d == named) {
            return;
        }
        let Some((table, _)) = named.rsplit_once('/') else {
            return;
        };
        // A table nothing describes is the game's own, and a key in it is
        // whatever the game meant by it.
        if declared.iter().any(|d| d.starts_with(&format!("{table}/"))) {
            found.push(path.to_string());
        }
    });
    found
}

/// An override's path without the `override/<tag>/` it is stored under, so it
/// is checked against the setting it answers for.
fn without_tag(path: &str) -> &str {
    path.strip_prefix(&format!("{OVERRIDE}/"))
        .and_then(|rest| rest.split_once('/'))
        .map_or(path, |(_, rest)| rest)
}

/// Every leaf key in a document, as the path it is stored at.
fn walk(table: &toml::value::Table, prefix: &str, found: &mut impl FnMut(&str)) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}/{key}")
        };
        match value {
            toml::Value::Table(inner) => walk(inner, &path, found),
            _ => found(&path),
        }
    }
}

/// A manifest's tables with `tags`' overrides folded onto the keys they
/// override, and `[override]` itself dropped.
///
/// What an export reads. The engine resolves per read, against the tags the
/// machine running it holds; a build resolves for a machine it is not, so it
/// asks for the whole document at once. The precedence is the same one: the
/// narrowest tag the target answers to wins.
///
/// # Errors
/// When the text is not valid TOML.
pub fn resolve(text: &str, tags: &Tags) -> Result<toml::value::Table> {
    let mut doc: toml::value::Table = toml::from_str(text).context("parsing settings")?;
    let Some(toml::Value::Table(overrides)) = doc.remove(OVERRIDE) else {
        return Ok(doc);
    };
    for tag in &tags.0 {
        if let Some(toml::Value::Table(layer)) = overrides.get(tag.as_str()) {
            merge(&mut doc, layer.clone());
        }
    }
    Ok(doc)
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

/// Forget one value, so the next write drops the key it was stored at.
///
/// What removing an override is: the base value is a row that always exists,
/// but an override is one the project either holds or does not.
pub fn clear(eng: &Engine, path: &str) {
    let Some(values) = eng.try_resource::<SettingsValues>() else {
        return;
    };
    let Some((tables, key)) = split(path) else {
        return;
    };
    let mut values = values.borrow_mut();
    table_at(&mut values.0, &tables).remove(key);
}

/// The text one scope's settings would write, starting from `existing` so
/// anything no setting describes survives.
///
/// Edits the document rather than rebuilding it, so a manifest keeps its
/// comments, its key order and its unrelated tables. Only a value something
/// set is written: a key the project never named stays absent rather than
/// arriving as its default, which would turn "the engine decides" into a
/// number nobody chose.
///
/// # Errors
/// When `existing` is not valid TOML.
pub fn to_toml(eng: &Engine, scope: Scope, existing: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut =
        existing.parse().context("parsing the file being written")?;
    let Some(registry) = eng.try_resource::<SettingsRegistry>() else {
        return Ok(doc.to_string());
    };
    let paths: Vec<String> = registry
        .borrow()
        .0
        .iter()
        .filter(|d| d.scope == scope)
        .map(|d| d.path.clone())
        .collect();
    let tags = known_tags(eng);
    for path in paths {
        if let Some(value) = stored(eng, &path) {
            write_at(&mut doc, &path, &value);
        }
        // An override is a key the project either holds or does not, so a
        // cleared one is removed rather than written back as it was.
        if scope != Scope::Project {
            continue;
        }
        for tag in &tags {
            let at = format!("{OVERRIDE}/{tag}/{path}");
            match stored(eng, &at) {
                Some(value) => write_at(&mut doc, &at, &value),
                None => remove_at(&mut doc, &at),
            }
        }
    }
    prune_overrides(&mut doc);
    Ok(doc.to_string())
}

/// One value written into a manifest's text at its path, keeping everything
/// else as written: what an export uses to stamp its pack's copy.
///
/// # Errors
/// When `existing` is not valid TOML.
pub fn patch(existing: &str, path: &str, value: &toml::Value) -> Result<String> {
    let mut doc: toml_edit::DocumentMut =
        existing.parse().context("parsing the file being written")?;
    write_at(&mut doc, path, value);
    Ok(doc.to_string())
}

/// Put one value at its path, making the tables on the way. A value that has
/// not changed is left as written, so a save touches only what moved.
fn write_at(doc: &mut toml_edit::DocumentMut, path: &str, value: &toml::Value) {
    let Some((tables, key)) = split(path) else {
        return;
    };
    let mut at: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for table in tables {
        let entry = at.entry(table).or_insert_with(|| {
            let mut made = toml_edit::Table::new();
            // Only the table holding the key gets a header of its own.
            made.set_implicit(true);
            toml_edit::Item::Table(made)
        });
        if entry.as_table_like().is_none() {
            *entry = toml_edit::Item::Table(toml_edit::Table::new());
        }
        at = entry.as_table_like_mut().expect("made a table above");
    }
    let item = crate::file_api::as_item(value);
    // Compared without decor: `name = "g" # shown in the title` is the same
    // value as `"g"`, and rewriting it would take the comment with it.
    let bare = |v: &toml_edit::Value| {
        let mut v = v.clone();
        v.decor_mut().clear();
        v.to_string()
    };
    let same = at
        .get(key)
        .and_then(toml_edit::Item::as_value)
        .zip(item.as_value())
        .is_some_and(|(held, new)| bare(held) == bare(new));
    if !same {
        at.insert(key, item);
    }
}

/// Remove one key without making the tables on the way to it.
fn remove_at(doc: &mut toml_edit::DocumentMut, path: &str) {
    let Some((tables, key)) = split(path) else {
        return;
    };
    let mut at: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for table in tables {
        let Some(next) = at
            .get_mut(table)
            .and_then(toml_edit::Item::as_table_like_mut)
        else {
            return;
        };
        at = next;
    }
    at.remove(key);
}

/// Drop the tables a removed override left behind. Only under `override`: an
/// empty `[plugins]` elsewhere is a project saying something.
fn prune_overrides(doc: &mut toml_edit::DocumentMut) {
    let Some(overrides) = doc
        .get_mut(OVERRIDE)
        .and_then(toml_edit::Item::as_table_like_mut)
    else {
        return;
    };
    prune_empty(overrides);
    if overrides.is_empty() {
        doc.remove(OVERRIDE);
    }
}

fn prune_empty(table: &mut dyn toml_edit::TableLike) {
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
    for key in keys {
        let Some(inner) = table
            .get_mut(&key)
            .and_then(toml_edit::Item::as_table_like_mut)
        else {
            continue;
        };
        prune_empty(inner);
        if inner.is_empty() {
            table.remove(&key);
        }
    }
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
assets = { type = "enum", default = "files", options = ["files", "embedded", "embeddedthenfiles"], order = 4, applies = "restart", help = "Where a shipped game may read its bytes from. Only bites once packed; a dev run always reads the source tree." }
splash = { type = "string", default = "", order = 5, applies = "restart", help = "A project-relative picture shown over the first frames, on every target. Empty shows none." }
splash_seconds = { type = "float", default = 1.5, min = 0.0, max = 60.0, order = 6, applies = "restart", help = "How long the splash stays, in seconds of engine time." }
init_order = { type = "enum", default = "scene", options = ["scene", "children_first"], order = 7, applies = "restart", help = "The order a loading scene's scripts run init in: the file's, or every child before its parent, as Godot runs _ready." }
"#,
        ),
    );
    build_window_settings(eng, &parse);
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
    // The defaults every file of a kind is read with, which a sidecar beside
    // one file then overrides. The manifest is parsed once while starting, so
    // a change here reaches the picture on the next run.
    define_group(
        eng,
        "import/texture",
        Scope::Project,
        &parse(
            "settings.import.texture",
            r#"
filter = { type = "enum", default = "linear", options = ["linear", "nearest"], order = 1, applies = "restart", help = "Between texels. Nearest is what keeps pixel art crisp when it is magnified." }
repeat = { type = "enum", default = "clamp", options = ["clamp", "repeat", "mirror"], order = 2, applies = "restart", help = "What a coordinate past the edge reads. Mirror tiles without a seam." }
mipmaps = { type = "bool", default = false, order = 3, applies = "restart", help = "Build the smaller copies a texture drawn small samples, which stops it shimmering." }
anisotropy = { type = "int", default = 1, min = 1, max = 16, order = 4, applies = "restart", help = "Samples per fetch on a surface seen edge-on. Needs every filter linear." }
premultiply = { type = "bool", default = false, order = 5, applies = "restart", help = "Scale colour by alpha at upload, so a soft edge blends with no dark fringe. 2D nodes only." }
srgb = { type = "bool", default = true, order = 6, applies = "restart", help = "Off for a normal map or a mask, which carry data rather than colour." }
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

/// `[window]` and `[ui]`: what a windowed build opens and draws with, which
/// is the table a platform most often answers differently.
fn build_window_settings(eng: &Engine, parse: &impl Fn(&str, &str) -> std::rc::Rc<toml::Value>) {
    define_group(
        eng,
        "window",
        Scope::Project,
        &parse(
            "settings.window",
            r#"
width = { type = "int", default = 1600, min = 1, max = 16384, order = 1, applies = "restart", help = "Logical width. The backing store is this times the display's scale, which is what the render targets are sized from." }
height = { type = "int", default = 1000, min = 1, max = 16384, order = 2, applies = "restart", help = "Logical height." }
mode = { type = "enum", default = "windowed", options = ["windowed", "maximized", "fullscreen", "exclusive"], order = 3, applies = "restart", help = "How the window opens: at its size, maximized, borderless over the whole screen, or exclusive, which takes the monitor's largest video mode. render.set_window_mode changes it later through the same state." }
orientation = { type = "enum", default = "any", options = ["any", "portrait", "landscape"], order = 4, applies = "restart", help = "Which way up a phone may hold the game. Written into the export's own manifest, since a device decides this before the game runs." }
vsync = { type = "bool", default = true, order = 5, applies = "restart", help = "Present in step with the display." }
msaa = { type = "int", default = 1, min = 1, max = 4, order = 6, applies = "restart", help = "Samples per pixel. 1 is off and 4 is the only other count the renderer offers; it costs two render targets of four samples each." }
"#,
        ),
    );
    define_group(
        eng,
        "ui",
        Scope::Project,
        &parse(
            "settings.ui",
            r#"
system_fonts = { type = "bool", default = true, applies = "restart", help = "Append the operating system's own faces to every font chain, so text in a script balaur does not vendor draws instead of tofu. They are the largest files on the machine, so a game that only draws what it vendors can turn them off." }
theme = { type = "string", default = "", help = "A project-relative `widget_theme` every widget starts from, as Godot's project theme is. A widget that names its own `theme` still dresses its subtree with that one." }
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
