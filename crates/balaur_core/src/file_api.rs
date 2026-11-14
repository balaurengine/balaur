//! The script operations that touch files and the two text formats.
//!
//! Split out of `engine_api`, which registers them: `fs` reads and writes
//! inside the project, `toml` and `json` convert between text and script
//! values. The conversions are public because plugins speak JSON to servers.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use balaur_script::Value;

use crate::engine::Engine;
use crate::engine_api::text;
use crate::files::{self, FileBackend};
use crate::vocabulary::words;

/// The directories beyond the project's own that `fs.*` may reach.
///
/// The project root and the game's user-data directory are always allowed;
/// this is what a host adds. `balaur edit <game>` is the case it exists for:
/// the editor's own root is the editor's directory and it reaches the game it
/// is editing by absolute path.
#[derive(Default)]
pub struct FileRoots(pub Vec<PathBuf>);

/// Declare another directory `fs.*` may read and write inside.
pub fn add_root(eng: &Engine, root: impl AsRef<Path>) {
    let root = root.as_ref().to_path_buf();
    if eng.try_resource::<FileRoots>().is_none() {
        eng.insert_resource(FileRoots::default());
    }
    let roots = eng.resource::<FileRoots>();
    let mut roots = roots.borrow_mut();
    if !roots.0.contains(&root) {
        roots.0.push(root);
    }
}

/// Directories `fs.*` may read inside but not write: what an install ships.
#[derive(Default)]
pub struct ReadOnlyRoots(pub Vec<PathBuf>);

/// Declare a directory `fs.*` may read inside but never write.
pub fn add_read_only_root(eng: &Engine, root: impl AsRef<Path>) {
    let root = root.as_ref().to_path_buf();
    if eng.try_resource::<ReadOnlyRoots>().is_none() {
        eng.insert_resource(ReadOnlyRoots::default());
    }
    let roots = eng.resource::<ReadOnlyRoots>();
    let mut roots = roots.borrow_mut();
    if !roots.0.contains(&root) {
        roots.0.push(root);
    }
}

/// The project's own root and any a host added, for a reader resolving a
/// project-relative path. `fs.*` also reaches the user-data directory; a
/// scene file does not live there.
pub fn project_roots(eng: &Engine) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(root) = eng.try_resource::<crate::project::ProjectRoot>() {
        out.push(root.borrow().0.clone());
    }
    if let Some(extra) = eng.try_resource::<FileRoots>() {
        out.extend(extra.borrow().0.iter().cloned());
    }
    out
}

/// The editor project's folder, as the binary that runs the editor names it:
/// every script of that project is the editor's ([`editors_script`]).
pub struct EditorRoot(pub PathBuf);

/// Why a script is the editor's ([`editors_script`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorsScript {
    /// A file of the [`EditorRoot`] project.
    Project,
    /// A file under the `editor/` folder of a game or of one of its addons: a
    /// plugin the editor loads, which a game's runtime never runs.
    Plugin,
}

/// The roots [`editors_script`] judges against, resolved once while the roots
/// named stay the same: resolving each on every call cost a warm editor boot
/// 14 ms of 310.
#[derive(Default)]
struct ResolvedRoots {
    named: Vec<PathBuf>,
    editor_named: Option<PathBuf>,
    /// `named`, each resolved.
    real: Vec<PathBuf>,
    /// The project's own, resolved: what a relative path is inside.
    project: Option<PathBuf>,
    editor: Option<PathBuf>,
}

/// [`ResolvedRoots`] for the roots named now, resolved again when one was
/// added or removed since.
fn resolved_roots(
    eng: &Engine,
    fs: &dyn FileBackend,
) -> std::rc::Rc<std::cell::RefCell<ResolvedRoots>> {
    let named = project_roots(eng);
    let editor_named = eng
        .try_resource::<EditorRoot>()
        .map(|editor| editor.borrow().0.clone());
    let resolved = eng.try_resource::<ResolvedRoots>().unwrap_or_else(|| {
        eng.insert_resource(ResolvedRoots::default());
        eng.resource::<ResolvedRoots>()
    });
    let stale = {
        let kept = resolved.borrow();
        kept.named != named || kept.editor_named != editor_named
    };
    if stale {
        let own = eng.try_resource::<crate::project::ProjectRoot>().is_some();
        let real: Vec<PathBuf> = named.iter().map(|root| real_path(fs, root)).collect();
        *resolved.borrow_mut() = ResolvedRoots {
            project: own.then(|| real.first().cloned()).flatten(),
            editor: editor_named.as_ref().map(|editor| real_path(fs, editor)),
            real,
            named,
            editor_named,
        };
    }
    resolved
}

