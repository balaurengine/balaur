//! Scene node identity: `parent` refers to an id, so renaming is safe, or
//! to a path of names, which a hand-written scene would rather write.

use balaur_core::{App, AppConfig};

const WITH_IDS: &str = r#"
[[nodes]]
id = "n_root"
name = "World"

[[nodes]]
id = "n_child"
name = "Player"
parent = "n_root"
"#;

/// The same tree after someone renames the parent in the editor.
const RENAMED: &str = r#"
[[nodes]]
id = "n_root"
name = "Level"

[[nodes]]
id = "n_child"
name = "Player"
parent = "n_root"
"#;

/// Hand-edited scenes are missing ids; they are generated, not rejected.
const NO_ID: &str = r#"
[[nodes]]
name = "World"

[[nodes]]
name = "Player"
"#;

/// Two nodes may share a display name; the second id is regenerated.
const DUP_NAMES: &str = r#"
[[nodes]]
name = "Pupil"

[[nodes]]
name = "Pupil"
"#;

/// Parents must be declared before the children that name them.
const FORWARD_REF: &str = r#"
[[nodes]]
id = "n_child"
name = "Player"
parent = "n_root"

[[nodes]]
id = "n_root"
name = "World"
"#;

/// A hand-written scene names its parents by path instead of minting ids.
const BY_PATH: &str = r#"
[[nodes]]
name = "World"

[[nodes]]
name = "Ground"
parent = "World"

[[nodes]]
name = "Pebble"
parent = "World/Ground"
"#;

/// `parent` is an id first: this one names the id of the node *above* the
/// sibling whose name it matches.
const ID_BEATS_NAME: &str = r#"
[[nodes]]
id = "Ground"
name = "World"

[[nodes]]
name = "Ground"
parent = "World"

[[nodes]]
name = "Pebble"
parent = "Ground"
"#;

/// A prefab reaching out of its own scene through a parent path.
const ESCAPING_PATH: &str = r#"
[[nodes]]
name = "World"

[[nodes]]
name = "Stray"
parent = "../Elsewhere"
"#;

const DUPLICATE: &str = r#"
[[nodes]]
id = "same"
name = "A"

[[nodes]]
id = "same"
name = "B"
"#;

/// Every node's absolute path, sorted — the shape of the tree that loaded.
fn paths(source: &str) -> anyhow::Result<Vec<String>> {
    let dir = tempfile::tempdir()?;
    let app = App::new(AppConfig::bare(dir.path().to_path_buf()))?;
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, source, root, false)?;
    let world = app.engine.world();
    let mut out: Vec<String> = balaur_core::scene::collect_subtree(&world, root)
        .into_iter()
        .map(|entity| balaur_core::scene::node_path(&world, entity))
        .filter(|path| path != "Root")
        .collect();
    out.sort();
    Ok(out)
}

fn load(source: &str) -> anyhow::Result<usize> {
    let dir = tempfile::tempdir()?;
    let app = App::new(AppConfig::bare(dir.path().to_path_buf()))?;
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, source, root, false)?;
    let n = app.engine.world().len() as usize;
    Ok(n)
}

#[test]
fn renaming_a_parent_does_not_break_its_children() {
    let before = load(WITH_IDS).expect("scene with ids should load");
    let after = load(RENAMED).expect("renaming the parent must not break the link");
    assert_eq!(before, after, "the tree changed shape on a rename");
}

#[test]
fn missing_ids_are_generated_not_rejected() {
    let a = load(NO_ID).expect("a scene without ids should still load");
    let b = load(NO_ID).expect("and load again");
    assert_eq!(a, b, "generated ids must be deterministic across loads");
}

#[test]
fn nodes_sharing_a_name_get_distinct_ids() {
    load(DUP_NAMES).expect("two nodes may share a display name");
}

#[test]
fn a_forward_parent_reference_is_rejected() {
    let err = load(FORWARD_REF).expect_err("parents must come first");
    assert!(
        err.to_string().contains("no earlier node declares"),
        "unhelpful message: {err}"
    );
}

#[test]
fn a_duplicated_id_is_regenerated() {
    // The first node keeps the id it declared; the second is given a fresh one
    // rather than silently overwriting the first.
    load(DUPLICATE).expect("a duplicated id is repaired, not fatal");
}

#[test]
fn a_parent_path_of_names_builds_the_same_tree_as_ids() {
    assert_eq!(
        paths(BY_PATH).expect("a parent path should resolve"),
        ["Root/World", "Root/World/Ground", "Root/World/Ground/Pebble"]
    );
}

#[test]
fn a_parent_id_wins_over_a_sibling_name_that_matches_it() {
    // Pebble's `parent = "Ground"` is World's id, not the sibling of that
    // name, so Pebble lands beside Ground rather than under it.
    assert_eq!(
        paths(ID_BEATS_NAME).expect("an id parent should resolve"),
        ["Root/World", "Root/World/Ground", "Root/World/Pebble"]
    );
}

#[test]
fn a_parent_path_may_not_leave_its_scene() {
    let err = load(ESCAPING_PATH).expect_err("'..' must not reparent out of a scene");
    assert!(
        err.to_string().contains("may not leave its scene"),
        "unhelpful message: {err}"
    );
}
