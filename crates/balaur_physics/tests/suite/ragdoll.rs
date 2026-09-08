//! Physical bones: a rig walked into bodies and joints, and blended back.
//!
//! Everything here goes through the scene and the script seam, because that
//! is what an editor button and a game's own `fall over` both reach for.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, GlobalTransform};

use crate::LOG;

/// A 2D rig of three bones lying along +x, with the given script on it.
const RIG: &str = r#"[[nodes]]
id = "n_rig"
name = "Rig"
script = "scripts/s.rn"

[[nodes]]
id = "n_hip"
name = "Hip"
parent = "n_rig"

[nodes.bone2d]
rest_position = [0.0, 0.0]

[[nodes]]
id = "n_knee"
name = "Knee"
parent = "n_hip"

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.bone2d]
rest_position = [1.0, 0.0]

[[nodes]]
id = "n_foot"
name = "Foot"
parent = "n_knee"

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.bone2d]
rest_position = [1.0, 0.0]
length = 1.0
"#;

/// Run a project of one scene and one script for `frames`, and hand back the
/// app so a test can look at the tree, plus anything logged as an error.
fn run(scene: &str, script: &str, frames: usize) -> (App, Vec<String>) {
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
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    let errors = balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    // The temporary directory has to outlive the app that is reading it.
    std::mem::forget(dir);
    (app, errors)
}

fn find(app: &App, path: &str) -> Option<Entity> {
    scene::find_node(&app.engine.world(), app.engine.root(), path)
}

fn global_y(app: &App, entity: Entity) -> f32 {
    app.engine
        .world()
        .get::<&GlobalTransform>(entity)
        .unwrap()
        .position
        .y
}

#[test]
fn a_ragdoll_gives_every_bone_a_body_and_a_joint() {
    let (app, errors) = run(
        RIG,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{ blend: 0.0 }); }",
        2,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    // A container beside the rig, named after it, with one body per bone.
    for bone in ["Hip", "Knee", "Foot"] {
        let body = find(&app, &format!("Rig_ragdoll/{bone}"))
            .unwrap_or_else(|| panic!("no body for {bone}"));
        let world = app.engine.world();
        assert!(
            balaur_core::components::get(&app.engine, body, "body2d").is_some(),
            "{bone} has no body"
        );
        assert!(
            balaur_core::components::get(&app.engine, body, "collider2d").is_some(),
            "{bone} has no collider"
        );
        drop(world);
    }
    // The root of the chain hangs from nothing; the two below it hinge.
    let hip = find(&app, "Rig_ragdoll/Hip").unwrap();
    let knee = find(&app, "Rig_ragdoll/Knee").unwrap();
    assert!(balaur_core::components::get(&app.engine, hip, "joint2d").is_none());
    let joint = balaur_core::components::get(&app.engine, knee, "joint2d").unwrap();
    assert_eq!(joint.get("kind").unwrap().as_str(), Some("revolute"));
    // And the rig itself carries the component that drives the bones back.
    let rig = find(&app, "Rig").unwrap();
    let ragdoll = balaur_core::components::get(&app.engine, rig, "ragdoll").unwrap();
    assert_eq!(ragdoll.get("blend").unwrap().as_float(), Some(0.0));
}

#[test]
fn a_ragdoll_at_full_blend_makes_the_rig_fall() {
    let (app, errors) = run(
        RIG,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{}); }",
        90,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let hip = find(&app, "Rig/Hip").unwrap();
    let y = global_y(&app, hip);
    assert!(y < -0.5, "a limp rig should have fallen, the hip is at {y}");
}

#[test]
fn a_blend_of_zero_leaves_the_rig_where_the_clip_put_it() {
    let (app, errors) = run(
        RIG,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{ blend: 0.0 }); }",
        90,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let hip = find(&app, "Rig/Hip").unwrap();
    assert!(
        global_y(&app, hip).abs() < 1e-4,
        "the bones should not have moved, the hip is at {}",
        global_y(&app, hip)
    );
    // The bodies fell all the same: a ragdoll at zero simulates unseen, so
    // turning the blend up later does not snap the rig to a fresh pose.
    let body = find(&app, "Rig_ragdoll/Hip").unwrap();
    assert!(global_y(&app, body) < -0.5, "the body should still fall");
}

#[test]
fn ragdoll_blend_turns_it_on_partway_through() {
    let (app, errors) = run(
        RIG,
        r"pub fn init(this) {
    physics2d::ragdoll(this.node, #{ blend: 0.0 });
    this.ticks = 0;
}

pub fn update(this, dt) {
    this.ticks += 1;
    if this.ticks == 30 {
        physics::ragdoll_blend(this.node, 1.0);
    }
}",
        90,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let hip = find(&app, "Rig/Hip").unwrap();
    assert!(
        global_y(&app, hip) < -0.5,
        "the rig should have gone limp once the blend came up, the hip is at {}",
        global_y(&app, hip)
    );
}

#[test]
fn a_node_with_no_bones_says_so_rather_than_building_nothing() {
    let (_, errors) = run(
        r#"[[nodes]]
id = "n"
name = "Plain"
script = "scripts/s.rn"
"#,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{}); }",
        2,
    );
    assert!(
        errors.iter().any(|e| e.contains("no bones")),
        "expected a complaint about the missing bones, got {errors:#?}"
    );
}
