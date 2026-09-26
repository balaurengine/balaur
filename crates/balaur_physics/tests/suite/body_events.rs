//! What a body hears beyond its own collider: a collision on any collider
//! under it, its own sleep, and a soft body's collisions and torn edges.

use balaur::{AppConfig, standard_app};
use balaur_core::variables::Variables;

use crate::LOG;

/// Counters a script bumps, read back after the run: a script's own assert
/// logs nothing a Rust test sees.
const COUNTERS: &str = r#"[variables]
ticks = { type = "int", value = 0 }
hit = { type = "int", value = 0 }
slept = { type = "int", value = 0 }
moved = { type = "int", value = 0 }
touching = { type = "int", value = 0 }
edges = { type = "int", value = 0 }
"#;

const BUMP: &str = "fn bump(name, by) {
    scene::set_variable(name, scene::variable(name) + by);
}
";

fn run(scene: &str, script: &str, ticks: usize) -> balaur::App {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), format!("{COUNTERS}\n{scene}")).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), format!("{BUMP}\n{script}")).unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..ticks {
        app.tick(1.0 / 60.0);
    }
    app
}

fn counter(app: &balaur::App, name: &str) -> i64 {
    let variables = app.engine.resource::<Variables>();
    let held = variables.borrow();
    held.get(name).map_or(-1.0, balaur_core::variables::as_num) as i64
}

#[test]
fn a_body_hears_its_child_collider_land_and_itself_fall_asleep() {
    let app = run(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
body3d = { kind = "static" }

[nodes.collider3d]
kind = "box"
size = [20.0, 1.0, 20.0]

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
script = { source = "scripts/s.rn" }
body3d = { kind = "dynamic", time_to_sleep = 0.1 }

[nodes.transform]
position = [0.0, 2.0, 0.0]

[[nodes]]
id = "n_shape"
name = "Shape"
parent = "n_crate"

[nodes.collider3d]
kind = "box"
size = [1.0, 1.0, 1.0]
events = ["collision"]
"#,
        r#"pub fn on_collision_enter(this, other) { bump("hit", 1); }
pub fn on_sleeping_changed(this, asleep) { if asleep { bump("slept", 1); } }
pub fn fixed_update(this, dt) { bump("ticks", 1); }
"#,
        240,
    );
    assert!(counter(&app, "ticks") > 0, "the crate's script never ran");
    assert!(
        counter(&app, "hit") >= 1,
        "the crate never heard its collider land"
    );
    assert!(
        counter(&app, "slept") >= 1,
        "the crate never said it fell asleep"
    );
}

#[test]
fn a_2d_body_hears_the_same_and_lists_its_contacts() {
    let app = run(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
body2d = { kind = "static" }

[nodes.collider2d]
kind = "rectangle"
size = [20.0, 1.0]

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
script = { source = "scripts/s.rn" }
body2d = { kind = "dynamic", time_to_sleep = 0.1 }

[nodes.transform]
position = [0.0, 2.0, 0.0]

[[nodes]]
id = "n_shape"
name = "Shape"
parent = "n_crate"

[nodes.collider2d]
kind = "rectangle"
size = [1.0, 1.0]
events = ["collision"]
"#,
        r#"pub fn on_collision_enter(this, other) { bump("hit", 1); }
pub fn on_sleeping_changed(this, asleep) { if asleep { bump("slept", 1); } }
pub fn fixed_update(this, dt) {
    bump("ticks", 1);
    if this.node.body2d.is_moving() {
        scene::set_variable("moved", 1);
    }
    let touching = this.node.get_node("Shape").collider2d.contacts().len();
    if touching > scene::variable("touching") {
        scene::set_variable("touching", touching);
    }
}
"#,
        240,
    );
    assert!(counter(&app, "ticks") > 0, "the crate's script never ran");
    assert!(
        counter(&app, "hit") >= 1,
        "the crate never heard its collider land"
    );
    assert!(
        counter(&app, "slept") >= 1,
        "the crate never said it fell asleep"
    );
    assert_eq!(counter(&app, "moved"), 1, "is_moving never saw it fall");
    assert!(
        counter(&app, "touching") >= 1,
        "contacts never listed the ground"
    );
}

#[test]
fn a_soft_body_reports_its_own_collisions_and_the_edges_it_tore() {
    let app = run(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
body3d = { kind = "static" }

[nodes.transform]
position = [0.0, -4.0, 0.0]

[nodes.collider3d]
kind = "box"
size = [20.0, 1.0, 20.0]

[[nodes]]
id = "n_rope"
name = "Rope"
parent = "n_world"
script = { source = "scripts/s.rn" }
"#,
        r#"pub fn init(this) {
    // One end pinned and edges that break at a tenth of their rest length,
    // so the heavy free end tears it and falls onto the ground.
    this.node.softbody3d.set_softbody(#{
        kind: physics3d::SOFT_ROPE, a: [0.0, 0.0, 0.0], b: [0.0, -2.0, 0.0], particle_count: 12,
        pinned_particles: [0], tear_strain: 0.05, tear_force: 2.0,
        edge_frequency: 4.0, mass: 400.0, events: ["collision"],
    });
}
pub fn on_tear(this, tear) { bump("edges", tear["edges"].len()); }
pub fn on_collision_enter(this, other) { bump("hit", 1); }
pub fn fixed_update(this, dt) { bump("ticks", 1); }
"#,
        180,
    );
    assert!(counter(&app, "ticks") > 0, "the rope's script never ran");
    assert!(counter(&app, "edges") >= 1, "a tear named no torn edge");
    assert!(
        counter(&app, "hit") >= 1,
        "the rope never heard itself land"
    );
}
