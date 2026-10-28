//! `forge.*`: projects kept on Forge as the editor's pickers see them, and
//! adding one to the open game as a dependency. The same code `balaur add`
//! and `balaur fetch` run, on a thread, reporting through [`crate::jobs`].

use anyhow::Result;
use balaur::{Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};
use serde::{Deserialize, Serialize};

use crate::jobs::{Reported, Reporting, install_listen, pump};

/// One report of a Forge job, crossing from the thread into a tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum ForgeEvent {
    Listed {
        rows: Vec<Row>,
    },
    /// A dependency added and fetched, or every one fetched (`alias` empty).
    Fetched {
        alias: String,
    },
    /// A project copied into a folder of its own.
    Copied {
        path: String,
    },
    /// `job` is `search`, `add` or `fetch`.
    Failed {
        job: String,
        message: String,
    },
}

/// A project as a picker shows it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Row {
    project: String,
    description: String,
    tags: Vec<String>,
    license: String,
}

impl Row {
    fn value(&self) -> Value {
        Value::Map(vec![
            ("project".into(), Value::Str(self.project.clone())),
            ("description".into(), Value::Str(self.description.clone())),
            (
                "tags".into(),
                Value::List(self.tags.iter().cloned().map(Value::Str).collect()),
            ),
            ("license".into(), Value::Str(self.license.clone())),
        ])
    }
}

impl Reported for ForgeEvent {
    fn value(&self) -> Value {
        let (kind, mut pairs): (&str, Vec<(String, Value)>) = match self {
            Self::Listed { rows } => (
                "listed",
                vec![(
                    "rows".into(),
                    Value::List(rows.iter().map(Row::value).collect()),
                )],
            ),
            Self::Fetched { alias } => {
                ("fetched", vec![("alias".into(), Value::Str(alias.clone()))])
            }
            Self::Copied { path } => ("copied", vec![("path".into(), Value::Str(path.clone()))]),
            Self::Failed { job, message } => (
                "failed",
                vec![
                    ("job".into(), Value::Str(job.clone())),
                    ("message".into(), Value::Str(message.clone())),
                ],
            ),
        };
        pairs.insert(0, ("kind".into(), Value::Str(kind.into())));
        Value::Map(pairs)
    }
}

/// Who hears a Forge job, and the channel its threads report on.
pub(crate) struct ForgeState(Reporting<ForgeEvent>);

impl AsMut<Reporting<ForgeEvent>> for ForgeState {
    fn as_mut(&mut self) -> &mut Reporting<ForgeEvent> {
        &mut self.0
    }
}

pub(super) fn declare(reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
    reg.insert_resource(ForgeState(Reporting::new(std::path::PathBuf::new())));
    reg.add_system(Stage::First, pump::<ForgeState, ForgeEvent>);
    let mut m = reg.script_module("forge")?;
    install_forge_api(&mut *m);
    Ok(())
}

fn install_forge_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Projects kept on Forge, for the editor's pickers, and adding one to a game as a dependency. `balaur add` and `balaur fetch` are the same code. `BALAUR_FORGE` names the Forge and `BALAUR_FORGE_TOKEN` reads private projects.",
    );
    m.describe(&[
        (
            "url",
            &[],
            "-> string",
            "The Forge this machine asks, without a trailing slash.",
        ),
        (
            "search",
            &[],
            "(query: string, tag: string) -> bool",
            "List public projects on a thread, either filter empty for none. `listen` hears `{ kind: \"listed\", rows }`, each `{ project, description, tags, license }` with `project` as `owner/name`, or `{ kind: \"failed\", job: \"search\", message }`. False while a recording plays.",
        ),
        (
            "add",
            &[],
            "(game: string, project: string) -> bool",
            "Add `owner/name` to the game's `[dependencies]` and fetch it into `deps/<alias>/`, on a thread. `listen` hears `{ kind: \"fetched\", alias }` or `failed` with `job: \"add\"`.",
        ),
        (
            "fetch",
            &[],
            "(game: string, update: bool) -> bool",
            "Fill the game's `deps/` from `project.lock`, or with `update` move each dependency to its newest commit, on a thread. `listen` hears `{ kind: \"fetched\", alias: \"\" }` or `failed` with `job: \"fetch\"`.",
        ),
        (
            "copy",
            &[],
            "(project: string, parent: string) -> bool",
            "Copy `owner/name` at its default branch into a new folder under `parent`, with its dependencies fetched, on a thread: the project manager's Open a copy. `listen` hears `{ kind: \"copied\", path }` or `failed` with `job: \"copy\"`.",
        ),
        (
            "listen",
            &[],
            "(node: node, opts: map?)",
            "Call a method on `node` with every report: `on_forge_event` unless `opts.on_event` names another.",
        ),
    ]);
    m.function("url", |_: &Engine, ()| Ok(Value::Str(url())));
    m.function("search", |eng: &Engine, (query, tag): (String, String)| {
        Ok(run(eng, "search", move || {
            Ok(ForgeEvent::Listed {
                rows: search(&query, &tag)?,
            })
        }))
    });
    m.function("add", |eng: &Engine, (game, project): (String, String)| {
        Ok(run(eng, "add", move || {
            let alias = add(&game, &project)?;
            Ok(ForgeEvent::Fetched { alias })
        }))
    });
    m.function("fetch", |eng: &Engine, (game, update): (String, bool)| {
        Ok(run(eng, "fetch", move || {
            fetch(&game, update)?;
            Ok(ForgeEvent::Fetched {
                alias: String::new(),
            })
        }))
    });
    m.function(
        "copy",
        |eng: &Engine, (project, parent): (String, String)| {
            Ok(run(eng, "copy", move || {
                Ok(ForgeEvent::Copied {
                    path: copy(&project, &parent)?,
                })
            }))
        },
    );
    install_listen::<ForgeState, ForgeEvent>(m, "on_forge_event");
}