/// Whether the script at `path` is the editor's, and why; `None` for a game's.
/// Only the editor's scripts may call what only the editor has. A relative
/// path is the project's.
pub fn editors_script(eng: &Engine, path: &Path) -> Option<EditorsScript> {
    let fs = files::backend(eng);
    let roots = resolved_roots(eng, fs.as_ref());
    let roots = roots.borrow();
    let joined = match &roots.project {
        Some(project) if !files::rooted(path) => files::lexical(&project.join(path)),
        _ => files::lexical(path),
    };
    // A path spelled inside a resolved root needs no resolving of its own.
    let real = if roots.real.iter().any(|root| joined.starts_with(root)) {
        joined
    } else {
        real_path(fs.as_ref(), &joined)
    };
    // The innermost root holding it: a game the editor opened is a root of its own.
    let root = roots
        .real
        .iter()
        .filter(|root| real.starts_with(root))
        .max_by_key(|root| root.components().count())?;
    if roots.editor.as_ref() == Some(root) {
        return Some(EditorsScript::Project);
    }
    let inside: Vec<&str> = real
        .strip_prefix(root)
        .unwrap_or(&real)
        .iter()
        .map(|part| part.to_str().unwrap_or_default())
        .collect();
    let plugin = match inside.as_slice() {
        [folder, _, ..] if *folder == words::EDITOR => true,
        [addons, _, folder, _, ..] => *addons == words::ADDONS && *folder == words::EDITOR,
        _ => false,
    };
    plugin.then_some(EditorsScript::Plugin)
}

/// Every directory `fs.*` may write inside, in the order they are checked.
fn writable_roots(eng: &Engine) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(root) = eng.try_resource::<crate::project::ProjectRoot>() {
        out.push(root.borrow().0.clone());
    }
    out.push(crate::engine_api::user_data_dir_of(eng));
    if let Some(extra) = eng.try_resource::<FileRoots>() {
        out.extend(extra.borrow().0.iter().cloned());
    }
    out
}

/// Every directory `fs.*` may read inside: the writable ones, then the
/// read-only ones.
fn roots(eng: &Engine) -> Vec<PathBuf> {
    let mut out = writable_roots(eng);
    if let Some(shipped) = eng.try_resource::<ReadOnlyRoots>() {
        out.extend(shipped.borrow().0.iter().cloned());
    }
    out
}

/// Project-relative unless absolute, and inside a declared root either way.
///
/// A pack's bytecode and a third-party project opened in the editor are both
/// content, so a path they name is checked rather than trusted: `..` is
/// resolved before the check and a symlink out of a root is followed into the
/// place it really points.
///
/// # Errors
/// When the path lands outside every root the host declared.
pub fn resolve(eng: &Engine, path: &str) -> Result<PathBuf> {
    let named = Path::new(path);
    let joined = if crate::files::rooted(named) {
        named.to_path_buf()
    } else {
        match eng.try_resource::<crate::project::ProjectRoot>() {
            Some(root) => root.borrow().0.join(named),
            None => named.to_path_buf(),
        }
    };
    let fs = files::backend(eng);
    let real = real_path(fs.as_ref(), &joined);
    if roots(eng)
        .iter()
        .any(|root| real.starts_with(real_path(fs.as_ref(), root)))
    {
        return Ok(real);
    }
    bail!(
        "'{}' is outside every directory this project may reach",
        real.display()
    )
}

/// The path a root check is made against: `.` dropped, `..` popped, and on a
/// real filesystem every symlink in the part that exists resolved. A path
/// that does not exist yet still has to be checked, which is why the tail is
/// kept lexically rather than refused.
fn real_path(fs: &dyn FileBackend, path: &Path) -> PathBuf {
    fs.canonicalize(path)
}

