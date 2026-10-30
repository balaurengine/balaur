//! `picture.*`: a picture of a project, written as `balaur run --offscreen
//! --shot` writes one, by a second process running that same command, so the
//! editor's own window and its camera never stand in for the game's.
//!
//! The run takes a GPU and a second or two, so it is a job: it waits on the
//! process from a thread and reports through [`crate::jobs`].

use std::path::{Path, PathBuf};

use anyhow::Result;
use balaur::{Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};
use serde::{Deserialize, Serialize};

use crate::jobs::{Reported, Reporting, install_listen, pump};

/// Frames the run takes before its picture: a text is laid out and an image
/// uploaded a frame or two after the scene is built.
const FRAMES: u32 = 10;

/// How a picture went, crossing from the thread into a tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum PictureEvent {
    Written { path: String },
    Failed { path: String, message: String },
}

impl Reported for PictureEvent {
    fn value(&self) -> Value {
        let (kind, path, message) = match self {
            Self::Written { path } => ("written", path, None),
            Self::Failed { path, message } => ("failed", path, Some(message)),
        };
        let mut pairs = vec![
            ("kind".into(), Value::Str(kind.into())),
            ("path".into(), Value::Str(path.clone())),
        ];
        if let Some(message) = message {
            pairs.push(("message".into(), Value::Str(message.clone())));
        }
        Value::Map(pairs)
    }
}

/// Who hears a picture job, and the channel its threads report on.
pub(crate) struct PictureState(Reporting<PictureEvent>);

impl AsMut<Reporting<PictureEvent>> for PictureState {
    fn as_mut(&mut self) -> &mut Reporting<PictureEvent> {
        &mut self.0
    }
}

/// The `picture` module and the pump that delivers its jobs' reports.
pub(super) fn declare(reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
    reg.insert_resource(PictureState(Reporting::new(PathBuf::new())));
    reg.add_system(Stage::First, pump::<PictureState, PictureEvent>);
    let mut m = reg.script_module("picture")?;
    install_picture_api(&mut *m);
    Ok(())
}

fn install_picture_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "A picture of a project: its scene through its current camera, at its `[window]` size, written to a PNG by a second process running `balaur run --offscreen --shot`.",
    );
    m.describe(&[
        (
            "write",
            &[],
            "(project: string, scene: string) -> string?",
            "Write a picture of the project at `project`, of `scene` (project-relative; its main scene when empty), as the files on disk say. Answers where the PNG will be: `[export] output`'s `picture` folder, or beside the project's manifest. Nil where no second process can run, as in a browser tab.",
        ),
        (
            "listen",
            &[],
            "(node: node, options: map)",
            "Have the node's `on_picture_event(event)`, or the `on_event` method the options name, called when a picture is written, `{ kind: \"written\", path }`, or fails, `{ kind: \"failed\", path, message }`.",
        ),
    ]);
    m.function(
        "write",
        |eng: &Engine, (project, scene): (String, String)| {
            Ok(
                write(eng, Path::new(&project), &scene).map_or(Value::Nil, |path| {
                    Value::Str(path.to_string_lossy().into_owned())
                }),
            )
        },
    );
    install_listen::<PictureState, PictureEvent>(m, "on_picture_event");
}

/// Where a picture of `scene` lands: the folder `[export] output` names, or
/// the project's own, under the scene's name.
fn output_of(project: &Path, scene: &str) -> PathBuf {
    let stem = Path::new(scene).file_stem().map_or_else(
        || "picture".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let name = format!("{stem}.png");
    balaur_export::ExportConfig::load(project, None)
        .ok()
        .and_then(|config| config.output_for(project, "picture", &name))
        .unwrap_or_else(|| project.join(name))
}

#[cfg(not(target_family = "wasm"))]
fn write(eng: &Engine, project: &Path, scene: &str) -> Option<PathBuf> {
    let output = output_of(
        project,
        &if scene.is_empty() {
            main_scene_of(project)
        } else {
            scene.to_string()
        },
    );
    let exe = std::env::current_exe().ok()?;
    let mut command = std::process::Command::new(exe);
    command
        .arg("run")
        .arg(project)
        .arg("--offscreen")
        .arg("--frames")
        .arg(FRAMES.to_string())
        .arg("--shot")
        .arg(&output);
    if !scene.is_empty() {
        command.arg("--scene").arg(scene);
    }
    let state = eng.resource::<PictureState>();
    let state = state.borrow();
    let path = output.clone();
    let started = state.0.io.start(eng, |report| {
        let report = report.clone();
        std::thread::spawn(move || {
            let shown = path.to_string_lossy().into_owned();
            let event = match command.output() {
                // A run that wrote its shot exits well; the file is the
                // caller's from then on, and may already be gone.
                Ok(done) if done.status.success() => PictureEvent::Written { path: shown },
                Ok(done) => PictureEvent::Failed {
                    path: shown,
                    message: last_line(&done.stderr)
                        .unwrap_or_else(|| format!("the run ended with {}", done.status)),
                },
                Err(e) => PictureEvent::Failed {
                    path: shown,
                    message: format!("could not start the run: {e}"),
                },
            };
            balaur_core::replay::report(&report, event);
        });
    });
    started.then_some(output)
}

/// A tab cannot start a second process.
#[cfg(target_family = "wasm")]
fn write(_: &Engine, _: &Path, _: &str) -> Option<PathBuf> {
    None
}

/// The scene a run of the project opens, as its manifest names it.
#[cfg(not(target_family = "wasm"))]
fn main_scene_of(project: &Path) -> String {
    std::fs::read_to_string(project.join("project.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|manifest| {
            manifest
                .get("application")?
                .get("main_scene")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// The last line a failed run wrote, which is where it says why.
fn last_line(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::to_string)
}
