//! `import.*` for the editor: bring a model, a sprite sheet or a level into
//! the edited project, through the same code `balaur import` runs.
//!
//! The verb lives in the CLI, which assembles the editor's plugins; the
//! importers are `balaur_import`, and the engine never reads a `.glb` off
//! disk. What the editor gets back is the list of project-relative files that
//! were written and, for a model or a level, the scene it can instantiate.

use std::path::{Path, PathBuf};

use anyhow::Result;
use balaur::{Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

/// The project a drop lands in.
pub(crate) struct ImportState {
    project: PathBuf,
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
            project: self.project.clone(),
        });
        // No system: importing is a call, and the files it writes are read by
        // whatever asks for them next.
        let _ = Stage::First;
        let mut m = reg.script_module("import")?;
        install_import_api(&mut *m);
        Ok(())
    }
}

fn install_import_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Bringing a file into the project being edited: a `.glb` or `.gltf` \
         model, an `.aseprite` sprite, a `.tmx` or `.ldtk` level. The same \
         importers `balaur import` runs.",
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
            "Import one file into the edited project. Answers `{ files, scene, note }`: the project-relative paths written, the scene to instantiate when there is one, and a line for the log. Answers `{ error }` when the import failed.",
        ),
    ]);
    m.function("handles", |_: &Engine, path: String| {
        Ok(Value::Bool(handles(Path::new(&path))))
    });
    m.function("file", |eng: &Engine, path: String| {
        let project = eng.resource::<ImportState>().borrow().project.clone();
        Ok(import(Path::new(&path), &project))
    });
}

/// Extensions an importer claims. A file the editor should copy rather than
/// import (an image, a font) is not one of these.
const IMPORTED: &[&str] = &["glb", "gltf", "aseprite", "ase", "tmx", "ldtk"];

fn handles(file: &Path) -> bool {
    file.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|e| IMPORTED.contains(&e.as_str()))
}

#[cfg(not(target_family = "wasm"))]
fn import(file: &Path, project: &Path) -> Value {
    match balaur_import::import_file(file, project, &[]) {
        Ok(imported) => {
            let files = imported.files.into_iter().map(Value::Str).collect();
            Value::Map(vec![
                ("files".into(), Value::List(files)),
                (
                    "scene".into(),
                    imported.scene.map_or(Value::Nil, Value::Str),
                ),
                ("note".into(), Value::Str(imported.note)),
            ])
        }
        Err(e) => Value::Map(vec![("error".into(), Value::Str(format!("{e:#}")))]),
    }
}

/// A tab has none of the importers: they read a `.glb` or an `.aseprite`
/// off disk through crates the browser build leaves out.
#[cfg(target_family = "wasm")]
fn import(file: &Path, _project: &Path) -> Value {
    Value::Map(vec![(
        "error".into(),
        Value::Str(format!(
            "importing {} needs the desktop app; a tab has no importers",
            file.display()
        )),
    )])
}
