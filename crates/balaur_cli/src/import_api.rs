//! `import.*` for the editor: bring a model, a sprite sheet or a level into
//! the edited project, through the same code `balaur import` runs.
//!
//! The verb lives in the CLI, which assembles the editor's plugins; the
//! importers are `balaur_import`, and the engine never reads a `.glb` off
//! disk. What the editor gets back is the list of project-relative files that
//! were written and, for a model or a level, the scene it can instantiate.
//!
//! `start` is the one to reach for: it reports a file at a time and leaves the
//! frame alone. `file` is the same import in one call, which is what a test
//! and a command want and what an editor cannot afford.
//!
//! A job reports through the `ExternalIo` in [`crate::jobs::Reporting`], so
//! what crossed into a tick rides in a recording and a replay hands a script
//! the same steps without importing anything twice.

use smol_str::SmolStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
// The job and what it reports on: both are what `import` brings, and a build
// without it has the verbs that answer for their absence and nothing else.
#[cfg(all(feature = "import", target_family = "wasm", target_feature = "atomics"))]
use std::sync::mpsc::Receiver;
#[cfg(feature = "import")]
use std::sync::mpsc::Sender;

use anyhow::Result;
use balaur::{Engine, Stage};
#[cfg(feature = "import")]
use balaur_core::task::{self, Progress, Stepped};
use balaur_script::{Bindings, BindingsExt, Value};
use serde::{Deserialize, Serialize};

use crate::jobs::{Reported, Reporting, install_listen, pump};

/// How many files one slice of an import writes before giving the frame back.
///
/// Files are wildly uneven -- a sidecar is a line and a texture is megabytes
/// -- so this is a count rather than a budget and is deliberately generous:
/// Sponza's 143 files are five slices, and a project's ten thousand are three
/// hundred, which is five seconds of a responsive editor rather than one long
/// stop.
const FILES_PER_SLICE: usize = 32;

/// One step of an import, crossing from the work back into a tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum ImportEvent {
    /// `files` is how many will be written, or zero where the importer walks
    /// a folder as it goes and cannot say yet.
    Started {
        source: String,
        files: usize,
    },
    /// One file got through: written, for an import that planned its outputs,
    /// and read, for a project walk, which counts the files it reads.
    Wrote {
        source: String,
        path: String,
        done: usize,
        files: usize,
    },
    Done {
        source: String,
        scene: Option<String>,
        note: String,
    },
    Failed {
        source: String,
        message: String,
    },
    /// Asked to stop. What it had written stays: the files are the import's
    /// output rather than a transaction it could roll back.
    Cancelled {
        source: String,
        done: usize,
    },
}

impl ImportEvent {
    fn kind(&self) -> &'static str {
        match self {
            Self::Started { .. } => "started",
            Self::Wrote { .. } => "wrote",
            Self::Done { .. } => "done",
            Self::Failed { .. } => "failed",
            Self::Cancelled { .. } => "cancelled",
        }
    }

    fn source(&self) -> &str {
        match self {
            Self::Started { source, .. }
            | Self::Wrote { source, .. }
            | Self::Done { source, .. }
            | Self::Failed { source, .. }
            | Self::Cancelled { source, .. } => source,
        }
    }
}

/// A count as a script reads one: an integer, so a comparison with a number
/// written in a script does not meet a float.
fn count(n: usize) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

impl Reported for ImportEvent {
    fn value(&self) -> Value {
        let mut pairs = vec![
            ("kind".into(), Value::Str(self.kind().into())),
            ("source".into(), Value::Str(self.source().into())),
        ];
        match self {
            Self::Started { files, .. } => {
                pairs.push(("files".into(), count(*files)));
            }
            Self::Wrote {
                path, done, files, ..
            } => {
                pairs.push(("path".into(), Value::Str(SmolStr::new(path))));
                pairs.push(("done".into(), count(*done)));
                pairs.push(("files".into(), count(*files)));
            }
            Self::Done { scene, note, .. } => {
                pairs.push((
                    "scene".into(),
                    scene.clone().map_or(Value::Nil, Value::text),
                ));
                pairs.push(("note".into(), Value::Str(SmolStr::new(note))));
            }
            Self::Failed { message, .. } => {
                pairs.push(("message".into(), Value::Str(SmolStr::new(message))));
            }
            Self::Cancelled { done, .. } => {
                pairs.push(("done".into(), count(*done)));
            }
        }
        Value::Map(pairs)
    }
}

