//! A Godot project converted one file at a time.
//!
//! `project.godot` is the one import that is not a plan: what it writes is
//! found by walking the folder, and a project holds thousands of files. So
//! this is a walk with a place in it -- [`Walk::begin`] reads the settings and
//! the lookups every file after them needs, and each [`Walk::step`] converts
//! one file and says so. A caller under a tick takes a few steps a frame; a
//! caller in a command takes them all, which is what `import_project` is.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::files::{COPIED, Report, is_translation, walk};
use super::nodes::Project;
use crate::{Imported, ProjectSink, Sink};

/// A project import in flight.
pub(crate) struct Walk {
    root: PathBuf,
    /// Every file under the project, and how many of them are converted.
    files: Vec<String>,
    at: usize,
    lookups: Project,
    report: Report,
    sink: ProjectSink,
    scenes: usize,
    scripts: usize,
    failed: usize,
}

impl Walk {
    /// Read `project.godot`: the settings, the font a project names, and the
    /// lookups every scene reads. Everything else is a step.
    pub(crate) fn begin(file: &Path, project: &Path) -> Result<Self> {
        let root = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let text = super::io::text(file)?;
        let document =
            super::parse(&text).with_context(|| format!("reading {}", file.display()))?;
        let uids = super::project::uid_index(&root);
        let converted = super::project::convert(&document, &uids)?;

        let mut report = Report::default();
        let mut sink = ProjectSink::new(project);
        sink.put("project.toml", converted.project_toml.as_bytes())?;
        report.section("project.godot", converted.notes);
        // A project's own faces come first in every font chain, from `fonts/`.
        if let Some(font) = super::project::custom_font(&document, &uids, &root) {
            super::files::copy_font(&root, &mut sink, &font)?;
        }
        let files = walk(&root);
        let lookups = super::files::lookups(&root, &files, uids, &mut sink, &mut report)?;
        Ok(Self {
            root,
            files,
            at: 0,
            lookups,
            report,
            sink,
            scenes: 0,
            scripts: 0,
            failed: 0,
        })
    }

    /// How many files the walk will read, for a caller counting them off.
    pub(crate) fn files(&self) -> usize {
        self.files.len()
    }

    /// The next file to read, project-relative, or `None` when it is over.
    pub(crate) fn peek(&self) -> Option<&str> {
        self.files.get(self.at).map(String::as_str)
    }

    /// Convert the next file. A scene that will not convert is a note in the
    /// report rather than the end of the import.
    pub(crate) fn step(&mut self) -> Result<()> {
        let Some(relative) = self.files.get(self.at).cloned() else {
            return Ok(());
        };
        self.at += 1;
        let extension = Path::new(&relative)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "tscn" {
            match super::files::scene(&self.root, &relative, &mut self.sink, &self.lookups) {
                Ok(notes) => {
                    self.scenes += 1;
                    self.report.section(&relative, notes);
                }
                Err(why) => {
                    self.failed += 1;
                    self.report
                        .section(&relative, vec![format!("not converted: {why:#}")]);
                }
            }
        } else if extension == "gd" {
            let source = super::io::text(&self.root.join(&relative))?;
            let converted = super::script::convert(&source, &relative, &self.lookups.classes);
            let target = format!("{}.rn", relative.trim_end_matches(".gd"));
            self.sink.put(&target, converted.rune.as_bytes())?;
            self.scripts += 1;
            self.report.section(&relative, converted.notes);
        } else if extension == "tres" {
            if let Some(notes) =
                super::files::theme(&self.root, &relative, &mut self.sink, &self.lookups)?
            {
                self.report.section(&relative, notes);
            }
        } else if COPIED.contains(&extension.as_str()) && !is_translation(&self.root, &relative) {
            // Read and written one at a time, so a project's art never
            // gathers in memory on its way across.
            let bytes = super::io::bytes(&self.root.join(&relative))?;
            self.sink.put(&relative, &bytes)?;
        }
        Ok(())
    }

    /// The shim, the report, and what the whole walk wrote.
    pub(crate) fn finish(mut self) -> Result<Imported> {
        if self.scripts > 0 {
            // Every converted body calls into the shim, so it ships with them.
            self.sink.put("gd.rn", super::gdscript::SHIM.as_bytes())?;
        }
        let (scenes, scripts, failed) = (self.scenes, self.scripts, self.failed);
        let lines = self.report.write(&mut self.sink)?;
        let note = format!(
            "{scenes} scene{} and {scripts} script{} converted{}; {lines} note{} in import-report.md",
            if scenes == 1 { "" } else { "s" },
            if scripts == 1 { "" } else { "s" },
            if failed == 0 {
                String::new()
            } else {
                format!(", {failed} would not")
            },
            if lines == 1 { "" } else { "s" },
        );
        Ok(Imported {
            files: self.sink.written().to_vec(),
            note,
            ..Imported::default()
        })
    }
}

/// `project.godot`: the settings, every scene, and the files they name, in
/// one call. The same walk a job steps through, taken to the end.
pub(crate) fn import_project(file: &Path, project: &Path) -> Result<Imported> {
    let mut walk = Walk::begin(file, project)?;
    while walk.peek().is_some() {
        walk.step()?;
    }
    walk.finish()
}
