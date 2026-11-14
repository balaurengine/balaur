//! `export.*` for the editor: which targets this install can build, and an
//! export that runs off the frame and reports as it goes.
//!
//! The verb is `balaur_export`, the same library the command line drives, so
//! a game exported from the sheet and a game exported by hand take one path.
//! What lives here is what the library deliberately does not hold: the
//! per-user runtime cache, the release a download comes from, and the answer
//! to "may this one be fetched", which is a person's to give.
//!
//! An export takes seconds to minutes, so it runs on a thread and reports
//! through the replay layer: which means a recorded editor session replays
//! without ever exporting anything.

use std::path::PathBuf;

use anyhow::Result;
use balaur::{Engine, Stage};
use balaur_core::handler::opt;
use balaur_script::{Module, Value};

use crate::export_shared::{ExportCore, ExportEvent, PREVIEW_DOC};
use crate::jobs::pump;
use crate::vocabulary::keys;
use crate::vocabulary::words;

/// The project being edited plus what only a desktop install has: the
/// per-user runtime cache the roots are read from.
pub(crate) struct ExportState(ExportCore);

impl AsMut<ExportCore> for ExportState {
    fn as_mut(&mut self) -> &mut ExportCore {
        &mut self.0
    }
}

impl ExportState {
    fn roots() -> Vec<PathBuf> {
        balaur_export::default_roots(crate::runtimes::cache_dir())
    }
}

/// The editor's export verb, registered by the CLI after the editor's app is
/// built: the library is the CLI's dependency, not the engine's.
pub(crate) struct ExportPlugin {
    project: PathBuf,
}

impl ExportPlugin {
    #[must_use]
    pub(crate) fn new(project: PathBuf) -> Self {
        Self { project }
    }
}

impl balaur_plugin::Plugin for ExportPlugin {
    fn name(&self) -> &'static str {
        words::export_api::EXPORT
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        // The tables the exporter reads, described where every other table is:
        // the sheet is a button, and the settings screen is the configuration.
        balaur_export::settings::declare(reg.engine());
        reg.insert_resource(ExportState(ExportCore::new(self.project.clone())));
        reg.add_system(Stage::First, pump::<ExportState, ExportEvent>);
        let mut m = reg.script_module(words::export_api::EXPORT)?.editor_only();
        install_export_api(&mut m);
        Ok(())
    }
}

fn install_export_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Exports the project being edited. `targets` lists what this install can build; `start` runs one off the frame and reports as the `export_event` event: `events::listen(node, \"export_event\")`, heard as `on_export_event`, as each export starts, finishes or fails.",
    );
    m.records(&[
        (
            "Target",
            "{ name: string, runtime: string, bundle: bool, installed: bool, fetchable: bool, note: string }",
            "A target the project can be exported for, and what building it needs.",
        ),
        (
            "Preview",
            "{ source: string, before: int, after: int, width: int, height: int, drawn_width: int, drawn_height: int, gpu_bytes: int }",
            "What one file becomes in a target's pack.",
        ),
        (
            "StartOptions",
            "{ download: bool?, sign: string?, output: string? }",
            "How `start` exports: `download` fetches a missing template, `sign` names an identity, `output` overrides where it lands.",
        ),
    ]);
    m.describe(&[
        (words::functions::TARGETS, &[], "() -> [Target]", "Every target, each `{ name, runtime, bundle, installed, fetchable, note }`: the runtime the project's `[export] runtime` puts it on, whether that is already here, whether a missing one could be fetched, and what a signed build of it would also need."),
        (words::functions::START, &[], "(target: string, options: StartOptions?) -> bool", "Export the edited project for one target, on a thread. `download` allows fetching a missing template, `sign` names an identity, `output` overrides where it lands. Answers false while a recording plays."),
        (words::functions::OUTPUT, &[], "(target: string) -> string", "Where an export for this target will be written, as the project's `[export] output` decides."),
        (words::functions::RUNNING_COUNT, &[], "() -> int", "How many exports are in flight."),
        (words::functions::PREVIEW, &[], "(path: string, target: string) -> Preview?", PREVIEW_DOC),
    ]);
    m.function(words::functions::TARGETS, |eng: &Engine, ()| {
        let project = eng.resource::<ExportState>().borrow().0.project.clone();
        Ok(targets(&project))
    });
    m.function(
        words::functions::START,
        |eng: &Engine, (target, opts): (String, Option<Value>)| {
            Ok(start(eng, &target, opts.as_ref()))
        },
    );
    m.function(words::functions::OUTPUT, |eng: &Engine, target: String| {
        let state = eng.resource::<ExportState>();
        let project = state.borrow().0.project.clone();
        let config = balaur_export::ExportConfig::load(&project, Some(&target)).unwrap_or_default();
        Ok(Value::Str(
            config
                .output_for(&project, &target, "")
                .unwrap_or_else(|| project.clone())
                .to_string_lossy()
                .into_owned(),
        ))
    });
    m.function(words::functions::RUNNING_COUNT, |_: &Engine, ()| {
        Ok(i64::try_from(RUNNING.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(i64::MAX))
    });
    m.function(
        words::functions::PREVIEW,
        |eng: &Engine, (path, target): (String, String)| {
            let project = eng.resource::<ExportState>().borrow().0.project.clone();
            Ok(crate::export_shared::preview(&project, &path, &target))
        },
    );
}

