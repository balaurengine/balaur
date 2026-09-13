//! `balaur new`: a project from nothing, or from one of the editor's
//! templates.
//!
//! The templates live beside the editor rather than in this binary: they are
//! content, the library dock lists the same ones, and a project starting from
//! one should be a project a person could have written.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Where the editor's own project, and so its library, is installed.
pub(crate) fn library_dir() -> Option<PathBuf> {
    let here = std::env::current_exe().ok()?;
    // Beside the binary in an install, and up at the repository root in a
    // checkout; the editor project is found the same way.
    for base in [here.parent()?.to_path_buf(), std::env::current_dir().ok()?] {
        let mut dir = Some(base);
        while let Some(at) = dir {
            let library = at.join("editor/library");
            if library.is_dir() {
                return Some(library);
            }
            dir = at.parent().map(Path::to_path_buf);
        }
    }
    None
}

/// Copy a template's files, substituting the project's name where it says so.
///
/// Text only: every template is scenes, scripts and a manifest, and a binary
/// starting point would be content rather than a template.
fn copy_template(from: &Path, to: &Path, name: &str) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_template(&entry.path(), &target, name)?;
            continue;
        }
        let text = std::fs::read_to_string(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        std::fs::write(&target, text.replace("{{name}}", name))?;
    }
    Ok(())
}

pub(crate) fn create(path: &Path, template: Option<&str>) -> Result<()> {
    let name = path
        .file_name()
        .map_or_else(|| "game".to_string(), |n| n.to_string_lossy().into_owned());
    if let Some(template) = template {
        let Some(library) = library_dir() else {
            anyhow::bail!("no editor library beside this binary, so no templates to start from");
        };
        let from = library.join("templates").join(template);
        if !from.is_dir() {
            let mut known: Vec<String> = std::fs::read_dir(library.join("templates"))?
                .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
                .collect();
            known.sort();
            anyhow::bail!(
                "no template `{template}`; the templates are {}",
                known.join(", ")
            );
        }
        copy_template(&from, path, &name)?;
        tracing::info!(
            "created project '{name}' from the `{template}` template at {}",
            path.display()
        );
        tracing::info!("run it with: balaur run {}", path.display());
        return Ok(());
    }
    std::fs::create_dir_all(path.join("scenes"))?;
    std::fs::create_dir_all(path.join("scripts"))?;
    std::fs::write(
        path.join("project.toml"),
        format!("[application]\nname = \"{name}\"\nmain_scene = \"scenes/main.toml\"\n"),
    )?;
    std::fs::write(
        path.join("scenes/main.toml"),
        r#"[[nodes]]
name = "Hello"
script = { source = "scripts/hello.rn" }
"#,
    )?;
    std::fs::write(
        path.join("scripts/hello.rn"),
        r#"pub fn init(this) {
    log::info(format!("hello from {}", this.node.name()));
    this.elapsed = 0.0;
}

pub fn update(this, dt) {
    this.elapsed += dt;
}
"#,
    )?;
    tracing::info!("created project '{name}' at {}", path.display());
    tracing::info!("run it with: balaur run {}", path.display());
    Ok(())
}
