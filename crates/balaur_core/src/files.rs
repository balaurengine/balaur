//! Where the engine's files come from.
//!
//! Everything that reads or writes a project's files goes through a
//! [`FileBackend`] held as an engine resource, rather than calling
//! `std::fs` directly. On a desktop the backend is [`DiskFs`] and the calls
//! are the same ones as before; in a browser there is no project directory at
//! all, and [`MemoryFs`] serves a project fetched into memory — which is what
//! lets the editor, a program whose whole job is reading and writing a
//! project, run in a tab.
//!
//! The trait is synchronous because the script API is: `fs::read` returns a
//! value to a Rune script that is mid-frame, and no amount of plumbing makes
//! that await. A browser backend therefore has to have the bytes already,
//! which is why the web entry point seeds one from a pack before booting.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use anyhow::{anyhow, Context as _, Result};

use crate::engine::Engine;

/// The file operations the engine and its scripts need.
///
/// Paths arriving here are absolute and already checked against the roots the
/// host allowed (see `file_api::resolve`), so a backend does no permission
/// work of its own — it only has to answer for the paths it owns.
pub trait FileBackend {
    fn read(&self, path: &Path) -> Result<Vec<u8>>;
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// Delete a file, or a directory and everything under it. Answers whether
    /// there was anything there.
    fn remove(&self, path: &Path) -> Result<bool>;
    fn mkdir(&self, path: &Path) -> Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> Result<()>;
    /// When the file last changed, in seconds, or `None` for one that is not
    /// there. Only ever compared with another answer from the same backend:
    /// a virtual one counts edits rather than reading a clock.
    fn mtime(&self, path: &Path) -> Option<f64>;
    /// The names directly under `path`, each with whether it is a directory.
    /// Dotfiles are the caller's to filter; this reports what is there.
    fn list(&self, path: &Path) -> Vec<(String, bool)>;
    /// The path with `.` dropped, `..` popped and — on a real filesystem —
    /// every symlink resolved. What `file_api` compares against its roots, so
    /// a backend that can be tricked here can be escaped.
    fn canonicalize(&self, path: &Path) -> PathBuf;
}

/// The backend in use, as an engine resource. Absent means the thread's
/// default, which is [`DiskFs`] unless a host replaced it.
pub struct Files(pub Rc<dyn FileBackend>);

thread_local! {
    /// What an app with no backend of its own gets.
    ///
    /// A thread-local rather than a field on `AppConfig` because the choice is
    /// the host's and is made once, before any app exists — a browser tab has
    /// one filesystem and everything in it uses that one. Native hosts never
    /// set it, so every existing call keeps reaching the disk.
    static DEFAULT: RefCell<Option<Rc<dyn FileBackend>>> = const { RefCell::new(None) };
}

/// Install the backend every app on this thread uses unless it holds its own.
/// Call it before booting; changing it under a running app changes nothing
/// already resolved.
pub fn set_default(fs: Rc<dyn FileBackend>) {
    DEFAULT.with(|d| *d.borrow_mut() = Some(fs));
}

#[must_use]
pub fn default_backend() -> Rc<dyn FileBackend> {
    DEFAULT
        .with(|d| d.borrow().clone())
        .unwrap_or_else(|| Rc::new(DiskFs))
}

/// The backend this engine reads and writes through: its own if one was
/// installed, otherwise the thread's default.
pub fn backend(eng: &Engine) -> Rc<dyn FileBackend> {
    eng.try_resource::<Files>()
        .map_or_else(default_backend, |f| f.borrow().0.clone())
}

/// Give one engine a backend of its own.
pub fn set_backend(eng: &Engine, fs: Rc<dyn FileBackend>) {
    eng.insert_resource(Files(fs));
}

/// `.` dropped and `..` popped, without touching a filesystem.
pub fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The real filesystem.
pub struct DiskFs;

impl FileBackend for DiskFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path).with_context(|| format!("reading '{}'", path.display()))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes).with_context(|| format!("writing '{}'", path.display()))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn remove(&self, path: &Path) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        Ok(true)
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)?;
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        if let Some(parent) = to.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(from, to)?;
        Ok(())
    }

    fn mtime(&self, path: &Path) -> Option<f64> {
        std::fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs_f64())
    }

    fn list(&self, path: &Path) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                out.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path().is_dir(),
                ));
            }
        }
        out
    }

    /// Symlinks resolved in the part of the path that exists, so a link out of
    /// a permitted root is caught before the root check.
    fn canonicalize(&self, path: &Path) -> PathBuf {
        let lex = lexical(path);
        let mut at = lex.as_path();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        loop {
            if let Ok(real) = at.canonicalize() {
                let mut out = real;
                for part in tail.iter().rev() {
                    out.push(part);
                }
                return out;
            }
            match (at.parent(), at.file_name()) {
                (Some(parent), Some(name)) => {
                    tail.push(name.to_os_string());
                    at = parent;
                }
                _ => return lex,
            }
        }
    }
}

