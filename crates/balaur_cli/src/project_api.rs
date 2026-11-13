//! `project.*` for the editor: the projects this machine has opened, the
//! templates it can start one from, and the folder picker its OS provides.
//!
//! The list is a file of paths, not a registry: written when a project is
//! opened or created, read back newest first, and never scanned for. A row
//! whose folder is gone stays until somebody forgets it, because a missing
//! project is usually an unplugged disk rather than a deleted game.

use std::path::{Path, PathBuf};

use crate::vocabulary::keys;
use anyhow::Result;
use balaur::{Engine, Stage};
use balaur_script::{Module, Value};

const COPY: &str = "copy";
const COPY_SYSTEM_FONT: &str = "copy_system_font";
const CREATE: &str = "create";
const EXAMPLES: &str = "examples";
const EXAMPLES_DIRECTORY: &str = "examples_directory";
const EXISTS: &str = "exists";
const FORGET: &str = "forget";
const HOME: &str = "home";
const IN_TAB: &str = "in_tab";
const OPEN: &str = "open";
const OPEN_BESIDE: &str = "open_beside";
const PICK_DIRECTORY: &str = "pick_directory";
const RECENT: &str = "recent";
const SYSTEM_FONTS: &str = "system_fonts";
const USE_DATA: &str = "use_data";

mod forge;
mod picture;
mod release;

/// How many projects the list keeps. Past this the oldest falls off: the
/// list is for going back to last week's work, not for an archive.
const KEPT: usize = 20;

/// Where the list lives, and what a pick answered.
pub(crate) struct ProjectState {
    /// The per-user folder the editor shares across projects, named once at
    /// start-up because it is derived from the editor's own manifest.
    home: PathBuf,
}

pub(crate) struct ProjectPlugin {}

impl ProjectPlugin {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl balaur_plugin::Plugin for ProjectPlugin {
    fn name(&self) -> &'static str {
        keys::PROJECT
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(ProjectState {
            home: PathBuf::new(),
        });
        // The data directory is derived from the loaded manifest, which is
        // the editor's own by the time the first frame runs.
        reg.add_system(Stage::First, |eng, _| {
            let state = eng.resource::<ProjectState>();
            let mut state = state.borrow_mut();
            if state.home.as_os_str().is_empty() {
                state.home = shared_dir(eng);
            }
        });
        let mut m = reg.script_module(keys::PROJECT)?;
        install_project_api(&mut m);
        install_project_verbs(&mut m);
        install_font_verbs(&mut m);
        drop(m);
        picture::declare(reg)?;
        release::declare(reg)?;
        forge::declare(reg)
    }
}

fn install_project_api(m: &mut Module<Engine>) {
    m.module_doc(
        "The projects this machine has opened, the templates a new one starts from, and the folder picker the OS provides. What the editor's start screen is made of.",
    );
    project_records(m);
    describe_project_api(m);
}

/// The tables the verbs hand back, so a typed script reads them by field.
fn project_records(m: &mut Module<Engine>) {
    m.records(&[
        (
            "Recent",
            "{ path: string, name: string, opened: float, version: string, exists: bool }",
            "A project this machine has opened.",
        ),
        (
            "Project",
            "{ path: string?, name: string?, error: string? }",
            "A project just written to disk, or the error that stopped it.",
        ),
        (
            "Opened",
            "{ error: string? }",
            "What opening a project answers: an error, or nothing when the editor is on its way to it.",
        ),
        (
            "SystemFont",
            "{ family: string, style: string, path: string }",
            "A face the operating system has installed: its family, its weight and slant in words, and its file.",
        ),
    ]);
}

