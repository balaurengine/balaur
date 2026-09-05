//! Stable asset ids and rename refactoring.
//!
//! Paths stay the reference in files: readable, diffable, and every tool
//! understands them. Two things keep a rename from breaking them:
//!
//! - [`rename`] moves a file or directory and rewrites every reference to it
//!   in every `.toml` under the project, comments and key order kept.
//! - `assets/index.toml` maps `id → path`, and `id://<id>` stands in for a
//!   path wherever one is written. [`assign_id`] hands a file its id; the
//!   loader resolves the reference through `ProjectFiles::path_of`.
//!
//! Both work on whichever project a path is in — the nearest ancestor with
//! a `project.toml` — so the editor, whose own project is the editor, can
//! refactor the game it has open by absolute path.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};

use crate::assets::INDEX_PATH;
use crate::engine::Engine;
use crate::files::{self, FileBackend};

/// `assets/index.toml` as a map: every top-level key is an id, every value
/// the project-relative path it names.
pub fn parse(text: &str) -> Result<BTreeMap<String, String>> {
    let document: toml::Value = toml::from_str(text).context("parsing the id index")?;
    let table = document
        .as_table()
        .ok_or_else(|| anyhow!("the id index is not a table"))?;
    let mut out = BTreeMap::new();
    for (id, path) in table {
        let path = path
            .as_str()
            .ok_or_else(|| anyhow!("id '{id}' maps to {}, not a path", path.type_str()))?;
        out.insert(id.clone(), path.to_string());
    }
    Ok(out)
}

/// The index as the text of `assets/index.toml`: one `id = "path"` per line,
/// in id order, so two writes of the same map diff as nothing.
#[must_use]
pub fn encode(index: &BTreeMap<String, String>) -> String {
    let mut out = String::from(
        "# id = \"path\", one per asset. A reference \"id://<id>\" resolves here, so a\n\
         # file may move without every scene that names it changing.\n",
    );
    for (id, path) in index {
        let _ = writeln!(out, "{id} = {}", toml::Value::String(path.clone()));
    }
    out
}

fn is_toml(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
}

/// The project a path belongs to: the nearest ancestor holding a
/// `project.toml`.
pub fn project_root_of(backend: &dyn FileBackend, path: &Path) -> Result<PathBuf> {
    path.ancestors()
        .find(|dir| backend.exists(&dir.join("project.toml")))
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            anyhow!(
                "{} is in no project: no project.toml above it",
                path.display()
            )
        })
}