/// How many imports are in flight, shared with the jobs that count themselves
/// off. Atomic, because on a desktop each job runs on a thread of its own.
pub(crate) type Running = Arc<AtomicUsize>;

/// The press that stops one, read by a job at the top of its next slice.
pub(crate) type Cancel = Arc<AtomicBool>;

/// The project a drop lands in, and the imports in flight in it.
pub(crate) struct ImportState {
    core: Reporting<ImportEvent>,
    /// Imports this plugin started and has not seen the end of. Counted here
    /// rather than asked of `task`, which counts every job in the process.
    running: Running,
    cancel: Cancel,
}

impl AsMut<Reporting<ImportEvent>> for ImportState {
    fn as_mut(&mut self) -> &mut Reporting<ImportEvent> {
        &mut self.core
    }
}

pub(crate) struct ImportPlugin {
    manifest: balaur_plugin::Manifest,
    project: PathBuf,
}

impl ImportPlugin {
    #[must_use]
    pub(crate) fn new(project: PathBuf) -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("import", env!("CARGO_PKG_VERSION")),
            project,
        }
    }
}

impl balaur_plugin::Plugin for ImportPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(ImportState {
            core: Reporting::new(self.project.clone()),
            running: Running::default(),
            cancel: Cancel::default(),
        });
        // What a job reported reaches a script here, ahead of every plugin's
        // own First work.
        reg.add_system(Stage::First, pump::<ImportState, ImportEvent>);
        let mut m = reg.script_module("import")?;
        install_import_api(&mut *m);
        Ok(())
    }
}

fn install_import_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Imports a `.glb` or `.gltf` model, an `.aseprite` sprite, or a `.tmx` or `.ldtk` level into the project being edited, with the importers `balaur import` runs.",
    );
    m.describe(&[
        (
            "handles",
            &[],
            "(path: string)",
            "Whether an importer claims this file, by extension.",
        ),
        (
            "file",
            &[],
            "(path: string)",
            "Import one file into the edited project, in one call. Answers `{ files, scene, note }`: the project-relative paths written, the scene to instantiate when there is one, and a line for the log. Answers `{ error }` when the import failed. A whole project is the work of seconds, so an editor keeping its frame wants `start`.",
        ),
        (
            "start",
            &[],
            "(path: string, project: string?)",
            "Import one file into the edited project, or into `project` when one is named, a few files per frame, reporting each to whatever `listen` named. Answers false while a recording plays. A model and a sprite are read first and then written a slice at a time, and a Godot project is walked a few files at a time; a level walks its own folder and takes one long slice, which says so in `files`.",
        ),
        (
            "running",
            &[],
            "()",
            "How many imports are in flight.",
        ),
        ("listen", &[], "(node: node, options: map)", LISTEN_DOC),
        (
            "choose",
            &[],
            "()",
            "Ask the reader for a file and import what they pick, reporting as `start` does. On a desktop that is the OS dialog; in a tab it is the page's own chooser, which takes several files at once so a `.gltf` can be picked with the images it names. Answers whether the asking began.",
        ),
        (
            "cancel",
            &[],
            "()",
            "Stop the imports in flight at the end of the file each is writing. What they had written stays where it landed; each reports `cancelled` with the count it reached.",
        ),
    ]);
    m.function("handles", |_: &Engine, path: String| {
        Ok(Value::Bool(handles(Path::new(&path))))
    });
    m.function("file", |eng: &Engine, path: String| {
        let project = eng.resource::<ImportState>().borrow().core.project.clone();
        Ok(import(Path::new(&path), &project))
    });
    m.function(
        "start",
        |eng: &Engine, (path, project): (String, Option<String>)| {
            Ok(Value::Bool(start(
                eng,
                Path::new(&path),
                project.map(PathBuf::from),
            )))
        },
    );
    m.function("choose", |eng: &Engine, ()| Ok(Value::Bool(choose(eng))));
    m.function("cancel", |eng: &Engine, ()| {
        eng.resource::<ImportState>()
            .borrow()
            .cancel
            .store(true, Ordering::Relaxed);
        Ok(Value::Nil)
    });
    m.function("running", |eng: &Engine, ()| {
        let running = eng
            .resource::<ImportState>()
            .borrow()
            .running
            .load(Ordering::Relaxed);
        Ok(count(running))
    });
    install_listen::<ImportState, ImportEvent>(m, "on_import");
}

