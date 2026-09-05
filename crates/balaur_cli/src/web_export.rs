//! `export.*` in a browser: the exports a tab can finish by itself.
//!
//! Producing a pack is file work — every script compiled and checked, scenes
//! and manifest gathered, no linker involved — so it runs here exactly as it
//! does on a desktop. So does a web bundle, which is that pack zipped beside
//! the glue, the module and a shell page this tab already fetched to be
//! running at all. Every other target fuses a pack onto a native runtime
//! template, and a browser cannot link, so they are not offered: the sheet
//! shows what a tab can do, and `balaur export --target` on a machine is
//! still how the rest is built.
//!
//! What an export produces is not written into the project. It is handed to
//! the page as bytes to download, so a twenty-megabyte bundle never lands in
//! someone's browser storage and never ends up inside the next export.

use std::cell::RefCell;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow};
use balaur::replay::ExternalIo;
use balaur::{Engine, Stage};
use balaur_core::handler::{Handler, handler_of};
use balaur_script::{Bindings, BindingsExt, Value};
use serde::{Deserialize, Serialize};

/// The file a web bundle keeps its pack under, as the shell page loads it.
use balaur::standalone::BUNDLED_PACK;

thread_local! {
    /// What the last export produced, waiting for the page to take it. One
    /// slot: a second export replaces what the first left.
    static PRODUCED: RefCell<Option<(String, Vec<u8>)>> = const { RefCell::new(None) };

    /// How many exports are in flight, so the sheet can refuse a second click.
    static RUNNING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One step of an export, crossing from a spawned task back to a tick.
#[derive(Clone, Serialize, Deserialize)]
enum ExportEvent {
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

    fn target(&self) -> &str {
        match self {
            Self::Started { target } | Self::Done { target, .. } | Self::Failed { target, .. } => {
                target
            }
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
}

/// The project being edited, where the module it would ship is served from,
/// and who listens for what an export did.
struct ExportState {
    io: ExternalIo<ExportEvent>,
    listeners: Vec<Handler>,
    project: PathBuf,
    template: String,
}

/// The editor's export verb in a browser, registered by the web entry point.
pub(crate) struct WebExportPlugin {
    manifest: balaur_plugin::Manifest,
    project: PathBuf,
    template: String,
}

impl WebExportPlugin {
    /// `template` is where `balaur.js` and `balaur_bg.wasm` are served from,
    /// which a web bundle carries beside the pack.
    pub(crate) fn new(project: PathBuf, template: String) -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("export", env!("CARGO_PKG_VERSION")),
            project,
            template,
        }
    }
}

impl balaur_plugin::Plugin for WebExportPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(ExportState {
            io: ExternalIo::default(),
            listeners: Vec::new(),
            project: self.project.clone(),
            template: self.template.clone(),
        });
        reg.add_system(Stage::First, pump_export_system);
        let mut m = reg.script_module("export")?;
        install_export_api(&mut *m);
        Ok(())
    }
}

/// Deliver what the spawned exports reported, to whoever asked to hear it.
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
        "Exporting the project being edited, in a browser. `targets` is what \
         a tab can finish by itself: a pack, and a web bundle. Everything \
         else needs a linker, which no browser has. Nothing here exports \
         while a recording plays.",
    );
    m.describe(&[
        ("targets", &[], "()", "Every target this tab can build, each `{ name, bundle, installed, fetchable, note }`."),
        ("listen", &[], "(node: node, options: map)", "Have the node's `on_export(event)` — or the `on_event` method the options name — called as each export starts, finishes or fails."),
        ("start", &[], "(target: string, options: map)", "Export the edited project for one target. The bytes go to the page to download rather than into the project. Answers false while a recording plays."),
        ("output", &[], "(target: string)", "The file name an export for this target produces."),
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
            let _ = opts;
            Ok(start(eng, &target))
        },
    );
    m.function("output", |eng: &Engine, target: String| {
        let state = eng.resource::<ExportState>();
        let project = state.borrow().project.clone();
        let name = name_of(&project);
        Ok(Value::Str(match target.as_str() {
            "web" => format!("{name}-web.zip"),
            _ => format!("{name}.bpak"),
        }))
    });
    m.function("running", |_: &Engine, ()| {
        Ok(i64::try_from(RUNNING.with(std::cell::Cell::get)).unwrap_or(i64::MAX))
    });
}

/// What the sheet draws one row from. Both are ready the moment the tab is:
/// nothing to install, because the pack is built here and the module a bundle
/// ships is the one this page is running.
fn targets() -> Value {
    let rows = [
        ("pack", false, "a .bpak the engine plays anywhere"),
        ("web", true, "a zip to unpack on any static host"),
    ]
    .into_iter()
    .map(|(name, bundle, note)| {
        Value::Map(vec![
            ("name".into(), Value::Str(name.into())),
            ("bundle".into(), Value::Bool(bundle)),
            ("installed".into(), Value::Bool(true)),
            ("fetchable".into(), Value::Bool(false)),
            ("note".into(), Value::Str(note.into())),
        ])
    })
    .collect();
    Value::List(rows)
}

