//! `[dependencies]`: other projects a game imports whole into `deps/<alias>/`.
//!
//! A dependency is a project, and its files name each other from its own
//! root, which is `document_paths`' rule for any root a host declares. So a
//! dependency is declared a root, and a scene at
//! `deps/ui_kit/scenes/panel.toml` naming `art/button.png` draws
//! `deps/ui_kit/art/button.png` whatever the game keeps at `art/`.
//!
//! ```toml
//! [dependencies]
//! ui_kit = { project = "dragos/ui-kit", branch = "main" }
//! fonts = { project = "ana/fonts", tag = "v2" }
//! stash = { path = "../stash" }
//! ```
//!
//! The alias is the game's: a `.bs` identifier, so `use deps::ui_kit::x;`
//! reads it. `project.lock` pins each Forge dependency to a commit and a hash
//! of its files; a `path` dependency is the folder as it is, served through
//! [`Mounted`] rather than a symlink. What a dependency's own `project.toml`
//! says covers its own files only: its `ignore`, its `[import.<kind>]`
//! defaults and its `[export] include` globs.
//!
//! An installed dependency is read-only: `fs.*` refuses to write under one.
//! A game changes what it uses through a prefab's overrides, or copies a file
//! into its own tree.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::files::{self, FileBackend};

/// The folder dependencies are installed in, at the game's root.
pub const DIR: &str = "deps";

/// The lockfile beside `project.toml`. Committed; `deps/` is not.
pub const LOCK: &str = "project.lock";

/// Files at a dependency's root that describe the project rather than ship
/// with it: they are read where they are and never packed.
pub const OWN_FILES: &[&str] = &[crate::project::MANIFEST, LOCK, "catalog.toml"];

/// Where a dependency comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A project kept on Forge, `owner/name`. A branch follows, a tag pins,
    /// and neither follows the project's default branch.
    Forge {
        project: String,
        branch: Option<String>,
        tag: Option<String>,
    },
    /// A folder on this machine, relative to the game's root. Not locked:
    /// it is the folder as it is.
    Path(PathBuf),
}

/// One `[dependencies]` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub alias: String,
    pub source: Source,
}

impl Dependency {
    /// `deps/<alias>`, how the game spells the folder.
    #[must_use]
    pub fn folder(&self) -> String {
        folder(&self.alias)
    }
}

/// `deps/<alias>`.
#[must_use]
pub fn folder(alias: &str) -> String {
    format!("{DIR}/{alias}")
}

/// Whether `alias` names a dependency: `[a-z][a-z0-9]*(_[a-z0-9]+)*`, an
/// identifier `use` can spell.
#[must_use]
pub fn valid_alias(alias: &str) -> bool {
    let mut words = alias.split('_');
    let first = words.next().unwrap_or_default();
    let lower_or_digit = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    first.starts_with(|c: char| c.is_ascii_lowercase())
        && first.chars().all(lower_or_digit)
        && words.all(|word| !word.is_empty() && word.chars().all(lower_or_digit))
}

/// Whether `project` reads as a Forge project, `owner/name`.
#[must_use]
pub fn valid_project(project: &str) -> bool {
    let ok = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            && !part.starts_with('.')
    };
    matches!(project.split_once('/'), Some((owner, name)) if ok(owner) && ok(name))
}

/// The `[dependencies]` a manifest declares, sorted by alias.
///
/// # Errors
/// When the manifest does not parse, or an entry is not one of the three
/// shapes: an alias `use` cannot spell, both a project and a path, both a
/// branch and a tag, or a key nothing reads.
pub fn declared(manifest: &str) -> Result<Vec<Dependency>> {
    let document: toml::Table = toml::from_str(manifest).context("parsing project.toml")?;
    let Some(table) = document.get("dependencies") else {
        return Ok(Vec::new());
    };
    let table = table
        .as_table()
        .ok_or_else(|| anyhow!("[dependencies] is a table of `alias = {{ ... }}`"))?;
    let mut out = Vec::new();
    for (alias, entry) in table {
        out.push(entry_of(alias, entry).with_context(|| format!("[dependencies] {alias}"))?);
    }
    out.sort_by(|a, b| a.alias.cmp(&b.alias));
    Ok(out)
}

