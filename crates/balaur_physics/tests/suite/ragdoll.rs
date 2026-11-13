//! The ragdoll blend: a rig's bones moved toward bodies the scene carries
//! for them, one per bone and named after it.
//!
//! The bodies are written into each scene as the editor's Create Physical
//! Skeleton builds them, so these tests hold the blend and not the builder.

use balaur::standard_app;
use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, GlobalTransform};

use crate::LOG;

/// A 2D rig of three bones lying along +x, its knee turned by `knee`, with
/// one body per bone under `Rig_ragdoll` beside it and the `ragdoll` component at
/// `influence`. `follow` is a `follow2d` table every body takes, less its
/// `target`.
fn rig(influence: f32, knee: f32, follow: Option<&str>) -> String {
    let (cos, sin) = (libm::cosf(knee), libm::sinf(knee));
    let follows = |bone: &str| {
        follow.map_or_else(String::new, |table| {
            format!("follow2d = {{ {table}, target = \"../../Rig/{bone}\" }}\n")
        })
    };
    let capsule = "collider2d = { shape = { kind = \"capsule\", radius = 0.25, height = 1.0 }, offset = [0.5, 0.0], offset_rotation = -1.5707964 }";
    format!(
        r#"[[nodes]]
id = "n_main"
name = "Main"

[[nodes]]
id = "n_rig"
name = "Rig"
parent = "n_main"
script = {{ source = "scripts/scenario.bs" }}
ragdoll = {{ bodies = "../Rig_ragdoll", influence = {influence:?} }}

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
rotation_euler = [0.0, 0.0, {knee:?}]

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

[[nodes]]
id = "n_bodies"
name = "Rig_ragdoll"
parent = "n_main"

[[nodes]]
id = "b_hip"
name = "Hip"
parent = "n_bodies"
body2d = {{}}
{capsule}
{hip}
[[nodes]]
id = "b_knee"
name = "Knee"
parent = "n_bodies"
body2d = {{}}
{capsule}
joint2d = {{ kind = "hinge", connected_body = "../Hip", connected_anchor = [1.0, 0.0], connected_anchor_rotation = {knee:?} }}
{knee_follow}
[nodes.transform]
position = [1.0, 0.0, 0.0]
rotation_euler = [0.0, 0.0, {knee:?}]

[[nodes]]
id = "b_foot"
name = "Foot"
parent = "n_bodies"
body2d = {{}}
{capsule}
joint2d = {{ kind = "hinge", connected_body = "../Knee", connected_anchor = [1.0, 0.0] }}
{foot}
[nodes.transform]
position = [{foot_x:?}, {sin:?}, 0.0]
rotation_euler = [0.0, 0.0, {knee:?}]
"#,
        hip = follows("Hip"),
        knee_follow = follows("Hip/Knee"),
        foot = follows("Hip/Knee/Foot"),
        foot_x = 1.0 + cos,
    )
}

/// The script every scene above attaches, when a test needs nothing of it.
const IDLE: &str = "pub class Scenario { node: Node }\n";

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
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/scenario.bs"), script).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    let errors = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    // The temporary directory has to outlive the app that is reading it.
    std::mem::forget(dir);
    (app, errors)
}

/// A node by its path under the scene's `Main`.
fn find(app: &App, path: &str) -> Option<Entity> {
    scene::find_node(
        &app.engine.world(),
        app.engine.root(),
        &format!("Main/{path}"),
    )
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
fn a_ragdoll_at_full_blend_makes_the_rig_fall() {
    let (app, errors) = run(&rig(1.0, 0.0, None), IDLE, 90);
    assert!(errors.is_empty(), "{errors:#?}");
    let hip = find(&app, "Rig/Hip").unwrap();
    let y = global_y(&app, hip);
    assert!(y < -0.5, "a limp rig should have fallen, the hip is at {y}");
}

#[test]
fn a_blend_of_zero_leaves_the_rig_where_the_clip_put_it() {
    let (app, errors) = run(&rig(0.0, 0.0, None), IDLE, 90);
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
        &rig(0.0, 0.0, None),
        r"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    fn update(dt: float) {
        self.ticks += 1;
        if self.ticks == 30 {
            self.node.ragdoll.influence = 1.0;
        }
    }
}
",
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
    let (app, errors) = run(&rig(1.0, 0.0, None), IDLE, 1);
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
        &rig(1.0, 0.0, None),
        r"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    fn update(dt: float) {
        self.ticks += 1;
        if self.ticks == 30 {
            self.node.ragdoll.influence = 0.0;
        }
    }
}
",
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
    let (mut app, errors) = run(&rig(1.0, 0.0, None), IDLE, 29);
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
fn a_follow_drive_holds_a_limp_rig_on_its_bones() {
    let (app, errors) = run(&rig(1.0, 0.0, Some("kind = \"pid\"")), IDLE, 60);
    assert!(errors.is_empty(), "{errors:#?}");
    let y = global_at(&app, "Rig/Hip/Knee/Foot").y;
    assert!(y.abs() < 0.05, "the followed rig fell to {y}");
}

#[test]
fn a_ragdoll_joint_measures_its_angle_from_the_built_pose() {
    let (app, errors) = run(&rig(0.0, 0.5, None), IDLE, 1);
    assert!(errors.is_empty(), "{errors:#?}");
    let knee = find(&app, "Rig_ragdoll/Knee").unwrap();
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
