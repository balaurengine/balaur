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
        let mut v = reg.script_module("release")?;
        install_release_api(&mut *v);
        Ok(())
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
    ]);
}

fn install_project_verbs(m: &mut dyn Bindings<Engine>) {
    m.function("recent", |eng: &Engine, ()| {
        let home = home_of(eng);
        Ok(Value::List(
            read_list(&home).into_iter().map(row_value).collect(),
        ))
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
        let home = home_of(eng);
        let mut rows = read_list(&home);
        rows.retain(|row| row.path != path);
        write_list(&home, &rows);
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
            dirs::home_dir()
                .unwrap_or_else(|| home_of(eng))
                .to_string_lossy()
                .into_owned(),
        ))
    });
    m.function("version", |_: &Engine, ()| {
        Ok(Value::Str(crate::version::long().to_string()))
    });
    m.function("pick_folder", |eng: &Engine, ()| {
        let picked = pick_folder();
        let state = eng.resource::<ProjectState>();
        state.borrow_mut().picked.clone_from(&picked);
        Ok(picked.map_or(Value::Nil, Value::Str))
    });
}

/// `release.*`: which build this is, what the channels hold, and replacing
/// this install with one of them. The same code `balaur update` runs, so the
/// screen and the command cannot disagree.
fn install_release_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Which build of the engine this is, what the channels are publishing, and replacing this install with another. The editor's engine screen; `balaur update` is the same code.",
    );
    m.describe(&[
        (
            "installed",
            &[],
            "",
            "This build: `{ version, id, channel, tag, source }`, where `id` is the build id a release was tagged with, `tag` is the release its assets live under, and `source` is true for a build from a checkout.",
        ),
        (
            "channels",
            &[],
            "",
            "Every release line a build may follow, in the order a version moves through them.",
        ),
        (
            "releases",
            &[],
            "",
            "Every release the project has published, newest first: `{ tag, id, channel, when, current }` each, where `when` is how long ago it was published. Reads the network and blocks while it does. Answers `{ error }` as its one row when the feed could not be read.",
        ),
        (
            "install",
            &[],
            "(channel: string, tag: string, allow_downgrade: bool)",
            "Replace this install with what that channel or tag holds, and answer `{ note }` when it worked. Downloads while it blocks, and refuses inside a macOS bundle, which updates by its own download.",
        ),
    ]);
    m.function("installed", |_: &Engine, ()| {
        let id = crate::version::build_id();
        Ok(Value::Map(vec![
            (
                "version".into(),
                Value::Str(env!("CARGO_PKG_VERSION").to_string()),
            ),
            ("id".into(), Value::Str(id.unwrap_or_default().to_string())),
            (
                "channel".into(),
                Value::Str(crate::version::channel().unwrap_or_default().to_string()),
            ),
            (
                "tag".into(),
                Value::Str(
                    crate::version::release_tag()
                        .unwrap_or_default()
                        .to_string(),
                ),
            ),
            ("source".into(), Value::Bool(id.is_none())),
        ]))
    });
    m.function("channels", |_: &Engine, ()| {
        Ok(Value::List(
            crate::version::CHANNELS
                .iter()
                .map(|name| Value::Str((*name).to_string()))
                .collect(),
        ))
    });
    m.function("releases", |_: &Engine, ()| {
        Ok(Value::List(match crate::update::releases() {
            Ok(found) => found
                .into_iter()
                .map(|release| {
                    Value::Map(vec![
                        (
                            "current".into(),
                            Value::Bool(
                                crate::version::build_id().is_some_and(|own| own == release.id),
                            ),
                        ),
                        ("tag".into(), Value::Str(release.tag)),
                        ("id".into(), Value::Str(release.id)),
                        ("channel".into(), Value::Str(release.channel)),
                        ("when".into(), Value::Str(ago(&release.published))),
                    ])
                })
                .collect(),
            Err(e) => vec![Value::Map(vec![(
                "error".into(),
                Value::Str(format!("{e:#}")),
            )])],
        }))
    });
    m.function(
        "install",
        |_: &Engine, (channel, tag, allow_downgrade): (String, String, bool)| {
            let (tag, channel) = asked(&tag, &channel);
            Ok(
                match crate::update::install(tag, channel, allow_downgrade) {
                    Ok(note) => Value::Map(vec![("note".into(), Value::Str(note))]),
                    Err(e) => Value::Map(vec![("error".into(), Value::Str(format!("{e:#}")))]),
                },
            )
        },
    );
}