/// `path` relative to `root`, with forward slashes: how a scene spells it.
fn relative(root: &Path, path: &Path) -> Result<String> {
    let rel = path.strip_prefix(root).map_err(|_| {
        anyhow!(
            "{} is outside the project at {}",
            path.display(),
            root.display()
        )
    })?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// A path as `fs.*` takes it — project-relative or absolute inside a root —
/// placed in its project.
struct Located {
    root: PathBuf,
    absolute: PathBuf,
    relative: String,
}

fn locate(eng: &Engine, backend: &dyn FileBackend, path: &str) -> Result<Located> {
    let absolute = crate::file_api::resolve(eng, path)?;
    let root = project_root_of(backend, &absolute)?;
    let relative = relative(&root, &absolute)?;
    Ok(Located {
        root,
        absolute,
        relative,
    })
}

fn read_index(backend: &dyn FileBackend, root: &Path) -> Result<BTreeMap<String, String>> {
    let path = root.join(INDEX_PATH);
    if !backend.exists(&path) {
        return Ok(BTreeMap::new());
    }
    let text = String::from_utf8(backend.read(&path)?).context("the id index is not UTF-8")?;
    parse(&text)
}

fn write_index(
    backend: &dyn FileBackend,
    root: &Path,
    index: &BTreeMap<String, String>,
) -> Result<()> {
    backend.write(&root.join(INDEX_PATH), encode(index).as_bytes())
}

/// Tell the running engine its index moved, when the project written to is
/// the one it is running.
fn refresh(eng: &Engine, backend: &dyn FileBackend, root: &Path) {
    let own = eng
        .try_resource::<crate::project::ProjectRoot>()
        .map(|r| backend.canonicalize(&r.borrow().0));
    if own.is_some_and(|own| own == backend.canonicalize(root)) {
        let _ = crate::assets::reload(eng, INDEX_PATH);
    }
}

/// The id `assets/index.toml` gives a path, or `None` when it has none.
pub fn id_of(eng: &Engine, path: &str) -> Result<Option<String>> {
    let backend = files::backend(eng);
    let at = locate(eng, &*backend, path)?;
    Ok(read_index(&*backend, &at.root)?
        .into_iter()
        .find(|(_, p)| *p == at.relative)
        .map(|(id, _)| id))
}

/// The id a file has, giving it one if it has none.
///
/// The id is a digest of the path and the content, so a rebuilt index gives
/// a file the id it had. An asset document — one declaring a `type` — also
/// carries it as a top-level `id`, and one that already declares an id the
/// index does not know keeps that one.
pub fn assign_id(eng: &Engine, path: &str) -> Result<String> {
    let backend = files::backend(eng);
    let at = locate(eng, &*backend, path)?;
    if !backend.exists(&at.absolute) {
        bail!("nothing at {} to give an id", at.relative);
    }
    let mut index = read_index(&*backend, &at.root)?;
    if let Some((id, _)) = index.iter().find(|(_, p)| **p == at.relative) {
        return Ok(id.clone());
    }
    let bytes = backend.read(&at.absolute).unwrap_or_default();
    let declared = declared_id(&at.relative, &bytes);
    let stamp = declared.is_none() && is_toml(&at.relative);
    let id = match declared {
        Some(id) if !index.contains_key(&id) => id,
        _ => fresh_id(&index, &at.relative, &bytes),
    };
    index.insert(id.clone(), at.relative.clone());
    write_index(&*backend, &at.root, &index)?;
    if stamp {
        stamp_id(&*backend, &at.absolute, &bytes, &id)?;
    }
    refresh(eng, &*backend, &at.root);
    Ok(id)
}

/// The top-level `id` an asset document already declares.
fn declared_id(relative: &str, bytes: &[u8]) -> Option<String> {
    if !is_toml(relative) {
        return None;
    }
    let document: toml::Value = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
    document.get("type")?;
    document.get("id")?.as_str().map(str::to_string)
}

/// Write `id = "<id>"` at the top of an asset document, keeping the rest of
/// the text as it was. A document with no `type` is not an asset — a scene,
/// a strings catalogue — and is left alone.
fn stamp_id(backend: &dyn FileBackend, absolute: &Path, bytes: &[u8], id: &str) -> Result<()> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(());
    };
    let Ok(mut document) = text.parse::<toml_edit::DocumentMut>() else {
        return Ok(());
    };
    if !document.contains_key("type") || document.contains_key("id") {
        return Ok(());
    }
    document.insert("id", toml_edit::value(id));
    backend.write(absolute, document.to_string().as_bytes())
}

/// Sixteen hex digits from the path and the content, bumped until unused.
fn fresh_id(index: &BTreeMap<String, String>, relative: &str, bytes: &[u8]) -> String {
    let mut hash = crate::assets::digest_bytes(crate::assets::FNV_OFFSET, relative.as_bytes());
    hash = crate::assets::digest_bytes(hash, bytes);
    loop {
        let id = format!("{hash:016x}");
        if !index.contains_key(&id) {
            return id;
        }
        hash = crate::assets::digest_bytes(hash, b"again");
    }
}

/// Move a file or directory and rewrite every reference to it.
///
/// Every `.toml` under the project is read through `toml_edit`, so comments
/// and key order survive; a string equal to the old path, an `old#entry`,
/// or a path under a moved directory becomes the new spelling, and the id
/// index follows. Script sources are not touched: a path in a `.rn` is a
/// value the script computes, and `id://` is the reference for one that
/// must survive. Answers the project-relative files rewritten, in path
/// order.
pub fn rename(eng: &Engine, from: &str, to: &str) -> Result<Vec<String>> {
    let backend = files::backend(eng);
    let old = locate(eng, &*backend, from)?;
    let new_absolute = crate::file_api::resolve(eng, to)?;
    let new = relative(&old.root, &new_absolute)?;
    if !backend.exists(&old.absolute) {
        bail!("nothing at {} to rename", old.relative);
    }
    if backend.exists(&new_absolute) {
        bail!("{new} already exists");
    }
    let is_dir = backend.is_dir(&old.absolute);
    backend
        .rename(&old.absolute, &new_absolute)
        .with_context(|| format!("moving {} to {new}", old.relative))?;
    let mut rewritten = Vec::new();
    for file in toml_files(&*backend, &old.root) {
        let absolute = old.root.join(&file);
        let Ok(text) = backend
            .read(&absolute)
            .and_then(|b| Ok(String::from_utf8(b)?))
        else {
            continue;
        };
        let Ok(mut document) = text.parse::<toml_edit::DocumentMut>() else {
            continue;
        };
        let mut changed = false;
        rewrite_item(
            document.as_item_mut(),
            &old.relative,
            &new,
            is_dir,
            &mut changed,
        );
        if changed {
            backend.write(&absolute, document.to_string().as_bytes())?;
            rewritten.push(file);
        }
    }
    if let Ok(cache) = crate::assets::state_of(eng) {
        cache.borrow_mut().forget_under(&old.relative);
    }
    crate::assets::invalidate(eng);
    refresh(eng, &*backend, &old.root);
    Ok(rewritten)
}

