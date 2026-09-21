//! `project.*` for the editor: the projects this machine has opened, the
//! templates it can start one from, and the folder picker its OS provides.
//!
//! The list is a file of paths, not a registry: written when a project is
//! opened or created, read back newest first, and never scanned for. A row
//! whose folder is gone stays until somebody forgets it, because a missing
//! project is usually an unplugged disk rather than a deleted game.

use std::path::{Path, PathBuf};

use anyhow::Result;
use balaur::{Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

mod release;

/// How many projects the list keeps. Past this the oldest falls off: the
/// list is for going back to last week's work, not for an archive.
const KEPT: usize = 20;

/// Where the list lives, and what a pick answered.
pub(crate) struct ProjectState {
    /// The editor's own data directory, named once at start-up because the
    /// manifest it is derived from is the editor's until a project opens.
    home: PathBuf,
    /// What the last folder pick chose, read once by the script that asked.
    picked: Option<String>,
}

pub(crate) struct ProjectPlugin {
    manifest: balaur_plugin::Manifest,
}

impl ProjectPlugin {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("project", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for ProjectPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(ProjectState {
            home: PathBuf::new(),
            picked: None,
        });
        // The data directory is derived from the loaded manifest, which is
        // the editor's own by the time the first frame runs.
        reg.add_system(Stage::First, |eng, _| {
            let state = eng.resource::<ProjectState>();
            let mut state = state.borrow_mut();
            if state.home.as_os_str().is_empty() {
                state.home = balaur_core::engine_api::user_data_dir_of(eng);
            }
        });
        let mut m = reg.script_module("project")?;
        install_project_api(&mut *m);
        install_project_verbs(&mut *m);
        drop(m);
        release::declare(reg)
    }
}

fn install_project_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "The projects this machine has opened, the templates a new one starts from, and the folder picker the OS provides. What the editor's start screen is made of.",
    );
    describe_project_api(m);
}

/// What each verb is, for the reference and the editor's hover.
fn describe_project_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "recent",
            &[],
            "",
            "The projects opened on this machine, newest first: `{ path, name, opened, exists }` each, where `opened` is a Unix time in seconds and `exists` says whether the folder is still there.",
        ),
        (
            "templates",
            &[],
            "",
            "What a new project may start from: `{ id, note }` each, read from the editor's own library.",
        ),
        (
            "create",
            &[],
            "(path: string, template: string)",
            "Write a new project at `path` from a template id, or from nothing when the id is empty. Answers `{ path, name }`, or `{ error }`.",
        ),
        (
            "open",
            &[],
            "(path: string)",
            "Remember `path` and start the editor on it, then ask this one to quit. Answers `{ error }` when there is no project there.",
        ),
        (
            "forget",
            &[],
            "(path: string)",
            "Drop one project from the list. The folder is not touched.",
        ),
        (
            "home",
            &[],
            "",
            "Where a new project goes unless the reader says otherwise: the home directory on a desktop, and the app's own writable directory where there is no such thing.",
        ),
        (
            "in_tab",
            &[],
            "",
            "Whether this editor runs in a browser tab, where the page holds the projects and opening one is its call rather than this screen's.",
        ),
        (
            "version",
            &[],
            "",
            "The version of the binary the editor is running in.",
        ),
        (
            "examples",
            &[],
            "",
            "The example projects shipped beside the editor: `{ id, name, note, cover, path }` each, where `cover` is a picture the editor's own project holds, or empty.",
        ),
        (
            "copy_example",
            &[],
            "(id: string, into: string)",
            "Copy one example into `into` under its own name, so the shipped one stays as it is. Answers `{ path, name }`, or `{ error }`.",
        ),
        (
            "pick_folder",
            &[],
            "",
            "Open the OS folder picker and answer what was chosen, or `()` when it was dismissed. Blocks while the dialog is up, and answers `()` on a platform with no picker.",
        ),
        (
            "use_data",
            &[],
            "(name: string?)",
            "Keep `save::` slots and the device id in the user data directory of the game named `name`, and let `fs` reach it, so a game played here and run alone share saves and a device login. Nil goes back to the editor's own. Answers the directory, or nil.",
        ),
    ]);
}

