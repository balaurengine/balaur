//! Every setting the engine, its plugins and a game declare, addressed by
//! path.
//!
//! A setting is named the way Godot names one: `physics/solver_iterations`,
//! `multiplayer/faults`, `editor/appearance/theme`. The first segment is the
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
//! it lives in the per-user `balaur/editor.toml` and never touches the project,
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

use crate::components::Schema;
use crate::engine::Engine;
use crate::tags::{OVERRIDE, Tags};
use crate::vocabulary::keys;
use crate::vocabulary::words;

/// Whose setting this is, and therefore where it is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The game's. Written to `project.toml`, shipped, version-controlled.
    Project,
    /// The person's. Written to the per-user `balaur/editor.toml`, never shipped.
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
            .is_none_or(|when| when == words::NOW)
    }

    /// Whether this describes the process running it rather than the game:
    /// its window, its checker, its own files. A project opened to be edited
    /// does not change it here.
    #[must_use]
    pub fn host(&self) -> bool {
        self.spec
            .get(keys::HOST)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
    }
}

/// Every setting, in definition order.
#[derive(Default)]
pub struct SettingsRegistry(pub Vec<SettingDef>);

/// The values settings currently hold, as the nested tables they are stored
/// in.
#[derive(Default)]
pub struct SettingsValues(pub toml::value::Table);

/// How many times a setting was defined, set, cleared or loaded.
#[derive(Default)]
pub struct SettingsRevision(pub u64);

fn moved(eng: &Engine) {
    if let Some(revision) = eng.try_resource::<SettingsRevision>() {
        revision.borrow_mut().0 += 1;
    }
}

/// A count that moves with every change to what is defined or held, so a
/// screen listing settings rebuilds its rows when it moves, not every frame.
#[must_use]
pub fn revision(eng: &Engine) -> u64 {
    eng.try_resource::<SettingsRevision>()
        .map_or(0, |revision| revision.borrow().0)
}

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
    moved(eng);
}

/// Define a group at once: every key in `schema` becomes `<prefix>/<key>`,
/// in the order the schema writes them.
///
/// A key may itself contain slashes, so one block can declare
/// `appearance/theme` and `recordings/keep` under the same prefix.
pub fn define_group(eng: &Engine, prefix: &str, scope: Scope, schema: &Schema) {
    for (key, spec) in schema.ordered() {
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
    answered(eng, path, |answer| {
        answer.stated.as_ref().or(answer.default.as_ref()).cloned()
    })
}

/// What this run was told, and nothing more: the narrowest override, else what
/// the file set, else `None`.
///
/// The read for a subsystem whose own default is not a number a schema can
/// hold — rapier's solver, say, where absent means "leave it alone" and a
/// declared default would quietly become an instruction.
#[must_use]
pub fn stated(eng: &Engine, path: &str) -> Option<toml::Value> {
    answered(eng, path, |answer| answer.stated.clone())
}

/// A value worked out from settings, kept until one moves, for a reader that
/// wants a whole table's worth every frame.
#[must_use]
pub fn kept<T: 'static>(eng: &Engine, work_out: impl FnOnce(&Engine) -> T) -> Rc<T> {
    let revision = revision(eng);
    if let Some(held) = eng.try_resource::<Kept<T>>() {
        let held = held.borrow();
        if held.revision == revision && tags_match(eng, &held.tags) {
            return Rc::clone(&held.value);
        }
    }
    let value = Rc::new(work_out(eng));
    eng.insert_resource(Kept {
        revision,
        tags: tags_now(eng),
        value: Rc::clone(&value),
    });
    value
}

/// One setting as the tags and the file answer it, and its schema's default.
struct Answer {
    stated: Option<toml::Value>,
    default: Option<toml::Value>,
}

impl Answer {
    fn of(eng: &Engine, path: &str) -> Self {
        Self {
            stated: read_stated(eng, path),
            default: def(eng, path).and_then(|d| d.spec.get(keys::DEFAULT).cloned()),
        }
    }
}