/// What each verb is, for the reference and the editor's hover.
fn describe_project_api(m: &mut Module<Engine>) {
    m.describe(&[
        (
            RECENT,
            &[],
            "-> [Recent]",
            "The projects opened on this machine, newest first: `{ path, name, opened, exists }` each, where `opened` is a Unix time in seconds and `exists` says whether the folder is still there.",
        ),
        (
            CREATE,
            &[],
            "(path: string, template: string) -> Project",
            "Write a new project at `path` from a template id, or from nothing when the id is empty. Answers `{ path, name }`, or `{ error }`.",
        ),
        (
            OPEN,
            &[],
            "(path: string) -> Opened",
            "Remember `path` and start the editor on it, then ask this one to quit. Answers `{ error }` when there is no project there.",
        ),
        (
            OPEN_BESIDE,
            &[],
            "(path: string) -> Opened",
            "Start a second editor on `path` and keep this one: Edit dependency on a `path` dependency, whose project is another folder. Answers `{ error }` when there is no project there or no second editor can start.",
        ),
        (
            FORGET,
            &[],
            "(path: string)",
            "Drop one project from the list. The folder is not touched.",
        ),
        (
            HOME,
            &[],
            "-> string",
            "Where a new project goes unless the reader says otherwise: the home directory on a desktop, and the app's own writable directory where there is no such thing.",
        ),
        (
            IN_TAB,
            &[],
            "-> bool",
            "Whether this editor runs in a browser tab, where the page holds the projects and opening one is its call rather than this screen's.",
        ),
        (
            keys::VERSION,
            &[],
            "-> string",
            "The version of the binary the editor is running in.",
        ),
        (
            EXAMPLES_DIRECTORY,
            &[],
            "-> string?",
            "The folder of example projects shipped beside the editor, which `fs.*` may read and not write from then on. Nil where none ship.",
        ),
        (
            COPY,
            &[],
            "(from: string, into: string) -> Project",
            "Copy the project folder at `from`, one `fs.*` may read, to `into`, which must not exist yet, and remember the copy. Answers `{ path, name }`, or `{ error }`.",
        ),
        (
            EXISTS,
            &[],
            "(path: string) -> bool",
            "Whether anything is at `path`, wherever it is: how a new project's folder is picked clear of one already there.",
        ),
        (
            PICK_DIRECTORY,
            &[],
            "-> string?",
            "Open the OS folder picker and answer what was chosen, or `()` when it was dismissed. Blocks while the dialog is up, and answers `()` on a platform with no picker.",
        ),
        (
            SYSTEM_FONTS,
            &[],
            "-> [SystemFont]",
            "The faces this machine has installed, by family and style: what the font picker offers to copy into a project, since an exported game cannot count on a face being installed. Empty in a browser tab.",
        ),
        (
            COPY_SYSTEM_FONT,
            &[],
            "(path: string, to: string) -> bool",
            "Copy an installed face, one `system_fonts` lists, to `to` inside the project. Answers whether it was copied; a path the list does not name is refused.",
        ),
        (
            USE_DATA,
            &[],
            "(name: string?) -> string?",
            "Answer `engine::user_data_directory()` with the user data directory of the game named `name`, keep the device id there, and let `fs` reach it, so a game played here and run alone share saves and a device login. The editor names its own folder with `engine::user_data_directory_of`. Nil goes back to the editor's own. Answers the directory, or nil.",
        ),
    ]);
}

/// The faces this machine has installed, and copying one into a project.
fn install_font_verbs(m: &mut Module<Engine>) {
    m.function(SYSTEM_FONTS, |_: &Engine, ()| {
        Ok(Value::List(
            balaur_text::fonts::installed_faces()
                .into_iter()
                .map(|face| {
                    Value::Map(vec![
                        ("family".into(), Value::text(face.family)),
                        ("style".into(), Value::text(face.style)),
                        (keys::PATH.into(), Value::text(face.path.to_string_lossy())),
                    ])
                })
                .collect(),
        ))
    });
    // Only a face the list names: a script reading any file it names off the
    // disk is what `fs`'s roots are there to stop.
    m.function(
        COPY_SYSTEM_FONT,
        |eng: &Engine, (path, to): (String, String)| {
            let installed = balaur_text::fonts::installed_faces()
                .into_iter()
                .any(|face| face.path == std::path::Path::new(&path));
            if !installed {
                return Ok(false);
            }
            let to = balaur::file_api::resolve(eng, &to)?;
            let bytes = std::fs::read(&path)?; // os files: an installed face, checked above
            balaur_core::files::backend(eng).write(&to, &bytes)?;
            Ok(true)
        },
    );
}