/// The project files of a game run from a pack, for a project-relative path:
/// its data files are there, and the backend holds only what it wrote. The
/// pack answers first, so a file in the working directory cannot stand in.
fn packed_files(
    eng: &Engine,
    path: &str,
) -> Option<std::rc::Rc<std::cell::RefCell<crate::project::ProjectFiles>>> {
    if files::rooted(Path::new(path)) {
        return None;
    }
    let project = eng.try_resource::<crate::project::ProjectFiles>()?;
    let from_pack = project.borrow().from_pack();
    from_pack.then_some(project)
}

/// A document a pack holds apart from its assets, a scene or a `strings/` file.
fn packed_document(eng: &Engine, path: &str) -> Option<String> {
    packed_files(eng, path)?;
    eng.script_host()?.scene_source(path)
}

/// The path a script named, as its own project spells it.
fn named<'a>(eng: &Engine, args: &'a [Value], at: usize) -> Result<std::borrow::Cow<'a, str>> {
    Ok(crate::dependencies::scoped(eng, text(args, at)?))
}

/// A path a script writes to: resolved, and refused inside a dependency or a
/// root that is only read.
fn writable(eng: &Engine, args: &[Value], at: usize) -> Result<PathBuf> {
    let path = resolve(eng, &named(eng, args, at)?)?;
    crate::dependencies::refuse_write(eng, &path)?;
    let fs = files::backend(eng);
    if !writable_roots(eng)
        .iter()
        .any(|root| path.starts_with(real_path(fs.as_ref(), root)))
    {
        bail!(
            "'{}' is read only: it ships with the install",
            path.display()
        );
    }
    Ok(path)
}

fn strings(args: &[Value], at: usize) -> Result<Vec<&str>> {
    let Some(Value::List(items)) = args.get(at) else {
        bail!("argument {at} has to be a list of strings");
    };
    items
        .iter()
        .map(|item| match item {
            Value::Str(text) => Ok(text.as_str()),
            _ => Err(anyhow!("argument {at} has to be a list of strings")),
        })
        .collect()
}

pub(crate) fn engine_watch(eng: &Engine, args: &[Value]) -> Result<Value> {
    let named = text(args, 0)?;
    let real = resolve(eng, named)?;
    let extensions: Vec<String> = strings(args, 1)?.into_iter().map(String::from).collect();
    Ok(Value::Bool(
        eng.script_host()
            .is_some_and(|host| host.watch(&real, named, &extensions)),
    ))
}

pub(crate) fn reload_scripts(eng: &Engine, args: &[Value]) -> Result<Value> {
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    // A file that does not compile is the answer, not a failure of the call:
    // the tool that saved it says what broke and carries on.
    Ok(match host.reload(&strings(args, 0)?) {
        Ok(()) => Value::text(""),
        Err(err) => Value::text(format!("{err:#}")),
    })
}

pub(crate) fn fs_read(eng: &Engine, args: &[Value]) -> Result<Value> {
    let named = &*named(eng, args, 0)?;
    if let Some(project) = packed_files(eng, named) {
        let project = project.borrow();
        if project.packs(named)
            && let Ok(bytes) = project.read(named)
        {
            return Ok(String::from_utf8(bytes).map_or(Value::Nil, Value::text));
        }
        drop(project);
        if let Some(text) = packed_document(eng, named) {
            return Ok(Value::text(text));
        }
    }
    let path = resolve(eng, named)?;
    Ok(files::backend(eng)
        .read(&path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map_or(Value::Nil, Value::text))
}

/// Write a file, making the directory it goes in.
///
/// Creating the parent is part of writing: a tool that saves an asset into
/// `animations/` should not fail because the project has never had one.
pub(crate) fn fs_write(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = writable(eng, args, 0)?;
    write_whole(&*files::backend(eng), &path, text(args, 1)?.as_bytes())?;
    Ok(Value::Nil)
}

/// Write `bytes` beside `path` and rename them over it, so a crash or a power
/// cut mid-write leaves the old file whole, never a truncated one.
pub fn write_whole(fs: &dyn FileBackend, path: &Path, bytes: &[u8]) -> Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    let part = path.with_file_name(name);
    fs.write(&part, bytes)
        .with_context(|| format!("writing {}", part.display()))?;
    // A rename alone survives a crash; the bytes survive a power cut only once synced.
    fs.sync(&part);
    fs.rename(&part, path)
        .with_context(|| format!("replacing {}", path.display()))
}