/// What reads have answered until a setting or this run's tags move: a frame
/// reads the same few settings, and one read builds a path per tag, walks the
/// tables and searches every definition.
#[derive(Default)]
pub struct SettingsAnswers {
    revision: u64,
    tags: Vec<String>,
    answers: rustc_hash::FxHashMap<Box<str>, Answer>,
}

struct Kept<T> {
    revision: u64,
    tags: Vec<String>,
    value: Rc<T>,
}

fn tags_match(eng: &Engine, tags: &[String]) -> bool {
    eng.try_resource::<Tags>()
        .map_or(tags.is_empty(), |now| now.borrow().0 == tags)
}

fn tags_now(eng: &Engine) -> Vec<String> {
    eng.try_resource::<Tags>()
        .map_or_else(Vec::new, |now| now.borrow().0.clone())
}

fn answered<R>(eng: &Engine, path: &str, read: impl FnOnce(&Answer) -> R) -> R {
    let Some(held) = eng.try_resource::<SettingsAnswers>() else {
        return read(&Answer::of(eng, path));
    };
    {
        let mut held = held.borrow_mut();
        let revision = revision(eng);
        if held.revision != revision || !tags_match(eng, &held.tags) {
            held.revision = revision;
            held.tags = tags_now(eng);
            held.answers.clear();
        }
        if let Some(answer) = held.answers.get(path) {
            return read(answer);
        }
    }
    let answer = Answer::of(eng, path);
    let out = read(&answer);
    held.borrow_mut().answers.insert(path.into(), answer);
    out
}

fn read_stated(eng: &Engine, path: &str) -> Option<toml::Value> {
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
    stored(eng, path).or_else(|| def(eng, path).and_then(|d| d.spec.get(keys::DEFAULT).cloned()))
}

/// One path's value as loaded, with no definition behind it: what an override
/// has, and what an undeclared table of the game's own has.
#[must_use]
fn stored(eng: &Engine, path: &str) -> Option<toml::Value> {
    let values = eng.try_resource::<SettingsValues>()?;
    stored_in(&values.borrow().0, path)
}

fn stored_in(root: &toml::value::Table, path: &str) -> Option<toml::Value> {
    let (tables, key) = split(path)?;
    let mut at = root;
    for table in tables {
        at = at.get(table).and_then(toml::Value::as_table)?;
    }
    at.get(key).cloned()
}

fn remove_in(root: &mut toml::value::Table, path: &str) {
    let Some((tables, key)) = split(path) else {
        return;
    };
    let mut at = root;
    for table in tables {
        match at.get_mut(table).and_then(toml::Value::as_table_mut) {
            Some(next) => at = next,
            None => return,
        }
    }
    at.remove(key);
}

/// Change one setting for this run. Not written to disk until a caller asks
/// for the text.
pub fn set(eng: &Engine, path: &str, value: toml::Value) {
    change_live(eng, |root| insert_in(root, path, value));
}

fn change_live(eng: &Engine, change: impl FnOnce(&mut toml::value::Table)) {
    if let Some(values) = eng.try_resource::<SettingsValues>() {
        change(&mut values.borrow_mut().0);
        moved(eng);
    }
}