/// What `listen` is documented as.
const LISTEN_DOC: &str = "Have the node's `on_import(event)`, or the `on_event` method the options name, called as an import starts, writes each file, finishes or fails.";

/// Ask for a file and import it. A desktop dialog answers with a path, so
/// this is the picker and `start`; a tab's chooser answers on an event, so
/// there it is `import_web`.
#[cfg(all(feature = "import", not(target_family = "wasm")))]
fn choose(eng: &Engine) -> bool {
    match pick() {
        Some(path) => start(eng, Path::new(&path), None),
        None => false,
    }
}

/// In a tab the chooser is an element and the import follows its event.
#[cfg(all(feature = "import", target_family = "wasm"))]
fn choose(eng: &Engine) -> bool {
    let state = eng.resource::<ImportState>();
    let (project, running, cancel) = {
        let state = state.borrow();
        (
            state.core.project.clone(),
            state.running.clone(),
            state.cancel.clone(),
        )
    };
    let mut began = false;
    state.borrow().core.io.start(eng, |report| {
        began = crate::import_web::choose(
            project.clone(),
            report.clone(),
            running.clone(),
            cancel.clone(),
        );
    });
    began
}

/// Without the importers there is nothing to ask for.
#[cfg(not(feature = "import"))]
fn choose(_: &Engine) -> bool {
    false
}

/// Start an import that reports as it goes, answering whether it started.
///
/// On a desktop the job takes a thread and reads and writes the disk there.
/// A tab keeps it under the tick, where its filesystem is; see [`task::step`].
#[cfg(feature = "import")]
fn start(eng: &Engine, file: &Path, into: Option<PathBuf>) -> bool {
    let state = eng.resource::<ImportState>();
    let (project, running, cancel) = {
        let state = state.borrow();
        (
            into.unwrap_or_else(|| state.core.project.clone()),
            state.running.clone(),
            state.cancel.clone(),
        )
    };
    let file = file.to_path_buf();
    state.borrow().core.io.start(eng, |report| {
        running.fetch_add(1, Ordering::Relaxed);
        task::step(ImportJob::new(
            Source::Beside(file.clone()),
            project.clone(),
            report.clone(),
            running.clone(),
            cancel.clone(),
        ));
    })
}

/// A tab has none of the importers, so there is nothing to start.
#[cfg(not(feature = "import"))]
fn start(_: &Engine, file: &Path, into: Option<PathBuf>) -> bool {
    let _ = (file, into);
    false
}

/// An import in flight.
#[cfg(feature = "import")]
pub(crate) struct ImportJob {
    source: Source,
    project: PathBuf,
    report: Sender<ImportEvent>,
    running: Running,
    cancel: Cancel,
    state: JobState,
}

/// Where an import has got to.
#[cfg(feature = "import")]
enum JobState {
    /// Nothing read yet.
    Reading,
    /// Read and understood; writing what the plan holds, a slice at a time.
    Writing {
        plan: balaur_import::Plan,
        sink: balaur_import::ProjectSink,
        done: usize,
        files: usize,
    },
    /// A project, which has no plan: walking it, a few files a slice. Boxed
    /// because a walk is twice the size of the whole of the other variants.
    Walking {
        walk: Box<balaur_import::ProjectWalk>,
        done: usize,
        files: usize,
    },
    /// Planning on a worker, in a tab built with shared memory. What comes
    /// back is the plan and the picked files, which the writes still read.
    #[cfg(all(target_family = "wasm", target_feature = "atomics"))]
    Planning(Receiver<(Result<balaur_import::Plan>, Picked)>),
    /// Finished, one way or the other.
    Over,
}

/// Files a reader picked, by name, with their bytes.
#[cfg(feature = "import")]
type Picked = Vec<(String, Vec<u8>)>;

/// One picked file's bytes, or an error naming it as missing.
#[cfg(feature = "import")]
fn picked(with: &Picked, uri: &str) -> Result<Vec<u8>> {
    with.iter()
        .find(|(name, _)| name == uri)
        .map(|(_, bytes)| bytes.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("'{uri}' was not among the files picked, so it cannot be read")
        })
}