pub(crate) fn fs_exists(eng: &Engine, args: &[Value]) -> Result<Value> {
    let named = &*named(eng, args, 0)?;
    if packed_files(eng, named).is_some_and(|project| project.borrow().packs(named))
        || packed_document(eng, named).is_some()
    {
        return Ok(Value::Bool(true));
    }
    let path = resolve(eng, named)?;
    Ok(Value::Bool(files::backend(eng).exists(&path)))
}

/// Delete a file, or a directory and everything under it.
///
/// Answers whether there was anything there, rather than failing: a tool
/// deleting what a previous run already deleted has nothing to report.
pub(crate) fn fs_remove(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = writable(eng, args, 0)?;
    Ok(Value::Bool(files::backend(eng).remove(&path)?))
}

pub(crate) fn fs_mkdir(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = writable(eng, args, 0)?;
    files::backend(eng).mkdir(&path)?;
    Ok(Value::Nil)
}

/// Move a file or directory. The destination's parent is made first, for the
/// same reason `fs::write` makes one.
pub(crate) fn fs_rename(eng: &Engine, args: &[Value]) -> Result<Value> {
    let from = writable(eng, args, 0)?;
    let to = writable(eng, args, 1)?;
    files::backend(eng).rename(&from, &to)?;
    Ok(Value::Nil)
}

/// Copy a file, bytes and all, making the destination's directory first.
///
/// Not `read` then `write`: those go through script strings, and a `.png` or
/// a `.glb` is not text.
pub(crate) fn fs_copy(eng: &Engine, args: &[Value]) -> Result<Value> {
    let from = resolve(eng, &named(eng, args, 0)?)?;
    let to = writable(eng, args, 1)?;
    let bytes = files::backend(eng).read(&from)?;
    files::backend(eng).write(&to, &bytes)?;
    Ok(Value::Nil)
}

/// When a file last changed, in seconds since the epoch, or `()` for one that
/// is not there. A tool polling for edits compares this instead of re-reading.
pub(crate) fn fs_mtime(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = resolve(eng, &named(eng, args, 0)?)?;
    Ok(files::backend(eng)
        .mtime(&path)
        .map_or(Value::Nil, Value::Num))
}

pub(crate) fn fs_list(eng: &Engine, args: &[Value]) -> Result<Value> {
    let dir = &*named(eng, args, 0)?;
    let path = resolve(eng, dir)?;
    let mut names: Vec<(String, bool)> = files::backend(eng)
        .list(&path)
        .into_iter()
        .filter(|(name, _)| !name.starts_with('.'))
        .collect();
    if let Some(project) = packed_files(eng, dir) {
        names.extend(project.borrow().packed_children(dir));
    }
    // Sorted for stable UI and reproducible tooling runs.
    names.sort();
    names.dedup();
    Ok(Value::List(
        names
            .into_iter()
            .map(|(name, is_dir)| {
                Value::Map(vec![
                    (words::scene_toml::NAME.into(), Value::Str(name)),
                    ("is_dir".into(), Value::Bool(is_dir)),
                ])
            })
            .collect(),
    ))
}

// Text that does not parse is nil rather than an error: a script reading a
// file a person edits checks what it got, and has no way to catch.
pub(crate) fn toml_parse(_: &Engine, args: &[Value]) -> Result<Value> {
    match toml::from_str::<toml::Value>(text(args, 0)?) {
        Ok(parsed) => crate::node_api::from_toml(&parsed),
        Err(_) => Ok(Value::Nil),
    }
}

pub(crate) fn toml_encode(_: &Engine, args: &[Value]) -> Result<Value> {
    let value = args.first().ok_or_else(|| anyhow!("nothing to encode"))?;
    Ok(Value::Str(toml::to_string(&crate::node_api::to_toml(
        value,
    )?)?))
}