/// A project held in memory: the browser's filesystem.
///
/// Seeded from a pack — or from nothing, for a project that starts empty —
/// and written to as the editor works. Nothing here outlives the tab by
/// itself; keeping a project is the page's job, and [`MemoryFs::snapshot`] is
/// what it saves.
#[derive(Default)]
pub struct MemoryFs {
    inner: RefCell<Inner>,
}

#[derive(Default)]
struct Inner {
    files: BTreeMap<String, Vec<u8>>,
    /// Directories that exist with nothing in them. A directory holding files
    /// needs no entry: it is implied by their keys.
    dirs: BTreeSet<String>,
    /// Stands in for a clock. Every write bumps it, so a script comparing two
    /// answers sees the later edit as later, which is all `mtime` is for.
    clock: f64,
}

/// The key a path is stored under: lexically normalised, forward slashes, no
/// trailing separator. A virtual filesystem has no working directory, so a
/// relative path is stored as given rather than resolved against one.
fn key(path: &Path) -> String {
    let text = lexical(path).to_string_lossy().replace('\\', "/");
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Every key that is `dir`'s child, and whether the child is a directory.
fn children(inner: &Inner, dir: &str) -> Vec<(String, bool)> {
    let prefix = if dir == "/" {
        "/".to_string()
    } else {
        format!("{dir}/")
    };
    let mut out: BTreeMap<String, bool> = BTreeMap::new();
    let names = inner.files.keys().chain(inner.dirs.iter());
    for full in names {
        let Some(rest) = full.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        if let Some((name, _)) = rest.split_once('/') {
            out.insert(name.to_string(), true);
        } else {
            // A key that is also a prefix of another is a directory; the
            // `dirs` set holds the empty ones, which are never files.
            let is_dir = inner.dirs.contains(full) && !inner.files.contains_key(full);
            out.entry(rest.to_string()).or_insert(is_dir);
        }
    }
    out.into_iter().collect()
}

impl MemoryFs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed `root/<key>` for every entry of a map keyed the way a pack is:
    /// project-relative, forward slashes.
    pub fn seed(&self, root: &Path, entries: impl IntoIterator<Item = (String, Vec<u8>)>) {
        let base = key(root);
        let mut inner = self.inner.borrow_mut();
        for (rel, bytes) in entries {
            let rel = rel.trim_start_matches('/');
            inner.files.insert(format!("{base}/{rel}"), bytes);
        }
        inner.clock += 1.0;
    }

    /// Every file, keyed as it is stored. What a page saves to keep a project.
    #[must_use]
    pub fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        self.inner.borrow().files.clone()
    }

    /// How many times this filesystem has been written to. A page watching for
    /// unsaved work polls it rather than diffing a snapshot.
    #[must_use]
    pub fn revision(&self) -> f64 {
        self.inner.borrow().clock
    }
}

