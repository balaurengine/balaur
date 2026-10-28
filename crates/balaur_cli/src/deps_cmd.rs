//! `balaur add` and `balaur fetch`: another project as a dependency, kept in
//! `deps/<alias>/` and pinned by `project.lock`.
//!
//! A Forge dependency is read over Forge's HTTP API, never git: the tree at a
//! commit, then each blob this machine has not kept yet. Blobs are kept once
//! under `<data>/balaur-editor/deps/`, named by their git blob id and checked
//! against it, so every game on one dependency shares one download. The files
//! of a dependency at its commit are hashed as `project.lock` records
//! ([`balaur::dependencies::tree_hash`]); a fetch that does not match the lock
//! is refused and writes nothing. Installing runs nothing: a dependency is
//! files.
//!
//! `BALAUR_FORGE` names the Forge to ask (`https://balaurengine.org` unless
//! set), and `BALAUR_FORGE_TOKEN` is a personal API token for a private
//! project the token's owner may read.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use balaur::dependencies::{self, Dependency, Locked, Source};

const DEFAULT_FORGE: &str = "https://balaurengine.org";

/// The Forge this machine asks: `BALAUR_FORGE`, or balaurengine.org.
pub(crate) fn forge_url() -> String {
    Forge::from_env().base
}

/// A project Forge lists, as `/api/v1/forge/deps` answers one.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Listed {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) license: Option<String>,
    #[serde(default)]
    pub(crate) default_ref: String,
}

/// Public projects on Forge matching `query` and `tag` (either may be
/// empty), and with a token the reader's own private ones.
pub(crate) fn search(query: &str, tag: &str) -> Result<Vec<Listed>> {
    use std::fmt::Write as _;
    let mut path = String::from("?page_size=60");
    if !query.is_empty() {
        let _ = write!(path, "&q={}", encode(query));
    }
    if !tag.is_empty() {
        let _ = write!(path, "&tag={}", encode(tag));
    }
    Forge::from_env().json(&path)
}

/// `balaur add <owner/name | folder>`.
pub(crate) fn add(
    project: &Path,
    source: &str,
    alias: Option<&str>,
    branch: Option<&str>,
    tag: Option<&str>,
) -> Result<()> {
    // A folder is named from here or from the project; either way the entry
    // spells it from the project.
    let folder = [PathBuf::from(source), project.join(source)]
        .into_iter()
        .find(|dir| dir.join("project.toml").is_file());
    let alias = match alias {
        Some(alias) => alias.to_string(),
        None => alias_from(
            Path::new(source)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
                .as_str(),
        ),
    };
    if !dependencies::valid_alias(&alias) {
        bail!(
            "`{alias}` is not a dependency name: lowercase words joined by `_`, such as `ui_kit`"
        );
    }
    let mut entry = toml_edit::InlineTable::new();
    if let Some(folder) = folder {
        if branch.is_some() || tag.is_some() {
            bail!("a folder is a dependency as it is: no branch or tag");
        }
        let relative = pathdiff(
            &std::path::absolute(folder)?,
            &std::path::absolute(project)?,
        );
        entry.insert("path", relative.to_string_lossy().replace('\\', "/").into());
    } else {
        if !dependencies::valid_project(source) {
            bail!("`{source}` is neither a project folder nor `owner/name` on Forge");
        }
        entry.insert("project", source.into());
        if let Some(branch) = branch {
            entry.insert("branch", branch.into());
        }
        if let Some(tag) = tag {
            entry.insert("tag", tag.into());
        }
    }
    let manifest = project.join("project.toml");
    let text = std::fs::read_to_string(&manifest)
        .with_context(|| format!("no project.toml in {}", project.display()))?;
    let mut document: toml_edit::DocumentMut = text.parse().context("parsing project.toml")?;
    let table = document
        .entry("dependencies")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or_else(|| anyhow!("[dependencies] in project.toml is not a table"))?;
    if table.contains_key(&alias) {
        bail!("the project already has a dependency named `{alias}`; name this one with --as");
    }
    table.insert(&alias, toml_edit::value(entry));
    std::fs::write(&manifest, document.to_string())?;
    // A dependency that does not fetch is not added: the manifest goes back
    // to what it said.
    if let Err(why) = fetch(project, false, Some(&alias)) {
        std::fs::write(&manifest, text)?;
        return Err(why);
    }
    crate::new_project::keep_deps_out_of_git(project)?;
    tracing::info!("added `{alias}` to [dependencies]");
    Ok(())
}

