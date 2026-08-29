//! Scene node identity: `parent` refers to an id, so renaming is safe.

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

/// A node without an id is a scene we cannot safely edit.
const NO_ID: &str = r#"
[[nodes]]
name = "World"
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

const DUPLICATE: &str = r#"
[[nodes]]
id = "same"
name = "A"

[[nodes]]
id = "same"
name = "B"
"#;

fn load(source: &str) -> anyhow::Result<usize> {
    let dir = tempfile::tempdir()?;
    let app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
    })?;
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
fn a_node_without_an_id_is_rejected() {
    let err = load(NO_ID).expect_err("id is required");
    let chain = format!("{err:#}");
    assert!(chain.contains("id"), "unhelpful message: {chain}");
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
fn duplicate_ids_are_rejected() {
    let err = load(DUPLICATE).expect_err("two nodes sharing an id is a broken scene");
    assert!(
        err.to_string().contains("share the id"),
        "unhelpful message: {err}"
    );
}