fn entry_of(alias: &str, entry: &toml::Value) -> Result<Dependency> {
    if !valid_alias(alias) {
        bail!(
            "the name is a folder and a `use` path: lowercase words joined by `_`, such as `ui_kit`"
        );
    }
    let entry = entry.as_table().ok_or_else(|| {
        anyhow!("an entry is `{{ project = \"owner/name\" }}` or `{{ path = \"../folder\" }}`")
    })?;
    let word = |key: &str| -> Result<Option<String>> {
        entry
            .get(key)
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("`{key}` is text"))
            })
            .transpose()
    };
    if let Some(key) = entry
        .keys()
        .find(|key| !matches!(key.as_str(), "project" | "branch" | "tag" | "path"))
    {
        bail!("`{key}` is not a dependency key: `project`, `branch`, `tag` or `path`");
    }
    let source = match (word("project")?, word("path")?) {
        (Some(_), Some(_)) => bail!("a dependency is a Forge `project` or a `path`, not both"),
        (None, None) => bail!("a dependency names a Forge `project` or a `path`"),
        (Some(project), None) => {
            if !valid_project(&project) {
                bail!("`project` is `owner/name`, as the project's address on Forge spells it");
            }
            let (branch, tag) = (word("branch")?, word("tag")?);
            if branch.is_some() && tag.is_some() {
                bail!("a branch follows and a tag pins: name one");
            }
            Source::Forge {
                project,
                branch,
                tag,
            }
        }
        (None, Some(path)) => {
            if word("branch")?.is_some() || word("tag")?.is_some() {
                bail!("a `path` dependency is the folder as it is: no branch or tag");
            }
            Source::Path(PathBuf::from(path))
        }
    };
    Ok(Dependency {
        alias: alias.to_string(),
        source,
    })
}

/// One `[[dependency]]` of `project.lock`: a Forge dependency at one commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locked {
    /// The alias in `[dependencies]`.
    pub name: String,
    /// The Forge project's id, so a rename of the project or its owner
    /// leaves the lock pointing at the same one.
    pub project: String,
    /// The commit.
    pub rev: String,
    /// [`tree_hash`] of the files at that commit.
    pub sha256: String,
}

#[derive(Default, Serialize, Deserialize)]
struct LockFile {
    #[serde(default)]
    dependency: Vec<Locked>,
}

/// The entries of a `project.lock`.
///
/// # Errors
/// When it does not parse, or an entry's alias, commit or hash is not one.
pub fn read_lock(text: &str) -> Result<Vec<Locked>> {
    let lock: LockFile = toml::from_str(text).context("parsing project.lock")?;
    for entry in &lock.dependency {
        if !valid_alias(&entry.name) {
            bail!("project.lock: `{}` is not a dependency name", entry.name);
        }
        if !is_hex(&entry.rev, 40) {
            bail!("project.lock: {}'s `rev` is not a commit", entry.name);
        }
        if !is_hex(&entry.sha256, 64) {
            bail!("project.lock: {}'s `sha256` is not a hash", entry.name);
        }
    }
    Ok(lock.dependency)
}

