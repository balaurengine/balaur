//! The browser's files: a memory filesystem whose user directory is mirrored
//! into `localStorage`, so settings, saves, rebindings and a session token
//! survive a reload.
//!
//! `localStorage` rather than the origin-private file system because a
//! `FileBackend` is synchronous and OPFS is not on the main thread. The cost
//! is the quota, a few megabytes: a write that no longer fits stays in
//! memory for the tab's life and is logged once, and the game keeps running.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use anyhow::Result;
use balaur::files::{FileBackend, MemoryFs, lexical};
use base64::Engine as _;

/// The directory under the project root that a running game mirrors, as
/// `engine.user_data_dir` resolves it on a platform with no data directory.
const MIRRORED: &str = "user_data";

pub(crate) struct StorageFs {
    inner: MemoryFs,
    storage: Option<web_sys::Storage>,
    /// Every mirrored key starts with this, so one origin may host more than
    /// one game.
    key_prefix: String,
    /// Directories whose writes are kept. A game keeps its user directory; the
    /// editor keeps the project it is editing.
    mirrored: Vec<String>,
    quota_warned: Cell<bool>,
}

impl StorageFs {
    /// Open a game's mirror over its user directory and reload what an
    /// earlier visit kept.
    pub(crate) fn open(namespace: &str) -> Self {
        let fs = Self::mirroring(namespace, &[MIRRORED]);
        fs.restore();
        fs
    }

    /// A mirror over other directories, holding nothing yet.
    ///
    /// Restoring is the caller's to order: a caller that seeds the memory
    /// from a pack has to do it before [`Self::restore`], or the pack's copy
    /// of a file would land on top of the edit that was kept.
    pub(crate) fn mirroring(namespace: &str, mirrored: &[&str]) -> Self {
        let storage = web_sys::window().and_then(|w| w.local_storage().ok().flatten());
        Self {
            inner: MemoryFs::new(),
            storage,
            key_prefix: format!("balaur:{namespace}:"),
            mirrored: mirrored
                .iter()
                .map(|dir| dir.trim_matches('/').to_string())
                .collect(),
            quota_warned: Cell::new(false),
        }
    }

    /// Fill the memory from what earlier visits kept, over anything already
    /// there.
    pub(crate) fn restore(&self) {
        let Some(storage) = &self.storage else {
            return;
        };
        let count = storage.length().unwrap_or(0);
        let mut entries = Vec::new();
        for index in 0..count {
            let Ok(Some(key)) = storage.key(index) else {
                continue;
            };
            let Some(path) = key.strip_prefix(&self.key_prefix) else {
                continue;
            };
            let Ok(Some(encoded)) = storage.get_item(&key) else {
                continue;
            };
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                entries.push((path.to_string(), bytes));
            }
        }
        for (path, bytes) in entries {
            let _ = self.inner.write(Path::new(&path), &bytes);
        }
    }

    /// The mirror key for a path under one of the mirrored directories, or
    /// `None` for a path under none of them.
    fn key_of(&self, path: &Path) -> Option<String> {
        let text = lexical(path).to_string_lossy().replace('\\', "/");
        let text = text.trim_start_matches('/').to_string();
        self.mirrored
            .iter()
            .any(|dir| text == *dir || text.starts_with(&format!("{dir}/")))
            .then(|| format!("{}{text}", self.key_prefix))
    }

    /// Put a pack's files in memory without mirroring them: what was shipped
    /// is fetched again on the next visit, and only edits are worth keeping.
    pub(crate) fn seed(&self, root: &Path, entries: impl IntoIterator<Item = (String, Vec<u8>)>) {
        self.inner.seed(root, entries);
    }

    fn store(&self, path: &Path, bytes: &[u8]) {
        let (Some(storage), Some(key)) = (&self.storage, self.key_of(path)) else {
            return;
        };
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        if storage.set_item(&key, &encoded).is_err() && !self.quota_warned.replace(true) {
            tracing::warn!("localStorage refused a write; files past the quota stay in memory");
        }
    }

    /// Every mirror key at `path` or under it.
    fn keys_under(&self, path: &Path) -> Vec<String> {
        let (Some(storage), Some(key)) = (&self.storage, self.key_of(path)) else {
            return Vec::new();
        };
        let prefix = format!("{key}/");
        let count = storage.length().unwrap_or(0);
        (0..count)
            .filter_map(|index| storage.key(index).ok().flatten())
            .filter(|k| *k == key || k.starts_with(&prefix))
            .collect()
    }
}

impl FileBackend for StorageFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.inner.read(path)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.inner.write(path, bytes)?;
        self.store(path, bytes);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.inner.is_dir(path)
    }

    fn remove(&self, path: &Path) -> Result<bool> {
        let doomed = self.keys_under(path);
        let removed = self.inner.remove(path)?;
        if let Some(storage) = &self.storage {
            for key in doomed {
                let _ = storage.remove_item(&key);
            }
        }
        Ok(removed)
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        self.inner.mkdir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let old_keys = self.keys_under(from);
        self.inner.rename(from, to)?;
        if let Some(storage) = &self.storage {
            for key in old_keys {
                let _ = storage.remove_item(&key);
            }
        }
        // Re-store from memory: the moved files are exactly what is now at `to`.
        let moved = if self.inner.is_dir(to) {
            let mut out = Vec::new();
            collect_files(&self.inner, to, &mut out);
            out
        } else {
            vec![to.to_path_buf()]
        };
        for path in moved {
            if let Ok(bytes) = self.inner.read(&path) {
                self.store(&path, &bytes);
            }
        }
        Ok(())
    }

    fn mtime(&self, path: &Path) -> Option<f64> {
        self.inner.mtime(path)
    }

    fn list(&self, path: &Path) -> Vec<(String, bool)> {
        self.inner.list(path)
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        self.inner.canonicalize(path)
    }
}

fn collect_files(fs: &MemoryFs, dir: &Path, out: &mut Vec<PathBuf>) {
    for (name, is_dir) in fs.list(dir) {
        let child = dir.join(name);
        if is_dir {
            collect_files(fs, &child, out);
        } else {
            out.push(child);
        }
    }
}
