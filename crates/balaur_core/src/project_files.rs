//! Where a project's bytes come from: the source tree, a pack, or both, as
//! the `assets` rule in `project.toml` allows.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use serde::Deserialize;

use crate::engine::Engine;

/// Where a project is allowed to read its bytes from, set by `assets` in
/// `project.toml`. The default is deliberately the strict one: a shipped game
/// that quietly falls back to the working directory runs on the machine that
/// built it and nowhere else.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetSource {
    /// The pack only. A miss is an error naming the file.
    #[default]
    Embedded,
    /// The project directory only, ignoring anything packed.
    Files,
    /// The pack first, then the directory — loose DLC, mods, or an override
    /// folder shipped beside the executable.
    #[serde(rename = "embedded+files")]
    EmbeddedThenFiles,
}

/// Where a project's bytes come from: the source tree while developing, the
/// pack itself in a shipped game. Every reader of a binary asset goes through
/// this, which is what lets a packed game ship as one file.
pub struct ProjectFiles {
    root: std::path::PathBuf,
    packed: std::collections::BTreeMap<String, Vec<u8>>,
    source: AssetSource,
    /// Where loose files come from. Held rather than reached for through the
    /// engine, because an asset loader has the files and not the engine.
    fs: std::rc::Rc<dyn crate::files::FileBackend>,
    /// The `assets/index.toml` a pack carries; a dev run reads the file.
    packed_index: Option<String>,
    /// `id → path`, parsed on the first `id:
    // ` and dropped by
    /// [`Self::reload_index`].
    index: std::cell::RefCell<Option<BTreeMap<String, String>>>,
}

impl ProjectFiles {
    /// Reads from the source tree. Nothing is packed, so `assets` cannot
    /// forbid the only source there is.
    #[must_use]
    pub fn directory(root: std::path::PathBuf) -> Self {
        Self {
            root,
            packed: std::collections::BTreeMap::new(),
            source: AssetSource::Files,
            fs: crate::files::default_backend(),
            packed_index: None,
            index: std::cell::RefCell::new(None),
        }
    }

    /// Serves a pack's assets under the project's `assets` rule.
    #[must_use]
    pub fn packed(
        root: std::path::PathBuf,
        assets: std::collections::BTreeMap<String, Vec<u8>>,
        source: AssetSource,
    ) -> Self {
        Self {
            root,
            packed: assets,
            source,
            fs: crate::files::default_backend(),
            packed_index: None,
            index: std::cell::RefCell::new(None),
        }
    }

    /// Serve loose files from `fs` rather than the disk.
    #[must_use]
    pub fn on(mut self, fs: std::rc::Rc<dyn crate::files::FileBackend>) -> Self {
        self.fs = fs;
        self
    }

    /// The id index a pack carries, as the text of `assets/index.toml`.
    #[must_use]
    pub fn with_index(mut self, text: Option<String>) -> Self {
        self.packed_index = text;
        self
    }

    /// Drop the parsed id index so the next `id:
    // ` re-reads
    /// `assets/index.toml`. What the watcher calls when that file is saved.
    pub fn reload_index(&self) {
        *self.index.borrow_mut() = None;
    }

    /// The path an `id:
    // <id>` reference names, with any `#entry` kept; a
    /// reference that is already a path comes back as it is.
    ///
    /// # Errors
    /// When the id is not in `assets/index.toml`.
    pub fn path_of(&self, reference: &str) -> Result<String> {
        let Some(rest) = reference.strip_prefix(crate::assets::ID_PREFIX) else {
            return Ok(reference.to_string());
        };
        let (id, entry) = rest.split_once('#').map_or((rest, ""), |(id, e)| (id, e));
        let path = self.index_entry(id).ok_or_else(|| {
            anyhow!(
                "'{reference}' names no asset: '{id}' is not in {}",
                crate::assets::INDEX_PATH
            )
        })?;
        Ok(if entry.is_empty() {
            path
        } else {
            format!("{path}#{entry}")
        })
    }

    /// The id `assets/index.toml` gives a project-relative path, if any.
    #[must_use]
    pub fn id_of(&self, path: &str) -> Option<String> {
        self.ensure_index();
        self.index
            .borrow()
            .as_ref()?
            .iter()
            .find(|(_, p)| p.as_str() == path)
            .map(|(id, _)| id.clone())
    }