/// How many exports are in flight, so a sheet can refuse a second click on a
/// target that is still building.
static RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// What the sheet draws one row from.
fn targets(project: &std::path::Path) -> Value {
    let roots = ExportState::roots();
    let smallest = balaur_export::SmallestRuntime::default();
    let rows = balaur_export::TARGETS
        .iter()
        .map(|name| {
            let runtime = balaur_export::runtime_on_hand(project, name, &smallest, &roots)
                .unwrap_or_else(|_| (*name).to_string());
            let installed = balaur_export::runtime_installed(&runtime, &roots);
            Value::Map(vec![
                (keys::NAME.into(), Value::Str((*name).into())),
                ("runtime".into(), Value::Str(runtime)),
                (
                    "bundle".into(),
                    Value::Bool(balaur_export::ships_bundle(name)),
                ),
                (keys::INSTALLED.into(), Value::Bool(installed)),
                (
                    "fetchable".into(),
                    Value::Bool(crate::version::release_tag().is_some()),
                ),
                (keys::NOTE.into(), Value::Str(note(name).into())),
            ])
        })
        .collect();
    Value::List(rows)
}

/// What a signed or installable build of this target needs beyond the export
/// itself, so a row says it before the click rather than after.
fn note(target: &str) -> &'static str {
    match target {
        words::targets::MACOS_UNIVERSAL | words::targets::IOS if !cfg!(target_os = "macos") => {
            "signing needs macOS"
        }
        words::targets::ANDROID => {
            "an installable APK needs the Android SDK, an AAB bundletool too"
        }
        words::targets::WINDOWS_X64 | words::targets::WINDOWS_ARM64 if !cfg!(windows) => {
            "signing needs osslsigncode"
        }
        _ => "",
    }
}

/// Begin one export on a thread, unless a recording is playing.
fn start(eng: &Engine, target: &str, opts: Option<&Value>) -> bool {
    let state = eng.resource::<ExportState>();
    let (project, download, sign, output) = {
        let state = state.borrow();
        (
            state.0.project.clone(),
            matches!(opt(opts, "download"), Some(Value::Bool(true))),
            match opt(opts, "sign") {
                Some(Value::Str(identity)) => Some(identity.clone()),
                _ => None,
            },
            match opt(opts, words::functions::OUTPUT) {
                Some(Value::Str(path)) => Some(PathBuf::from(path)),
                _ => None,
            },
        )
    };
    let target = target.to_string();
    state.borrow().0.io.start(eng, |report| {
        let report = report.clone();
        RUNNING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::thread::spawn(move || {
            balaur_core::replay::report(
                &report,
                ExportEvent::Started {
                    target: target.clone(),
                },
            );
            let event = match run_export(&project, &target, download, sign, output) {
                Ok(path) => ExportEvent::Done {
                    target,
                    path: path.to_string_lossy().into_owned(),
                },
                Err(err) => ExportEvent::Failed {
                    target,
                    message: format!("{err:#}"),
                },
            };
            balaur_core::replay::report(&report, event);
            RUNNING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        });
    })
}

/// The export itself, on the worker thread. The prompt a terminal would show
/// is not available here, so a download is allowed only when the sheet
/// already asked.
fn run_export(
    project: &std::path::Path,
    target: &str,
    download: bool,
    sign: Option<String>,
    output: Option<PathBuf>,
) -> Result<PathBuf> {
    let fetch = move |wanted: &str| crate::runtimes::obtain(wanted, true);
    let config = balaur_export::ExportConfig::load(project, Some(target))?;
    let name = project.file_name().map_or_else(
        || words::web_export::GAME.to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let landed = output.clone().or_else(|| {
        config
            .output_for(project, target, &name)
            .map(|p| p.parent().unwrap_or(&p).to_path_buf())
    });
    let modules = {
        let project = project.to_path_buf();
        move || -> Vec<Box<dyn balaur_plugin::Plugin>> {
            vec![Box::new(ExportPlugin::new(project.clone()))]
        }
    };
    balaur_export::export(&balaur_export::Options {
        path: project.to_path_buf(),
        plugins: Some(&modules),
        output,
        target: Some(target.to_string()),
        sign,
        runtime_roots: balaur_export::default_roots(crate::runtimes::cache_dir()),
        obtain: if download { Some(&fetch) } else { None },
        ..balaur_export::Options::default()
    })?;
    Ok(landed.unwrap_or_else(|| project.to_path_buf()))
}