fn insert_in(root: &mut toml::value::Table, path: &str, value: toml::Value) {
    if let Some((tables, key)) = split(path) {
        table_at(root, &tables).insert(key.to_string(), value);
    }
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
    moved(eng);
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
    change_live(eng, |root| remove_in(root, path));
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
pub fn build_core_settings(eng: &Engine) {
    let parse = |name: &str, text: &str| crate::components::ComponentDef::parse_schema(name, text);
    define_group(
        eng,
        keys::APPLICATION,
        Scope::Project,
        &parse(
            "settings.application",
            r#"
name = { type = "string", host = true, default = "", order = 1, help = "The game's name, used for its window title and its data directory." }
main_scene = { type = "string", host = true, default = "", order = 2, help = "The scene a run opens with." }
asset_source = { type = "enum", host = true, default = "embedded", options = ["embedded", "files", "embedded_then_files"], order = 4, applies = "restart", help = "Where a shipped game may read its bytes from. Only bites once packed; a dev run always reads the source tree." }
splash = { type = "string", host = true, default = "", order = 5, applies = "restart", help = "A project-relative picture shown over the first frames, on every target. Empty shows none." }
splash_seconds = { type = "float", host = true, default = 1.5, min = 0.0, max = 60.0, order = 6, applies = "restart", help = "How long the splash stays, in seconds of engine time, and the least it stays when a script is reporting a load through `ui.set_load_progress`." }
ignore = { type = "list", of = { type = "string" }, default = [], order = 7, applies = "restart", help = "Project paths that are not the game's, as globs: `art/wip/**`, `**/*.blend1`. The asset index skips them and a pack leaves them out." }
icon = { type = "string", host = true, default = "", order = 8, applies = "restart", help = "A project-relative square picture, a PNG or an SVG, that every export writes as that platform's app icon, and the window shows while the game runs. 1024 pixels is what a store asks for." }
icon_dark = { type = "string", host = true, default = "", order = 9, applies = "restart", help = "The icon's form for a dark system: the dark appearance on iOS and macOS, the favicon a dark browser shows, and the window icon while `dark_mode()` is true." }
icon_monochrome = { type = "string", host = true, default = "", order = 10, applies = "restart", help = "A single-colour form, white on transparency, that iOS and macOS tint and Android 13 draws in the themed launcher." }
"#,
        ),
    );
    build_window_settings(eng, &parse);
    build_time_settings(eng, &parse);
    define_group(
        eng,
        "check",
        Scope::Project,
        &parse(
            "settings.check",
            r#"
strict = { type = "bool", host = true, default = false, applies = "restart", help = "`balaur check` fails on a warning as well as an error, for this project on every run." }
"#,
        ),
    );
    define_group(
        eng,
        words::functions::LOCALE,
        Scope::Project,
        &parse(
            "settings.locale",
            r#"
initial = { type = "string", default = "", help = "The locale a fresh run starts in. Empty starts in the system's language where the project ships it, else in `fallback`." }
fallback = { type = "string", default = "en", help = "Where a key missing from the current locale is looked for next." }
"#,
        ),
    );
    define_group(
        eng,
        "multiplayer",
        Scope::Editor,
        &parse(
            "settings.multiplayer",
            r#"
simulate_faults = { type = "bool", default = false, order = 1, help = "Put delay, jitter and packet loss on every multiplayer link, to test rollback against a link that misbehaves." }
delay_ticks = { type = "int", default = 9, min = 0, max = 60, order = 2, help = "Ticks every payload waits before delivery. Nine is about 150 ms at 60 Hz." }
jitter_ticks = { type = "int", default = 3, min = 0, max = 30, order = 3, help = "Extra ticks drawn per payload. Jitter is what reorders a stream." }
loss = { type = "float", default = 0.05, min = 0.0, max = 1.0, order = 4, help = "The fraction of datagrams dropped. Datagrams only: losing a reliable payload would break the transport's contract." }
"#,
        ),
    );
    build_import_settings(eng, &parse);
}

/// `[import.<kind>]`: the defaults every file of a kind is read with, which a
/// sidecar beside one file then overrides. The manifest is parsed once while
/// starting, so a change here reaches the picture on the next run.
fn build_import_settings(eng: &Engine, parse: &impl Fn(&str, &str) -> std::rc::Rc<Schema>) {
    define_group(
        eng,
        "import/texture",
        Scope::Project,
        &parse(
            "settings.import.texture",
            r#"
filter = { type = "enum", default = "linear", options = ["linear", "nearest"], order = 1, applies = "restart", help = "Between texels. Nearest is what keeps pixel art crisp when it is magnified." }
repeat = { type = "enum", default = "clamp", options = ["clamp", "repeat", "mirror"], order = 2, applies = "restart", help = "What a coordinate past the edge reads. Mirror tiles without a seam." }
mipmaps = { type = "bool", default = false, order = 3, applies = "restart", help = "Build the smaller copies a texture drawn small samples, which stops it shimmering. A picture on a 3D surface builds them unless this says or its filter is nearest." }
anisotropy = { type = "int", default = 1, min = 1, max = 16, order = 4, applies = "restart", help = "Samples per fetch on a surface seen edge-on. Needs every filter linear." }
premultiply = { type = "bool", default = false, order = 5, applies = "restart", help = "Scale colour by alpha at upload, so a soft edge blends with no dark fringe. 2D nodes only." }
srgb = { type = "bool", default = true, order = 6, applies = "restart", help = "Off for a normal map or a mask, which carry data rather than colour." }
bleed = { type = "bool", default = true, order = 7, applies = "restart", help = "Spread the edge colour into fully transparent texels, so a linear filter never samples a dark or white fringe. Colour sampled linearly only." }
pixels_per_unit = { type = "float", default = 100.0, min = 0.01, order = 8, applies = "restart", help = "Texture pixels to one world unit, for every sprite whose own pixels_per_unit is 0. A pixel-art project sets its tile size here once." }
"#,
        ),
    );
    define_group(
        eng,
        "import/audio",
        Scope::Project,
        &parse(
            "settings.import.audio",
            r#"
volume_linear = { type = "float", default = 1.0, min = 0.0, max = 4.0, order = 1, applies = "restart", help = "Every sound's own level, multiplied into each play of it. A file's sidecar sets its own." }
loop = { type = "bool", default = false, order = 2, applies = "restart", help = "Loop every sound wherever it is played. A music folder usually sets this per file instead." }
force_mono = { type = "bool", default = false, order = 3, applies = "restart", help = "Mix every WAV to one channel at export, which halves a stereo file. A sound played from a place in the world is heard mono anyway." }
max_rate_hz = { type = "int", default = 0, min = 0, max = 192000, order = 4, applies = "restart", help = "The highest sample rate a WAV ships at, in Hz; 0 keeps each file's own. 22050 is plenty for most effects. Set it per target to ship a phone less." }
gapless = { type = "bool", default = true, order = 5, applies = "restart", help = "Trim the padding an encoder adds to a compressed file, where the file records it, so files queued back to back play without a gap." }
"#,
        ),
    );
    define_group(
        eng,
        "import/font",
        Scope::Project,
        &parse(
            "settings.import.font",
            r#"
scale = { type = "float", default = 1.0, min = 0.1, max = 10.0, order = 1, applies = "restart", help = "How large a project face's glyphs are drawn and spaced, in the UI and in world text." }
hinting = { type = "bool", default = true, order = 2, applies = "restart", help = "Snap a face's outlines to the pixel grid. Off for a smooth face drawn large." }
antialias = { type = "bool", default = true, order = 3, applies = "restart", help = "Smooth glyph edges. Off draws each pixel fully on or off, for a pixel face at its own size; egui's own text and magnified world text stay smooth." }
"#,
        ),
    );
    define_group(
        eng,
        "import/model",
        Scope::Project,
        &parse(
            "settings.import.model",
            r#"
scale = { type = "float", default = 1.0, min = 0.0001, max = 1000.0, order = 1, applies = "restart", help = "A model file's units to the scene's: 0.01 for files modelled in centimetres. A skinned mesh is left as authored." }
lods = { type = "int", default = 0, min = 0, max = 6, order = 2, applies = "restart", help = "How many simpler copies of every rigid model to build at load, each with half the triangles of the one before." }
lod_distance = { type = "float", default = 20.0, min = 0.01, order = 3, applies = "restart", help = "The camera distance, in world units, at which the first simpler copy takes over; each further one takes over at twice it." }
"#,
        ),
    );
}

/// `[time]`: how fast the simulation ticks, and whether the frames between
/// two ticks are drawn.
fn build_time_settings(eng: &Engine, parse: &impl Fn(&str, &str) -> std::rc::Rc<Schema>) {
    define_group(
        eng,
        keys::TIME,
        Scope::Project,
        &parse(
            "settings.time",
            r#"
tick_hz = { type = "int", default = 60, min = 1, max = 480, order = 1, applies = "restart", help = "How many fixed steps a second the simulation takes. Sixty is the rate the cross-platform digest job runs, and the one every recording made before this setting existed used." }
"#,
        ),
    );
}

/// `[window]` and `[ui]`: what a windowed build opens and draws with, which
/// is the table a platform most often answers differently.
fn build_window_settings(eng: &Engine, parse: &impl Fn(&str, &str) -> std::rc::Rc<Schema>) {
    define_group(
        eng,
        "window",
        Scope::Project,
        &parse(
            "settings.window",
            r#"
width = { type = "int", host = true, default = 1600, min = 1, max = 16384, order = 1, applies = "restart", help = "Logical width. The backing store is this times the display's scale, which is what the render targets are sized from." }
height = { type = "int", host = true, default = 1000, min = 1, max = 16384, order = 2, applies = "restart", help = "Logical height." }
mode = { type = "enum", host = true, default = "windowed", options = ["windowed", "maximized", "fullscreen", "exclusive"], order = 3, applies = "restart", help = "How the window opens: at its size, maximized, borderless over the whole screen, or exclusive, which takes the monitor's largest video mode. window.set_window_mode changes it later through the same state." }
orientation = { type = "enum", host = true, default = "any", options = ["any", "portrait", "landscape"], order = 4, applies = "restart", help = "Which way up a phone may hold the game. Written into the export's own manifest, since a device decides this before the game runs." }
vsync = { type = "bool", host = true, default = true, order = 5, help = "Present in step with the display." }
msaa = { type = "int", host = true, default = 1, min = 1, max = 4, order = 6, help = "Samples per pixel. 1 is off and 4 is the only other count the renderer offers; it costs two render targets of four samples each." }
max_fps = { type = "int", host = true, default = 0, min = 0, max = 1000, order = 7, applies = "restart", help = "The most frames a second the loop draws. Zero paces it against the tick instead, which is what vsync already does on a display that runs at the tick rate." }
low_processor = { type = "bool", host = true, default = false, order = 8, applies = "restart", help = "Run a frame only while something asks for one: input, ui.request_repaint, a log line, a changed file, work finished on another thread, or simulation still under way. That is a timer or a task wait counting, a clip or a tween playing, a body awake, particles alive, a sound that will end, a replay or a match. With none of them the loop sleeps. What a tool or an app of still screens wants; a game that moves every frame gains nothing from it." }
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
system_fonts = { type = "bool", default = true, applies = "restart", help = "Let a character no face of the project has a glyph for draw in the face the operating system picks for it, instead of tofu. A worker reads every font on the machine once, after the first frame, so a game that only draws what it vendors can turn it off." }
theme = { type = "string", default = "", help = "A project-relative `widget_theme` every widget and egui's own controls start from, as Godot's project theme is. A widget that names its own `theme` still dresses its subtree with that one." }
theme_dark = { type = "string", default = "", help = "The theme's form for a dark system, worn while `dark_mode()` is true. `ui.set_theme` goes over both." }
scale = { type = "float", default = 1.0, min = 0.25, max = 3.0, applies = "restart", help = "How large the game's own UI is drawn, as egui's zoom factor: every control and every font together, on top of the display's own scale. A phone usually wants more than a desktop, which is what `[override.mobile.ui] scale` is for. `ui.set_scale` changes it later." }
system_text_size = { type = "bool", default = true, applies = "restart", help = "Multiply the scale by the size the reader asked their system for. Off for a game whose layout cannot give text more room." }
narrow_below_pixels = { type = "float", host = true, default = 600.0, min = 1.0, max = 8192.0, applies = "restart", help = "Under this many design pixels of width a layout reads `narrow`: one column, one panel at a time. Android's own line between its compact and medium window classes." }
wide_from_pixels = { type = "float", host = true, default = 840.0, min = 1.0, max = 8192.0, applies = "restart", help = "From this many design pixels of width a layout reads `wide` and may spread out. Between the two lines it reads `medium`." }
short_below_pixels = { type = "float", host = true, default = 480.0, min = 1.0, max = 8192.0, applies = "restart", help = "Under this many design pixels of height a layout reads `short`, which is a phone on its side." }
"#,
        ),
    );
}
