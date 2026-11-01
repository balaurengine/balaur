//! Whether a game runs on the runtime it is put on. A `2d` or `3d` runtime
//! registers one physics world, so a scene or script using the other would
//! load with that half missing; the export refuses it instead.

use std::path::Path;

use anyhow::Result;
use balaur::Pack;
use balaur::physics::worlds;

use crate::config::Runtime;

/// Where `pack`'s scenes and scripts name one of `components` or `modules`.
///
/// Scripts are read from `project`, since a pack keeps their bytecode.
fn named_in(project: &Path, pack: &Pack, components: &[&str], modules: &[&str]) -> Vec<String> {
    let mut found = in_scenes(pack, components);
    let fs = balaur::files::default_backend();
    for path in pack.scripts.keys() {
        let Ok(bytes) = fs.read(&project.join(path)) else {
            continue;
        };
        let source = String::from_utf8_lossy(&bytes);
        for (line, text) in source.lines().enumerate() {
            let code = text.split("//").next().unwrap_or_default();
            if let Some(word) = components
                .iter()
                .chain(modules)
                .find(|word| names(code, word))
            {
                found.push(format!("{path}:{} names {word}", line + 1));
            }
        }
    }
    found
}

/// Fail when `runtime`, a resolved runtime name such as `web-2d`, leaves out
/// a physics world a scene or script in `pack` uses.
///
/// # Errors
/// Naming every place the missing world is used, and the setting to change.
pub fn fits(project: &Path, pack: &Pack, runtime: &str) -> Result<()> {
    let (missing, components, modules) = match crate::split_variant(runtime).1 {
        Some("2d") => ("3D", worlds::COMPONENTS_3D, worlds::MODULES_3D),
        Some("3d") => ("2D", worlds::COMPONENTS_2D, worlds::MODULES_2D),
        _ => return Ok(()),
    };
    let found = named_in(project, pack, components, modules);
    if found.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "the {runtime} runtime has no {missing} physics, which this game uses:\n  {}\n\
         set `[export] runtime` to \"full\", or to the world the game uses",
        found.join("\n  ")
    )
}

/// The smallest runtime `pack` fits: one physics world where it names at most
/// one, the 2D world being the lighter where it names neither.
#[must_use]
pub fn smallest(project: &Path, pack: &Pack) -> Runtime {
    let names_3d = !named_in(project, pack, worlds::COMPONENTS_3D, worlds::MODULES_3D).is_empty();
    let names_2d = !named_in(project, pack, worlds::COMPONENTS_2D, worlds::MODULES_2D).is_empty();
    match (names_2d, names_3d) {
        (true, true) => Runtime::Full,
        (false, true) => Runtime::World3d,
        (_, false) => Runtime::World2d,
    }
}

/// What a pack of `project` would hold of what [`fits`] reads: its scenes, and
/// its scripts by path. For a caller with no pack in hand, such as a sheet
/// listing what each target builds on.
#[must_use]
pub(crate) fn sources(project: &Path) -> Pack {
    let fs = balaur::files::default_backend();
    let ignored = fs
        .read(&project.join("project.toml"))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map_or_else(Vec::new, |manifest| {
            balaur::ignore::from_manifest(&manifest)
        });
    let mut pack = Pack::default();
    let mut pending = vec![String::new()];
    while let Some(dir) = pending.pop() {
        for (name, is_dir) in fs.list(&project.join(&dir)) {
            let rel = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            if name.starts_with('.') || balaur::ignore::ignored(&ignored, &rel) {
                continue;
            }
            let extension = std::path::Path::new(&name)
                .extension()
                .and_then(|extension| extension.to_str());
            if is_dir {
                pending.push(rel);
            } else if extension == Some("bs") {
                pack.scripts.insert(rel, Vec::new());
            } else if extension == Some("toml")
                && let Some(text) = fs
                    .read(&project.join(&rel))
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
            {
                pack.scenes.insert(rel, text);
            }
        }
    }
    pack
}

/// Each node in a scene of `pack` carrying one of `components`.
fn in_scenes(pack: &Pack, components: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in &pack.scenes {
        let Ok(doc) = text.parse::<toml::Table>() else {
            continue;
        };
        let nodes = doc.get("nodes").and_then(toml::Value::as_array);
        for node in nodes
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_table)
        {
            for key in components.iter().filter(|key| node.contains_key(**key)) {
                let name = node
                    .get("name")
                    .or_else(|| node.get("id"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or("?");
                found.push(format!("{path}: node \"{name}\" has {key}"));
            }
        }
    }
    found
}

/// Whether `word` stands on its own in `code`, not inside a longer name.
fn names(code: &str, word: &str) -> bool {
    let part = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code.match_indices(word).any(|(at, _)| {
        let before = code[..at].chars().next_back();
        let after = code[at + word.len()..].chars().next();
        !before.is_some_and(part) && !after.is_some_and(part)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(scene: &str) -> Pack {
        let mut pack = Pack::default();
        pack.scenes.insert("scenes/main.toml".into(), scene.into());
        pack
    }

    const CRATE_3D: &str =
        "[[nodes]]\nid = \"n\"\nname = \"Crate\"\n\n[nodes.body3d]\nkind = \"dynamic\"\n";

    #[test]
    fn a_2d_runtime_refuses_a_scene_with_a_3d_body() {
        let dir = tempfile::tempdir().unwrap();
        let err = fits(dir.path(), &pack(CRATE_3D), "web-2d")
            .unwrap_err()
            .to_string();
        assert!(err.contains("node \"Crate\" has body3d"), "{err}");
        assert!(err.contains("[export] runtime"), "{err}");
    }

    #[test]
    fn the_runtime_of_that_world_and_the_full_one_take_it() {
        let dir = tempfile::tempdir().unwrap();
        for runtime in ["web-3d", "web", "linux-x64-server"] {
            fits(dir.path(), &pack(CRATE_3D), runtime).unwrap();
        }
    }

    #[test]
    fn a_script_calling_the_missing_world_is_named_by_its_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("scripts")).unwrap();
        std::fs::write(
            dir.path().join("scripts/game.rn"),
            "// physics2d::gravity in a comment is no use\npub fn init() {\n    physics2d::set_gravity(0.0, -9.8);\n}\n",
        )
        .unwrap();
        let mut pack = pack("");
        pack.scripts.insert("scripts/game.rn".into(), Vec::new());
        let err = fits(dir.path(), &pack, "ios-3d").unwrap_err().to_string();
        assert!(err.contains("scripts/game.rn:3 names physics2d"), "{err}");
        assert!(!err.contains("game.rn:1"), "{err}");
    }

    #[test]
    fn a_longer_name_holding_the_word_is_not_a_use() {
        assert!(names("node.body3d.wake()", "body3d"));
        assert!(!names("let body3d_count = 2;", "body3d"));
        assert!(!names("my_physics3d", "physics3d"));
    }
}