    fn index_entry(&self, id: &str) -> Option<String> {
        self.ensure_index();
        self.index.borrow().as_ref()?.get(id).cloned()
    }

    fn ensure_index(&self) {
        if self.index.borrow().is_some() {
            return;
        }
        let text = match (&self.packed_index, self.source) {
            (Some(text), _) => Some(text.clone()),
            (None, AssetSource::Embedded) => None,
            (None, _) => self
                .fs
                .read(&self.root.join(crate::assets::INDEX_PATH))
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok()),
        };
        let parsed = text.map_or_else(BTreeMap::new, |text| {
            crate::asset_index::parse(&text).unwrap_or_else(|err| {
                tracing::warn!("{}: {err}", crate::assets::INDEX_PATH);
                BTreeMap::new()
            })
        });
        *self.index.borrow_mut() = Some(parsed);
    }

    #[must_use]
    pub const fn source(&self) -> AssetSource {
        self.source
    }

    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// When the file behind a path last changed, in the backend's own units,
    /// or `None` for one only the pack holds — the pack never changes.
    #[must_use]
    pub fn mtime(&self, path: &str) -> Option<f64> {
        let path = &self.path_of(path).ok()?;
        let p = std::path::Path::new(path);
        if crate::files::rooted(p) {
            return self.fs.mtime(p);
        }
        if self.source == AssetSource::Embedded {
            return None;
        }
        self.fs.mtime(&self.root.join(p))
    }

    /// The bytes for a project-relative path. An absolute path is read from
    /// disk as given, so a tool pointing outside the project still works.
    ///
    /// # Errors
    /// If no permitted source has the file. The message names every place
    /// that was tried, because "asset not found" without a location is the
    /// least useful sentence a shipped game can print.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let path = &self.path_of(path)?;
        let p = std::path::Path::new(path);
        if crate::files::rooted(p) {
            return self.fs.read(p);
        }
        // Separators are normalised because a pack is keyed the way it was
        // built, which is always with forward slashes. Rewritten only where
        // there is a separator to rewrite: every read takes this path.
        let key = if path.contains('\\') {
            std::borrow::Cow::Owned(path.replace('\\', "/"))
        } else {
            std::borrow::Cow::Borrowed(path.as_str())
        };
        let embedded = matches!(
            self.source,
            AssetSource::Embedded | AssetSource::EmbeddedThenFiles
        );
        if embedded && let Some(bytes) = self.packed.get(key.as_ref()) {
            return Ok(bytes.clone());
        }
        if self.source != AssetSource::Embedded {
            let full = self.root.join(p);
            if let Ok(bytes) = self.fs.read(&full) {
                return Ok(bytes);
            }
            if embedded {
                return Err(anyhow!(
                    "no asset '{path}': not in the pack, and not at {}",
                    full.display()
                ));
            }
            return Err(anyhow!("no asset '{path}': nothing at {}", full.display()));
        }
        Err(anyhow!(
            "no asset '{path}' in the pack. It ships only what `balaur export` \
             collected; set `assets = \"embedded+files\"` in project.toml to also \
             read files beside the game."
        ))
    }

    /// Project-relative paths directly under `dir`, from the pack and from
    /// disk, sorted and deduplicated. A packed game has no directory to walk,
    /// so anything that discovers files by scanning one asks here instead.
    #[must_use]
    pub fn list(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        let mut out: Vec<String> = if self.source == AssetSource::Files {
            Vec::new()
        } else {
            self.packed
                .keys()
                .filter(|k| k.starts_with(&prefix) && !k[prefix.len()..].contains('/'))
                .cloned()
                .collect()
        };
        if self.source != AssetSource::Embedded {
            for (name, is_dir) in self.fs.list(&self.root.join(dir)) {
                if !is_dir {
                    out.push(format!("{prefix}{name}"));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// The path a reference names: what `id:
// <id>` resolves to through
/// `assets/index.toml`, or the reference itself when it is already a path.
///
/// # Errors
/// When the id is not in the index.
pub fn path_of(eng: &Engine, reference: &str) -> Result<String> {
    if !reference.starts_with(crate::assets::ID_PREFIX) {
        return Ok(reference.to_string());
    }
    let files = eng
        .try_resource::<ProjectFiles>()
        .ok_or_else(|| anyhow!("'{reference}' cannot resolve: this app has no project files"))?;
    let path = files.borrow().path_of(reference)?;
    Ok(path)
}
