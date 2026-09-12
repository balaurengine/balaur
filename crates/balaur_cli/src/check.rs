//! `balaur check`: every finding in a project, with its file and its line.
//!
//! The editor's Problems list, at a prompt and in CI. The findings come from
//! `balaur::check_project_using`; what is printed and what the exit code says
//! about it are decided here.

use anyhow::Result;

#[cfg(not(target_family = "wasm"))]
use crate::{export_api, import_api};

pub(crate) fn project(path: &std::path::Path, strict: bool) -> Result<()> {
    // A project that means to stay clean says so in its own manifest; the
    // flag is for the run that wants it anyway.
    let strict = strict || project_is_strict(path);
    #[cfg(not(target_family = "wasm"))]
    let found = balaur::check_project_using(
        path,
        &mut [
            Box::new(export_api::ExportPlugin::new(path.to_path_buf())),
            Box::new(import_api::ImportPlugin::new(path.to_path_buf())),
        ],
    )?;
    #[cfg(target_family = "wasm")]
    let found = balaur::check_project(path)?;
    let mut errors = 0;
    let mut warnings = 0;
    for one in &found {
        if one.severity == "error" {
            errors += 1;
        } else {
            warnings += 1;
            if !strict {
                continue;
            }
        }
        let at = if one.line > 0 {
            format!("{}:{}:{}", one.file, one.line, one.column)
        } else {
            one.file.clone()
        };
        println!("{at}: {}: {}", one.severity, one.message);
    }
    if errors == 0 && (!strict || warnings == 0) {
        // Warnings are counted even when they are not printed, so a quiet
        // run still says there is something --strict would show.
        let quiet = if strict || warnings == 0 {
            String::new()
        } else {
            format!(" ({warnings} warning(s); --strict shows them)")
        };
        println!("no problems{quiet}");
        return Ok(());
    }
    std::process::exit(1);
}

/// Whether `project.toml` asked for `[check] strict = true`.
///
/// A manifest that will not parse reads as no: booting the project is about
/// to say so with the error a person can act on.
fn project_is_strict(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path.join("project.toml"))
        .ok()
        .and_then(|text| balaur_core::project::ProjectManifest::parse(&text).ok())
        .is_some_and(|manifest| manifest.check.strict)
}