/// `balaur fetch`: every Forge dependency into `deps/`, as the lock pins it,
/// locking any the lock does not name yet. `update` moves each to the newest
/// commit of its branch or tag and says which of the game's references the
/// new files no longer have.
pub(crate) fn fetch(project: &Path, update: bool, only: Option<&str>) -> Result<()> {
    fetch_from(
        &Forge::from_env(),
        cache_dir().as_deref(),
        project,
        update,
        only,
    )
}

/// [`fetch`] from one Forge, keeping blobs in `cache` when there is one.
fn fetch_from(
    forge: &Forge,
    cache: Option<&Path>,
    project: &Path,
    update: bool,
    only: Option<&str>,
) -> Result<()> {
    let manifest = std::fs::read_to_string(project.join("project.toml"))
        .with_context(|| format!("no project.toml in {}", project.display()))?;
    let declared = dependencies::declared(&manifest)?;
    if let Some(only) = only
        && !declared.iter().any(|dep| dep.alias == only)
    {
        bail!("no dependency named `{only}` in [dependencies]");
    }
    let lock_path = project.join(dependencies::LOCK);
    let mut lock: BTreeMap<String, Locked> = match std::fs::read_to_string(&lock_path) {
        Ok(text) => dependencies::read_lock(&text)?
            .into_iter()
            .map(|entry| (entry.name.clone(), entry))
            .collect(),
        Err(_) => BTreeMap::new(),
    };
    let deps_dir = project.join(dependencies::DIR);
    refuse_link(&deps_dir)?;
    for dep in &declared {
        if only.is_some_and(|only| only != dep.alias) {
            continue;
        }
        match &dep.source {
            Source::Path(path) => {
                let folder = project.join(path);
                if !folder.join("project.toml").is_file() {
                    bail!(
                        "dependency `{}`: {} is not a project",
                        dep.alias,
                        folder.display()
                    );
                }
                tracing::info!("{}: {} (a folder, as it is)", dep.alias, path.display());
            }
            Source::Forge { .. } => {
                let locked = lock.get(&dep.alias).filter(|_| !update).cloned();
                let fetched = fetch_one(forge, cache, project, dep, locked.as_ref())?;
                lock.insert(dep.alias.clone(), fetched);
            }
        }
    }
    // A lock entry or a fetched folder for a dependency no longer declared.
    let names: BTreeSet<&str> = declared.iter().map(|dep| dep.alias.as_str()).collect();
    lock.retain(|name, _| names.contains(name.as_str()));
    if let Ok(entries) = std::fs::read_dir(&deps_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if dependencies::valid_alias(&name)
                && !names.contains(name.as_str())
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                std::fs::remove_dir_all(entry.path())?;
                tracing::info!("removed deps/{name}: no longer a dependency");
            }
        }
    }
    let entries: Vec<Locked> = lock.into_values().collect();
    let text = dependencies::write_lock(&entries);
    if entries.is_empty() {
        let _ = std::fs::remove_file(&lock_path);
    } else if std::fs::read_to_string(&lock_path).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&lock_path, text)?;
        tracing::info!("wrote {}", dependencies::LOCK);
    }
    Ok(())
}