/// Where a job's bytes come from.
#[cfg(feature = "import")]
pub(crate) enum Source {
    /// A file to read through the backend, with whatever a `.gltf` names
    /// beside it read from the same directory.
    Beside(PathBuf),
    /// Bytes in hand, and the files that came with them. What a browser has
    /// once a reader has picked: there is no directory to look in, so a
    /// `.gltf` whose images were not picked too fails by naming one.
    #[cfg_attr(
        not(target_family = "wasm"),
        allow(
            dead_code,
            reason = "a desktop dialog answers with a path; only a tab picks bytes"
        )
    )]
    Chosen {
        name: String,
        bytes: Vec<u8>,
        with: Picked,
    },
}

#[cfg(feature = "import")]
impl Source {
    /// What the reports call this import.
    fn describe(&self) -> String {
        match self {
            Self::Beside(file) => file.display().to_string(),
            Self::Chosen { name, .. } => name.clone(),
        }
    }

    /// The file's own name, which picks the importer.
    fn name(&self) -> Option<String> {
        match self {
            Self::Beside(file) => file
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string),
            Self::Chosen { name, .. } => Some(name.clone()),
        }
    }

    /// The bytes to import, read now for a path and already here otherwise.
    fn bytes(&self) -> Result<Vec<u8>> {
        match self {
            Self::Beside(file) => balaur::files::default_backend().read(file),
            Self::Chosen { bytes, .. } => Ok(bytes.clone()),
        }
    }

    /// Reads what the source named beside itself.
    fn side(&self, uri: &str) -> Result<Vec<u8>> {
        match self {
            Self::Beside(file) => {
                let directory = file.parent().map(Path::to_path_buf).unwrap_or_default();
                let path = directory.join(uri);
                balaur::files::default_backend().read(&path)
            }
            Self::Chosen { with, .. } => picked(with, uri),
        }
    }
}

#[cfg(feature = "import")]
impl ImportJob {
    pub(crate) fn new(
        source: Source,
        project: PathBuf,
        report: Sender<ImportEvent>,
        running: Running,
        cancel: Cancel,
    ) -> Self {
        Self {
            source,
            project,
            report,
            running,
            cancel,
            state: JobState::Reading,
        }
    }

    /// How many files this job has written, for a report.
    fn written(&self) -> usize {
        match &self.state {
            JobState::Writing { done, .. } | JobState::Walking { done, .. } => *done,
            JobState::Reading | JobState::Over => 0,
            #[cfg(all(target_family = "wasm", target_feature = "atomics"))]
            JobState::Planning(_) => 0,
        }
    }

    fn source(&self) -> String {
        self.source.describe()
    }

    /// Say how it ended, and stop.
    fn over(&mut self, event: ImportEvent) -> Progress {
        // Let go first: on a thread, the tick may read the count as soon as
        // the end arrives. Counted up once when the job was made.
        self.running.fetch_sub(1, Ordering::Relaxed);
        let _ = self.report.send(event);
        self.state = JobState::Over;
        Progress::Done
    }

    /// Read the file and work out what it writes, or do the whole import for
    /// the importers that cannot be sliced.
    fn begin(&mut self) -> Progress {
        let source = self.source();
        let Some(name) = self.source.name() else {
            return self.over(ImportEvent::Failed {
                source,
                message: "that file has no name".to_string(),
            });
        };
        if balaur_import::walks(&name) {
            let Source::Beside(file) = &self.source else {
                return self.over(ImportEvent::Failed {
                    source,
                    message: format!(
                        "{name} is a whole project, so it is imported from the folder it sits in \
                         rather than from what was picked"
                    ),
                });
            };
            return match balaur_import::ProjectWalk::begin(file, &self.project) {
                Ok(walk) => {
                    let files = walk.files();
                    let _ = self.report.send(ImportEvent::Started { source, files });
                    self.state = JobState::Walking {
                        walk: Box::new(walk),
                        done: 0,
                        files,
                    };
                    Progress::More
                }
                Err(why) => self.over(ImportEvent::Failed {
                    source,
                    message: format!("{why:#}"),
                }),
            };
        }
        if !balaur_import::slices(&name) {
            // A level and a Godot project walk their own folder, so there is
            // nothing to plan and this is the one long slice.
            let _ = self.report.send(ImportEvent::Started {
                source: source.clone(),
                files: 0,
            });
            let Source::Beside(file) = &self.source else {
                return self.over(ImportEvent::Failed {
                    source,
                    message: format!(
                        "{name} names the files around it, so it is imported from the folder it \
                         sits in rather than from what was picked"
                    ),
                });
            };
            return match balaur_import::import_file(file, &self.project, &[]) {
                Ok(imported) => self.over(ImportEvent::Done {
                    source,
                    scene: imported.scene,
                    note: imported.note,
                }),
                Err(why) => self.over(ImportEvent::Failed {
                    source,
                    message: format!("{why:#}"),
                }),
            };
        }
        let bytes = match self.source.bytes() {
            Ok(bytes) => bytes,
            Err(why) => {
                return self.over(ImportEvent::Failed {
                    source,
                    message: format!("{why:#}"),
                });
            }
        };
        // A tab with workers plans on one: the parse and the encode are the
        // heavy part, and the writes stay here with the tab's filesystem.
        #[cfg(all(target_family = "wasm", target_feature = "atomics"))]
        if let Source::Chosen { with, .. } = &mut self.source {
            let with = std::mem::take(with);
            self.state = JobState::Planning(task::compute(move || {
                let side = |uri: &str| picked(&with, uri);
                (balaur_import::plan_bytes(&name, &bytes, &side, &[]), with)
            }));
            return Progress::More;
        }
        let planned = {
            let side = |uri: &str| self.source.side(uri);
            balaur_import::plan_bytes(&name, &bytes, &side, &[])
        };
        self.planned(source, planned)
    }