/// Write a table into an existing document, keeping everything the table
/// does not describe: comments, key order, and tables nobody named.
///
/// `toml::to_string` builds a document from a value and so has neither to
/// keep, which is why an editor saving through `encode` loses a hand-written
/// file's comments. This edits the document instead.
pub(crate) fn toml_patch(_: &Engine, args: &[Value]) -> Result<Value> {
    let existing = text(args, 0)?;
    let value = args
        .get(1)
        .ok_or_else(|| anyhow!("nothing to write into the document"))?;
    let toml::Value::Table(table) = crate::node_api::to_toml(value)? else {
        bail!("toml.patch writes a table over a document");
    };
    let before: toml::Table = toml::from_str(existing).context("parsing the document")?;
    let mut doc: toml_edit::DocumentMut = existing.parse().context("parsing the document")?;
    for (key, value) in &table {
        patch_key(doc.as_table_mut(), key, before.get(key), value);
    }
    Ok(Value::Str(doc.to_string()))
}

/// Write `value` at `key`, `was` being what the document held there. A table
/// both hold is patched key by key: a key it no longer holds goes, a changed
/// one is written with the comment beside it, and a table left empty goes.
fn patch_key(
    table: &mut toml_edit::Table,
    key: &str,
    was: Option<&toml::Value>,
    value: &toml::Value,
) {
    if was == Some(value) {
        return;
    }
    if let (Some(toml::Value::Table(was)), toml::Value::Table(now)) = (was, value)
        && let Some(existing) = table.get_mut(key).and_then(toml_edit::Item::as_table_mut)
    {
        let gone: Vec<String> = was
            .keys()
            .filter(|k| !now.contains_key(*k))
            .cloned()
            .collect();
        for name in gone {
            existing.remove(&name);
        }
        for (name, inner) in now {
            patch_key(existing, name, was.get(name), inner);
        }
        if now.is_empty() {
            table.remove(key);
        }
        return;
    }
    let mut item = as_item(value);
    match (table.get(key), &mut item) {
        (Some(toml_edit::Item::Value(old)), toml_edit::Item::Value(new)) => {
            *new.decor_mut() = old.decor().clone();
        }
        (old, _) => keep_leading_comment(old, &mut item),
    }
    table[key] = item;
}

