//! Save games: a table in, a table out, and a version so an old file can be
//! brought forward rather than rejected.
//!
//! Nothing here is engine state. A save is whatever the game puts in it; the
//! engine only decides where it lives, that a half-written file cannot
//! replace a good one, and what version it was written at.
//!
//! ```toml
//! [save]
//! version = 3                       # what this build writes
//! migrate = "scripts/saves.rn"      # brings an older file forward
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::engine::Engine;

/// What `project.toml` says about saves.
///
/// A project with no `[save]` table writes version 1 and migrates nothing,
/// which is the right behaviour for a game that has not needed to change a
/// save's shape yet.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SaveConfig {
    /// The version this build writes. A file read at a lower one is migrated;
    /// a file at a higher one is refused, because a future save is not
    /// something an older build can guess at.
    pub version: u32,
    /// A script whose `migrate_save(version, data)` brings a file forward one
    /// version per call. Empty means the game has none.
    pub migrate: String,
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self {
            version: 1,
            migrate: String::new(),
        }
    }
}

impl SaveConfig {
    /// The `[save]` table of the project's manifest, or the defaults.
    #[must_use]
    pub fn load(eng: &Engine) -> Self {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            save: SaveConfig,
        }
        let Some(source) = crate::project::manifest_source(eng) else {
            return Self::default();
        };
        match toml::from_str::<Manifest>(&source) {
            Ok(manifest) => manifest.save,
            Err(err) => {
                tracing::warn!("project.toml [save]: {err}; using the defaults");
                Self::default()
            }
        }
    }
}

/// Where a slot lives. Slots are named by the game, so the name is checked
/// rather than trusted: a save called `../../id_rsa` is a bug or an attack.
fn path_of(eng: &Engine, slot: &str) -> Result<PathBuf> {
    if slot.is_empty() {
        bail!("a save slot needs a name");
    }
    if !slot
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        bail!("'{slot}' is not a slot name: letters, digits, '-' and '_' only");
    }
    let dir = crate::engine_api::user_data_dir_of(eng).join("saves");
    Ok(dir.join(format!("{slot}.toml")))
}

/// Write `data` to `slot`, stamped with the version this build writes.
///
/// Written beside the target and renamed over it, because the file a game
/// saves at a checkpoint is the one it cannot afford to find truncated.
pub fn write(eng: &Engine, slot: &str, data: &balaur_script::Value) -> Result<()> {
    let path = path_of(eng, slot)?;
    let config = SaveConfig::load(eng);
    let body = crate::node_api::to_toml(data).context("a save is a table of plain values")?;
    let mut doc = toml::map::Map::new();
    doc.insert(
        "version".into(),
        toml::Value::Integer(config.version.into()),
    );
    doc.insert("data".into(), body);
    let text = toml::to_string(&toml::Value::Table(doc))?;
    let fs = crate::files::backend(eng);
    let temporary = path.with_extension("toml.part");
    fs.write(&temporary, text.as_bytes())
        .with_context(|| format!("writing {}", temporary.display()))?;
    // The rename is only atomic against a crash; without this the promise
    // that a save cannot be found truncated does not survive a power cut.
    fs.sync(&temporary);
    fs.rename(&temporary, &path)
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Read `slot`, brought forward to the version this build writes.
///
/// Nil for a slot that does not exist — a game asking whether there is a save
/// should not have to handle an error to find out there is not.
pub fn read(eng: &Engine, slot: &str) -> Result<balaur_script::Value> {
    let path = path_of(eng, slot)?;
    let Ok(text) = crate::files::backend(eng)
        .read(&path)
        .and_then(|b| String::from_utf8(b).map_err(anyhow::Error::from))
    else {
        return Ok(balaur_script::Value::Nil);
    };
    let doc: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let version = doc
        .get("version")
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(1);
    let data = doc
        .get("data")
        .cloned()
        .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
    let config = SaveConfig::load(eng);
    if version > config.version {
        bail!(
            "{} was written by a newer build (save version {version}, this build writes {})",
            path.display(),
            config.version
        );
    }
    let data = crate::node_api::from_toml(&data)?;
    migrate(eng, &config, version, data)
}

/// Call the project's `migrate_save(version, data)` once per version step.
///
/// One step at a time is what makes migrations writable: each one only has to
/// know how the shape changed between two adjacent versions, never how to get
/// from any version to any other.
fn migrate(
    eng: &Engine,
    config: &SaveConfig,
    from: u32,
    mut data: balaur_script::Value,
) -> Result<balaur_script::Value> {
    if from == config.version {
        return Ok(data);
    }
    if config.migrate.is_empty() {
        bail!(
            "a save at version {from} needs bringing to {}, and no `[save] migrate` script says how",
            config.version
        );
    }
    let host = eng
        .script_host()
        .context("migrating a save needs a script backend")?;
    for version in from..config.version {
        let args = [balaur_script::Value::Int(i64::from(version)), data.clone()];
        data = host
            .call_in(&config.migrate, "migrate_save", &args)
            .with_context(|| format!("migrating a save from version {version}"))?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} has no `migrate_save(version, data)`, so a save at version {version} \
                     cannot be brought forward",
                    config.migrate
                )
            })?;
    }
    Ok(data)
}

/// Every slot that has been written, in name order.
pub fn slots(eng: &Engine) -> Vec<String> {
    let dir = crate::engine_api::user_data_dir_of(eng).join("saves");
    let mut out: Vec<String> = crate::files::backend(eng)
        .list(&dir)
        .into_iter()
        .filter(|(_, is_dir)| !is_dir)
        .filter_map(|(name, _)| name.strip_suffix(".toml").map(str::to_string))
        .collect();
    out.sort();
    out
}

/// Delete a slot. Not an error when it was not there — the caller wanted it
/// gone, and it is.
pub fn remove(eng: &Engine, slot: &str) -> Result<()> {
    let path = path_of(eng, slot)?;
    crate::files::backend(eng)
        .remove(&path)
        .map(|_| ())
        .with_context(|| format!("removing {}", path.display()))
}