fn install_project_verbs(m: &mut dyn Bindings<Engine>) {
    m.function("recent", |eng: &Engine, ()| {
        Ok(Value::List(recent_rows(eng)))
    });
    m.function("templates", |_: &Engine, ()| Ok(Value::List(templates())));
    m.function(
        "create",
        |eng: &Engine, (path, template): (String, String)| {
            let home = home_of(eng);
            Ok(create(&home, Path::new(&path), &template))
        },
    );
    m.function("open", |eng: &Engine, path: String| {
        let home = home_of(eng);
        Ok(open(eng, &home, Path::new(&path)))
    });
    m.function("forget", |eng: &Engine, path: String| {
        forget(eng, Path::new(&path));
        Ok(Value::Nil)
    });
    m.function("examples", |_: &Engine, ()| Ok(Value::List(examples())));
    m.function(
        "copy_example",
        |eng: &Engine, (id, into): (String, String)| {
            let home = home_of(eng);
            Ok(copy_example(&home, &id, Path::new(&into)))
        },
    );
    m.function("home", |eng: &Engine, ()| {
        Ok(Value::Str(
            user_home()
                .unwrap_or_else(|| home_of(eng))
                .to_string_lossy()
                .into_owned()
                .into(),
        ))
    });
    m.function("in_tab", |_: &Engine, ()| {
        Ok(Value::Bool(cfg!(target_family = "wasm")))
    });
    m.function("version", |_: &Engine, ()| {
        Ok(Value::Str(crate::version::long().to_string().into()))
    });
    // A name, never a path: the directory stays under the user data base
    // whatever a script passes.
    m.function("use_data", |eng: &Engine, name: Option<String>| {
        let home = name.filter(|name| !name.is_empty()).map(|name| {
            let dir = balaur::engine_api::user_data_dir_named(eng, &name);
            balaur::file_api::add_root(eng, &dir);
            dir
        });
        balaur::save::set_home(eng, home.clone());
        balaur::facts::reread(eng);
        Ok(home.map_or(Value::Nil, |dir| {
            Value::Str(dir.to_string_lossy().into_owned().into())
        }))
    });
    m.function("pick_folder", |eng: &Engine, ()| {
        let picked = pick_folder();
        // What the reader picked is theirs to hand over, so `fs.*` may read
        // it: Import project looks inside for the manifest.
        if let Some(folder) = &picked {
            balaur::file_api::add_root(eng, folder);
        }
        let state = eng.resource::<ProjectState>();
        state.borrow_mut().picked.clone_from(&picked);
        Ok(picked.map_or(Value::Nil, Value::text))
    });
}

/// The same words for a time this machine wrote down itself.
pub(crate) fn said_ago(seconds: i64) -> String {
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if minutes < 2 {
        "just now".to_string()
    } else if hours < 1 {
        format!("{minutes} minutes ago")
    } else if hours < 2 {
        "an hour ago".to_string()
    } else if days < 1 {
        format!("{hours} hours ago")
    } else if days < 2 {
        "yesterday".to_string()
    } else if days < 30 {
        format!("{days} days ago")
    } else if days < 60 {
        "last month".to_string()
    } else {
        format!("{} months ago", days / 30)
    }
}

/// Where the example projects are, looked for the way the library is: an
/// install ships them beside the binary, a macOS bundle under `Resources`, and
/// a checkout holds them at the repository root.
fn examples_dir() -> Option<PathBuf> {
    crate::new_project::data_dirs()
        .into_iter()
        .map(|dir| dir.join("examples"))
        .find(|examples| examples.join("hello/project.toml").is_file())
}