/// A `project.lock`'s text, entries sorted by name.
#[must_use]
pub fn write_lock(entries: &[Locked]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let body = toml::to_string(&LockFile { dependency: sorted }).unwrap_or_default();
    format!("# Written by `balaur fetch`. Commit it; `deps/` is not.\n\n{body}")
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The hash `project.lock` keeps of a dependency's files: sha256 over each
/// file in path order as `path`, a newline, its length in decimal, a newline
/// and its bytes. Forge computes the same, so a desktop and a runner refuse
/// the same changed byte.
#[must_use]
pub fn tree_hash<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> String {
    use sha2::{Digest, Sha256};
    let mut files: Vec<(&str, &[u8])> = files.into_iter().collect();
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update(format!("\n{}\n", bytes.len()).as_bytes());
        hasher.update(bytes);
    }
    hasher.finalize().iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A project-relative path under `deps/<alias>/` as `(alias, rest)`.
#[must_use]
pub fn split(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("./").unwrap_or(path).strip_prefix(DIR)?;
    let rest = rest.strip_prefix('/')?;
    let (alias, inner) = rest.split_once('/').unwrap_or((rest, ""));
    valid_alias(alias).then_some((alias, inner))
}

/// A dependency as a running engine has it.
#[derive(Debug)]
pub struct Installed {
    /// The game root it belongs to, as the host spelled it.
    pub game: PathBuf,
    pub alias: String,
    pub source: Source,
    /// The commit `project.lock` pins a Forge dependency to.
    pub rev: Option<String>,
    /// `<game>/deps/<alias>`, a declared root.
    pub root: PathBuf,
    /// The folder a `path` dependency is served from.
    pub mount: Option<PathBuf>,
    /// Its own `application/ignore`.
    pub ignore: Vec<String>,
    /// Its own `[import]` table: `[import.<kind>]` defaults by kind.
    pub import: toml::Table,
    /// Its own `[export] include` globs, against its own root.
    pub include: Vec<String>,
    /// `[application] license`, for the credits an export writes.
    pub license: Option<String>,
}

/// Every dependency installed on this engine, for every game root that has
/// some: the project's own, and the game `balaur edit` opens.
#[derive(Default)]
pub struct Installs(pub Vec<Rc<Installed>>);

/// The backend before any mount, and every mount made on it, so a second
/// install mounts onto the first rather than wrapping it again.
struct Mounts {
    base: Rc<dyn FileBackend>,
    table: Vec<(PathBuf, PathBuf)>,
}

/// Declare the dependencies of the project at `game`: each a root, each
/// `path` one mounted, each one's own settings read.
///
/// A manifest with no `[dependencies]` changes nothing. One that does not
/// parse, or a dependency whose folder is empty, is reported and skipped
/// rather than failing the run: the references into it warn as any missing
/// file does, and `balaur fetch` is the fix the message names.
pub fn install(eng: &Engine, game: &Path) {
    let backend = files::backend(eng);
    let Ok(manifest) = backend.read(&game.join(crate::project::MANIFEST)) else {
        return;
    };
    let declared = match declared(&String::from_utf8_lossy(&manifest)) {
        Ok(found) => found,
        Err(why) => {
            tracing::error!("{}: {why:#}", game.display());
            return;
        }
    };
    if declared.is_empty() {
        return;
    }
    let mounts: Vec<(PathBuf, PathBuf)> = declared
        .iter()
        .filter_map(|dep| match &dep.source {
            Source::Path(path) => Some((
                files::lexical(&game.join(dep.folder())),
                files::lexical(&game.join(path)),
            )),
            Source::Forge { .. } => None,
        })
        .collect();
    if !mounts.is_empty() {
        mount(eng, backend, mounts);
    }
    let backend = files::backend(eng);
    let locked: BTreeMap<String, String> = backend
        .read(&game.join(LOCK))
        .ok()
        .and_then(|bytes| read_lock(&String::from_utf8_lossy(&bytes)).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|entry| (entry.name, entry.rev))
        .collect();
    if eng.try_resource::<Installs>().is_none() {
        eng.insert_resource(Installs::default());
    }
    for dep in declared {
        let root = game.join(dep.folder());
        let own = backend
            .read(&root.join(crate::project::MANIFEST))
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok());
        if own.is_none() {
            let fix = match dep.source {
                Source::Path(ref path) => format!("{} has no project.toml", path.display()),
                Source::Forge { .. } => "not fetched yet: run `balaur fetch`".to_string(),
            };
            tracing::warn!("dependency {}: {fix}", dep.alias);
        }
        let own = own.unwrap_or_default();
        let table: toml::Table = toml::from_str(&own).unwrap_or_default();
        crate::file_api::add_root(eng, &root);
        let installed = Installed {
            game: game.to_path_buf(),
            mount: match &dep.source {
                Source::Path(path) => Some(files::lexical(&game.join(path))),
                Source::Forge { .. } => None,
            },
            rev: locked.get(&dep.alias).cloned(),
            alias: dep.alias,
            source: dep.source,
            root,
            ignore: crate::ignore::from_manifest(&own),
            import: table
                .get("import")
                .and_then(toml::Value::as_table)
                .cloned()
                .unwrap_or_default(),
            include: strings_at(&table, "export", "include"),
            license: table
                .get("application")
                .and_then(|app| app.get("license"))
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        };
        let installs = eng.resource::<Installs>();
        let mut installs = installs.borrow_mut();
        installs
            .0
            .retain(|held| !(held.game == installed.game && held.alias == installed.alias));
        installs.0.push(Rc::new(installed));
    }
}

/// Serve `table`'s folders under their mount points from now on, on top of
/// any mounted before.
fn mount(eng: &Engine, backend: Rc<dyn FileBackend>, table: Vec<(PathBuf, PathBuf)>) {
    if eng.try_resource::<Mounts>().is_none() {
        eng.insert_resource(Mounts {
            base: backend,
            table: Vec::new(),
        });
    }
    let mounts = eng.resource::<Mounts>();
    let mounted: Rc<dyn FileBackend> = {
        let mut mounts = mounts.borrow_mut();
        for (at, to) in table {
            mounts.table.retain(|(held, _)| *held != at);
            mounts.table.push((at, to));
        }
        Rc::new(Mounted::new(Rc::clone(&mounts.base), mounts.table.clone()))
    };
    files::set_backend(eng, Rc::clone(&mounted));
    if let Some(project) = eng.try_resource::<crate::project::ProjectFiles>() {
        project.borrow_mut().use_backend(mounted);
    }
}