/// A copy of a Forge project at its default branch, in a new folder under
/// `parent` named after it, with its own dependencies fetched: the project
/// manager's Open a copy. Answers the folder.
pub(crate) fn download(project: &str, parent: &Path) -> Result<PathBuf> {
    if !dependencies::valid_project(project) {
        bail!("`{project}` is not `owner/name`");
    }
    let forge = Forge::from_env();
    let found = forge.project(project)?;
    let tree = forge.tree(&found.id, &found.default_ref)?;
    let name = project.rsplit('/').next().unwrap_or(project);
    let mut folder = parent.join(name);
    let mut n = 2;
    while folder.exists() {
        folder = parent.join(format!("{name}-{n}"));
        n += 1;
    }
    let cache = cache_dir();
    for file in &tree.files {
        check_path(&file.path)?;
        let bytes = forge.blob(&found.id, &file.sha, cache.as_deref())?;
        let target = folder.join(&file.path);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&target, bytes)?;
    }
    if !folder.join("project.toml").is_file() {
        bail!("{project} has no project.toml at {}", short(&tree.commit));
    }
    fetch(&folder, false, None)?;
    Ok(folder)
}

/// One Forge dependency at the lock's commit, or at the newest of its branch
/// or tag when `locked` is `None`; answers its lock entry.
fn fetch_one(
    forge: &Forge,
    cache: Option<&Path>,
    project: &Path,
    dep: &Dependency,
    locked: Option<&Locked>,
) -> Result<Locked> {
    let Source::Forge {
        project: name,
        branch,
        tag,
    } = &dep.source
    else {
        unreachable!("only Forge dependencies are fetched");
    };
    let (id, rev) = if let Some(entry) = locked {
        (entry.project.clone(), entry.rev.clone())
    } else {
        let found = forge.project(name)?;
        let wanted = tag
            .clone()
            .or_else(|| branch.clone())
            .unwrap_or(found.default_ref);
        (found.id, wanted)
    };
    let tree = forge
        .tree(&id, &rev)
        .with_context(|| format!("dependency `{}` ({name})", dep.alias))?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(tree.files.len());
    for file in &tree.files {
        check_path(&file.path).with_context(|| format!("dependency `{}`", dep.alias))?;
        let bytes = forge.blob(&id, &file.sha, cache)?;
        files.push((file.path.clone(), bytes));
    }
    let hash = dependencies::tree_hash(files.iter().map(|(p, b)| (p.as_str(), b.as_slice())));
    if let Some(entry) = locked
        && entry.sha256 != hash
    {
        bail!(
            "dependency `{}` at {} does not match project.lock: its files hash to {hash}, the lock \
             says {}. Refused; nothing was written",
            dep.alias,
            short(&entry.rev),
            entry.sha256
        );
    }
    let folder = project.join(dep.folder());
    let before = listed(&folder);
    install(project, &dep.alias, &files)?;
    if locked.is_none() && !before.is_empty() {
        report_missing(project, &dep.alias, &before, &files);
    }
    tracing::info!(
        "{}: {name} @ {} ({} files)",
        dep.alias,
        short(&tree.commit),
        files.len()
    );
    Ok(Locked {
        name: dep.alias.clone(),
        project: id,
        rev: tree.commit,
        sha256: hash,
    })
}

/// Replace `deps/<alias>` with `files`, whole: written beside it first, so a
/// failed write leaves the old folder as it was.
fn install(project: &Path, alias: &str, files: &[(String, Vec<u8>)]) -> Result<()> {
    let deps = project.join(dependencies::DIR);
    refuse_link(&deps)?;
    std::fs::create_dir_all(&deps)?;
    let folder = deps.join(alias);
    refuse_link(&folder)?;
    let partial = deps.join(format!(".{alias}.partial"));
    if partial.exists() {
        std::fs::remove_dir_all(&partial)?;
    }
    for (path, bytes) in files {
        let target = partial.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, bytes)?;
    }
    std::fs::create_dir_all(&partial)?;
    if folder.exists() {
        std::fs::remove_dir_all(&folder)?;
    }
    std::fs::rename(&partial, &folder)?;
    Ok(())
}

/// A path out of a tree is the dependency's to name and this machine's to
/// write: refused unless it stays below the folder it lands in.
fn check_path(path: &str) -> Result<()> {
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || dependencies::split(path).is_some()
        || path.starts_with("deps/");
    if bad {
        bail!("its tree names `{path}`, which is not a path inside the project");
    }
    Ok(())
}