/// The line under each example's name, from the editor's own library rather
/// than from the example, which has nowhere in its manifest to say it.
fn example_notes() -> toml::Table {
    let Some(library) = crate::new_project::library_dir() else {
        return toml::Table::new();
    };
    std::fs::read_to_string(library.join("examples.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .unwrap_or_default()
}

fn examples() -> Vec<Value> {
    let Some(dir) = examples_dir() else {
        return Vec::new();
    };
    let notes = example_notes();
    let library = crate::new_project::library_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut rows: Vec<(String, Value)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.join("project.toml").is_file() {
                return None;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let note = notes
                .get(&id)
                .and_then(|table| table.get("note"))
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_string();
            // The picture is the editor's own file, since that is the project
            // `ui.image` reads from; a missing one draws the name alone.
            let cover = library
                .as_ref()
                .filter(|lib| lib.join("examples").join(format!("{id}.png")).is_file())
                .map(|_| format!("library/examples/{id}.png"))
                .unwrap_or_default();
            let row = Value::Map(vec![
                ("id".into(), Value::Str(id.clone())),
                ("name".into(), Value::Str(name_of(&path).into())),
                ("note".into(), Value::Str(note.into())),
                ("cover".into(), Value::Str(cover.into())),
                (
                    "path".into(),
                    Value::Str(path.to_string_lossy().into_owned().into()),
                ),
            ]);
            Some((id, row))
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// Copy an example into the reader's own folder. The shipped one is read
/// only in an install and is the engine's in a checkout; either way it is not
/// the copy somebody is about to edit.
fn copy_example(home: &Path, id: &str, into: &Path) -> Value {
    let Some(dir) = examples_dir() else {
        return Value::Map(vec![(
            "error".into(),
            Value::Str("no examples ship beside this build".into()),
        )]);
    };
    let from = dir.join(id);
    if !from.join("project.toml").is_file() {
        return Value::Map(vec![(
            "error".into(),
            Value::text(format!("no example named {id}")),
        )]);
    }
    let to = free_name(into, id);
    if let Err(e) = copy_tree(&from, &to) {
        return Value::Map(vec![("error".into(), Value::text(format!("{e:#}")))]);
    }
    let name = name_of(&to);
    remember(home, &to, &name);
    Value::Map(vec![
        (
            "path".into(),
            Value::Str(to.to_string_lossy().into_owned().into()),
        ),
        ("name".into(), Value::Str(name.into())),
    ])
}

/// `hello`, then `hello-2`: a second copy of an example is a second project,
/// not an overwrite of the first.
fn free_name(into: &Path, id: &str) -> PathBuf {
    let first = into.join(id);
    if !first.exists() {
        return first;
    }
    for n in 2..100 {
        let next = into.join(format!("{id}-{n}"));
        if !next.exists() {
            return next;
        }
    }
    first
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// One remembered project.
struct Row {
    path: String,
    name: String,
    opened: i64,
    /// The engine that last opened it, so a row says what it was edited with.
    version: String,
}

/// The reader's own home directory, where a new project goes by default.
#[cfg(not(target_family = "wasm"))]
fn user_home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// A tab has no home directory, so the editor's writable root stands in.
#[cfg(target_family = "wasm")]
fn user_home() -> Option<PathBuf> {
    None
}

fn home_of(eng: &Engine) -> PathBuf {
    let state = eng.resource::<ProjectState>();
    let home = state.borrow().home.clone();
    if home.as_os_str().is_empty() {
        return balaur_core::engine_api::user_data_dir_of(eng);
    }
    home
}

fn list_path(home: &Path) -> PathBuf {
    home.join("recent.toml")
}

/// The list as it is on disk, newest first. A file that will not parse is a
/// list nobody has yet, which is what a first run has.
fn read_list(home: &Path) -> Vec<Row> {
    let Ok(text) = std::fs::read_to_string(list_path(home)) else {
        return Vec::new();
    };
    let Ok(doc) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let mut rows: Vec<Row> = doc
        .get("project")
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let path = item.get("path")?.as_str()?.to_string();
                    Some(Row {
                        name: item
                            .get("name")
                            .and_then(toml::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        opened: item
                            .get("opened")
                            .and_then(toml::Value::as_integer)
                            .unwrap_or_default(),
                        version: item
                            .get("version")
                            .and_then(toml::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        path,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by_key(|row| std::cmp::Reverse(row.opened));
    rows
}

fn write_list(home: &Path, rows: &[Row]) {
    let items: Vec<toml::Value> = rows
        .iter()
        .take(KEPT)
        .map(|row| {
            let mut table = toml::Table::new();
            table.insert("path".into(), toml::Value::String(row.path.clone()));
            table.insert("name".into(), toml::Value::String(row.name.clone()));
            table.insert("opened".into(), toml::Value::Integer(row.opened));
            table.insert("version".into(), toml::Value::String(row.version.clone()));
            toml::Value::Table(table)
        })
        .collect();
    let mut doc = toml::Table::new();
    doc.insert("project".into(), toml::Value::Array(items));
    let _ = std::fs::create_dir_all(home);
    let _ = std::fs::write(
        list_path(home),
        toml::to_string_pretty(&doc).unwrap_or_default(),
    );
}

#[allow(
    clippy::disallowed_methods,
    reason = "orders a list a person reads, not simulation"
)]
fn row_value(row: Row) -> Value {
    let exists = Path::new(&row.path).join("project.toml").is_file();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().cast_signed());
    Value::Map(vec![
        ("path".into(), Value::Str(row.path.into())),
        ("name".into(), Value::Str(row.name.into())),
        ("opened".into(), Value::Num(row.opened as f64)),
        ("when".into(), Value::Str(said_ago(now - row.opened).into())),
        ("version".into(), Value::Str(row.version.into())),
        ("exists".into(), Value::Bool(exists)),
    ])
}

/// Put a project at the head of the list, with this moment as its time.
#[allow(
    clippy::disallowed_methods,
    reason = "orders a list a person reads, not simulation"
)]
fn remember(home: &Path, project: &Path, name: &str) {
    let path = project.to_string_lossy().into_owned();
    let mut rows = read_list(home);
    rows.retain(|row| row.path != path);
    let opened = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().cast_signed());
    rows.insert(
        0,
        Row {
            path,
            name: name.to_string(),
            opened,
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    );
    write_list(home, &rows);
}

/// A project's own name, which is what its manifest says rather than what its
/// folder is called.
fn name_of(project: &Path) -> String {
    let fallback = || {
        project
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    let Ok(text) = std::fs::read_to_string(project.join("project.toml")) else {
        return fallback();
    };
    text.parse::<toml::Table>()
        .ok()
        .and_then(|doc| {
            doc.get("application")?
                .get("name")?
                .as_str()
                .map(str::to_string)
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(fallback)
}

fn templates() -> Vec<Value> {
    let Some(library) = crate::new_project::library_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(library.join("templates")) else {
        return Vec::new();
    };
    let mut rows: Vec<(String, String)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let note = name_of(&entry.path());
            Some((id, note))
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter()
        .map(|(id, note)| {
            Value::Map(vec![
                ("id".into(), Value::Str(id.into())),
                ("note".into(), Value::Str(note.into())),
            ])
        })
        .collect()
}

fn create(home: &Path, path: &Path, template: &str) -> Value {
    let template = (!template.is_empty()).then_some(template);
    if let Err(e) = crate::new_project::create(path, template) {
        return Value::Map(vec![("error".into(), Value::text(format!("{e:#}")))]);
    }
    let name = name_of(path);
    remember(home, path, &name);
    Value::Map(vec![
        (
            "path".into(),
            Value::Str(path.to_string_lossy().into_owned().into()),
        ),
        ("name".into(), Value::Str(name.into())),
    ])
}

/// The projects to offer, newest first: a desktop reads the list it wrote,
/// and a tab the stores this browser keeps.
#[cfg(not(target_family = "wasm"))]
fn recent_rows(eng: &Engine) -> Vec<Value> {
    read_list(&home_of(eng))
        .into_iter()
        .map(row_value)
        .collect()
}

#[cfg(target_family = "wasm")]
fn recent_rows(_: &Engine) -> Vec<Value> {
    crate::project_web::recent()
}

/// Drop a project from the list, leaving its files alone. In a tab the store
/// *is* the project, so forgetting one is deleting it.
#[cfg(not(target_family = "wasm"))]
fn forget(eng: &Engine, path: &Path) {
    let home = home_of(eng);
    let gone = path.to_string_lossy();
    let mut rows = read_list(&home);
    rows.retain(|row| row.path != gone);
    write_list(&home, &rows);
}

#[cfg(target_family = "wasm")]
fn forget(_: &Engine, path: &Path) {
    crate::project_web::forget(path);
}

/// Open a project in a tab: a request to the page, since one editor run is
/// one project and a tab cannot start a second.
#[cfg(target_family = "wasm")]
fn open(_: &Engine, _home: &Path, path: &Path) -> Value {
    crate::project_web::open(path)
}

#[cfg(not(target_family = "wasm"))]
fn open(eng: &Engine, home: &Path, path: &Path) -> Value {
    if !path.join("project.toml").is_file() {
        return Value::Map(vec![(
            "error".into(),
            Value::text(format!("no project.toml in {}", path.display())),
        )]);
    }
    remember(home, path, &name_of(path));
    match relaunch(path) {
        Ok(()) => {
            eng.request_quit();
            Value::Map(Vec::new())
        }
        Err(e) => Value::Map(vec![("error".into(), Value::text(format!("{e:#}")))]),
    }
}

/// Start this same binary on another project. One process is one project:
/// the export and import modules and the file root are built for the project
/// the editor booted with, so the way to another is a new process.
#[cfg(not(target_family = "wasm"))]
fn relaunch(path: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("edit")
        .arg(path)
        .spawn()?;
    Ok(())
}

#[cfg(target_family = "wasm")]
fn relaunch(_path: &Path) -> Result<()> {
    anyhow::bail!("a tab opens a project through the page, not by starting a second editor")
}

/// The OS picker, where there is one: a desktop with a window. Blocking on
/// purpose: a native dialog owns the screen while it is up, and the editor
/// has nothing to draw behind it that a reader could act on.
#[cfg(all(desktop, feature = "window"))]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(all(desktop, feature = "window")))]
fn pick_folder() -> Option<String> {
    None
}