    /// Start writing what a plan holds, or say why there is none.
    fn planned(&mut self, source: String, planned: Result<balaur_import::Plan>) -> Progress {
        match planned {
            Ok(plan) => {
                let files = plan.outputs();
                let _ = self.report.send(ImportEvent::Started { source, files });
                self.state = JobState::Writing {
                    plan,
                    sink: balaur_import::ProjectSink::new(&self.project),
                    done: 0,
                    files,
                };
                Progress::More
            }
            Err(why) => self.over(ImportEvent::Failed {
                source,
                message: format!("{why:#}"),
            }),
        }
    }
}

#[cfg(feature = "import")]
impl Stepped for ImportJob {
    fn step(&mut self) -> Progress {
        // Cleared by the job that reads it, so one press stops the one
        // running rather than everything started after it.
        if self.cancel.swap(false, Ordering::Relaxed) {
            let source = self.source();
            let done = self.written();
            return self.over(ImportEvent::Cancelled { source, done });
        }
        match &mut self.state {
            JobState::Reading => self.begin(),
            JobState::Over => Progress::Done,
            JobState::Writing { .. } => self.write_slice(),
            JobState::Walking { .. } => self.read_slice(),
            #[cfg(all(target_family = "wasm", target_feature = "atomics"))]
            JobState::Planning(_) => self.await_plan(),
        }
    }
}

#[cfg(feature = "import")]
impl ImportJob {
    /// Write up to [`FILES_PER_SLICE`] files, reporting each.
    ///
    /// The state is owned for the slice rather than borrowed out of `self`,
    /// because finishing reports through `self` and the two cannot overlap.
    fn write_slice(&mut self) -> Progress {
        let source = self.source();
        let JobState::Writing {
            mut plan,
            mut sink,
            mut done,
            files,
        } = std::mem::replace(&mut self.state, JobState::Over)
        else {
            return Progress::Done;
        };
        let mut failed = None;
        {
            let side = |uri: &str| self.source.side(uri);
            for _ in 0..FILES_PER_SLICE {
                let Some(path) = plan.peek().map(str::to_string) else {
                    break;
                };
                match plan.write_next(&mut sink, &side) {
                    Ok(progress) => {
                        done += 1;
                        let _ = self.report.send(ImportEvent::Wrote {
                            source: source.clone(),
                            path,
                            done,
                            files,
                        });
                        if progress == Progress::Done {
                            break;
                        }
                    }
                    Err(why) => {
                        failed = Some(format!("{why:#}"));
                        break;
                    }
                }
            }
        }
        if let Some(message) = failed {
            return self.over(ImportEvent::Failed { source, message });
        }
        if plan.peek().is_some() {
            self.state = JobState::Writing {
                plan,
                sink,
                done,
                files,
            };
            return Progress::More;
        }
        let scene = plan.scene().map(str::to_string);
        let note = plan.note().to_string();
        self.over(ImportEvent::Done {
            source,
            scene,
            note,
        })
    }
}

