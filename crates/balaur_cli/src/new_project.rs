//! `balaur new`: a project from nothing, or from one of the editor's
//! templates.
//!
//! The templates live beside the editor rather than in this binary: they are
//! content, the library dock lists the same ones, and a project starting from
//! one should be a project a person could have written.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Every directory this install's shipped data may sit under, in the order to
/// look: beside the binary, then up to the repository root in a checkout.
///
/// Through `balaur_export::data_roots`, which knows a macOS bundle keeps its
/// data in `Contents/Resources` — codesign seals `Contents/MacOS` as code, so
/// nothing may ship beside the executable there, and `Resources` is a sibling
/// of it rather than an ancestor, which no walk upwards would reach.
pub(crate) fn data_dirs() -> Vec<PathBuf> {
    let exe = std::env::current_exe().ok();
    let cwd = std::env::current_dir().ok();
    dirs_under(exe.as_deref().and_then(Path::parent), cwd.as_deref())
}

/// [`data_dirs`] over the two directories it reads from the process, so a test
/// can hand it a bundle it built rather than the one it is running from.
fn dirs_under(exe_dir: Option<&Path>, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(dir) = exe_dir {
        bases.extend(balaur_export::data_roots(dir));
    }
    bases.extend(cwd.map(Path::to_path_buf));
    let mut found = Vec::new();
    for base in bases {
        let mut at = Some(base);
        while let Some(dir) = at {
            at = dir.parent().map(Path::to_path_buf);
            found.push(dir);
        }
    }
    found
}

/// Where the editor's own project, and so its library, is installed.
pub(crate) fn library_dir() -> Option<PathBuf> {
    data_dirs()
        .into_iter()
        .map(|dir| dir.join("editor/library"))
        .find(|library| library.is_dir())
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

#[cfg(test)]
mod tests {
    use super::dirs_under;
    use std::path::Path;

    /// `library_dir` and `examples_dir` over a set of candidate directories.
    fn holds(dirs: &[std::path::PathBuf], marker: &str) -> bool {
        dirs.iter().any(|dir| dir.join(marker).exists())
    }

    #[test]
    fn a_macos_bundle_keeps_its_data_beside_the_executable_rather_than_above_it() {
        let root = tempfile::tempdir().unwrap();
        let contents = root.path().join("Balaur.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::create_dir_all(contents.join("Resources/editor/library")).unwrap();
        std::fs::create_dir_all(contents.join("Resources/examples/hello")).unwrap();
        std::fs::write(
            contents.join("Resources/examples/hello/project.toml"),
            "[application]\n",
        )
        .unwrap();

        // Finder starts a bundle with a working directory of `/`, so the
        // executable's own directory is the only thing pointing at the data.
        let dirs = dirs_under(Some(&contents.join("MacOS")), Some(Path::new("/")));
        assert!(
            holds(&dirs, "editor/library"),
            "the templates the New button copies"
        );
        assert!(
            holds(&dirs, "examples/hello/project.toml"),
            "the examples the Examples tab lists"
        );
    }

    #[test]
    fn a_checkout_is_found_by_walking_up_from_the_binary() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("editor/library")).unwrap();
        let exe_dir = root.path().join("target/debug");
        std::fs::create_dir_all(&exe_dir).unwrap();

        let dirs = dirs_under(Some(&exe_dir), None);
        assert!(holds(&dirs, "editor/library"));
    }
}
