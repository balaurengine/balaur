//! A parent hears a child arrive and leave, and a node hears itself renamed
//! and moved, through its own hook, a scene row and a subscription alike.

use balaur::{AppConfig, standard_app};
use balaur_core::variables::Variables;

/// Each event adds its own digit to `score`, so a missing or doubled one
/// shows in the total.
const NEST: &str = r#"
[variables]
score = { type = "int", value = 0 }
stage = { type = "int", value = 0 }

[[nodes]]
id = "n_scene"
name = "Scene"
script = { source = "scenes/nest.rn" }

[[nodes.bindings.rows]]
event = "emitted:child_added"
action = "add_variable"
target = "score"
value = 10

[[nodes]]
id = "n_other"
name = "Other"
parent = "n_scene"

[[nodes.bindings.rows]]
event = "emitted:child_added"
action = "add_variable"
target = "score"
value = 1000000
"#;

const NEST_SCRIPT: &str = r#"
fn add(n) {
    scene::set_variable("score", scene::variable("score") + n);
}

pub fn update(this, dt) {
    let stage = scene::variable("stage");
    if stage == 0 {
        let kid = this.node.add_child("Kid");
        events::listen(this.node, "renamed", kid);
        events::listen(this.node, "reparented", kid);
        kid.set_name("Renamed");
        kid.set_parent(this.node.get_node("Other"));
    }
    if stage == 1 {
        this.node.get_node("Other").queue_free();
    }
    scene::set_variable("stage", stage + 1);
}

pub fn on_child_added(this, child) {
    if child.name() == "Kid" {
        add(1);
    }
}

pub fn on_child_removed(this, child) {
    if child.name() == "Renamed" {
        add(1000);
    }
    if child.name() == "Other" {
        add(100000);
    }
}

pub fn on_renamed(this, was) {
    if was == "Kid" {
        add(100);
    }
}

pub fn on_reparented(this, left) {
    if left.name() == "Scene" {
        add(10000);
    }
}
"#;

fn score(app: &balaur::App) -> i64 {
    let variables = app.engine.resource::<Variables>();
    let held = variables.borrow();
    held.get("score")
        .map_or(-1.0, balaur_core::variables::as_num) as i64
}

#[test]
fn a_tree_edit_is_heard_by_the_parent_and_the_node_it_moved() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), NEST).unwrap();
    std::fs::write(dir.path().join("scenes/nest.rn"), NEST_SCRIPT).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    for _ in 0..5 {
        app.tick(1.0 / 60.0);
    }
    // 1 own hook and 10 row for Kid added, 100 renamed, 1000 Kid leaving,
    // 10000 reparented, 100000 Other freed, 1000000 Other's row.
    assert_eq!(score(&app), 1_111_111);
}