/// `[<table>] <key>` as a list of strings.
pub(crate) fn strings_at(document: &toml::Table, table: &str, key: &str) -> Vec<String> {
    document
        .get(table)
        .and_then(|t| t.get(key))
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Every dependency installed on this engine.
#[must_use]
pub fn installed(eng: &Engine) -> Vec<Rc<Installed>> {
    eng.try_resource::<Installs>()
        .map(|installs| installs.borrow().0.clone())
        .unwrap_or_default()
}

/// The dependency a path lies in: project-relative under `deps/<alias>/`,
/// or absolute under an installed root.
#[must_use]
pub fn owning(eng: &Engine, path: &str) -> Option<Rc<Installed>> {
    let installs = eng.try_resource::<Installs>()?;
    let installs = installs.borrow();
    if let Some((alias, _)) = split(path) {
        let project = crate::file_api::project_roots(eng).into_iter().next()?;
        return installs
            .0
            .iter()
            .find(|dep| dep.alias == alias && dep.game == project)
            .cloned();
    }
    let named = files::lexical(Path::new(path));
    installs
        .0
        .iter()
        .find(|dep| named.starts_with(files::lexical(&dep.root)))
        .cloned()
}

/// The root a dependency document read from `read_from` owns, for a
/// project-relative `path` under `deps/<alias>/`: the declared one, or `None`
/// when no dependency of that root is installed under that name.
pub(crate) fn root_under(eng: &Engine, read_from: &Path, path: &str) -> Option<PathBuf> {
    let (alias, _) = split(path)?;
    let installs = eng.try_resource::<Installs>()?;
    let installs = installs.borrow();
    installs
        .0
        .iter()
        .find(|dep| dep.alias == alias && dep.game == read_from)
        .map(|dep| dep.root.clone())
}

/// How a game spells the files of a dependency at `root`: `deps/<alias>`
/// when the game is the project, which is what a packed run's keys are too;
/// the root itself for a game a host added, which the editor names absolutely.
pub(crate) fn spelling(eng: &Engine, root: &Path) -> PathBuf {
    let project = crate::file_api::project_roots(eng).into_iter().next();
    let installs = eng.try_resource::<Installs>();
    let found = installs.as_ref().and_then(|installs| {
        installs
            .borrow()
            .0
            .iter()
            .find(|dep| dep.root == root)
            .map(|dep| (dep.game.clone(), dep.alias.clone()))
    });
    match (found, project) {
        (Some((game, alias)), Some(project)) if game == project => PathBuf::from(folder(&alias)),
        _ => root.to_path_buf(),
    }
}

/// The installed dependency a path lies in, when it does: what `fs.*` and
/// an asset save refuse to write into.
#[must_use]
pub fn read_only(eng: &Engine, path: &Path) -> Option<Rc<Installed>> {
    let backend = files::backend(eng);
    let resolved = backend.canonicalize(path);
    installed(eng)
        .into_iter()
        .find(|dep| resolved.starts_with(backend.canonicalize(&dep.root)))
}

/// Refuse a write into an installed dependency, naming what to do instead.
///
/// # Errors
/// When `resolved` lies in one.
pub fn refuse_write(eng: &Engine, path: &Path) -> Result<()> {
    if let Some(dep) = read_only(eng, path) {
        bail!(
            "{} is in the dependency `{}`, which is read-only here: copy the file into the game \
             to change it, or edit the project it comes from",
            path.display(),
            dep.alias
        );
    }
    Ok(())
}

/// The scripts calling into the engine right now, innermost last: each the
/// `deps/<alias>` its file lies in, or `None` for the game's own.
#[derive(Default)]
struct Calling(Vec<Option<String>>);

/// Held while a script runs; dropping it ends that script's turn.
pub struct Turn {
    eng: Engine,
}

impl Drop for Turn {
    fn drop(&mut self) {
        if let Some(calling) = self.eng.try_resource::<Calling>() {
            calling.borrow_mut().0.pop();
        }
    }
}

/// Mark the script at `script` (a project-relative key) as the one calling
/// into the engine until the answer is dropped. A path that script builds
/// reads from its own project ([`scoped`]).
#[must_use]
pub fn calling(eng: &Engine, script: &str) -> Turn {
    let owner = split(script).map(|(alias, _)| folder(alias));
    enter(eng, owner)
}

/// The game's own turn inside a script's: what building a scene document is,
/// since a document's paths are already the game's spelling.
#[must_use]
pub fn as_game(eng: &Engine) -> Turn {
    enter(eng, None)
}

fn enter(eng: &Engine, owner: Option<String>) -> Turn {
    if eng.try_resource::<Calling>().is_none() {
        eng.insert_resource(Calling::default());
    }
    eng.resource::<Calling>().borrow_mut().0.push(owner);
    Turn { eng: eng.clone() }
}

/// `path` as the calling script's project names it: a relative path from a
/// script under `deps/<alias>/` reads under that folder. A path already under
/// `deps/`, rooted, or an `id://`, is the game's spelling and stays.
#[must_use]
pub fn scoped<'a>(eng: &Engine, path: &'a str) -> Cow<'a, str> {
    let Some(calling) = eng.try_resource::<Calling>() else {
        return Cow::Borrowed(path);
    };
    let owner = calling.borrow().0.last().cloned().flatten();
    match owner {
        Some(prefix)
            if !path.is_empty()
                && !files::rooted(Path::new(path))
                && !path.starts_with(crate::assets::ID_PREFIX)
                && split(path).is_none()
                && !path.contains("://") =>
        {
            Cow::Owned(format!("{prefix}/{}", path.trim_start_matches("./")))
        }
        _ => Cow::Borrowed(path),
    }
}