/// `path` refused when it is a symlink: `deps/` is written into, and a link
/// committed in its place would send the writes elsewhere.
fn refuse_link(path: &Path) -> Result<()> {
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        bail!(
            "{} is a symlink; dependencies are written there, so it must be a folder",
            path.display()
        );
    }
    Ok(())
}

/// Every file now under a dependency folder, relative to it.
fn listed(folder: &Path) -> BTreeSet<String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(folder, folder, &mut out);
    out
}

/// Warn for each reference the game makes into a file the update removed.
fn report_missing(
    project: &Path,
    alias: &str,
    before: &BTreeSet<String>,
    now: &[(String, Vec<u8>)],
) {
    let now: BTreeSet<&str> = now.iter().map(|(path, _)| path.as_str()).collect();
    let gone: Vec<String> = before
        .iter()
        .filter(|path| !now.contains(path.as_str()))
        .map(|path| format!("{}/{path}", dependencies::folder(alias)))
        .collect();
    if gone.is_empty() {
        return;
    }
    let mut stack = vec![project.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || (dir == project && matches!(name.as_str(), "deps" | "target"))
            {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let wanted = path
                .extension()
                .is_some_and(|ext| ext == "toml" || ext == "bs");
            let Some(text) = wanted
                .then(|| std::fs::read_to_string(&path).ok())
                .flatten()
            else {
                continue;
            };
            for missing in &gone {
                if text.contains(&format!("\"{missing}")) {
                    let rel = path.strip_prefix(project).unwrap_or(&path);
                    tracing::warn!(
                        "{} names {missing}, which the update removed",
                        rel.display()
                    );
                }
            }
        }
    }
}

/// A Forge project's name as a dependency alias: `UI-Kit` is `ui_kit`.
fn alias_from(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    match out.chars().next() {
        None => "dep".to_string(),
        // Forge's Add to project names it the same way.
        Some(c) if c.is_ascii_digit() => format!("dep_{out}"),
        Some(_) => out,
    }
}

/// `to` relative to `from`, both absolute: `../stash` for a sibling.
fn pathdiff(to: &Path, from: &Path) -> PathBuf {
    let parts = |path: &Path| -> Vec<std::ffi::OsString> {
        balaur::files::lexical(path)
            .components()
            .map(|c| c.as_os_str().to_owned())
            .collect()
    };
    let (to, from) = (parts(to), parts(from));
    let common = to.iter().zip(&from).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for part in &to[common..] {
        out.push(part);
    }
    out
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(10)]
}

/// Where fetched blobs are kept, one file per git blob id.
fn cache_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(balaur::engine_api::EDITOR_NAME).join("deps"))
}

struct Forge {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

#[derive(serde::Deserialize)]
struct Found {
    id: String,
    default_ref: String,
}

#[derive(serde::Deserialize)]
struct Tree {
    commit: String,
    files: Vec<TreeFile>,
}

#[derive(serde::Deserialize)]
struct TreeFile {
    path: String,
    sha: String,
}

#[derive(serde::Deserialize)]
struct Data<T> {
    data: T,
}

impl Forge {
    fn from_env() -> Self {
        Self::at(
            &std::env::var("BALAUR_FORGE").unwrap_or_else(|_| DEFAULT_FORGE.to_string()),
            std::env::var("BALAUR_FORGE_TOKEN").ok(),
        )
    }

    fn at(base: &str, token: Option<String>) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token: token.filter(|t| !t.is_empty()),
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }

    fn get(&self, path: &str) -> Result<ureq::http::Response<ureq::Body>> {
        let url = if path.starts_with('?') {
            format!("{}/api/v1/forge/deps{path}", self.base)
        } else {
            format!("{}/api/v1/forge/deps/{path}", self.base)
        };
        let mut request = self.agent.get(&url);
        if let Some(token) = &self.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request.call().with_context(|| format!("asking {url}"))?;
        let status = response.status().as_u16();
        if status == 200 {
            return Ok(response);
        }
        // Forge answers `{ error, message }`; the code says which miss it was.
        let said: serde_json::Value = response
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let message = said["message"].as_str().unwrap_or_default();
        match said["error"].as_str().unwrap_or_default() {
            "project_not_found" => bail!(
                "{url}: no such project, or one this machine may not read (set BALAUR_FORGE_TOKEN \
                 for a private one)"
            ),
            "unknown_ref" => bail!("{url}: no branch, tag or commit by that name"),
            "bad_tree" => bail!("{url}: the project's tree is refused at `{message}`"),
            "" => bail!("{url}: HTTP {status}"),
            code => bail!("{url}: HTTP {status}, {code}: {message}"),
        }
    }

    fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let mut response = self.get(path)?;
        let text = response.body_mut().read_to_string()?;
        let data: Data<T> = serde_json::from_str(&text).context("reading Forge's answer")?;
        Ok(data.data)
    }

    fn project(&self, name: &str) -> Result<Found> {
        self.json(name)
    }

    fn tree(&self, id: &str, rev: &str) -> Result<Tree> {
        self.json(&format!("{id}/tree?ref={}", encode(rev)))
    }

    /// A blob's bytes: from the cache when it holds them under their id,
    /// fetched and kept otherwise. Either way they are checked against the id.
    fn blob(&self, id: &str, sha: &str, cache: Option<&Path>) -> Result<Vec<u8>> {
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("`{sha}` is not a blob id");
        }
        let kept = cache.map(|dir| dir.join(sha));
        if let Some(bytes) = kept.as_ref().and_then(|path| std::fs::read(path).ok())
            && blob_id(&bytes) == sha
        {
            return Ok(bytes);
        }
        let mut response = self.get(&format!("{id}/blob/{sha}"))?;
        let mut bytes = Vec::new();
        response.body_mut().as_reader().read_to_end(&mut bytes)?;
        if blob_id(&bytes) != sha {
            bail!("blob {sha} arrived as other bytes; refused");
        }
        if let Some(path) = kept {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, &bytes);
        }
        Ok(bytes)
    }
}