impl FileBackend for MemoryFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.inner
            .borrow()
            .files
            .get(&key(path))
            .cloned()
            .ok_or_else(|| anyhow!("no file '{}'", path.display()))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner.files.insert(key(path), bytes.to_vec());
        inner.clock += 1.0;
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        let k = key(path);
        let inner = self.inner.borrow();
        inner.files.contains_key(&k) || inner.dirs.contains(&k) || !children(&inner, &k).is_empty()
    }

    fn is_dir(&self, path: &Path) -> bool {
        let k = key(path);
        let inner = self.inner.borrow();
        inner.dirs.contains(&k) || !children(&inner, &k).is_empty()
    }

    fn remove(&self, path: &Path) -> Result<bool> {
        let k = key(path);
        let mut inner = self.inner.borrow_mut();
        let prefix = format!("{k}/");
        let doomed: Vec<String> = inner
            .files
            .keys()
            .filter(|f| **f == k || f.starts_with(&prefix))
            .cloned()
            .collect();
        let had_dir = inner.dirs.remove(&k);
        if doomed.is_empty() && !had_dir {
            return Ok(false);
        }
        for f in doomed {
            inner.files.remove(&f);
        }
        inner.dirs.retain(|d| !d.starts_with(&prefix));
        inner.clock += 1.0;
        Ok(true)
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner.dirs.insert(key(path));
        inner.clock += 1.0;
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let (from_key, to_key) = (key(from), key(to));
        let mut inner = self.inner.borrow_mut();
        let prefix = format!("{from_key}/");
        let moving: Vec<String> = inner
            .files
            .keys()
            .filter(|f| **f == from_key || f.starts_with(&prefix))
            .cloned()
            .collect();
        if moving.is_empty() {
            return Err(anyhow!("no file '{}'", from.display()));
        }
        for old in moving {
            let Some(bytes) = inner.files.remove(&old) else {
                continue;
            };
            let new = if old == from_key {
                to_key.clone()
            } else {
                format!("{to_key}/{}", &old[prefix.len()..])
            };
            inner.files.insert(new, bytes);
        }
        inner.clock += 1.0;
        Ok(())
    }

    fn mtime(&self, path: &Path) -> Option<f64> {
        let inner = self.inner.borrow();
        inner.files.contains_key(&key(path)).then_some(inner.clock)
    }

    fn list(&self, path: &Path) -> Vec<(String, bool)> {
        children(&self.inner.borrow(), &key(path))
    }

    /// Lexical only: there is no filesystem to ask and no symlink to follow.
    fn canonicalize(&self, path: &Path) -> PathBuf {
        lexical(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> MemoryFs {
        let fs = MemoryFs::new();
        fs.seed(
            Path::new("/p"),
            [
                ("project.toml".to_string(), b"name = 'x'".to_vec()),
                ("scenes/main.toml".to_string(), b"[[nodes]]".to_vec()),
                ("scripts/a.rn".to_string(), b"pub fn init(this) {}".to_vec()),
            ],
        );
        fs
    }

    #[test]
    fn reads_what_it_was_seeded_with() {
        let fs = seeded();
        assert_eq!(
            fs.read(Path::new("/p/project.toml")).unwrap(),
            b"name = 'x'"
        );
        assert!(fs.exists(Path::new("/p/scenes/main.toml")));
        assert!(fs.read(Path::new("/p/nope.toml")).is_err());
    }

    #[test]
    fn a_write_is_read_back_and_moves_the_clock() {
        let fs = seeded();
        let before = fs.mtime(Path::new("/p/project.toml")).unwrap();
        fs.write(Path::new("/p/scenes/main.toml"), b"[[nodes]]\nname='B'")
            .unwrap();
        assert_eq!(
            fs.read(Path::new("/p/scenes/main.toml")).unwrap(),
            b"[[nodes]]\nname='B'"
        );
        assert!(fs.mtime(Path::new("/p/scenes/main.toml")).unwrap() > before);
    }

    #[test]
    fn a_write_makes_its_directories() {
        let fs = seeded();
        fs.write(Path::new("/p/animations/walk.toml"), b"type='x'")
            .unwrap();
        assert!(fs.is_dir(Path::new("/p/animations")));
        assert_eq!(
            fs.list(Path::new("/p/animations")),
            vec![("walk.toml".to_string(), false)]
        );
    }

    #[test]
    fn listing_names_files_and_directories() {
        let fs = seeded();
        assert_eq!(
            fs.list(Path::new("/p")),
            vec![
                ("project.toml".to_string(), false),
                ("scenes".to_string(), true),
                ("scripts".to_string(), true),
            ]
        );
    }

    #[test]
    fn removing_a_directory_takes_what_is_under_it() {
        let fs = seeded();
        assert!(fs.remove(Path::new("/p/scenes")).unwrap());
        assert!(!fs.exists(Path::new("/p/scenes/main.toml")));
        assert!(!fs.remove(Path::new("/p/scenes")).unwrap());
    }

    #[test]
    fn renaming_moves_a_subtree() {
        let fs = seeded();
        fs.rename(Path::new("/p/scripts"), Path::new("/p/src"))
            .unwrap();
        assert!(fs.read(Path::new("/p/src/a.rn")).is_ok());
        assert!(!fs.exists(Path::new("/p/scripts/a.rn")));
    }

    #[test]
    fn dot_dot_is_popped_before_the_key() {
        let fs = seeded();
        assert!(fs.read(Path::new("/p/scenes/../project.toml")).is_ok());
        assert_eq!(lexical(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    }
}
