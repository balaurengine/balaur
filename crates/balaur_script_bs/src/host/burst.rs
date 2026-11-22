//! What the file watcher heard, gathered into one batch per burst: a checkout
//! of ten thousand files is one build and one `files_changed`, not ten thousand.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use balaur_core::time::Instant;

use super::{BsHost, EXTENSION, Value, key_of};

/// Quiet this long ends a burst: past the gap between a truncate and its
/// write, and between one file of a checkout and the next.
const QUIET: Duration = Duration::from_millis(60);
/// A writer that never rests is still heard.
const LONGEST: Duration = Duration::from_secs(2);

/// A folder `watch` added.
pub(super) struct Aside {
    /// As the watcher reports paths under it.
    pub(super) at: PathBuf,
    /// The folder as it was named, which `files_changed` spells paths from:
    /// a caller finds them under its own spelling, backslashes and all.
    pub(super) key: String,
    /// What `files_changed` names from it; empty is every file.
    pub(super) extensions: Vec<String>,
}

impl Aside {
    fn names(&self, extension: Option<&str>) -> bool {
        self.extensions.is_empty()
            || extension.is_some_and(|e| self.extensions.iter().any(|named| named == e))
    }

    fn spell(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.at).ok()?;
        Some(format!("{}/{}", self.key, key_of(&rel.to_string_lossy())))
    }
}

/// The paths heard since the last burst ended.
#[derive(Default)]
pub(super) struct Burst {
    keys: HashSet<String>,
    named: HashSet<String>,
    /// Every other file a run derives something from, which the asset cache
    /// and every consumer of its generation read again.
    content: HashSet<String>,
    /// The watcher lost track: anything may have changed.
    rescan: bool,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl BsHost {
    fn hear(&self, burst: &mut Burst, asides: &[Aside], path: &Path) {
        let extension = path.extension().and_then(|e| e.to_str());
        let aside = asides.iter().find(|aside| path.starts_with(&aside.at));
        if let Some(whole) = aside
            .filter(|aside| aside.names(extension))
            .and_then(|aside| aside.spell(path))
        {
            burst.named.insert(whole);
        }
        if extension != Some(EXTENSION) {
            self.hear_content(burst, aside, path);
            return;
        }
        let key = if let Some(whole) = aside.and_then(|aside| aside.spell(path)) {
            key_of(&whole)
        } else if let Ok(rel) = path.strip_prefix(&self.inner.watch_root) {
            key_of(&rel.to_string_lossy())
        } else if let Some((rel, folder)) = self
            .inner
            .watch_mounts
            .iter()
            .find_map(|(at, folder)| Some((path.strip_prefix(at).ok()?, folder)))
        {
            format!("{folder}/{}", key_of(&rel.to_string_lossy()))
        } else {
            return;
        };
        burst.keys.insert(key);
    }

    /// A file that is not a script, under the project or under a folder
    /// followed for every file. One under a dot folder is a tool's: a
    /// checkout's `.git` moves on every commit and nothing in a game reads it.
    fn hear_content(&self, burst: &mut Burst, aside: Option<&Aside>, path: &Path) {
        let (rel, key) = if let Some(aside) = aside {
            if !aside.extensions.is_empty() {
                return;
            }
            let Ok(rel) = path.strip_prefix(&aside.at) else {
                return;
            };
            (rel, aside.spell(path))
        } else if let Ok(rel) = path.strip_prefix(&self.inner.watch_root) {
            (rel, Some(key_of(&rel.to_string_lossy())))
        } else if let Some((rel, folder)) = self
            .inner
            .watch_mounts
            .iter()
            .find_map(|(at, folder)| Some((path.strip_prefix(at).ok()?, folder)))
        {
            (rel, Some(format!("{folder}/{}", key_of(&rel.to_string_lossy()))))
        } else {
            return;
        };
        let hidden = rel
            .components()
            .any(|part| part.as_os_str().to_string_lossy().starts_with('.'));
        let Some(key) = key.filter(|_| !hidden) else {
            return;
        };
        if aside.is_none() {
            burst.named.insert(key.clone());
        }
        burst.content.insert(key);
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "how long file writes have been quiet; never a simulation input"
    )]
    fn settled_burst(&self) -> Option<Burst> {
        let events = self.inner.events.as_ref()?;
        let asides = self.inner.watch_asides.borrow();
        let mut burst = self.inner.burst.borrow_mut();
        let mut heard = false;
        while let Ok(event) = events.try_recv() {
            heard = true;
            let Ok(event) = event else {
                burst.rescan = true;
                continue;
            };
            burst.rescan |= event.need_rescan();
            for path in &event.paths {
                self.hear(&mut burst, &asides, path);
            }
        }
        let now = Instant::now();
        if heard {
            burst.first.get_or_insert(now);
            burst.last = Some(now);
            balaur_core::wake::at(now + QUIET);
        }
        let (first, last) = (burst.first?, burst.last?);
        if now - last < QUIET && now - first < LONGEST {
            return None;
        }
        Some(std::mem::take(&mut *burst))
    }