/// The folder a script's `use` stops at: the dependency `key` lies in,
/// spelled as `key` is, or `None` for one of the game's own.
#[must_use]
pub fn boundary(eng: &Engine, key: &str) -> Option<PathBuf> {
    if let Some((alias, _)) = split(key) {
        return Some(PathBuf::from(folder(alias)));
    }
    let named = files::lexical(Path::new(key));
    installed(eng)
        .into_iter()
        .map(|dep| files::lexical(&dep.root))
        .find(|root| named.starts_with(root))
}

/// A file backend with `path` dependencies mounted into it: a path under
/// `<game>/deps/<alias>` reads and writes the folder it maps to.
///
/// Not a symlink: `file_api::resolve` refuses one that leaves a root, and a
/// Windows machine needs Developer Mode to make one. The mount answers
/// `canonicalize` under the mount point, so a root check sees the
/// dependency's own root; a symlink inside the folder that leaves it is
/// answered where it really points, and refused like any other.
pub struct Mounted {
    base: Rc<dyn FileBackend>,
    /// Mount point, then the folder it maps to.
    mounts: Vec<(PathBuf, PathBuf)>,
    /// Each mount point as `canonicalize` answers it, which a path that has
    /// been through a root check arrives spelled as (`/private/var` for
    /// `/var` on macOS).
    canonical: Vec<PathBuf>,
}

impl Mounted {
    #[must_use]
    pub fn new(base: Rc<dyn FileBackend>, mounts: Vec<(PathBuf, PathBuf)>) -> Self {
        let mounts: Vec<(PathBuf, PathBuf)> = mounts
            .into_iter()
            .map(|(at, to)| (files::lexical(&at), to))
            .collect();
        let canonical = mounts.iter().map(|(at, _)| base.canonicalize(at)).collect();
        Self {
            base,
            mounts,
            canonical,
        }
    }

    /// The mount `path` lies under, and the rest of it below the mount point.
    fn under(&self, path: &Path) -> Option<(usize, PathBuf)> {
        let lex = files::lexical(path);
        self.mounts
            .iter()
            .map(|(at, _)| at)
            .zip(&self.canonical)
            .enumerate()
            .find_map(|(i, (at, canonical))| {
                let rest = lex
                    .strip_prefix(at)
                    .or_else(|_| lex.strip_prefix(canonical));
                rest.ok().map(|rest| (i, rest.to_path_buf()))
            })
    }

    /// The real path behind `path`.
    fn real(&self, path: &Path) -> PathBuf {
        match self.under(path) {
            Some((i, rest)) if rest.as_os_str().is_empty() => self.mounts[i].1.clone(),
            Some((i, rest)) => self.mounts[i].1.join(rest),
            None => path.to_path_buf(),
        }
    }

    /// The folders directly under `dir` that lead to a mount point: the
    /// mount itself, or `deps` in a game folder that has no `deps/` of its own.
    fn mounted_in(&self, dir: &Path) -> Vec<(String, bool)> {
        let dir = files::lexical(dir);
        let mut out: Vec<(String, bool)> = Vec::new();
        for (at, canonical) in self.mounts.iter().map(|(at, _)| at).zip(&self.canonical) {
            let below = at
                .strip_prefix(&dir)
                .or_else(|_| canonical.strip_prefix(&dir));
            let Some(first) = below.ok().and_then(|rest| rest.components().next()) else {
                continue;
            };
            let name = first.as_os_str().to_string_lossy().into_owned();
            if !out.iter().any(|(held, _)| *held == name) {
                out.push((name, true));
            }
        }
        out
    }
}

