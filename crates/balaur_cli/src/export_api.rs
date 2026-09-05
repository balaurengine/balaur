//! `export.*` for the editor: which targets this install can build, and an
//! export that runs off the frame and reports as it goes.
//!
//! The verb is `balaur_export`, the same library the command line drives, so
//! a game exported from the sheet and a game exported by hand take one path.
//! What lives here is what the library deliberately does not hold: the
//! per-user template cache, the release a download comes from, and the answer
//! to "may this one be fetched", which is a person's to give.
//!
//! An export takes seconds to minutes, so it runs on a thread and reports
//! through [`ExternalIo`] — which means a recorded editor session replays
//! without ever exporting anything.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use balaur::replay::ExternalIo;
use balaur::{Engine, Stage};
use balaur_core::handler::{Handler, handler_of, opt};
use balaur_script::{Bindings, BindingsExt, Value};
use serde::{Deserialize, Serialize};

/// One step of an export, crossing from the worker thread back to a tick.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum ExportEvent {
    Started { target: String },
    Done { target: String, path: String },
    Failed { target: String, message: String },
}

impl ExportEvent {
    fn kind(&self) -> &'static str {
        match self {
            Self::Started { .. } => "started",
            Self::Done { .. } => "done",
            Self::Failed { .. } => "failed",
        }
    }

    fn value(&self) -> Value {
        let mut pairs = vec![
            ("kind".into(), Value::Str(self.kind().into())),
            ("target".into(), Value::Str(self.target().into())),
        ];
        match self {
            Self::Started { .. } => {}
            Self::Done { path, .. } => pairs.push(("path".into(), Value::Str(path.clone()))),
            Self::Failed { message, .. } => {
                pairs.push(("message".into(), Value::Str(message.clone())));
            }
        }
        Value::Map(pairs)
    }

    fn target(&self) -> &str {
        match self {
            Self::Started { target } | Self::Done { target, .. } | Self::Failed { target, .. } => {
                target
            }
        }
    }
}

/// The project being edited, the channel exports report on, and who listens.
pub(crate) struct ExportState {
    io: ExternalIo<ExportEvent>,
    listeners: Vec<Handler>,
    project: PathBuf,
}

impl ExportState {
    fn roots() -> Vec<PathBuf> {
        balaur_export::default_roots(crate::templates::cache_dir())
    }
}

/// The editor's export verb, registered by the CLI after the editor's app is
/// built — the library is the CLI's dependency, not the engine's.
pub(crate) struct ExportPlugin {
    manifest: balaur_plugin::Manifest,
    project: PathBuf,
}

impl ExportPlugin {
    #[must_use]
    pub(crate) fn new(project: PathBuf) -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("export", env!("CARGO_PKG_VERSION")),
            project,
        }
    }
}

impl balaur_plugin::Plugin for ExportPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(ExportState {
            io: ExternalIo::default(),
            listeners: Vec::new(),
            project: self.project.clone(),
        });
        reg.add_system(Stage::First, pump_export_system);
        let mut m = reg.script_module("export")?;
        install_export_api(&mut *m);
        Ok(())
    }
}

/// Deliver what the worker threads reported, to whoever asked to hear it.
fn pump_export_system(eng: &Engine, _: f32) {
    let mut dispatches = Vec::new();
    {
        let state = eng.resource::<ExportState>();
        let mut state = state.borrow_mut();
        let events = state.io.drain();
        for event in events {
            let value = event.value();
            for handler in &state.listeners {
                dispatches.push((handler.clone(), value.clone()));
            }
        }
    }
    if let Some(host) = eng.script_host() {
        for (handler, value) in dispatches {
            host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
        }
    }
}