    /// Every key something here was built from or has read, which is all an
    /// edit can matter to: the rest of a checkout is never opened.
    fn remembered_keys(&self) -> HashSet<String> {
        let mut keys = HashSet::new();
        for (key, file) in self.inner.files.borrow().iter() {
            keys.insert(key.clone());
            keys.extend(file.compiled.meta.uses.iter().cloned());
            keys.extend(
                file.compiled
                    .linked
                    .iter()
                    .map(|(linked, _)| linked.clone()),
            );
        }
        keys.extend(self.inner.units.borrow().keys().cloned());
        keys.extend(self.inner.sources.borrow().keys().cloned());
        keys
    }

    /// The `.bs` files whose text changed in the burst that just ended, as
    /// keys, and an empty `files_changed` when the watcher lost count.
    pub(super) fn edited(&self) -> Vec<String> {
        let Some(burst) = self.settled_burst() else {
            return vec![];
        };
        if burst.rescan || !burst.content.is_empty() {
            let mut content: Vec<String> = if burst.rescan {
                vec![]
            } else {
                burst.content.into_iter().collect()
            };
            content.sort_unstable();
            match content.as_slice() {
                [] => tracing::info!("hot reloaded every file"),
                [one] => tracing::info!("hot reloaded {one}"),
                many => tracing::info!("hot reloaded {} files", many.len()),
            }
            balaur_core::assets::changed_on_disk(&self.inner.engine, &content);
        }
        if burst.rescan || !burst.named.is_empty() {
            let mut paths: Vec<String> = if burst.rescan {
                vec![]
            } else {
                burst.named.into_iter().collect()
            };
            paths.sort_unstable();
            balaur_core::events::emit(
                &self.inner.engine,
                balaur_core::events::FILES_CHANGED,
                Value::List(paths.into_iter().map(Value::Str).collect()),
            );
        }
        let remembered = self.remembered_keys();
        let mut keys: Vec<String> = if burst.rescan {
            remembered.into_iter().collect()
        } else {
            burst
                .keys
                .into_iter()
                .filter(|key| remembered.contains(key))
                .collect()
        };
        keys.sort_unstable();
        keys.retain(|key| {
            let before = self.inner.read.borrow().get(key).cloned();
            // A file gone is a change; `source` records what it read.
            self.source(key).is_err() || self.inner.read.borrow().get(key) != before.as_ref()
        });
        keys
    }

    /// `edited` made stale, and each file that uses one queued to reload:
    /// a module is no script of its own, so what uses it reloads.
    pub(super) fn queue_users_of(&self, edited: &[String], todo: &mut Vec<String>) {
        if edited.is_empty() {
            return;
        }
        self.invalidate(edited);
        let edited: BTreeSet<&str> = edited.iter().map(String::as_str).collect();
        let mut queued: BTreeSet<String> = todo.iter().cloned().collect();
        for (key, file) in self.inner.files.borrow().iter() {
            let uses = file.compiled.meta.uses.iter().map(String::as_str);
            let links = file
                .compiled
                .linked
                .iter()
                .map(|(linked, _)| linked.as_str());
            if edited.contains(key.as_str()) || uses.chain(links).any(|k| edited.contains(k)) {
                self.inner.reload_owed.borrow_mut().push(key.clone());
                if queued.insert(key.clone()) {
                    todo.push(key.clone());
                }
            }
        }
    }
}