fn install_project_verbs(m: &mut Module<Engine>) {
    m.function(RECENT, |eng: &Engine, ()| Ok(Value::List(recent_rows(eng))));
    m.function(
        CREATE,
        |eng: &Engine, (path, template): (String, String)| {
            let home = home_of(eng);
            Ok(create(&home, Path::new(&path), &template))
        },
    );
    m.function(OPEN, |eng: &Engine, path: String| {
        let home = home_of(eng);
        Ok(open(eng, &home, Path::new(&path)))
    });
    m.function(OPEN_BESIDE, |_: &Engine, path: String| {
        let path = Path::new(&path);
        let error = if path.join("project.toml").is_file() {
            relaunch(path).err().map(|e| format!("{e:#}"))
        } else {
            Some(format!("no project.toml in {}", path.display()))
        };
        Ok(Value::Map(
            error
                .map(|e| vec![(keys::ERROR.into(), Value::text(e))])
                .unwrap_or_default(),
        ))
    });
    m.function(FORGET, |eng: &Engine, path: String| {
        forget(eng, Path::new(&path));
        Ok(Value::Nil)
    });
    m.function(EXAMPLES_DIRECTORY, |eng: &Engine, ()| {
        Ok(examples_dir().map_or(Value::Nil, |dir| {
            balaur::file_api::add_read_only_root(eng, &dir);
            Value::Str(dir.to_string_lossy().into_owned())
        }))
    });
    m.function(COPY, |eng: &Engine, (from, into): (String, String)| {
        let home = home_of(eng);
        Ok(copy(eng, &home, &from, Path::new(&into)))
    });
    m.function(EXISTS, |_: &Engine, path: String| {
        Ok(Value::Bool(Path::new(&path).exists()))
    });
    m.function(HOME, |eng: &Engine, ()| {
        Ok(Value::Str(
            user_home()
                .unwrap_or_else(|| home_of(eng))
                .to_string_lossy()
                .into_owned(),
        ))
    });
    m.function(IN_TAB, |_: &Engine, ()| {
        Ok(Value::Bool(cfg!(target_family = "wasm")))
    });
    m.function(keys::VERSION, |_: &Engine, ()| {
        Ok(Value::Str(crate::version::long().to_string()))
    });
    // A name, never a path: the directory stays under the user data base
    // whatever a script passes.
    m.function(USE_DATA, |eng: &Engine, name: Option<String>| {
        let home = name.filter(|name| !name.is_empty()).map(|name| {
            let dir = balaur::engine_api::user_data_dir_named(eng, &name);
            balaur::file_api::add_root(eng, &dir);
            dir
        });
        balaur::engine_api::set_data_home(eng, home.clone());
        balaur::facts::reread(eng);
        Ok(home.map_or(Value::Nil, |dir| {
            Value::Str(dir.to_string_lossy().into_owned())
        }))
    });
    m.function(PICK_DIRECTORY, |eng: &Engine, ()| {
        let picked = pick_folder();
        // What the reader picked is theirs to hand over, so `fs.*` may read
        // it: Import project looks inside for the manifest.
        if let Some(folder) = &picked {
            balaur::file_api::add_root(eng, folder);
        }
        Ok(picked.map_or(Value::Nil, Value::text))
    });
}

/// Where the example projects are, looked for the way the library is: an
/// install ships them beside the binary, a macOS bundle under `Resources`, and
/// a checkout holds them at the repository root.
fn examples_dir() -> Option<PathBuf> {
    crate::new_project::data_dirs()
        .into_iter()
        .map(|dir| dir.join(EXAMPLES))
        .find(|examples| examples.join("hello/project.toml").is_file())
}