fn install_export_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Exporting the project being edited. `targets` says what this install \
         can build and what it would have to fetch; `start` runs one off the \
         frame and reports to `on_export`. Nothing here exports while a \
         recording plays.",
    );
    m.describe(&[
        ("targets", &[], "()", "Every target, each `{ name, bundle, installed, note }`: whether its runtime template is already here, and what a signed build of it would additionally need."),
        ("listen", &[], "(node: node, options: map)", "Have the node's `on_export(event)` — or the `on_event` method the options name — called as each export starts, finishes or fails."),
        ("start", &[], "(target: string, options: map)", "Export the edited project for one target, on a thread. `download` allows fetching a missing template, `sign` names an identity, `output` overrides where it lands. Answers false while a recording plays."),
        ("output", &[], "(target: string)", "Where an export for this target will be written, as the project's `[export] output` decides."),
        ("running", &[], "()", "How many exports are in flight."),
    ]);
    m.function("targets", |_: &Engine, ()| Ok(targets()));
    m.function(
        "listen",
        |eng: &Engine, (node, opts): (balaur_script::NodeId, Option<Value>)| {
            let handler = handler_of(&Value::Node(node.0), opts.as_ref(), "on_event", "on_export")?
                .ok_or_else(|| anyhow!("export.listen needs a node"))?;
            eng.resource::<ExportState>()
                .borrow_mut()
                .listeners
                .push(handler);
            Ok(())
        },
    );
    m.function(
        "start",
        |eng: &Engine, (target, opts): (String, Option<Value>)| {
            Ok(start(eng, &target, opts.as_ref()))
        },
    );
    m.function("output", |eng: &Engine, target: String| {
        let state = eng.resource::<ExportState>();
        let project = state.borrow().project.clone();
        let config = balaur_export::ExportConfig::load(&project).unwrap_or_default();
        Ok(Value::Str(
            config
                .output_for(&project, &target, "")
                .unwrap_or_else(|| project.clone())
                .to_string_lossy()
                .into_owned(),
        ))
    });
    m.function("running", |_: &Engine, ()| {
        Ok(i64::try_from(RUNNING.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(i64::MAX))
    });
}

/// How many exports are in flight, so a sheet can refuse a second click on a
/// target that is still building.
static RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// What the sheet draws one row from.
fn targets() -> Value {
    let roots = ExportState::roots();
    let rows = balaur_export::TARGETS
        .iter()
        .map(|name| {
            let installed = balaur_export::template_installed(name, &roots);
            Value::Map(vec![
                ("name".into(), Value::Str((*name).into())),
                ("bundle".into(), Value::Bool(is_bundle(name))),
                ("installed".into(), Value::Bool(installed)),
                (
                    "fetchable".into(),
                    Value::Bool(crate::version::release_tag().is_some()),
                ),
                ("note".into(), Value::Str(note(name).into())),
            ])
        })
        .collect();
    Value::List(rows)
}

const fn is_bundle(target: &str) -> bool {
    matches!(target.as_bytes(), b"ios" | b"android" | b"web")
}

/// What a signed or installable build of this target needs beyond the export
/// itself, so a row says it before the click rather than after.
fn note(target: &str) -> &'static str {
    match target {
        "macos-universal" | "ios" if !cfg!(target_os = "macos") => "signing needs macOS",
        "android" => "an installable APK needs the Android SDK",
        "windows-x64" if !cfg!(windows) => "signing needs osslsigncode",
        _ => "",
    }
}

/// Begin one export on a thread, unless a recording is playing.
fn start(eng: &Engine, target: &str, opts: Option<&Value>) -> bool {
    let state = eng.resource::<ExportState>();
    let (project, download, sign, output) = {
        let state = state.borrow();
        (
            state.project.clone(),
            matches!(opt(opts, "download"), Some(Value::Bool(true))),
            match opt(opts, "sign") {
                Some(Value::Str(identity)) => Some(identity.clone()),
                _ => None,
            },
            match opt(opts, "output") {
                Some(Value::Str(path)) => Some(PathBuf::from(path)),
                _ => None,
            },
        )
    };
    let target = target.to_string();
    state.borrow().io.start(eng, |report| {
        let report = report.clone();
        RUNNING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::thread::spawn(move || {
            let _ = report.send(ExportEvent::Started {
                target: target.clone(),
            });
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
            let _ = report.send(event);
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
    let fetch = move |wanted: &str| crate::templates::obtain(wanted, true);
    let config = balaur_export::ExportConfig::load(project)?;
    let name = project
        .file_name()
        .map_or_else(|| "game".to_string(), |n| n.to_string_lossy().into_owned());
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
        template_roots: balaur_export::default_roots(crate::templates::cache_dir()),
        obtain: if download { Some(&fetch) } else { None },
        ..balaur_export::Options::default()
    })?;
    Ok(landed.unwrap_or_else(|| project.to_path_buf()))
}
