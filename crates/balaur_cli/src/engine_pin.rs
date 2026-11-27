//! The engine a project is made with. `[application] engine` names the
//! Balaur build as `balaur --version` does (`v0.2.0-alpha.1`,
//! `nightly-577592f`): written when a project is made and by `balaur pin`, said
//! by a run or an export on another build, and what the Forge builds the
//! project on, so a game is built with the engine it was made with.

use std::path::Path;

use anyhow::{Context as _, Result};

const APPLICATION: &str = "application";
const ENGINE: &str = "engine";
const MANIFEST: &str = "project.toml";

/// `engine`, or this build's own id, written as the project's at `path`.
pub(crate) fn pin(path: &Path, engine: Option<&str>) -> Result<()> {
    let engine = engine
        .or(crate::version::build_id())
        .context("a build from source has no id to pin: name one with --engine")?;
    write(path, engine)?;
    tracing::info!("{} is made with balaur {engine}", path.display());
    Ok(())
}

/// This build's id into a project it has just made; one from source pins
/// nothing, having no id of its own.
pub(crate) fn pin_new(path: &Path) -> Result<()> {
    crate::version::build_id().map_or(Ok(()), |own| write(path, own))
}

fn write(path: &Path, engine: &str) -> Result<()> {
    let file = path.join(MANIFEST);
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let mut document: toml_edit::DocumentMut = text.parse().context("parsing project.toml")?;
    let application = document
        .entry(APPLICATION)
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .context("[application] in project.toml is not a table")?;
    // The value alone is replaced: the comment above the key, the spacing
    // round it and one after it stay as the file has them.
    match application
        .get_mut(ENGINE)
        .and_then(toml_edit::Item::as_value_mut)
    {
        Some(held) => {
            let decor = held.decor().clone();
            *held = engine.into();
            *held.decor_mut() = decor;
        }
        None => {
            application.insert(ENGINE, toml_edit::value(engine));
        }
    }
    std::fs::write(&file, document.to_string())
        .with_context(|| format!("writing {}", file.display()))
}

/// The build the project at `path` names, where it names one.
pub(crate) fn named(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path.join(MANIFEST)).ok()?;
    let manifest: toml::Table = toml::from_str(&text).ok()?;
    manifest
        .get(APPLICATION)?
        .get(ENGINE)?
        .as_str()
        .filter(|engine| !engine.is_empty())
        .map(str::to_owned)
}

/// Said where the project names another build than this one. A build from
/// source is the engine's own work in progress, and says nothing.
pub(crate) fn say_if_other(path: &Path) {
    if let (Some(own), Some(named)) = (crate::version::build_id(), named(path))
        && own != named
    {
        tracing::warn!(
            "this project is made with balaur {named}, and this is {own}: `balaur pin` makes it this one"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_writes_the_build_into_application_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MANIFEST),
            "# kept\n[application]\nname = \"game\"\n\n[window]\nwidth = 390\n",
        )
        .unwrap();
        pin(dir.path(), Some("nightly-577592f")).unwrap();
        assert_eq!(named(dir.path()).as_deref(), Some("nightly-577592f"));
        let text = std::fs::read_to_string(dir.path().join(MANIFEST)).unwrap();
        assert!(text.starts_with("# kept\n"), "the rest as it was: {text}");
        assert!(text.contains("width = 390"), "{text}");
        pin(dir.path(), Some("v0.2.0-alpha.1")).unwrap();
        assert_eq!(named(dir.path()).as_deref(), Some("v0.2.0-alpha.1"));
    }

    #[test]
    fn a_pin_keeps_the_comments_and_layout_round_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let before = "[application]\nname = \"game\"\n# The Balaur build the game is made with\n# (`balaur pin`): the Forge builds on it.\nengine   =   \"nightly-9818040\"   # kept too\n\n# the window\n[window]\nwidth = 390\n";
        std::fs::write(dir.path().join(MANIFEST), before).unwrap();
        pin(dir.path(), Some("nightly-cac507c")).unwrap();
        let after = std::fs::read_to_string(dir.path().join(MANIFEST)).unwrap();
        assert_eq!(after, before.replace("nightly-9818040", "nightly-cac507c"));
    }

    #[test]
    fn a_project_with_no_application_table_gets_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MANIFEST), "[window]\nwidth = 390\n").unwrap();
        pin(dir.path(), Some("v0.2.0-alpha.1")).unwrap();
        assert_eq!(named(dir.path()).as_deref(), Some("v0.2.0-alpha.1"));
    }
}