/// Begin one export, unless a recording is playing.
fn start(eng: &Engine, target: &str) -> bool {
    let state = eng.resource::<ExportState>();
    let (project, template) = {
        let state = state.borrow();
        (state.project.clone(), state.template.clone())
    };
    let target = target.to_string();
    state.borrow().io.start(eng, |report| {
        let report = report.clone();
        RUNNING.with(|running| running.set(running.get() + 1));
        wasm_bindgen_futures::spawn_local(async move {
            let _ = report.send(ExportEvent::Started {
                target: target.clone(),
            });
            let event = match run(&project, &target, &template).await {
                Ok(name) => {
                    ExportEvent::Done {
                        target,
                        // The page downloads it under this name; there is no
                        // directory in a tab for it to have landed in.
                        path: name,
                    }
                }
                Err(why) => ExportEvent::Failed {
                    target,
                    message: format!("{why:#}"),
                },
            };
            let _ = report.send(event);
            RUNNING.with(|running| running.set(running.get().saturating_sub(1)));
        });
    })
}

/// One export, leaving its bytes for the page to take.
async fn run(project: &Path, target: &str, template: &str) -> Result<String> {
    let (name, bytes) = match target {
        "pack" => {
            let pack = build(project)?;
            (format!("{}.bpak", name_of(project)), pack.encode())
        }
        "web" => bundle(project, template).await?,
        other => {
            return Err(anyhow!(
                "'{other}' fuses the pack onto a native runtime, and a browser \
                 cannot link; export a pack or a web bundle here, and run \
                 `balaur export --target {other}` on a machine for the rest"
            ));
        }
    };
    PRODUCED.with(|slot| *slot.borrow_mut() = Some((name.clone(), bytes)));
    Ok(name)
}

/// The pack, with sources kept: the runtime that loads it is 32-bit, and
/// compiled script bytes do not read back across pointer widths.
fn build(project: &Path) -> Result<balaur::Pack> {
    balaur::build_pack_using(project, true, &mut [])
}

/// A web bundle: the pack, the shell page, and the glue and module this tab
/// is itself running, zipped as one directory to drop on a static host.
async fn bundle(project: &Path, template: &str) -> Result<(String, Vec<u8>)> {
    let pack = build(project)?;
    let name = name_of(project);
    let shell = balaur_export::web_shell(project)?
        .replace("{{title}}", &name)
        .replace("{{pack}}", BUNDLED_PACK);
    let glue = crate::web::fetch_bytes(&beside(template, "balaur.js"))
        .await
        .map_err(|why| anyhow!("fetching the web glue: {}", described(&why)))?;
    let module = crate::web::fetch_bytes(&beside(template, "balaur_bg.wasm"))
        .await
        .map_err(|why| anyhow!("fetching the web module: {}", described(&why)))?;
    let files = vec![
        ("index.html".to_string(), shell.into_bytes()),
        (BUNDLED_PACK.to_string(), pack.encode()),
        ("balaur.js".to_string(), glue),
        ("balaur_bg.wasm".to_string(), module),
    ];
    Ok((format!("{name}-web.zip"), zip(&files)?))
}

/// One deflated archive, dated from nothing, so the same project exports the
/// same bytes twice.
fn zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (name, bytes) in files {
        writer
            .start_file(name.as_str(), options)
            .with_context(|| format!("adding {name} to the bundle"))?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

/// A URL beside the one the editor's own files came from.
fn beside(template: &str, file: &str) -> String {
    format!("{}/{file}", template.trim_end_matches('/'))
}

/// What a page threw, as something to put in a message.
fn described(why: &wasm_bindgen::JsValue) -> String {
    why.as_string()
        .unwrap_or_else(|| "the request failed".to_string())
}

/// The project's name from its manifest, as a file name: a browser has no
/// directory whose name could have served instead.
fn name_of(project: &Path) -> String {
    let fs = balaur::files::default_backend();
    let manifest = fs
        .read(&project.join("project.toml"))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.parse::<toml::Value>().ok());
    let name = manifest
        .as_ref()
        .and_then(|value| value.get("application")?.get("name")?.as_str())
        .unwrap_or("game");
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_ascii_lowercase();
    if trimmed.is_empty() {
        "game".to_string()
    } else {
        trimmed
    }
}

/// The project as it stands, zipped for someone to take away. What makes a
/// folder opened in a tab a folder again, and the answer to keeping work that
/// outlives a browser someone might clear.
///
/// # Errors
/// If a file cannot be added to the archive.
pub(crate) fn archive(project: &Path, files: &[(String, Vec<u8>)]) -> Result<(String, Vec<u8>)> {
    Ok((format!("{}.zip", name_of(project)), zip(files)?))
}

/// What the last export produced, taken rather than read: the page downloads
/// it once, and holding twenty megabytes after that is waste.
pub(crate) fn take() -> Option<(String, Vec<u8>)> {
    PRODUCED.with(|slot| slot.borrow_mut().take())
}
