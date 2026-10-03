//! The joint settings past limits and motors: the frames, the softness, the
//! coupled axes, the switch, an articulation's own numbers and gears, and what
//! `joint_state` reads back.

use balaur::{AppConfig, standard_app};
use balaur_core::components::StableId;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, components};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};

use crate::LOG;

fn app() -> App {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

fn node(app: &App, name: &str, at: [f32; 3]) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .world_mut()
        .insert_one(e, StableId(format!("n_{name}")))
        .unwrap();
    app.engine
        .world()
        .get::<&mut Transform>(e)
        .unwrap()
        .position = at.into();
    e
}

fn add(app: &App, e: Entity, component: &str, text: &str) -> anyhow::Result<()> {
    components::add(
        &app.engine,
        e,
        component,
        Some(&toml::from_str(text).unwrap()),
    )
}

fn tick(app: &mut App, steps: u32) {
    for _ in 0..steps {
        app.tick(1.0 / 60.0);
    }
}

/// A static anchor at the origin and a 2 kg ball at `at`, joined by a
/// `joint3d` anchored at the anchor's middle; the ball's node is returned.
fn hanging(app: &App, at: [f32; 3], joint: &str) -> Entity {
    let anchor = node(app, "Anchor", [0.0; 3]);
    add(app, anchor, "body3d", "kind = \"static\"").unwrap();
    add(app, anchor, "collider3d", "kind = \"sphere\"\nradius = 0.1").unwrap();
    let ball = node(app, "Ball", at);
    add(app, ball, "body3d", "kind = \"dynamic\"\nmass = 2.0").unwrap();
    add(app, ball, "collider3d", "kind = \"sphere\"\nradius = 0.1").unwrap();
    let table = format!(
        "connected_body = \"/Anchor\"\nanchor = [{}, {}, {}]\n{joint}",
        -at[0], -at[1], -at[2]
    );
    add(app, ball, "joint3d", &table).unwrap();
    ball
}

fn pose(app: &App, e: Entity) -> Transform {
    *app.engine.world().get::<&Transform>(e).unwrap()
}

#[test]
fn a_fixed_joint_holds_the_turn_its_anchor_rotation_names() {
    let mut app = app();
    let ball = hanging(
        &app,
        [1.0, 0.0, 0.0],
        "kind = \"fixed\"\nanchor_rotation = [0.0, 0.0, 0.5]",
    );
    tick(&mut app, 60);
    let (_, _, z) = pose(&app, ball).rotation.to_euler(glamx::EulerRot::XYZ);
    // This end's frame is turned by 0.5, so the body turns back by as much.
    assert!((z + 0.5).abs() < 0.05, "the fixed joint held a turn of {z}");
}

#[test]
fn a_hinge_turns_about_the_axis_each_end_names() {
    let mut app = app();
    let ball = hanging(
        &app,
        [0.0, 0.0, 0.0],
        "kind = \"hinge\"\naxis = [0.0, 0.0, 1.0]\nconnected_axis = [1.0, 0.0, 0.0]",
    );
    tick(&mut app, 60);
    let z = pose(&app, ball).rotation * glamx::Vec3::Z;
    assert!(
        z.dot(glamx::Vec3::X).abs() > 0.95,
        "the body's own axis was not brought onto the anchor's: it points along {z}"
    );
}

/// How far below the anchor's height a ball held level by `joint` sags.
fn sag(joint: &str) -> f32 {
    let mut app = app();
    let ball = hanging(&app, [1.0, 0.0, 0.0], joint);
    tick(&mut app, 60);
    -pose(&app, ball).position.y
}

#[test]
fn a_soft_joint_gives_where_a_stiff_one_holds() {
    let stiff = sag("kind = \"fixed\"");
    let soft = sag("kind = \"fixed\"\nsoftness_hz = 1.0");
    assert!(stiff < 0.05, "the default joint sagged {stiff}");
    assert!(
        soft > stiff + 0.1,
        "a 1 Hz joint sagged {soft}, the default {stiff}"
    );
}

#[test]
fn coupled_axes_limit_one_distance() {
    let free = "kind = \"generic\"\nlock_rotation = [\"x\", \"y\", \"z\"]";
    let rope = format!(
        "{free}\ncoupled_translation = [\"x\", \"y\", \"z\"]\naxes = [{{ axis = \"x\", limits = [0.0, 1.0] }}]"
    );
    let mut app = app();
    let ball = hanging(&app, [0.5, 0.0, 0.0], &rope);
    tick(&mut app, 120);
    // The ball's anchor is half a unit to its left, where the anchor body is.
    let anchor = pose(&app, ball).position - glamx::Vec3::new(0.5, 0.0, 0.0);
    assert!(
        (anchor.length() - 1.0).abs() < 0.05 && anchor.y < -0.9,
        "the coupled distance did not hold the anchor a length of 1 below: it is at {anchor}"
    );
    assert!(sag(free) > 2.0, "the uncoupled generic joint held the ball");
}