/// `work` on a thread, its answer or its error reported; false while a
/// recording plays.
fn run(
    eng: &Engine,
    job: &'static str,
    work: impl FnOnce() -> Result<ForgeEvent> + Send + 'static,
) -> Value {
    let state = eng.resource::<ForgeState>();
    let state = state.borrow();
    Value::Bool(state.0.io.start(eng, |report| {
        let report = report.clone();
        let task = move || {
            let event = work().unwrap_or_else(|e| ForgeEvent::Failed {
                job: job.into(),
                message: format!("{e:#}"),
            });
            balaur_core::replay::report(&report, event);
        };
        #[cfg(not(target_family = "wasm"))]
        std::thread::spawn(task);
        #[cfg(target_family = "wasm")]
        task();
    }))
}

#[cfg(not(target_family = "wasm"))]
fn url() -> String {
    crate::deps_cmd::forge_url()
}

#[cfg(not(target_family = "wasm"))]
fn search(query: &str, tag: &str) -> Result<Vec<Row>> {
    Ok(crate::deps_cmd::search(query, tag)?
        .into_iter()
        .map(|found| Row {
            project: format!("{}/{}", found.owner, found.name),
            description: found.description.unwrap_or_default(),
            tags: found.tags,
            license: found.license.unwrap_or_default(),
        })
        .collect())
}

#[cfg(not(target_family = "wasm"))]
fn add(game: &str, project: &str) -> Result<String> {
    let before = declared(game);
    crate::deps_cmd::add(std::path::Path::new(game), project, None, None, None)?;
    Ok(declared(game)
        .into_iter()
        .find(|alias| !before.contains(alias))
        .unwrap_or_default())
}

#[cfg(not(target_family = "wasm"))]
fn declared(game: &str) -> Vec<String> {
    std::fs::read_to_string(std::path::Path::new(game).join("project.toml"))
        .ok()
        .and_then(|text| balaur::dependencies::declared(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|dep| dep.alias)
        .collect()
}

#[cfg(not(target_family = "wasm"))]
fn fetch(game: &str, update: bool) -> Result<()> {
    crate::deps_cmd::fetch(std::path::Path::new(game), update, None)
}

#[cfg(not(target_family = "wasm"))]
fn copy(project: &str, parent: &str) -> Result<String> {
    let folder = crate::deps_cmd::download(project, std::path::Path::new(parent))?;
    Ok(folder.to_string_lossy().into_owned())
}

// A tab reaches Forge through the page, and its project's dependencies
// arrive with the project from Forge's sync.
#[cfg(target_family = "wasm")]
fn url() -> String {
    String::new()
}

#[cfg(target_family = "wasm")]
fn search(_: &str, _: &str) -> Result<Vec<Row>> {
    anyhow::bail!("a tab lists Forge's projects on the site")
}

#[cfg(target_family = "wasm")]
fn add(_: &str, _: &str) -> Result<String> {
    anyhow::bail!("a tab adds a dependency from the project's page on Forge")
}

#[cfg(target_family = "wasm")]
fn fetch(_: &str, _: bool) -> Result<()> {
    anyhow::bail!("a tab's dependencies come with the project from Forge")
}

#[cfg(target_family = "wasm")]
fn copy(_: &str, _: &str) -> Result<String> {
    anyhow::bail!("a tab opens a Forge project from its page on Forge")
}