impl FileBackend for Mounted {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.base.read(&self.real(path))
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.base.write(&self.real(path), bytes)
    }
    fn append(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.base.append(&self.real(path), bytes)
    }
    fn exists(&self, path: &Path) -> bool {
        self.base.exists(&self.real(path)) || !self.mounted_in(path).is_empty()
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.base.is_dir(&self.real(path)) || !self.mounted_in(path).is_empty()
    }
    fn remove(&self, path: &Path) -> Result<bool> {
        self.base.remove(&self.real(path))
    }
    fn mkdir(&self, path: &Path) -> Result<()> {
        self.base.mkdir(&self.real(path))
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.base.rename(&self.real(from), &self.real(to))
    }
    fn mtime(&self, path: &Path) -> Option<f64> {
        self.base.mtime(&self.real(path))
    }
    fn list(&self, path: &Path) -> Vec<(String, bool)> {
        let mut out = self.base.list(&self.real(path));
        for entry in self.mounted_in(path) {
            if !out.iter().any(|(name, _)| *name == entry.0) {
                out.push(entry);
            }
        }
        out
    }
    fn canonicalize(&self, path: &Path) -> PathBuf {
        let Some((i, rest)) = self.under(path) else {
            return self.base.canonicalize(path);
        };
        let to = &self.mounts[i].1;
        let target = self.base.canonicalize(to);
        let real = self.base.canonicalize(&to.join(rest));
        match real.strip_prefix(&target) {
            Ok(inside) if inside.as_os_str().is_empty() => self.canonical[i].clone(),
            Ok(inside) => self.canonical[i].join(inside),
            Err(_) => real,
        }
    }
    fn sync(&self, path: &Path) {
        self.base.sync(&self.real(path));
    }
}