#[cfg(feature = "import")]
impl ImportJob {
    /// Read up to [`FILES_PER_SLICE`] of a project's files, reporting each.
    ///
    /// A project has no plan to write from, so what is counted off is what it
    /// reads; the files it writes are its own business and are answered for
    /// when the walk ends.
    fn read_slice(&mut self) -> Progress {
        let source = self.source();
        let JobState::Walking {
            mut walk,
            mut done,
            files,
        } = std::mem::replace(&mut self.state, JobState::Over)
        else {
            return Progress::Done;
        };
        for _ in 0..FILES_PER_SLICE {
            let Some(path) = walk.peek().map(str::to_string) else {
                break;
            };
            match walk.read_next() {
                Ok(_) => {
                    done += 1;
                    let _ = self.report.send(ImportEvent::Wrote {
                        source: source.clone(),
                        path,
                        done,
                        files,
                    });
                }
                Err(why) => {
                    return self.over(ImportEvent::Failed {
                        source,
                        message: format!("{why:#}"),
                    });
                }
            }
        }
        if walk.peek().is_some() {
            self.state = JobState::Walking { walk, done, files };
            return Progress::More;
        }
        match walk.finish() {
            Ok(imported) => self.over(ImportEvent::Done {
                source,
                scene: imported.scene,
                note: imported.note,
            }),
            Err(why) => self.over(ImportEvent::Failed {
                source,
                message: format!("{why:#}"),
            }),
        }
    }
}

#[cfg(all(feature = "import", target_family = "wasm", target_feature = "atomics"))]
impl ImportJob {
    /// Wait for the worker's plan without holding the frame, then write it.
    fn await_plan(&mut self) -> Progress {
        let JobState::Planning(answer) = &self.state else {
            return Progress::Done;
        };
        let (planned, with) = match answer.try_recv() {
            Ok(answered) => answered,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Progress::More,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let source = self.source();
                return self.over(ImportEvent::Failed {
                    source,
                    message: "the worker planning it stopped before it answered".to_string(),
                });
            }
        };
        if let Source::Chosen { with: held, .. } = &mut self.source {
            *held = with;
        }
        let source = self.source();
        self.planned(source, planned)
    }
}

/// The OS picker, filtered to what an importer reads. Blocking on purpose, as
/// the project picker is: a native dialog owns the screen while it is up.
#[cfg(all(
    not(any(target_family = "wasm", target_os = "ios", target_os = "android")),
    feature = "window"
))]
fn pick() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("models, sprites and levels", balaur_import::claimed())
        .pick_file()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(all(
    not(any(target_family = "wasm", target_os = "ios", target_os = "android")),
    feature = "window"
)))]
fn pick() -> Option<String> {
    None
}

/// Whether an importer reads this file. The list is `balaur_import`'s, so the
/// editor cannot drift from what `balaur import` actually reads.
#[cfg(feature = "import")]
fn handles(file: &Path) -> bool {
    file.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(balaur_import::claims)
}

/// A tab has no importers, so it claims nothing.
#[cfg(not(feature = "import"))]
fn handles(_: &Path) -> bool {
    false
}

#[cfg(feature = "import")]
fn import(file: &Path, project: &Path) -> Value {
    match balaur_import::import_file(file, project, &[]) {
        Ok(imported) => {
            let files = imported.files.into_iter().map(Value::text).collect();
            Value::Map(vec![
                ("files".into(), Value::List(files)),
                (
                    "scene".into(),
                    imported.scene.map_or(Value::Nil, Value::text),
                ),
                ("note".into(), Value::Str(imported.note.into())),
            ])
        }
        Err(e) => Value::Map(vec![("error".into(), Value::text(format!("{e:#}")))]),
    }
}

/// A tab has none of the importers: they read a `.glb` or an `.aseprite`
/// off disk through crates the browser build leaves out.
#[cfg(not(feature = "import"))]
fn import(file: &Path, _project: &Path) -> Value {
    Value::Map(vec![(
        "error".into(),
        Value::text(format!(
            "importing {} needs the desktop app; a tab has no importers",
            file.display()
        )),
    )])
}