/// A copy of the project at `from` at `into`, remembered: what the start
/// screen opens, so a shipped example stays as it is.
fn copy(eng: &Engine, home: &Path, from: &str, into: &Path) -> Value {
    let failed = |why: String| Value::Map(vec![(keys::ERROR.into(), Value::text(why))]);
    let from = match balaur::file_api::resolve(eng, from) {
        Ok(path) => path,
        Err(e) => return failed(format!("{e:#}")),
    };
    if !from.join("project.toml").is_file() {
        return failed(format!("no project.toml in {}", from.display()));
    }
    if into.exists() {
        return failed(format!("{} is there already", into.display()));
    }
    if let Err(e) = copy_tree(&from, into) {
        return failed(format!("{e:#}"));
    }
    let name = name_of(into);
    remember(home, into, &name);
    Value::Map(vec![
        (
            keys::PATH.into(),
            Value::Str(into.to_string_lossy().into_owned()),
        ),
        (keys::NAME.into(), Value::Str(name)),
    ])
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
        return shared_dir(eng);
    }
    home
}

/// The editor's per-user folder: its own user data directory, which sits
/// beside the games' rather than among them.
fn shared_dir(eng: &Engine) -> PathBuf {
    balaur_core::engine_api::user_data_dir_of(eng)
}

fn list_path(home: &Path) -> PathBuf {
    home.join("projects.toml")
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
        .get(keys::PROJECT)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let path = item.get(keys::PATH)?.as_str()?.to_string();
                    Some(Row {
                        name: item
                            .get(keys::NAME)
                            .and_then(toml::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        opened: item
                            .get(keys::OPENED)
                            .and_then(toml::Value::as_integer)
                            .unwrap_or_default(),
                        version: item
                            .get(keys::VERSION)
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
            table.insert(keys::PATH.into(), toml::Value::String(row.path.clone()));
            table.insert(keys::NAME.into(), toml::Value::String(row.name.clone()));
            table.insert(keys::OPENED.into(), toml::Value::Integer(row.opened));
            table.insert(
                keys::VERSION.into(),
                toml::Value::String(row.version.clone()),
            );
            toml::Value::Table(table)
        })
        .collect();
    let mut doc = toml::Table::new();
    doc.insert(keys::PROJECT.into(), toml::Value::Array(items));
    let _ = std::fs::create_dir_all(home);
    let _ = std::fs::write(
        list_path(home),
        toml::to_string_pretty(&doc).unwrap_or_default(),
    );
}

fn row_value(row: Row) -> Value {
    let exists = Path::new(&row.path).join("project.toml").is_file();
    Value::Map(vec![
        (keys::PATH.into(), Value::Str(row.path)),
        (keys::NAME.into(), Value::Str(row.name)),
        (keys::OPENED.into(), Value::Num(row.opened as f64)),
        (keys::VERSION.into(), Value::Str(row.version)),
        (EXISTS.into(), Value::Bool(exists)),
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
            doc.get(keys::APPLICATION)?
                .get(keys::NAME)?
                .as_str()
                .map(str::to_string)
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(fallback)
}

fn create(home: &Path, path: &Path, template: &str) -> Value {
    let template = (!template.is_empty()).then_some(template);
    if let Err(e) = crate::new_project::create(path, template, None) {
        return Value::Map(vec![(keys::ERROR.into(), Value::text(format!("{e:#}")))]);
    }
    let name = name_of(path);
    remember(home, path, &name);
    Value::Map(vec![
        (
            keys::PATH.into(),
            Value::Str(path.to_string_lossy().into_owned()),
        ),
        (keys::NAME.into(), Value::Str(name)),
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
            keys::ERROR.into(),
            Value::text(format!("no project.toml in {}", path.display())),
        )]);
    }
    remember(home, path, &name_of(path));
    match relaunch(path) {
        Ok(()) => {
            eng.request_quit();
            Value::Map(Vec::new())
        }
        Err(e) => Value::Map(vec![(keys::ERROR.into(), Value::text(format!("{e:#}")))]),
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
