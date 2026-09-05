//! The bytes behind a sound path, kept between plays.
//!
//! Every `play` used to re-read the file from disk or the pack, so a footstep
//! cost a read per step. The cache is bounded and holds the *encoded* bytes:
//! rodio decodes per playback, and two overlapping plays of one file need two
//! decoders anyway.

use anyhow::Result;
use balaur_core::project::ProjectFiles;
use balaur_core::{DetHashMap, Engine};

/// How much audio the cache may hold, and the largest file it will take. A
/// music track streams past it rather than evicting every effect.
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRY: usize = 2 * 1024 * 1024;

struct Entry {
    bytes: Vec<u8>,
    /// When the file on disk was last written, so an edit in the editor is
    /// picked up; `None` for bytes that came out of a pack and cannot change.
    stamp: Option<f64>,
}

/// Sound bytes by project-relative path, oldest first so the bound evicts in
/// a fixed order rather than a hashed one.
#[derive(Default)]
pub struct SoundCache {
    entries: DetHashMap<String, Entry>,
    held: usize,
}

impl SoundCache {
    fn get(&self, path: &str, stamp: Option<f64>) -> Option<Vec<u8>> {
        let entry = self.entries.get(path)?;
        (entry.stamp == stamp).then(|| entry.bytes.clone())
    }

    fn put(&mut self, path: &str, stamp: Option<f64>, bytes: &[u8]) {
        if bytes.len() > MAX_ENTRY {
            return;
        }
        if let Some(old) = self.entries.shift_remove(path) {
            self.held -= old.bytes.len();
        }
        while self.held + bytes.len() > MAX_BYTES {
            let Some((_, evicted)) = self.entries.shift_remove_index(0) else {
                break;
            };
            self.held -= evicted.bytes.len();
        }
        self.held += bytes.len();
        self.entries.insert(
            path.to_string(),
            Entry {
                bytes: bytes.to_vec(),
                stamp,
            },
        );
    }

    /// How many bytes the cache is holding, for a test that it stays bounded.
    #[must_use]
    pub const fn held(&self) -> usize {
        self.held
    }
}

/// The bytes a sound path names, through the pack-aware project reader — from
/// the cache when the file is unchanged since it was read.
///
/// # Errors
/// If no permitted source has the file; the reader says where it looked.
pub fn read(eng: &Engine, path: &str) -> Result<Vec<u8>> {
    let files = eng.resource::<ProjectFiles>();
    let stamp = modified(&files.borrow(), path);
    let cache = eng.resource::<SoundCache>();
    if let Some(bytes) = cache.borrow().get(path, stamp) {
        return Ok(bytes);
    }
    let bytes = files.borrow().read(path)?;
    cache.borrow_mut().put(path, stamp, &bytes);
    Ok(bytes)
}

/// When the file behind a path was last written, through the project's own
/// backend, so a browser's virtual files are stamped like disk ones.
fn modified(files: &ProjectFiles, path: &str) -> Option<f64> {
    files.mtime(path)
}
