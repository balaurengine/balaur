//! The Godot project's own files, read through the engine's file backend.
//!
//! An import reads a project it did not write, and every read goes through
//! `balaur_core::files` rather than `std::fs`: in a browser tab that backend
//! is the memory the project lives in, and there is no disk to reach for.

use std::path::Path;

use anyhow::{Context, Result};
use balaur_core::files;

/// One file as text.
pub(crate) fn text(path: &Path) -> Result<String> {
    let bytes = files::default_backend()
        .read(path)
        .with_context(|| format!("reading {}", path.display()))?;
    String::from_utf8(bytes).with_context(|| format!("{} is not text", path.display()))
}

/// One file's bytes.
pub(crate) fn bytes(path: &Path) -> Result<Vec<u8>> {
    files::default_backend()
        .read(path)
        .with_context(|| format!("reading {}", path.display()))
}

/// Whether there is anything at `path`.
pub(crate) fn exists(path: &Path) -> bool {
    files::default_backend().exists(path)
}

/// The names directly under `path`, each with whether it is a directory.
/// Empty for a path that is not a directory, which is what a walk wants.
pub(crate) fn list(path: &Path) -> Vec<(String, bool)> {
    files::default_backend().list(path)
}

/// Whether there is a file, rather than a directory or nothing, at `path`.
pub(crate) fn is_file(path: &Path) -> bool {
    let fs = files::default_backend();
    fs.exists(path) && !fs.is_dir(path)
}

/// `path` with `.` dropped, `..` popped and, on a real filesystem, every
/// symlink resolved. What a path is stripped against.
pub(crate) fn real(path: &Path) -> std::path::PathBuf {
    files::default_backend().canonicalize(path)
}