#[test]
fn switching_a_joint_off_keeps_it_and_switching_it_back_on_holds_again() {
    let mut app = app();
    let ball = hanging(&app, [1.0, 0.0, 0.0], "kind = \"fixed\"\nenabled = false");
    let handle = |app: &App| {
        let state = app.engine.resource::<PhysicsState3d>();
        state.borrow().joints.get(&ball).map(|j| match j.handle {
            balaur_physics::joint::JointHandle3d::Impulse(h) => h,
            balaur_physics::joint::JointHandle3d::Multibody(_) => panic!("an impulse joint"),
        })
    };
    let before = handle(&app).expect("a switched-off joint is still made");
    tick(&mut app, 30);
    let fallen = pose(&app, ball).position.y;
    assert!(fallen < -0.5, "the switched-off joint held: {fallen}");
    components::patch(
        &app.engine,
        ball,
        "joint3d",
        &toml::from_str("enabled = true").unwrap(),
    )
    .unwrap();
    assert!(
        handle(&app) == Some(before),
        "switching the joint on made it again"
    );
    tick(&mut app, 120);
    let held = pose(&app, ball).position;
    assert!(
        held.length() < 1.2,
        "the switched-on joint did not pull the ball back: {held}"
    );
}

/// A ball hung level on an articulated hinge about z, given `extra`, after
/// half a second: as long as a free swing takes to reach the bottom.
fn articulated(extra: &str) -> (App, Entity) {
    let mut app = app();
    let ball = hanging(
        &app,
        [1.0, 0.0, 0.0],
        &format!("kind = \"hinge\"\narticulation = true\n{extra}"),
    );
    tick(&mut app, 30);
    (app, ball)
}

#[test]
fn a_kinematic_link_holds_its_pose_against_gravity() {
    let (app, ball) = articulated("kinematic_link = true");
    let y = pose(&app, ball).position.y;
    assert!(y.abs() < 0.01, "the kinematic link fell to {y}");
    let (app, ball) = articulated("");
    assert!(
        pose(&app, ball).position.y < -0.5,
        "the ordinary link never swung"
    );
}

#[test]
fn joint_friction_and_a_passive_spring_hold_a_link_up() {
    let (app, ball) = articulated("axes = [{ axis = \"rotation_x\", friction = 1000.0 }]");
    let y = pose(&app, ball).position.y;
    assert!(y > -0.05, "dry friction let the link fall to {y}");
    let (app, ball) = articulated("passive_stiffness = 5000.0");
    let y = pose(&app, ball).position.y;
    assert!(y > -0.1, "the passive spring let the link fall to {y}");
}

#[test]
fn an_articulation_reads_back_what_its_chain_holds() {
    let (app, ball) = articulated(
        "axes = [{ axis = \"rotation_x\", link_damping = 0.5, armature = 0.25 }]\npassive_stiffness = 2.0\npassive_rest = 0.3",
    );
    let table = components::get(&app.engine, ball, "joint3d").unwrap();
    let number = |key: &str| table.get(key).and_then(toml::Value::as_float).unwrap();
    let record = &table["axes"][0];
    let field = |key: &str| record.get(key).and_then(toml::Value::as_float).unwrap();
    assert!((field("link_damping") - 0.5).abs() < 1e-6);
    assert!((field("armature") - 0.25).abs() < 1e-6);
    assert!((number("passive_stiffness") - 2.0).abs() < 1e-6);
    assert!((number("passive_rest") - 0.3).abs() < 1e-6);
}

/// The damping each axis of an articulated link takes when its record names
/// none: rapier's own, which damps a turn and leaves a slide free.
fn link_damping_left_to_rapier(joint: &str) -> f64 {
    let mut app = app();
    let ball = hanging(
        &app,
        [1.0, 0.0, 0.0],
        &format!("articulation = true\n{joint}"),
    );
    tick(&mut app, 1);
    let table = components::get(&app.engine, ball, "joint3d").unwrap();
    table["axes"][0]["link_damping"].as_float().unwrap()
}

#[test]
fn an_articulated_slide_is_undamped_and_a_turn_is_damped_unless_a_record_says() {
    let slide = link_damping_left_to_rapier("kind = \"slider\"\naxes = [{ axis = \"x\" }]");
    assert!(slide.abs() < 1e-6, "a linear axis took damping {slide}");
    let turn = link_damping_left_to_rapier("kind = \"hinge\"\naxes = [{ axis = \"rotation_x\" }]");
    assert!(
        (turn - 0.1).abs() < 1e-6,
        "a rotation axis took damping {turn}"
    );
    let named = link_damping_left_to_rapier(
        "kind = \"slider\"\naxes = [{ axis = \"x\", link_damping = 0.4 }]",
    );
    assert!(
        (named - 0.4).abs() < 1e-6,
        "the record's damping became {named}"
    );
}

