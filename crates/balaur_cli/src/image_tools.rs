//! `balaur shrink` and `balaur atlas`: the image verbs, which need the
//! importers and say so in a build without them.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::Command;

/// `balaur shrink` or `balaur atlas`, whichever `command` is.
pub(crate) fn run(command: Command) -> Result<()> {
    match command {
        Command::Shrink {
            project,
            tag,
            scale,
        } => shrink(&project, &tag, scale),
        Command::Atlas {
            inputs,
            name,
            project,
            fps,
        } => atlas(&inputs, &project, &name, fps),
        _ => anyhow::bail!("only the image verbs are routed here"),
    }
}

#[cfg(feature = "import")]
/// `balaur shrink`: the copies written, what was left alone and why, and the
/// bytes it came to.
fn shrink(project: &Path, tag: &str, scale: f32) -> Result<()> {
    let done = balaur_import::shrink::shrink(project, tag, scale)?;
    for (path, why) in &done.skipped {
        tracing::info!("left {path} alone: {why}");
    }
    let saved = done.before.saturating_sub(done.after);
    tracing::info!(
        "{} images at {scale} for '{tag}': {:.1} MB -> {:.1} MB, {:.1} MB saved",
        done.written.len(),
        done.before as f64 / 1e6,
        done.after as f64 / 1e6,
        saved as f64 / 1e6,
    );
    Ok(())
}

/// `balaur atlas`: the files written and what went onto the page.
#[cfg(feature = "import")]
fn atlas(inputs: &[PathBuf], project: &Path, name: &str, fps: f32) -> Result<()> {
    if !(fps > 0.0 && fps.is_finite()) {
        anyhow::bail!("--fps is frames a second, above zero, not {fps}");
    }
    let milliseconds = (1000.0 / fps).round().max(1.0) as u32;
    let done = balaur_import::atlas_into(inputs, project, name, milliseconds)?;
    for rel in &done.files {
        tracing::info!("wrote {}", project.join(rel).display());
    }
    tracing::info!("{}", done.note);
    Ok(())
}

#[cfg(not(feature = "import"))]
fn atlas(inputs: &[PathBuf], project: &Path, name: &str, fps: f32) -> Result<()> {
    let _ = (inputs, project, name, fps);
    anyhow::bail!("this build has no importers: build with the `import` feature")
}

/// Shrinking reads and writes images, which is the importers' half of the
/// tree; a build without them says so rather than not offering the verb.
#[cfg(not(feature = "import"))]
fn shrink(project: &Path, tag: &str, scale: f32) -> Result<()> {
    let _ = (project, tag, scale);
    anyhow::bail!("this build has no importers: build with the `import` feature")
}
