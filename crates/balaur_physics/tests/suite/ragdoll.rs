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
script = { source = "scripts/s.rn" }

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

fn global_at(app: &App, path: &str) -> balaur_core::glamx::Vec3 {
    let entity = find(app, path).unwrap();
    app.engine
        .world()
        .get::<&GlobalTransform>(entity)
        .unwrap()
        .position
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
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{ influence: 0.0 }); }",
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
    assert_eq!(joint.get("kind").unwrap().as_str(), Some("hinge"));
    // And the rig itself carries the component that drives the bones back.
    let rig = find(&app, "Rig").unwrap();
    let ragdoll = balaur_core::components::get(&app.engine, rig, "ragdoll").unwrap();
    assert_eq!(ragdoll.get("influence").unwrap().as_float(), Some(0.0));
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
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{ influence: 0.0 }); }",
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
    physics2d::ragdoll(this.node, #{ influence: 0.0 });
    this.ticks = 0;
}

pub fn update(this, dt) {
    this.ticks += 1;
    if this.ticks == 30 {
        this.node.ragdoll.set_ragdoll_influence(1.0);
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
fn a_limp_rig_starts_from_the_pose_it_was_built_in() {
    // These bones lie along +x while a capsule stands along y: the bodies
    // turn their shapes, never the bones they hand back.
    let (app, errors) = run(
        RIG,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{}); }",
        1,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    for (bone, x) in [("Rig/Hip/Knee", 1.0), ("Rig/Hip/Knee/Foot", 2.0)] {
        let at = global_at(&app, bone);
        assert!(
            (at.x - x).abs() < 0.05 && at.y.abs() < 0.05,
            "{bone} should not have moved in one tick, it is at {at}"
        );
    }
}

#[test]
fn a_blend_back_to_zero_hands_the_rig_back_its_own_pose() {
    // No clip keys these bones, so nothing but the ragdoll ever moves them:
    // limp for half a second, then the blend returns and they must too.
    let (app, errors) = run(
        RIG,
        r"pub fn init(this) {
    physics2d::ragdoll(this.node, #{ influence: 1.0 });
    this.ticks = 0;
}

pub fn update(this, dt) {
    this.ticks += 1;
    if this.ticks == 30 {
        this.node.ragdoll.set_ragdoll_influence(0.0);
    }
}",
        60,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    for (bone, x) in [
        ("Rig/Hip", 0.0),
        ("Rig/Hip/Knee", 1.0),
        ("Rig/Hip/Knee/Foot", 2.0),
    ] {
        let at = global_at(&app, bone);
        assert!(
            (at.x - x).abs() < 1e-4 && at.y.abs() < 1e-4,
            "{bone} should be back at its own pose, it is at {at}"
        );
    }
}

#[test]
fn a_rollback_to_a_limp_tick_gets_back_up_the_same_way() {
    let (mut app, errors) = run(
        RIG,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{ influence: 1.0 }); }",
        29,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let rig = find(&app, "Rig").unwrap();
    let stand = |app: &mut App| {
        let zero = toml::from_str::<toml::Value>("influence = 0.0").unwrap();
        balaur_core::components::patch(&app.engine, rig, "ragdoll", &zero).unwrap();
        app.tick(1.0 / 60.0);
        global_at(app, "Rig/Hip/Knee")
    };
    let taken = balaur_core::snapshot::capture(&app.engine);
    let first = stand(&mut app);
    balaur_core::snapshot::restore(&app.engine, &taken);
    let again = stand(&mut app);
    assert!(
        (first.x - 1.0).abs() < 1e-4 && first.y.abs() < 1e-4,
        "the knee should be back at its own pose, it is at {first}"
    );
    assert_eq!(again, first, "the rolled-back tick stood up somewhere else");
}

#[test]
fn a_node_with_no_bones_says_so_rather_than_building_nothing() {
    let (_, errors) = run(
        r#"[[nodes]]
id = "n"
name = "Plain"
script = { source = "scripts/s.rn" }
"#,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{}); }",
        2,
    );
    assert!(
        errors.iter().any(|e| e.contains("no bones")),
        "expected a complaint about the missing bones, got {errors:#?}"
    );
}

#[test]
fn a_ragdoll_carries_the_body_shape_and_joint_options_it_was_built_with() {
    let (app, errors) = run(
        RIG,
        r#"pub fn init(this) {
    physics2d::ragdoll(this.node, #{
        influence: 0.0, shape: "rectangle", restitution: 0.3, collision_layer: [2],
        linear_damping: 0.5, gravity_scale: 0.25, continuous_collision: true,
        articulation: true, collide_connected: true, drive: "motors", stiffness: 40.0,
        axes: [#{ axis: physics2d::AXIS_ROTATION, limits: [-0.5, 0.5] }],
    });
}"#,
        2,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let knee = find(&app, "Rig_ragdoll/Knee").unwrap();
    let get = |component: &str| balaur_core::components::get(&app.engine, knee, component).unwrap();
    let body = get("body2d");
    assert_eq!(
        body.get("gravity_scale").and_then(toml::Value::as_float),
        Some(0.25)
    );
    assert_eq!(
        body.get("linear_damping").and_then(toml::Value::as_float),
        Some(0.5)
    );
    assert_eq!(
        body.get("continuous_collision")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    let collider = get("collider2d");
    assert_eq!(
        collider.get("kind").and_then(toml::Value::as_str),
        Some("rectangle")
    );
    let bounce = collider
        .get("restitution")
        .and_then(toml::Value::as_float)
        .unwrap();
    assert!((bounce - 0.3).abs() < 1e-6, "restitution {bounce}");
    let layers = collider
        .get("collision_layer")
        .and_then(toml::Value::as_array)
        .unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].as_str(), Some("2"));
    let joint = get("joint2d");
    assert_eq!(
        joint.get("articulation").and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        joint
            .get("collide_connected")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    let axes = joint.get("axes").and_then(toml::Value::as_array).unwrap();
    assert_eq!(axes.len(), 1, "{axes:?}");
    assert_eq!(
        axes[0].get("motor").and_then(toml::Value::as_str),
        Some("position")
    );
    assert_eq!(
        axes[0].get("stiffness").and_then(toml::Value::as_float),
        Some(40.0)
    );
    let limits = axes[0]
        .get("limits")
        .and_then(toml::Value::as_array)
        .unwrap();
    assert_eq!(
        limits[1].as_float(),
        Some(0.5),
        "the drive dropped the call's limit"
    );
}

#[test]
fn a_follow_drive_holds_a_limp_rig_on_its_bones() {
    let (app, errors) = run(
        RIG,
        r#"pub fn init(this) {
    physics2d::ragdoll(this.node, #{ drive: "follow", follow: #{ kind: "pid" } });
}"#,
        60,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let foot = find(&app, "Rig_ragdoll/Foot").unwrap();
    let follow = balaur_core::components::get(&app.engine, foot, "follow2d").unwrap();
    assert_eq!(follow["kind"].as_str(), Some("pid"));
    let target = follow["target"].as_str().unwrap();
    assert_eq!(
        scene::find_node(&app.engine.world(), foot, target),
        find(&app, "Rig/Hip/Knee/Foot"),
        "the foot's body follows {target}"
    );
    let y = global_at(&app, "Rig/Hip/Knee/Foot").y;
    assert!(y.abs() < 0.05, "the followed rig fell to {y}");
}

/// The rig above with its knee bent by half a radian.
fn bent_rig() -> String {
    RIG.replace(
        "[nodes.transform]\nposition = [1.0, 0.0, 0.0]\n\n[nodes.bone2d]\nrest_position = [1.0, 0.0]\n\n[[nodes]]\nid = \"n_foot\"",
        "[nodes.transform]\nposition = [1.0, 0.0, 0.0]\nrotation_euler = [0.0, 0.0, 0.5]\n\n[nodes.bone2d]\nrest_position = [1.0, 0.0]\n\n[[nodes]]\nid = \"n_foot\"",
    )
}

#[test]
fn a_ragdoll_joint_measures_its_angle_from_the_built_pose() {
    let rig = bent_rig();
    assert!(
        rig.contains("rotation_euler = [0.0, 0.0, 0.5]"),
        "the knee was not bent"
    );
    let (app, errors) = run(
        &rig,
        r"pub fn init(this) { physics2d::ragdoll(this.node, #{ influence: 0.0, gravity_scale: 0.0 }); }",
        2,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let knee = find(&app, "Rig_ragdoll/Knee").unwrap();
    let joint = balaur_core::components::get(&app.engine, knee, "joint2d").unwrap();
    let rest = joint
        .get("connected_anchor_rotation")
        .and_then(toml::Value::as_float)
        .unwrap();
    assert!((rest - 0.5).abs() < 1e-4, "the knee's joint holds {rest}");
    let state = app.engine.resource::<balaur_physics::PhysicsState2d>();
    let state = state.borrow();
    let reference = state.joints.get(&knee).unwrap();
    let balaur_physics::dim2::joint::JointHandle2d::Impulse(handle) = reference.handle else {
        panic!("an impulse joint");
    };
    let joint = state.world.impulse_joints.get(handle).unwrap();
    let rot = |body: balaur_physics::rapier2d::prelude::RigidBodyHandle| {
        *state.world.bodies[body].rotation()
    };
    let angle = balaur_physics::rapier2d::dynamics::RevoluteJoint { data: joint.data }
        .angle(&rot(joint.body1()), &rot(joint.body2()));
    assert!(angle.abs() < 1e-3, "the built pose reads {angle}, not zero");
}
