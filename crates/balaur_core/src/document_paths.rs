//! Which root owns a file, and how the paths inside it read.
//!
//! A document names its files from its own project's root. `balaur edit
//! <game>` runs with the editor as the project root and the game as a second
//! one, so `models/column.glb` inside a game's scene reads under `editor/` and
//! the node draws nothing. A document read from a root other than the
//! project's has its paths made absolute against the root that owns it; a host
//! with one root never gets here.
//!
//! `relative_in` is the inverse, for a document on its way back to a file:
//! the editor loads a material, edits one key and saves it, and an absolute
//! path written into the game would not survive another machine.

use std::path::{Path, PathBuf};

use crate::engine::Engine;
use crate::files::FileBackend;

/// The root that owns `path`: the longest declared root it lies under, or the
/// nearest directory above it holding a `project.toml`.
///
/// The walk is for a file from a root nobody declared — a material handed in
/// by absolute path. It needs a manifest, which a host handing the engine a
/// bare directory has not got, so the declared roots are asked first.
#[must_use]
pub fn owner_of(eng: &Engine, path: &Path) -> Option<PathBuf> {
    let declared = crate::file_api::project_roots(eng);
    let owner = declared
        .iter()
        .filter(|root| !root.as_os_str().is_empty() && path.starts_with(root))
        .max_by_key(|root| root.components().count());
    if let Some(root) = owner {
        return Some(root.clone());
    }
    let files = crate::files::backend(eng);
    crate::asset_index::project_root_of(files.as_ref(), path).ok()
}

/// The root a document read from `path` names its files from, or `None` when
/// that is the project's own and nothing needs rewriting. `read_from` is the
/// root that answered the read, which is what a relative path belongs to.
pub(crate) fn foreign_root(eng: &Engine, path: &str, read_from: &Path) -> Option<PathBuf> {
    let named = Path::new(path);
    let owner = if crate::files::rooted(named) {
        owner_of(eng, named)?
    } else {
        read_from.to_path_buf()
    };
    let project = crate::file_api::project_roots(eng).into_iter().next()?;
    (owner != project).then_some(owner)
}

/// Rewrite every file path in a document against `root`. `ids` also resolves
/// `id://` through that root's own index, which only a document the engine
/// never writes back may ask for: the spelling is lost, not translated.
pub(crate) fn absolute_in(eng: &Engine, root: &Path, value: &mut toml::Value, ids: Ids) {
    let files = crate::files::backend(eng);
    let index = match ids {
        Ids::Resolve => index_at(files.as_ref(), root),
        Ids::Keep => None,
    };
    walk(
        &mut |text: &mut String| {
            if let Some(rest) = text.strip_prefix(crate::assets::ID_PREFIX) {
                if let Some(named) = index.as_ref().and_then(|index| id_path(index, rest)) {
                    *text = join(root, &named);
                }
            } else if names_a_file(text) && files.exists(&root.join(&text)) {
                *text = join(root, text);
            }
        },
        value,
    );
}

/// Put every path under `root` back to how that root's own project spells it,
/// for a document about to be written to a file inside it.
pub(crate) fn relative_in(root: &Path, value: &mut toml::Value) {
    let prefix = join(root, "");
    walk(
        &mut |text: &mut String| {
            if let Some(rest) = text.strip_prefix(&prefix) {
                *text = rest.to_string();
            }
        },
        value,
    );
}

/// Whether `id://` references are resolved on the way in.
#[derive(Clone, Copy)]
pub(crate) enum Ids {
    Resolve,
    Keep,
}

/// Every string in a document, in declaration order.
fn walk(on: &mut dyn FnMut(&mut String), value: &mut toml::Value) {
    match value {
        toml::Value::String(text) => on(text),
        toml::Value::Array(items) => {
            for item in items {
                walk(on, item);
            }
        }
        toml::Value::Table(table) => {
            for (_, item) in table.iter_mut() {
                walk(on, item);
            }
        }
        _ => {}
    }
}

/// `root` and `path` joined, always with forward slashes: a document is read
/// back by a reader that splits on `#`, and Windows separators in a value that
/// a scene also compares as text would make one path two spellings.
fn join(root: &Path, path: &str) -> String {
    let root = root.to_string_lossy().replace('\\', "/");
    match root.strip_suffix('/') {
        Some(trimmed) => format!("{trimmed}/{path}"),
        None => format!("{root}/{path}"),
    }
}

/// The id index a root ships, or `None` when it has none.
fn index_at(
    files: &dyn FileBackend,
    root: &Path,
) -> Option<std::collections::BTreeMap<String, String>> {
    let bytes = files.read(&root.join(crate::assets::INDEX_PATH)).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    crate::asset_index::parse(&text).ok()
}

/// The path an `id://` names, with any `#entry` kept.
fn id_path(index: &std::collections::BTreeMap<String, String>, rest: &str) -> Option<String> {
    let (id, entry) = rest.split_once('#').map_or((rest, ""), |(id, e)| (id, e));
    let path = index.get(id)?;
    Some(if entry.is_empty() {
        path.clone()
    } else {
        format!("{path}#{entry}")
    })
}

/// Whether a value reads as a file name: `..` and `../Rig` are node paths, and
/// an absolute one is already resolved. The rule `editor/scripts/model.rn`
/// uses, so the editor's rewrite and this one cannot disagree.
fn names_a_file(value: &str) -> bool {
    if value.is_empty() || crate::files::rooted(Path::new(value)) {
        return false;
    }
    let mut last = "";
    for segment in value.split('/') {
        if segment == "." || segment == ".." {
            return false;
        }
        last = segment;
    }
    match last.split_once('.') {
        Some((_, extension)) => {
            !extension.is_empty() && extension.chars().all(char::is_alphanumeric)
        }
        None => false,
    }
}