/// The files a dependency ships under `root`: every file the backend lists,
/// relative and sorted, less dot files, its own `deps/`, its
/// `application/ignore` and the files that describe it ([`OWN_FILES`]).
#[must_use]
pub fn files_of(backend: &dyn FileBackend, root: &Path, ignore: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending = vec![String::new()];
    while let Some(dir) = pending.pop() {
        let full = if dir.is_empty() {
            root.to_path_buf()
        } else {
            root.join(&dir)
        };
        for (name, is_dir) in backend.list(&full) {
            if name.starts_with('.') || (dir.is_empty() && name == DIR) {
                continue;
            }
            let rel = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if crate::ignore::ignored(ignore, &rel) {
                continue;
            }
            if is_dir {
                pending.push(rel);
            } else if !(OWN_FILES.contains(&rel.as_str())) {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// Every `id → path` a dependency's own `assets/index.toml` holds.
#[must_use]
pub fn index_of(backend: &dyn FileBackend, root: &Path) -> BTreeMap<String, String> {
    backend
        .read(&root.join(crate::assets::INDEX_PATH))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| crate::asset_index::parse(&text).ok())
        .unwrap_or_default()
}

/// The `.gitignore` line `balaur new` and `balaur add` keep: fetched files
/// are the lock's to restore, never the repository's to hold.
pub const GITIGNORE_LINE: &str = "/deps/";

/// `text` with [`GITIGNORE_LINE`] in it, or `None` when it is there already.
#[must_use]
pub fn gitignored(text: &str) -> Option<String> {
    let has = text
        .lines()
        .map(str::trim)
        .any(|line| matches!(line, "/deps/" | "deps/" | "/deps" | "deps"));
    if has {
        return None;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(GITIGNORE_LINE);
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alias_is_a_use_path() {
        for good in ["ui_kit", "fonts", "a1", "kit_2", "x_y_z"] {
            assert!(valid_alias(good), "{good}");
        }
        for bad in [
            "", "Ui", "1kit", "ui-kit", "_kit", "kit_", "ui__kit", "../x", "a/b",
        ] {
            assert!(!valid_alias(bad), "{bad}");
        }
    }

    #[test]
    fn the_three_shapes_parse_and_nothing_else_does() {
        let deps = declared(
            r#"
[dependencies]
ui_kit = { project = "dragos/ui-kit", branch = "main" }
fonts = { project = "ana/fonts", tag = "v2" }
stash = { path = "../stash" }
"#,
        )
        .unwrap();
        let aliases: Vec<&str> = deps.iter().map(|d| d.alias.as_str()).collect();
        assert_eq!(aliases, ["fonts", "stash", "ui_kit"]);
        assert_eq!(deps[1].source, Source::Path(PathBuf::from("../stash")));
        for bad in [
            "[dependencies]\nUi = { path = \"x\" }",
            "[dependencies]\na = { project = \"x/y\", path = \"z\" }",
            "[dependencies]\na = { project = \"x/y\", branch = \"m\", tag = \"t\" }",
            "[dependencies]\na = { project = \"nope\" }",
            "[dependencies]\na = { path = \"x\", tag = \"t\" }",
            "[dependencies]\na = { project = \"x/y\", version = \"1\" }",
        ] {
            assert!(declared(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_lock_round_trips_and_refuses_what_is_not_a_commit() {
        let entry = Locked {
            name: "ui_kit".into(),
            project: "0190-id".into(),
            rev: "a".repeat(40),
            sha256: "b".repeat(64),
        };
        let text = write_lock(std::slice::from_ref(&entry));
        assert_eq!(read_lock(&text).unwrap(), vec![entry]);
        let short = text.replace(&"a".repeat(40), "main");
        assert!(read_lock(&short).is_err());
    }

    #[test]
    fn the_hash_is_order_free_and_changes_with_one_byte() {
        let a = tree_hash([("b.txt", &b"two"[..]), ("a.txt", &b"one"[..])]);
        let b = tree_hash([("a.txt", &b"one"[..]), ("b.txt", &b"two"[..])]);
        assert_eq!(a, b);
        assert_ne!(
            a,
            tree_hash([("a.txt", &b"onf"[..]), ("b.txt", &b"two"[..])])
        );
        // The length is part of each record, so moving bytes between files changes it.
        assert_ne!(
            tree_hash([("a", &b"xy"[..]), ("b", &b""[..])]),
            tree_hash([("a", &b"x"[..]), ("b", &b"y"[..])])
        );
    }

    #[test]
    fn split_reads_only_a_dependency_folder() {
        assert_eq!(
            split("deps/ui_kit/art/x.png"),
            Some(("ui_kit", "art/x.png"))
        );
        assert_eq!(split("./deps/ui_kit/x"), Some(("ui_kit", "x")));
        assert_eq!(split("deps/ui_kit"), Some(("ui_kit", "")));
        assert_eq!(split("depsy/a/x"), None);
        assert_eq!(split("art/deps/a/x"), None);
        assert_eq!(split("deps/Bad/x"), None);
    }

    #[test]
    fn gitignore_gains_the_line_once() {
        assert_eq!(gitignored("target/\n").unwrap(), "target/\n/deps/\n");
        assert!(gitignored("target\ndeps/\n").is_none());
        assert_eq!(gitignored("").unwrap(), "/deps/\n");
    }
}

/// The `dependency` script module: what the editor shows and does with them.
// A raw binding answers a `Result`, whether or not it can fail.
#[allow(clippy::unnecessary_wraps)]
pub(crate) mod api {
    use anyhow::{Result, anyhow, bail};
    use balaur_script::{Bindings, Value};

    use super::{Source, folder, install, installed, owning, spelling, valid_alias};
    use crate::engine::Engine;
    use crate::engine_api::text;

    /// The `dependency` module and its reference text.
    pub(crate) fn install_dependencies_api(m: &mut dyn Bindings<Engine>) {
        m.module_doc(
            "The projects a game imports whole under `deps/<alias>/`, declared in `[dependencies]`. An installed dependency is read-only: `fs.*` and `assets.save` refuse to write into one. A relative path a dependency's script builds reads from that dependency's own folder.",
        );
        m.describe(&[
            ("list", &[], "() -> [map]", "Every installed dependency: `#{ alias, folder, game, kind, project, branch, tag, path, rev, license }`. `folder` is how the game names it, `kind` is `\"forge\"` or `\"path\"`, and a key that does not apply is empty."),
            ("of", &[], "(path: string) -> string?", "The alias of the dependency a path lies in, or nil for one of the game's own."),
            ("make_local", &[], "(from: string, to: string) -> [string]", "Copy a dependency's file into the game and point the game's references at the copy; answers the game's files rewritten. The copy never updates. A document's own paths come out as the game names the dependency's files."),
            ("move_into", &[], "(paths: [string], alias: string) -> [string]", "Move game files into a `path` dependency, each at the same place under its root, and point the game's references at `deps/<alias>/…`; answers the files rewritten."),
            ("add_path", &[], "(game: string, alias: string, path: string)", "Declare `alias = { path = \"<path>\" }` in a game's `[dependencies]`, comments kept, and install it. An empty game is the open project."),
            ("install", &[], "(game: string)", "Read a game's `[dependencies]` again and install each: what a dependency fetched while the editor runs needs to become a root. An empty game is the open project."),
        ]);
        m.function_raw("list", Box::new(list));
        m.function_raw("of", Box::new(of));
        m.function_raw("make_local", Box::new(make_local));
        m.function_raw("move_into", Box::new(move_into));
        m.function_raw("add_path", Box::new(add_path));
        m.function_raw("install", Box::new(reinstall));
    }

    fn word(value: Option<&String>) -> Value {
        Value::text(value.cloned().unwrap_or_default())
    }

    /// Every installed dependency, one map each.
    pub(crate) fn list(eng: &Engine, _: &[Value]) -> Result<Value> {
        Ok(Value::List(
            installed(eng)
                .iter()
                .map(|dep| {
                    let (kind, project, branch, tag, path) = match &dep.source {
                        Source::Forge {
                            project,
                            branch,
                            tag,
                        } => (
                            "forge",
                            Some(project.clone()),
                            branch.clone(),
                            tag.clone(),
                            None,
                        ),
                        Source::Path(path) => (
                            "path",
                            None,
                            None,
                            None,
                            Some(path.to_string_lossy().into_owned()),
                        ),
                    };
                    Value::Map(vec![
                        ("alias".into(), Value::text(dep.alias.clone())),
                        (
                            "folder".into(),
                            Value::text(spelling(eng, &dep.root).to_string_lossy().into_owned()),
                        ),
                        (
                            "game".into(),
                            Value::text(dep.game.to_string_lossy().into_owned()),
                        ),
                        ("kind".into(), Value::text(kind)),
                        ("project".into(), word(project.as_ref())),
                        ("branch".into(), word(branch.as_ref())),
                        ("tag".into(), word(tag.as_ref())),
                        ("path".into(), word(path.as_ref())),
                        ("rev".into(), word(dep.rev.as_ref())),
                        ("license".into(), word(dep.license.as_ref())),
                    ])
                })
                .collect(),
        ))
    }

    /// The alias of the dependency a path lies in, or nil.
    pub(crate) fn of(eng: &Engine, args: &[Value]) -> Result<Value> {
        let path = super::scoped(eng, text(args, 0)?);
        Ok(owning(eng, &path).map_or(Value::Nil, |dep| Value::text(dep.alias.clone())))
    }

    pub(crate) fn make_local(eng: &Engine, args: &[Value]) -> Result<Value> {
        let rewritten = crate::asset_index::make_local(eng, text(args, 0)?, text(args, 1)?)?;
        Ok(Value::List(
            rewritten.into_iter().map(Value::text).collect(),
        ))
    }

    pub(crate) fn move_into(eng: &Engine, args: &[Value]) -> Result<Value> {
        let Some(Value::List(items)) = args.first() else {
            bail!("argument 0 should be a list of paths");
        };
        let paths = items
            .iter()
            .map(|item| match item {
                Value::Str(path) => Ok(path.clone()),
                other => Err(anyhow!("a path should be a string, got {other:?}")),
            })
            .collect::<Result<Vec<_>>>()?;
        let rewritten = crate::asset_index::move_into(eng, &paths, text(args, 1)?)?;
        Ok(Value::List(
            rewritten.into_iter().map(Value::text).collect(),
        ))
    }

    /// Read a game's `[dependencies]` again and install what it declares: a
    /// dependency fetched while the editor runs.
    pub(crate) fn reinstall(eng: &Engine, args: &[Value]) -> Result<Value> {
        let game = text(args, 0)?;
        let game = if game.is_empty() {
            crate::file_api::project_roots(eng)
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no project is open"))?
        } else {
            std::path::PathBuf::from(game)
        };
        install(eng, &game);
        Ok(Value::Nil)
    }

    /// Declare a `path` dependency in a game's `project.toml`, comments kept,
    /// and install it.
    pub(crate) fn add_path(eng: &Engine, args: &[Value]) -> Result<Value> {
        let (game, alias, path) = (text(args, 0)?, text(args, 1)?, text(args, 2)?);
        if !valid_alias(alias) {
            bail!("`{alias}` is not a dependency name: lowercase words joined by `_`");
        }
        let game = if game.is_empty() {
            crate::file_api::project_roots(eng)
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no project is open"))?
        } else {
            std::path::PathBuf::from(game)
        };
        let backend = crate::files::backend(eng);
        let manifest = game.join(crate::project::MANIFEST);
        let text = String::from_utf8(backend.read(&manifest)?)?;
        let mut document: toml_edit::DocumentMut = text.parse()?;
        let table = document
            .entry("dependencies")
            .or_insert_with(toml_edit::table)
            .as_table_mut()
            .ok_or_else(|| anyhow!("[dependencies] is not a table"))?;
        if table.contains_key(alias) {
            bail!("the game already has a dependency named `{alias}`");
        }
        let mut entry = toml_edit::InlineTable::new();
        entry.insert("path", path.into());
        table.insert(alias, toml_edit::value(entry));
        backend.write(&manifest, document.to_string().as_bytes())?;
        install(eng, &game);
        if !installed(eng).iter().any(|dep| dep.alias == alias) {
            bail!("{} did not install", folder(alias));
        }
        Ok(Value::Nil)
    }
}