#[cfg(all(test, feature = "import"))]
mod tests {
    use super::{Cancel, ImportEvent, ImportJob, Running, Source};
    use balaur_core::task::{self, Progress, Stepped};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    const ASEPRITE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../balaur_render/tests/fixtures/walk.aseprite"
    );

    /// Drive a job to its end, answering what it reported and how many slices
    /// it took.
    fn run(source: Source, project: &Path) -> (Vec<ImportEvent>, usize, usize) {
        run_cancelled(source, project, false)
    }

    /// `run`, with the stop pressed before the first slice or not.
    fn run_cancelled(
        source: Source,
        project: &Path,
        pressed: bool,
    ) -> (Vec<ImportEvent>, usize, usize) {
        // The job's own channel, standing in for the `ExternalIo` the plugin
        // hands it. It is read in this function and never leaves the process.
        let (report, events) = std::sync::mpsc::channel();
        let running = Running::default();
        running.store(1, Ordering::Relaxed);
        let cancel = Cancel::default();
        cancel.store(pressed, Ordering::Relaxed);
        let mut job = ImportJob::new(
            source,
            project.to_path_buf(),
            report,
            running.clone(),
            cancel,
        );
        let mut slices = 0;
        while job.step() == Progress::More {
            slices += 1;
            assert!(slices < 1000, "the job never finished");
        }
        slices += 1;
        (
            events.try_iter().collect(),
            slices,
            running.load(Ordering::Relaxed),
        )
    }

    /// What moves to a desktop thread has to be `Send`, and a field that is
    /// not would fail here rather than at the one call that spawns.
    #[test]
    fn a_job_can_move_to_a_thread() {
        fn sendable<T: Send>() {}
        sendable::<ImportJob>();
    }

    /// The steps a job takes on its own thread are the steps it takes under
    /// the tick: the count, one line per file, the end.
    #[test]
    fn a_job_on_a_thread_reports_every_file_then_the_end() {
        let project = tempfile::tempdir().unwrap();
        let (report, events) = std::sync::mpsc::channel();
        let running = Running::default();
        running.fetch_add(1, Ordering::Relaxed);
        task::step(ImportJob::new(
            Source::Beside(PathBuf::from(ASEPRITE)),
            project.path().to_path_buf(),
            report,
            running.clone(),
            Cancel::default(),
        ));
        let mut seen = Vec::new();
        while !matches!(
            seen.last(),
            Some(ImportEvent::Done { .. } | ImportEvent::Failed { .. })
        ) {
            let event = events
                .recv_timeout(Duration::from_secs(20))
                .expect("the threaded import never finished");
            seen.push(event);
        }
        let kinds: Vec<&str> = seen.iter().map(ImportEvent::kind).collect();
        assert_eq!(
            kinds,
            ["started", "wrote", "wrote", "wrote", "wrote", "done"]
        );
        assert_eq!(
            running.load(Ordering::Relaxed),
            0,
            "it let go before it said so"
        );
        for event in &seen {
            if let ImportEvent::Wrote { path, .. } = event {
                assert!(
                    project.path().join(path).exists(),
                    "{path} was reported, not written"
                );
            }
        }
    }

    /// Stop read before the first slice: nothing written, and the count says so.
    #[test]
    fn a_stop_before_the_first_slice_writes_nothing() {
        let project = tempfile::tempdir().unwrap();
        let (events, slices, running) = run_cancelled(
            Source::Beside(PathBuf::from(ASEPRITE)),
            project.path(),
            true,
        );
        assert_eq!(slices, 1);
        assert_eq!(running, 0, "a stopped job let go of its place");
        let [ImportEvent::Cancelled { done, .. }] = events.as_slice() else {
            panic!(
                "expected one cancelled report, got {:?}",
                events.iter().map(ImportEvent::kind).collect::<Vec<_>>()
            );
        };
        assert_eq!(*done, 0);
        assert!(
            std::fs::read_dir(project.path()).unwrap().next().is_none(),
            "it wrote a file"
        );
    }

    /// The sequence a progress bar reads: what is coming, each file as it
    /// lands, and the end.
    #[test]
    fn a_job_reports_the_count_then_every_file_then_the_end() {
        let project = tempfile::tempdir().unwrap();
        let (events, slices, running) =
            run(Source::Beside(PathBuf::from(ASEPRITE)), project.path());

        assert_eq!(
            slices, 2,
            "one slice to read it and one to write four files"
        );
        assert_eq!(running, 0, "the job let go of its place");

        let ImportEvent::Started { files, .. } = &events[0] else {
            panic!("it did not start: {:?}", events[0].kind());
        };
        assert_eq!(
            *files, 4,
            "a page, its sampling, a sheet and a clip library"
        );

        let wrote: Vec<(&str, usize)> = events
            .iter()
            .filter_map(|e| match e {
                ImportEvent::Wrote { path, done, .. } => Some((path.as_str(), *done)),
                _ => None,
            })
            .collect();
        assert_eq!(wrote.len(), 4, "one report per file: {wrote:?}");
        assert_eq!(
            wrote.iter().map(|(_, done)| *done).collect::<Vec<_>>(),
            vec![1, 2, 3, 4],
            "counted up as they landed"
        );
        for (path, _) in &wrote {
            assert!(
                project.path().join(path).exists(),
                "{path} was reported and not written"
            );
        }

        let last = events.last().unwrap();
        let ImportEvent::Done { note, .. } = last else {
            panic!("it did not finish: {}", last.kind());
        };
        assert!(note.contains("frames"), "unhelpful note: {note}");
    }

    /// A project is counted off by the files it reads, over as many slices as
    /// that takes. This is the import that used to be one long stop.
    #[test]
    fn a_project_is_walked_a_slice_at_a_time() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("project.godot"),
            "config_version=5\n\n[application]\n\nconfig/name=\"Harbour\"\n",
        )
        .unwrap();
        // More files than one slice takes, so the walk has to come back for a
        // second: a real project is thousands of these.
        for n in 0..40 {
            std::fs::write(source.path().join(format!("note{n}.txt")), "a\n").unwrap();
        }
        let project = tempfile::tempdir().unwrap();
        let (events, slices, running) = run(
            Source::Beside(source.path().join("project.godot")),
            project.path(),
        );

        assert!(slices > 2, "41 files went in {slices} slices");
        assert_eq!(running, 0, "the job let go of its place");
        let ImportEvent::Started { files, .. } = &events[0] else {
            panic!("it did not start: {}", events[0].kind());
        };
        assert_eq!(*files, 41, "project.godot and the forty beside it");

        let counted: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                ImportEvent::Wrote { done, .. } => Some(*done),
                _ => None,
            })
            .collect();
        assert_eq!(
            counted,
            (1..=41).collect::<Vec<_>>(),
            "counted up as they were read"
        );
        let last = events.last().unwrap();
        let ImportEvent::Done { .. } = last else {
            panic!("it did not finish: {}", last.kind());
        };
    }

    /// A failure is reported, not logged and dropped, and the job still lets
    /// go of its place.
    /// Bytes in hand and no directory to look in: what a browser has once a
    /// reader has picked, and the same import either way.
    #[test]
    fn a_job_imports_bytes_that_came_with_no_path() {
        let project = tempfile::tempdir().unwrap();
        let bytes = std::fs::read(ASEPRITE).unwrap();
        let (events, slices, running) = run(
            Source::Chosen {
                name: "walk.aseprite".to_string(),
                bytes,
                with: Vec::new(),
            },
            project.path(),
        );

        assert_eq!(slices, 2, "read on one slice, written on the next");
        assert_eq!(running, 0);
        let last = events.last().unwrap();
        let ImportEvent::Done { .. } = last else {
            panic!("it did not finish: {}", last.kind());
        };
        for rel in ["art/walk.webp", "sheets/walk.toml", "animations/walk.toml"] {
            assert!(
                project.path().join(rel).exists(),
                "{rel} was not written from the bytes"
            );
        }
    }

    /// A `.gltf` names its images beside itself, and a reader who picked only
    /// the `.gltf` is told which one is missing rather than left with half a
    /// model.
    #[test]
    fn what_was_not_picked_is_named() {
        let chosen = Source::Chosen {
            name: "hall.gltf".to_string(),
            bytes: Vec::new(),
            with: vec![("marble.png".to_string(), vec![1, 2, 3])],
        };
        assert_eq!(chosen.side("marble.png").unwrap(), vec![1, 2, 3]);
        let missing = format!("{:#}", chosen.side("stone.png").unwrap_err());
        assert!(
            missing.contains("stone.png") && missing.contains("picked"),
            "unhelpful: {missing}"
        );
    }

    #[test]
    fn a_job_that_cannot_read_its_file_reports_it() {
        let project = tempfile::tempdir().unwrap();
        let missing = PathBuf::from("/nowhere/ghost.aseprite");
        let (events, _, running) = run(Source::Beside(missing), project.path());

        assert_eq!(running, 0, "a failed job let go of its place");
        assert_eq!(events.len(), 1, "nothing but the failure: {events:?}");
        let ImportEvent::Failed { message, .. } = &events[0] else {
            panic!("it did not fail: {}", events[0].kind());
        };
        assert!(
            message.contains("ghost.aseprite"),
            "the message does not name the file: {message}"
        );
    }
}