/// Every `.toml` under `root`, project-relative and sorted. Dot directories
/// and `target` are skipped: neither holds content a scene names.
fn toml_files(backend: &dyn FileBackend, root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending = vec![String::new()];
    while let Some(dir) = pending.pop() {
        let full = if dir.is_empty() {
            root.to_path_buf()
        } else {
            root.join(&dir)
        };
        for (name, is_dir) in backend.list(&full) {
            if name.starts_with('.') || (dir.is_empty() && name == "target") {
                continue;
            }
            let rel = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                pending.push(rel);
            } else if is_toml(&rel) {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// The new spelling of a reference the rename touched, or `None`.
fn rewritten(value: &str, from: &str, to: &str, is_dir: bool) -> Option<String> {
    if value == from {
        return Some(to.to_string());
    }
    let rest = value.strip_prefix(from)?;
    (rest.starts_with('#') || (is_dir && rest.starts_with('/'))).then(|| format!("{to}{rest}"))
}

fn rewrite_item(
    item: &mut toml_edit::Item,
    from: &str,
    to: &str,
    is_dir: bool,
    changed: &mut bool,
) {
    match item {
        toml_edit::Item::Value(value) => rewrite_value(value, from, to, is_dir, changed),
        toml_edit::Item::Table(table) => {
            for (_, inner) in table.iter_mut() {
                rewrite_item(inner, from, to, is_dir, changed);
            }
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (_, inner) in table.iter_mut() {
                    rewrite_item(inner, from, to, is_dir, changed);
                }
            }
        }
        toml_edit::Item::None => {}
    }
}

fn rewrite_value(
    value: &mut toml_edit::Value,
    from: &str,
    to: &str,
    is_dir: bool,
    changed: &mut bool,
) {
    match value {
        toml_edit::Value::String(text) => {
            if let Some(new) = rewritten(text.value(), from, to, is_dir) {
                let decor = text.decor().clone();
                let mut formatted = toml_edit::Formatted::new(new);
                *formatted.decor_mut() = decor;
                *value = toml_edit::Value::String(formatted);
                *changed = true;
            }
        }
        toml_edit::Value::Array(items) => {
            for inner in items.iter_mut() {
                rewrite_value(inner, from, to, is_dir, changed);
            }
        }
        toml_edit::Value::InlineTable(table) => {
            for (_, inner) in table.iter_mut() {
                rewrite_value(inner, from, to, is_dir, changed);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{encode, parse, rewritten};

    #[test]
    fn the_index_round_trips_in_id_order() {
        let text = "b = \"art/b.png\"\na = \"art/a.png\"\n";
        let index = parse(text).unwrap();
        let encoded = encode(&index);
        assert!(encoded.find("a = ").unwrap() < encoded.find("b = ").unwrap());
        assert_eq!(parse(&encoded).unwrap(), index);
    }

    #[test]
    fn a_reference_is_rewritten_whole_with_its_entry_or_under_a_directory() {
        assert_eq!(
            rewritten("art/a.png", "art/a.png", "art/b.png", false).as_deref(),
            Some("art/b.png")
        );
        assert_eq!(
            rewritten("clips/a.toml#run", "clips/a.toml", "clips/b.toml", false).as_deref(),
            Some("clips/b.toml#run")
        );
        assert_eq!(
            rewritten("art/a.png", "art", "images", true).as_deref(),
            Some("images/a.png")
        );
        assert_eq!(rewritten("art/a.png", "art", "images", false), None);
        assert_eq!(rewritten("artful/a.png", "art", "images", true), None);
    }
}
