//! `balaur import` over a Godot project, or one scene of it, as files.
//!
//! The converters in the `import_godot_*` modules turn text into text; this
//! finds the files, writes what they return, copies the art and audio the
//! scenes name, and gathers everything that did not carry into one
//! `import-report.md`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::import::Imported;

/// File kinds copied across as they are: the engine reads each directly.
const COPIED: &[&str] = &[
    "png", "webp", "jpg", "jpeg", "bmp", "tga", "ogg", "wav", "mp3", "flac", "ttf", "otf",
    "json", "csv", "txt",
];

/// `project.godot`: the settings, every scene, and the files they name.
pub(crate) fn import_project(file: &Path, project: &Path) -> Result<Imported> {
    let root = file.parent().unwrap_or(Path::new("."));
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let document = crate::import_godot::parse(&text).with_context(|| format!("reading {}", file.display()))?;
    let uids = crate::import_godot_project::uid_index(root)?;
    let converted = crate::import_godot_project::convert(&document, &uids)?;

    let mut out = Imported::default();
    let mut report = Report::default();
    write(project, "project.toml", &converted.project_toml, &mut out)?;
    report.section("project.godot", converted.notes);

    let mut scenes = 0;
    let mut failed = 0;
    for relative in walk(root)? {
        let extension = Path::new(&relative)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "tscn" {
            match scene(root, &relative, project, &mut out) {
                Ok(notes) => {
                    scenes += 1;
                    report.section(&relative, notes);
                }
                Err(why) => {
                    failed += 1;
                    report.section(&relative, vec![format!("not converted: {why:#}")]);
                }
            }
        } else if COPIED.contains(&extension.as_str()) {
            let target = project.join(&relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(root.join(&relative), &target)
                .with_context(|| format!("copying {relative}"))?;
            out.files.push(relative);
        }
    }
    let lines = report.write(project, &mut out)?;
    out.note = format!(
        "{scenes} scene{} converted{}; {lines} note{} in import-report.md",
        if scenes == 1 { "" } else { "s" },
        if failed == 0 { String::new() } else { format!(", {failed} would not") },
        if lines == 1 { "" } else { "s" },
    );
    Ok(out)
}

/// One `.tscn`, and the clip files it writes beside itself.
pub(crate) fn import_scene(file: &Path, project: &Path) -> Result<Imported> {
    let file = file
        .canonicalize()
        .with_context(|| format!("reading {}", file.display()))?;
    let root = godot_root(&file)?;
    let relative = file
        .strip_prefix(&root)
        .context("the scene is not inside its project")?
        .to_string_lossy()
        .replace('\\', "/");
    let mut out = Imported::default();
    let notes = scene(&root, &relative, project, &mut out)?;
    let mut report = Report::default();
    report.section(&relative, notes);
    let lines = report.write(project, &mut out)?;
    out.scene = Some(crate::import_godot_scene::scene_path(&relative));
    out.note = if lines == 0 {
        "everything in the scene carried across".to_string()
    } else {
        format!("{lines} note{} in import-report.md", if lines == 1 { "" } else { "s" })
    };
    Ok(out)
}

/// Convert the scene at `relative` under `root`, writing it into `project`.
fn scene(root: &Path, relative: &str, project: &Path, out: &mut Imported) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(root.join(relative))?;
    let document = crate::import_godot::parse(&text)?;
    let converted = crate::import_godot_scene::convert(&document, relative, root)?;
    let path = crate::import_godot_scene::scene_path(relative);
    write(project, &path, &converted.scene_toml, out)?;
    for (file, text) in &converted.files {
        write(project, file, text, out)?;
    }
    Ok(converted.notes)
}

/// The directory holding the `project.godot` a file belongs to.
fn godot_root(file: &Path) -> Result<PathBuf> {
    let mut dir = file.parent();
    while let Some(here) = dir {
        if here.join("project.godot").is_file() {
            return Ok(here.to_path_buf());
        }
        dir = here.parent();
    }
    bail!(
        "{} is not inside a Godot project: no project.godot above it",
        file.display()
    )
}

/// Every file under `root`, project-relative with `/`, sorted. `.godot` is
/// the editor's cache and every dot-directory is someone's tooling, so both
/// are skipped.
fn walk(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .flatten()
        {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                dirs.push(path);
            } else if let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    files.sort();
    Ok(files)
}

fn write(project: &Path, relative: &str, text: &str, out: &mut Imported) -> Result<()> {
    let path = project.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    out.files.push(relative.to_string());
    Ok(())
}

/// What did not carry, one heading per file.
#[derive(Default)]
struct Report {
    sections: Vec<(String, Vec<String>)>,
}

impl Report {
    fn section(&mut self, file: &str, notes: Vec<String>) {
        if !notes.is_empty() {
            self.sections.push((file.to_string(), notes));
        }
    }

    /// Write `import-report.md` when there is anything in it, and say how
    /// many notes it holds.
    fn write(self, project: &Path, out: &mut Imported) -> Result<usize> {
        let count = self.sections.iter().map(|(_, notes)| notes.len()).sum();
        if count == 0 {
            return Ok(0);
        }
        let mut text = String::from("# What did not convert\n\n");
        text.push_str(
            "Every line is something `balaur import` read and could not carry. \
             Each names the file it came from, and the node where there is one.\n",
        );
        for (file, notes) in self.sections {
            text.push_str(&format!("\n## {file}\n\n"));
            for note in notes {
                text.push_str(&format!("- {note}\n"));
            }
        }
        write(project, "import-report.md", &text, out)?;
        Ok(count)
    }
}