/// How long ago, in the words a person uses. The clock is the machine's,
/// which is what "yesterday" is measured against.
#[allow(
    clippy::disallowed_methods,
    reason = "orders a list a person reads, not simulation"
)]
fn ago(published: &str) -> String {
    let Some(then) = unix_of(published) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().cast_signed());
    said_ago(now - then)
}

/// The same words for a time this machine wrote down itself.
fn said_ago(seconds: i64) -> String {
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

/// An ISO 8601 stamp as Unix seconds. Only the shape the feed writes,
/// `2026-09-14T18:02:17Z`, because that is the only one it writes.
fn unix_of(stamp: &str) -> Option<i64> {
    let (date, rest) = stamp.split_once('T')?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let mut clock = rest.trim_end_matches('Z').split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    // Days since the epoch by the civil-from-days algorithm, which needs no
    // table and no leap-year special case past the one in it.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Where the example projects are: beside the binary in an install, and at
/// the repository root in a checkout, the way the library is found.
fn examples_dir() -> Option<PathBuf> {
    let here = std::env::current_exe().ok()?;
    for base in [here.parent()?.to_path_buf(), std::env::current_dir().ok()?] {
        let mut dir = Some(base);
        while let Some(at) = dir {
            let examples = at.join("examples");
            if examples.join("hello/project.toml").is_file() {
                return Some(examples);
            }
            dir = at.parent().map(Path::to_path_buf);
        }
    }
    None
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
                ("name".into(), Value::Str(name_of(&path))),
                ("note".into(), Value::Str(note)),
                ("cover".into(), Value::Str(cover)),
                (
                    "path".into(),
                    Value::Str(path.to_string_lossy().into_owned()),
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
            Value::Str(format!("no example named {id}")),
        )]);
    }
    let to = free_name(into, id);
    if let Err(e) = copy_tree(&from, &to) {
        return Value::Map(vec![("error".into(), Value::Str(format!("{e:#}")))]);
    }
    let name = name_of(&to);
    remember(home, &to, &name);
    Value::Map(vec![
        ("path".into(), Value::Str(to.to_string_lossy().into_owned())),
        ("name".into(), Value::Str(name)),
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

/// An empty string is "not asked for": a script passes both and names one.
fn asked<'a>(tag: &'a str, channel: &'a str) -> (Option<&'a str>, Option<&'a str>) {
    (
        (!tag.is_empty()).then_some(tag),
        (!channel.is_empty()).then_some(channel),
    )
}

/// One remembered project.
struct Row {
    path: String,
    name: String,
    opened: i64,
    /// The engine that last opened it, so a row says what it was edited with.
    version: String,
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
        ("path".into(), Value::Str(row.path)),
        ("name".into(), Value::Str(row.name)),
        ("opened".into(), Value::Num(row.opened as f64)),
        ("when".into(), Value::Str(said_ago(now - row.opened))),
        ("version".into(), Value::Str(row.version)),
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
                ("id".into(), Value::Str(id)),
                ("note".into(), Value::Str(note)),
            ])
        })
        .collect()
}

fn create(home: &Path, path: &Path, template: &str) -> Value {
    let template = (!template.is_empty()).then_some(template);
    if let Err(e) = crate::new_project::create(path, template) {
        return Value::Map(vec![("error".into(), Value::Str(format!("{e:#}")))]);
    }
    let name = name_of(path);
    remember(home, path, &name);
    Value::Map(vec![
        (
            "path".into(),
            Value::Str(path.to_string_lossy().into_owned()),
        ),
        ("name".into(), Value::Str(name)),
    ])
}

fn open(eng: &Engine, home: &Path, path: &Path) -> Value {
    if !path.join("project.toml").is_file() {
        return Value::Map(vec![(
            "error".into(),
            Value::Str(format!("no project.toml in {}", path.display())),
        )]);
    }
    remember(home, path, &name_of(path));
    match relaunch(path) {
        Ok(()) => {
            eng.request_quit();
            Value::Map(Vec::new())
        }
        Err(e) => Value::Map(vec![("error".into(), Value::Str(format!("{e:#}")))]),
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

/// The OS picker, where there is one. Blocking on purpose: a native dialog
/// owns the screen while it is up, and the editor has nothing to draw behind
/// it that a reader could act on.
#[cfg(all(not(target_family = "wasm"), feature = "window"))]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(all(not(target_family = "wasm"), feature = "window")))]
fn pick_folder() -> Option<String> {
    None
}