/// Git's id for a blob: sha1 over `blob <length>\0` and the bytes.
fn blob_id(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    use std::fmt::Write as _;
    let mut hasher = sha1::Sha1::new();
    hasher.update(format!("blob {}\0", bytes.len()).as_bytes());
    hasher.update(bytes);
    hasher.finalize().iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A ref as a query value: branch and tag names are free text.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_name_becomes_an_alias() {
        assert_eq!(alias_from("ui-kit"), "ui_kit");
        assert_eq!(alias_from("UI  Kit!"), "ui_kit");
        assert_eq!(alias_from("3d-props"), "dep_3d_props");
        assert_eq!(alias_from("--"), "dep");
        assert!(dependencies::valid_alias(&alias_from("Fonts.v2")));
    }

    #[test]
    fn a_tree_path_stays_inside_the_folder() {
        assert!(check_path("art/x.png").is_ok());
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a//b",
            "./a",
            "c:\\x",
            "deps/kit/x",
        ] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_blob_id_is_gits() {
        // `git hash-object` of "hello\n".
        assert_eq!(
            blob_id(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }

    #[test]
    fn a_sibling_folder_is_relative() {
        assert_eq!(
            pathdiff(Path::new("/a/stash"), Path::new("/a/game")),
            PathBuf::from("../stash")
        );
        assert_eq!(
            pathdiff(Path::new("/a/game/vendor/x"), Path::new("/a/game")),
            PathBuf::from("vendor/x")
        );
    }

    #[test]
    fn a_fetch_replaces_the_folder_whole() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "kit", &[("a.txt".into(), b"one".to_vec())]).unwrap();
        install(dir.path(), "kit", &[("b/c.txt".into(), b"two".to_vec())]).unwrap();
        assert!(!dir.path().join("deps/kit/a.txt").exists());
        assert_eq!(
            std::fs::read(dir.path().join("deps/kit/b/c.txt")).unwrap(),
            b"two"
        );
        assert!(!dir.path().join("deps/.kit.partial").exists());
    }

    /// A stand-in for Forge's three reads, answering one project, `ana/kit`,
    /// whose blob at `art/x.png` is `bytes`; it serves `served` for that blob,
    /// which a test makes differ to play a server that lies.
    fn forge(bytes: &'static [u8], served: &'static [u8]) -> (Forge, String) {
        use std::io::{BufRead as _, BufReader, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let sha = blob_id(bytes);
        let commit = "c".repeat(40);
        let tree = format!(
            r#"{{"data":{{"commit":"{commit}","files":[{{"path":"art/x.png","sha":"{sha}","size":{}}}]}}}}"#,
            bytes.len()
        );
        let blob_path = format!("/api/v1/forge/deps/p1/blob/{sha}");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                // The headers, up to the blank line.
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let (status, body): (&str, Vec<u8>) = if path == "/api/v1/forge/deps/ana/kit" {
                    (
                        "200 OK",
                        br#"{"data":{"id":"p1","default_ref":"main"}}"#.to_vec(),
                    )
                } else if path.starts_with("/api/v1/forge/deps/p1/tree?ref=") {
                    ("200 OK", tree.clone().into_bytes())
                } else if path == blob_path {
                    ("200 OK", served.to_vec())
                } else {
                    ("404 Not Found", Vec::new())
                };
                let mut stream = &stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        (Forge::at(&base, None), commit)
    }

    fn game(dir: &Path) {
        std::fs::write(
            dir.join("project.toml"),
            "[application]\nname = \"g\"\nmain_scene = \"m.toml\"\n\n[dependencies]\nkit = { project = \"ana/kit\" }\n",
        )
        .unwrap();
    }

    #[test]
    fn a_fetch_fills_deps_and_locks_the_commit_and_hash() {
        let (forge, commit) = forge(b"pixels", b"pixels");
        let dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        game(dir.path());
        fetch_from(&forge, Some(cache.path()), dir.path(), false, None).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("deps/kit/art/x.png")).unwrap(),
            b"pixels"
        );
        let lock = dependencies::read_lock(
            &std::fs::read_to_string(dir.path().join("project.lock")).unwrap(),
        )
        .unwrap();
        assert_eq!(lock[0].name, "kit");
        assert_eq!(lock[0].project, "p1");
        assert_eq!(lock[0].rev, commit);
        assert_eq!(
            lock[0].sha256,
            dependencies::tree_hash([("art/x.png", &b"pixels"[..])])
        );
        assert!(cache.path().join(blob_id(b"pixels")).is_file());
        // Fetched again from the lock, with the folder gone: the same files.
        std::fs::remove_dir_all(dir.path().join("deps")).unwrap();
        fetch_from(&forge, Some(cache.path()), dir.path(), false, None).unwrap();
        assert!(dir.path().join("deps/kit/art/x.png").is_file());
    }

    #[test]
    fn a_lock_that_does_not_match_is_refused_and_writes_nothing() {
        let (forge, commit) = forge(b"pixels", b"pixels");
        let dir = tempfile::tempdir().unwrap();
        game(dir.path());
        let lock = dependencies::write_lock(&[Locked {
            name: "kit".into(),
            project: "p1".into(),
            rev: commit,
            sha256: "0".repeat(64),
        }]);
        std::fs::write(dir.path().join("project.lock"), &lock).unwrap();
        let err = fetch_from(&forge, None, dir.path(), false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("does not match project.lock"),
            "{err:#}"
        );
        assert!(!dir.path().join("deps/kit").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("project.lock")).unwrap(),
            lock
        );
    }

    #[test]
    fn a_blob_that_is_not_its_id_is_refused() {
        let (forge, _) = forge(b"pixels", b"pixelz");
        let dir = tempfile::tempdir().unwrap();
        game(dir.path());
        let err = fetch_from(&forge, None, dir.path(), false, None).unwrap_err();
        assert!(format!("{err:#}").contains("other bytes"), "{err:#}");
        assert!(!dir.path().join("deps/kit").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_deps_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("deps")).unwrap();
        assert!(refuse_link(&dir.path().join("deps")).is_err());
        assert!(install(dir.path(), "kit", &[("a.txt".into(), b"one".to_vec())]).is_err());
        assert!(!elsewhere.path().join("kit").exists());
    }
}
