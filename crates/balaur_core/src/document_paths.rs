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
///
/// A relative path under `deps/<alias>/` belongs to that dependency, which is
/// a project of its own inside the root that answered.
pub(crate) fn foreign_root(eng: &Engine, path: &str, read_from: &Path) -> Option<PathBuf> {
    let named = Path::new(path);
    let owner = if crate::files::rooted(named) {
        owner_of(eng, named)?
    } else {
        crate::dependencies::root_under(eng, read_from, path)
            .unwrap_or_else(|| read_from.to_path_buf())
    };
    let project = crate::file_api::project_roots(eng).into_iter().next()?;
    (owner != project).then_some(owner)
}

/// Rewrite every file path in a document against `root`. `ids` also resolves
/// `id://` through that root's own index, which only a document the engine
/// never writes back may ask for: the spelling is lost, not translated. A
/// dependency's documents are never written back, so theirs always resolve.
///
/// The paths come out as the game spells that root: absolute for a game a
/// host added, `deps/<alias>/…` for a dependency of the project itself, which
/// is also how its files are keyed in an export pack.
pub(crate) fn absolute_in(eng: &Engine, root: &Path, value: &mut toml::Value, ids: Ids) {
    let files = crate::files::backend(eng);
    let dependency = crate::dependencies::owning(eng, &root.to_string_lossy()).is_some();
    let index = match ids {
        Ids::Resolve => index_at(files.as_ref(), root),
        Ids::Keep if dependency => index_at(files.as_ref(), root),
        Ids::Keep => None,
    };
    let spelled = crate::dependencies::spelling(eng, root);
    let mut kept: Vec<(String, String)> = Vec::new();
    walk(
        &mut |text: &mut String| {
            if let Some(rest) = text.strip_prefix(crate::assets::ID_PREFIX) {
                if let Some(named) = index.as_ref().and_then(|index| id_path(index, rest)) {
                    let resolved = join(&spelled, &named);
                    kept.push((resolved.clone(), text.clone()));
                    *text = resolved;
                }
            } else if names_a_file(text) && files.exists(&root.join(&text)) {
                *text = join(&spelled, text);
            }
        },
        value,
    );
    // What each resolved path was read as, so [`relative_in`] writes the id.
    if !kept.is_empty() {
        if eng
            .try_resource::<crate::scene_toml::IdSpellings>()
            .is_none()
        {
            eng.insert_resource(crate::scene_toml::IdSpellings::default());
        }
        let spellings = eng.resource::<crate::scene_toml::IdSpellings>();
        spellings.borrow_mut().0.extend(kept);
    }
}

/// Put every path under `root` back to how that root's own project spells it,
/// for a document about to be written to a file inside it: an `id://` as it
/// was read, any other path relative to the root.
pub(crate) fn relative_in(eng: &Engine, root: &Path, value: &mut toml::Value) {
    let prefix = join(root, "");
    let spellings = eng.try_resource::<crate::scene_toml::IdSpellings>();
    let spellings = spellings.as_ref().map(|held| held.borrow());
    walk(
        &mut |text: &mut String| {
            if let Some(reference) = spellings
                .as_ref()
                .and_then(|held| held.0.get(text.as_str()))
            {
                *text = reference.clone();
            } else if let Some(rest) = text.strip_prefix(&prefix) {
                *text = rest.to_string();
            }
        },
        value,
    );
}

/// Rewrite a dependency's document for an export pack, whose one key space
/// is the game's: a path naming one of the dependency's files, or an `id://`
/// its own index knows, becomes `<prefix>/<path>`. `exists` answers for a
/// path under the dependency's root.
pub(crate) fn prefixed_in(
    prefix: &str,
    value: &mut toml::Value,
    exists: &dyn Fn(&str) -> bool,
    index: &std::collections::BTreeMap<String, String>,
) {
    walk(
        &mut |text: &mut String| {
            if let Some(rest) = text.strip_prefix(crate::assets::ID_PREFIX) {
                if let Some(named) = id_path(index, rest) {
                    *text = format!("{prefix}/{named}");
                }
            } else if names_a_file(text) && exists(text) {
                *text = format!("{prefix}/{text}");
            }
        },
        value,
    );
}

/// The root of the project whose content `entity` plays, when that is not
/// the engine's own: the game an editor edits, whose files a node there names
/// as that project spells them.
fn content_root_of(eng: &Engine, entity: hecs::Entity) -> Option<PathBuf> {
    let root = crate::project::content_root(eng)?;
    crate::project::in_content(eng, entity).then_some(root)
}

/// A value on its way onto a node playing another project's content, its
/// paths made absolute against that project; `None` for any other node, which
/// takes the value as it is.
pub(crate) fn written_on(
    eng: &Engine,
    entity: hecs::Entity,
    value: &toml::Value,
) -> Option<toml::Value> {
    let root = content_root_of(eng, entity)?;
    let mut value = value.clone();
    absolute_in(eng, &root, &mut value, Ids::Resolve);
    Some(value)
}

/// A value read off a node playing another project's content, its paths
/// spelled as that project spells them.
pub(crate) fn read_off(eng: &Engine, entity: hecs::Entity, value: &mut toml::Value) {
    if let Some(root) = content_root_of(eng, entity) {
        relative_in(eng, &root, value);
    }
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

/// `<root>/<path>`, the root spelled as the host declared it: the editor builds
/// the same string from its game root and strips that prefix to save a path back.
fn join(root: &Path, path: &str) -> String {
    let root = root.to_string_lossy();
    format!("{}/{path}", root.trim_end_matches(['/', '\\']))
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
/// an absolute one is already resolved. `model::names_a_file` in the editor is
/// the same rule and must stay so, down to a Windows drive counting as absolute:
/// the editor walks what this hands back.
pub(crate) fn names_a_file(value: &str) -> bool {
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