/// A whole document written over `existing`: a key `table` no longer holds
/// goes, a key whose value moved is written again, and every other key keeps
/// its text, its comments and its place.
pub(crate) fn rewritten(
    existing: &str,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<String> {
    let before: toml::map::Map<String, toml::Value> =
        toml::from_str(existing).context("parsing the document")?;
    let mut doc: toml_edit::DocumentMut = existing.parse().context("parsing the document")?;
    let gone: Vec<String> = before
        .keys()
        .filter(|key| !table.contains_key(*key))
        .cloned()
        .collect();
    for key in gone {
        doc.remove(&key);
    }
    for (key, value) in table {
        patch_key(doc.as_table_mut(), key, before.get(key), value);
    }
    Ok(doc.to_string())
}

/// Carry the comment above a list's first `[[block]]` over to the list that
/// replaces it. That comment is usually the file's own header, which a list
/// written whole would otherwise take with it.
fn keep_leading_comment(was: Option<&toml_edit::Item>, now: &mut toml_edit::Item) {
    let (Some(toml_edit::Item::ArrayOfTables(was)), toml_edit::Item::ArrayOfTables(now)) =
        (was, now)
    else {
        return;
    };
    let (Some(first), Some(replacing)) = (was.get(0), now.get_mut(0)) else {
        return;
    };
    if let Some(prefix) = first.decor().prefix() {
        replacing.decor_mut().set_prefix(prefix.clone());
    }
}

/// A parsed value as a document item. An array of tables is written as one,
/// so `[[nodes]]` comes back the way it was written rather than as an inline
/// list a scene file would not be recognisable in.
pub(crate) fn as_item(value: &toml::Value) -> toml_edit::Item {
    if let toml::Value::Table(table) = value {
        let mut out = toml_edit::Table::new();
        for (key, inner) in table {
            out.insert(key, as_item(inner));
        }
        return toml_edit::Item::Table(out);
    }
    if let toml::Value::Array(items) = value
        && items.iter().all(toml::Value::is_table)
        && !items.is_empty()
    {
        let mut out = toml_edit::ArrayOfTables::new();
        for item in items {
            if let toml_edit::Item::Table(table) = as_item(item) {
                out.push(table);
            }
        }
        return toml_edit::Item::ArrayOfTables(out);
    }
    let text = toml::to_string(&toml::toml! { v = (value.clone()) }).unwrap_or_default();
    text.parse::<toml_edit::DocumentMut>()
        .ok()
        .map_or_else(|| toml_edit::value(""), |doc| doc["v"].clone())
}

pub(crate) fn json_parse(_: &Engine, args: &[Value]) -> Result<Value> {
    match serde_json::from_str::<serde_json::Value>(text(args, 0)?) {
        Ok(parsed) => from_json(&parsed),
        Err(_) => Ok(Value::Nil),
    }
}

pub(crate) fn json_encode(_: &Engine, args: &[Value]) -> Result<Value> {
    let value = args.first().ok_or_else(|| anyhow!("nothing to encode"))?;
    Ok(Value::Str(serde_json::to_string(&to_json(value)?)?))
}

/// Unlike TOML, JSON has null, so nil survives a round trip.
pub fn from_json(v: &serde_json::Value) -> Result<Value> {
    Ok(match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Num(
                n.as_f64()
                    .ok_or_else(|| anyhow!("{n} does not fit a script number"))?,
            ),
        },
        serde_json::Value::String(s) => Value::Str(s.clone()),
        serde_json::Value::Array(items) => {
            Value::List(items.iter().map(from_json).collect::<Result<_>>()?)
        }
        serde_json::Value::Object(map) => Value::Map(
            map.iter()
                .map(|(k, val)| Ok((k.as_str().into(), from_json(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

pub fn to_json(v: &Value) -> Result<serde_json::Value> {
    Ok(match v {
        Value::Nil => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        // JSON has no NaN or infinity, so those error instead of encoding.
        Value::Num(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| anyhow!("{n} has no JSON representation"))?,
        Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Node(_) => {
            return Err(anyhow!("a node is not JSON data"));
        }
        // JSON has no byte string, and guessing an encoding here would make
        // the round trip lossy in a way the caller never asked for.
        Value::Bytes(_) => return Err(anyhow!("bytes are not JSON data; encode them first")),
        Value::Many(_) => return Err(anyhow!("several values are not one JSON document")),
        Value::Vec2(a) => json_number_list(a)?,
        Value::Vec3(a) => json_number_list(a)?,
        Value::Color(a) => json_number_list(a)?,
        Value::Transform2d(a) => json_number_list(a)?,
        Value::Transform3d(a) => json_number_list(a)?,
        Value::List(items) => {
            serde_json::Value::Array(items.iter().map(to_json).collect::<Result<_>>()?)
        }
        Value::Map(pairs) => serde_json::Value::Object(
            pairs
                .iter()
                .map(|(k, val)| Ok((k.clone(), to_json(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn json_number_list(a: &[f32]) -> Result<serde_json::Value> {
    Ok(serde_json::Value::Array(
        a.iter()
            .map(|n| {
                serde_json::Number::from_f64(f64::from(*n))
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| anyhow!("{n} has no JSON representation"))
            })
            .collect::<Result<_>>()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A memory backend that also lists what was asked of it, in order.
    #[derive(Default)]
    struct Recording {
        fs: files::MemoryFs,
        asked: std::cell::RefCell<Vec<String>>,
    }

    impl Recording {
        fn note(&self, what: &str, path: &Path) {
            self.asked
                .borrow_mut()
                .push(format!("{what} {}", path.display()));
        }
    }

    impl FileBackend for Recording {
        fn read(&self, path: &Path) -> Result<Vec<u8>> {
            self.fs.read(path)
        }
        fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
            self.note("write", path);
            self.fs.write(path, bytes)
        }
        fn exists(&self, path: &Path) -> bool {
            self.fs.exists(path)
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.fs.is_dir(path)
        }
        fn remove(&self, path: &Path) -> Result<bool> {
            self.fs.remove(path)
        }
        fn mkdir(&self, path: &Path) -> Result<()> {
            self.fs.mkdir(path)
        }
        fn rename(&self, from: &Path, to: &Path) -> Result<()> {
            self.note("rename", from);
            self.fs.rename(from, to)
        }
        fn mtime(&self, path: &Path) -> Option<f64> {
            self.fs.mtime(path)
        }
        fn list(&self, path: &Path) -> Vec<(String, bool)> {
            self.fs.list(path)
        }
        fn canonicalize(&self, path: &Path) -> PathBuf {
            self.note("canonicalize", path);
            self.fs.canonicalize(path)
        }
        fn sync(&self, path: &Path) {
            self.note("sync", path);
        }
    }

    #[test]
    fn a_whole_write_lands_beside_the_file_is_synced_then_renamed_over_it() {
        let fs = Recording::default();
        let path = Path::new("/data/saves/slot.toml");
        // Joined as the code joins it: Windows puts its own separator there.
        let part = Path::new("/data/saves").join("slot.toml.part");
        fs.fs.write(path, b"old").unwrap();
        write_whole(&fs, path, b"new").unwrap();
        assert_eq!(
            *fs.asked.borrow(),
            ["write", "sync", "rename"].map(|step| format!("{step} {}", part.display()))
        );
        assert_eq!(fs.read(path).unwrap(), b"new");
        assert!(!fs.exists(&part));
    }

    #[test]
    fn a_judgment_resolves_each_root_once_until_one_is_added() {
        let fs = std::rc::Rc::new(Recording::default());
        let engine = crate::Engine::default();
        files::set_backend(&engine, fs.clone());
        engine.insert_resource(crate::project::ProjectRoot(PathBuf::from("/editor")));
        engine.insert_resource(EditorRoot(PathBuf::from("/editor")));
        let resolved = || {
            let asked = fs.asked.borrow();
            asked
                .iter()
                .filter(|one| one.starts_with("canonicalize"))
                .count()
        };
        let own = Path::new("scripts/tools.bs");
        assert_eq!(editors_script(&engine, own), Some(EditorsScript::Project));
        let first = resolved();
        assert_eq!(editors_script(&engine, own), Some(EditorsScript::Project));
        assert_eq!(resolved(), first, "the roots stay resolved");
        let plugin = Path::new("/game/editor/plugin.bs");
        assert_eq!(
            editors_script(&engine, plugin),
            None,
            "no root holds it yet"
        );
        add_root(&engine, "/game");
        assert_eq!(editors_script(&engine, plugin), Some(EditorsScript::Plugin));
        let game = Path::new("/game/scripts/main.bs");
        let before = resolved();
        assert_eq!(editors_script(&engine, game), None);
        assert_eq!(resolved(), before, "a path inside a root resolves nothing");
    }

    fn patched(existing: &str, table: &str) -> String {
        let value =
            crate::node_api::from_toml(&toml::from_str::<toml::Value>(table).unwrap()).unwrap();
        let engine = crate::Engine::default();
        match toml_patch(&engine, &[Value::Str(existing.into()), value]).unwrap() {
            Value::Str(text) => text,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_changed_key_is_written_alone_and_the_table_keeps_its_comments() {
        let text = patched(
            "# The window.\n[window]\n# Wide enough.\nwidth = 1280 # points\nheight = 800\n",
            "[window]\nwidth = 900\nheight = 800\n",
        );
        assert_eq!(
            text,
            "# The window.\n[window]\n# Wide enough.\nwidth = 900 # points\nheight = 800\n"
        );
    }

    #[test]
    fn a_key_the_table_no_longer_holds_goes_and_so_does_an_emptied_table() {
        let text = patched(
            "[app]\nname = \"g\"\n\n[override.android.window]\nwidth = 400\n",
            "[app]\nname = \"g\"\n[override]\n",
        );
        let back: toml::Value = toml::from_str(&text).unwrap();
        assert!(back.get("override").is_none(), "{text}");
        assert_eq!(back["app"]["name"].as_str(), Some("g"));
    }

    // A list written whole keeps the comment its first block carried, which
    // is the file's header when the list opens the file.
    #[test]
    fn a_list_written_whole_keeps_the_header_above_it() {
        let text = patched(
            "# The file's header.\n[[assets]]\nid = \"a\"\n\n[[nodes]]\nid = \"n\"\n",
            "[[assets]]\nid = \"b\"\n[[assets]]\nid = \"c\"\n",
        );
        assert!(text.starts_with("# The file's header."), "{text}");
        let back: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(back["assets"].as_array().unwrap().len(), 2);
        assert_eq!(back["nodes"][0]["id"].as_str(), Some("n"));
    }
}
