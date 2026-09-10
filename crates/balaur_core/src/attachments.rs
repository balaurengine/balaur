//! Which scripts a project's scenes attach, and what the nodes attaching them
//! carry.
//!
//! The checker's view of a project: a script a scene names is a compile root,
//! and the components beside it are what a handle call on `this.node` can
//! resolve against. One walk answers both, so the two cannot disagree.

use std::collections::{BTreeMap, BTreeSet};

use crate::project::SceneNode;

/// Every script a project's scenes attach, with the components the nodes
/// attaching it carry.
///
/// One walk rather than two: the checker wants the components beside a
/// script and the tools that only want the paths read the keys. A scene that
/// will not parse is skipped — it is the scene loader's error to report, not
/// the checker's — and so is a directory carrying a `project.toml` of its
/// own, which is another project rather than a part of this one.
///
/// A node's components are the keys its table holds that no scene field
/// claims, which is exactly what the loader dispatches to component handlers;
/// whether a name is registered is the caller's question, not this walk's.
/// The set is the union over every node attaching that script, so a call is
/// answered by any node that could receive it. `None` is a node this walk
/// could not read: unknown rather than none, so nothing concludes the node
/// carries nothing.
#[must_use]
pub fn scene_attachments(
    project_root: &std::path::Path,
) -> BTreeMap<String, Option<BTreeSet<String>>> {
    let mut out: BTreeMap<String, Option<BTreeSet<String>>> = BTreeMap::new();
    let fs = crate::files::default_backend();
    let mut dirs = vec![project_root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for (name, is_dir) in fs.list(&dir) {
            let path = dir.join(&name);
            if is_dir {
                // A directory holding a manifest is another project: its
                // scenes name scripts from its own root, checked from there.
                if fs.read(&path.join("project.toml")).is_err() {
                    dirs.push(path);
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Ok(bytes) = fs.read(&path) else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let Ok(document) = text.parse::<toml::Table>() else {
                continue;
            };
            let Some(nodes) = document.get("nodes").and_then(toml::Value::as_array) else {
                continue;
            };
            for node in nodes {
                let Some((script, carried)) = attachment(node) else {
                    continue;
                };
                let entry = out.entry(script).or_insert_with(|| Some(BTreeSet::new()));
                match (entry.as_mut(), carried) {
                    (Some(known), Some(more)) => known.extend(more),
                    _ => *entry = None,
                }
            }
        }
    }
    out
}

/// The script one scene node attaches, and the components beside it.
///
/// The node is read as the loader reads it, so the components are whatever
/// `SceneNode` did not claim as a field of its own and nothing here holds a
/// second list of those names. A node the loader would reject still reports
/// its script — it is a root the checker should compile — with its components
/// unknown.
fn attachment(node: &toml::Value) -> Option<(String, Option<Vec<String>>)> {
    if let Ok(parsed) = node.clone().try_into::<SceneNode>() {
        let source = parsed.script.as_ref()?.source();
        if source.is_empty() {
            return None;
        }
        return Some((source.to_string(), Some(parsed.extra.into_keys().collect())));
    }
    // `script` is a path, or a table whose `source` is one.
    let script = match node.get("script") {
        Some(toml::Value::String(path)) => path.clone(),
        Some(toml::Value::Table(table)) => table.get("source")?.as_str()?.to_string(),
        _ => return None,
    };
    (!script.is_empty()).then_some((script, None))
}