/// Run a project of `scene` with one script, and pass when the only error it
/// logged is the script's `done` line.
fn run_checked(scene: &str, script: &str, ticks: u32, done: &str) {
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
    for _ in 0..ticks {
        app.tick(1.0 / 60.0);
    }
    let errors: Vec<String> = balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    assert!(
        errors.len() == 1 && errors[0].contains(done),
        "the check did not run clean: {errors:#?}"
    );
}

const PENDULUMS: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }

[[nodes]]
id = "n_swing"
name = "Swing"
parent = "n_world"
body3d = { kind = "dynamic", mass = 1.0 }
script = { source = "scripts/s.rn" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.1

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
anchor = [-1.0, 0.0, 0.0]
articulation = true

[[nodes]]
id = "n_impulse"
name = "Impulse"
parent = "n_world"
body3d = { kind = "dynamic", mass = 1.0 }

[nodes.transform]
position = [0.0, 0.0, 3.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.1

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
anchor = [-1.0, 0.0, 0.0]
connected_anchor = [-1.0, 0.0, 3.0]
axes = [{ axis = "rotation_x", limits = [-0.3, 0.3] }]
"#;

#[test]
fn joint_state_reads_a_hinges_angle_its_coordinates_and_its_limit() {
    run_checked(
        PENDULUMS,
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks += 1;
    if this.ticks == 20 {
        let swing = this.node.joint3d.joint_state();
        assert!(swing.status == physics3d::JOINT_ENABLED, "status {}", swing.status);
        let angle = swing.angle;
        let coordinate = swing.coordinates.rotation_x;
        assert!(angle.abs() > 0.1, "the pendulum never swung: {}", angle);
        assert!((angle - coordinate).abs() < 0.02, "angle {} against coordinate {}", angle, coordinate);
        assert!(swing.velocities.rotation_x.abs() > 0.1, "no joint velocity");
        let held = scene::get_node("World/Impulse").joint3d.joint_state();
        assert!(held.angle.abs() < 0.35, "the limit did not hold: {}", held.angle);
        assert!(held.limit_impulses.rotation_x.abs() > 0.0, "the limit carried no impulse");
        log::error("checked: joint state");
    }
}
"#,
        24,
        "checked: joint state",
    );
}

const GEARED: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }

[[nodes]]
id = "n_upper"
name = "Upper"
parent = "n_world"
body3d = { kind = "dynamic", mass = 1.0 }
script = { source = "scripts/s.rn" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.1

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
anchor = [-1.0, 0.0, 0.0]
articulation = true

[[nodes]]
id = "n_lower"
name = "Lower"
parent = "n_world"
body3d = { kind = "dynamic", mass = 1.0 }

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.1

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Upper"
anchor = [-1.0, 0.0, 0.0]
articulation = true
gear_with = "/World/Upper"
gear_ratio = -1.0
gear_offset = 0.2
"#;

#[test]
fn a_geared_joint_follows_the_joint_it_names() {
    run_checked(
        GEARED,
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks += 1;
    if this.ticks == 30 {
        let upper = this.node.joint3d.joint_state().coordinates.rotation_x;
        let lower = scene::get_node("World/Lower").joint3d.joint_state().coordinates.rotation_x;
        assert!(upper.abs() > 0.1, "the chain never swung: {}", upper);
        assert!((lower - (0.2 - upper)).abs() < 0.05, "upper {} lower {}", upper, lower);
        log::error("checked: geared");
    }
}
"#,
        32,
        "checked: geared",
    );
}

#[test]
fn a_2d_joint_reads_its_softness_back_and_turns_its_frame() {
    let mut app = app();
    let anchor = node(&app, "Anchor", [0.0; 3]);
    add(&app, anchor, "body2d", "kind = \"static\"").unwrap();
    let ball = node(&app, "Ball", [1.0, 0.0, 0.0]);
    add(&app, ball, "body2d", "kind = \"dynamic\"").unwrap();
    add(&app, ball, "collider2d", "kind = \"circle\"\nradius = 0.1").unwrap();
    add(
        &app,
        ball,
        "joint2d",
        "kind = \"hinge\"\nconnected_body = \"/Anchor\"\nanchor = [-1.0, 0.0]\nsoftness_hz = 50.0\nanchor_rotation = 0.25",
    )
    .unwrap();
    tick(&mut app, 2);
    let table = components::get(&app.engine, ball, "joint2d").unwrap();
    let hz = table.get("softness_hz").and_then(toml::Value::as_float);
    assert!(hz.is_some_and(|hz| (hz - 50.0).abs() < 1e-3), "{table}");
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let joint = state.joints.get(&ball).expect("the joint was made");
    let balaur_physics::dim2::joint::JointHandle2d::Impulse(handle) = joint.handle else {
        panic!("an impulse joint");
    };
    let data = state.world.impulse_joints.get(handle).unwrap().data;
    let turn = data.local_frame1.rotation.angle();
    assert!(
        (turn - 0.25).abs() < 1e-4,
        "this end's frame turned by {turn}"
    );
}
