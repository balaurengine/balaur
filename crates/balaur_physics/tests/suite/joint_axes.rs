//! A joint's `axes` records, its break thresholds and inverse kinematics:
//! what each record reaches in rapier, and what a joint refuses to be told
//! twice.

use balaur_core::components::StableId;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, components};
use balaur_physics::rapier3d::prelude::{JointAxis, MotorModel};
use balaur_physics::{PhysicsPlugin, PhysicsState3d};

fn app() -> App {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

/// A named node under the root at `at`, with an id so a break can be ordered.
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
/// `joint3d` whose table is `joint`; the ball's node is returned.
fn hanging(app: &App, at: [f32; 3], joint: &str) -> anyhow::Result<Entity> {
    let anchor = node(app, "Anchor", [0.0; 3]);
    add(app, anchor, "body3d", "kind = \"static\"")?;
    add(app, anchor, "collider3d", "kind = \"sphere\"\nradius = 0.2")?;
    let ball = node(app, "Ball", at);
    add(app, ball, "body3d", "kind = \"dynamic\"\nmass = 2.0")?;
    add(app, ball, "collider3d", "kind = \"sphere\"\nradius = 0.2")?;
    // Anchored at the anchor body's middle unless the table says otherwise.
    let anchor_at = if joint.contains("anchor =") {
        String::new()
    } else {
        format!("anchor = [{}, {}, {}]\n", -at[0], -at[1], -at[2])
    };
    let table = format!("connected_body = \"/Anchor\"\n{anchor_at}{joint}");
    add(app, ball, "joint3d", &table)?;
    Ok(ball)
}

fn height(app: &App, e: Entity) -> f32 {
    app.engine.world().get::<&Transform>(e).unwrap().position.y
}

fn joined(app: &App, e: Entity) -> bool {
    let state = app.engine.resource::<PhysicsState3d>();
    state.borrow().joints.contains_key(&e)
}

/// Where a ball held out level on `joint` hangs after two seconds.
fn swing(joint: &str) -> f32 {
    let mut app = app();
    let ball = hanging(&app, [1.0, 0.0, 0.0], joint).unwrap();
    tick(&mut app, 120);
    height(&app, ball)
}

#[test]
fn a_generic_joint_limits_the_turn_its_record_names() {
    let generic = "kind = \"generic\"\naxis = [0.0, 0.0, 1.0]\nlock_translation = [\"x\", \"y\", \"z\"]\nlock_rotation = [\"y\", \"z\"]";
    let free = swing(generic);
    assert!(
        free < -0.6,
        "the generic joint never swung: the ball is at {free}"
    );
    let limited = swing(&format!(
        "{generic}\naxes = [{{ axis = \"rotation_x\", limits = [-0.2, 0.2] }}]"
    ));
    assert!(
        limited > -0.3,
        "the record's limit did not hold the turn: the ball fell to {limited}"
    );
}

#[test]
fn a_ball_socket_limits_only_the_turn_its_record_names() {
    let socket = "kind = \"ball_socket\"";
    let about_z = swing(&format!(
        "{socket}\naxes = [{{ axis = \"rotation_z\", limits = [-0.2, 0.2] }}]"
    ));
    assert!(about_z > -0.3, "rotation_z's limit did not hold: {about_z}");
    let about_x = swing(&format!(
        "{socket}\naxes = [{{ axis = \"rotation_x\", limits = [-0.2, 0.2] }}]"
    ));
    assert!(
        about_x < -0.6,
        "a limit on rotation_x held the swing about z as well: {about_x}"
    );
}

#[test]
fn a_record_on_a_locked_axis_is_refused_with_the_free_axes_named() {
    let app = app();
    let why = hanging(
        &app,
        [1.0, 0.0, 0.0],
        "kind = \"hinge\"\naxes = [{ axis = \"x\", limits = [-1.0, 1.0] }]",
    )
    .unwrap_err();
    let message = format!("{why:#}");
    assert!(
        message.contains("rotation_x") && message.contains("`x`"),
        "the refusal does not name the free axis and the locked one: {message}"
    );
}

#[test]
fn two_records_on_one_axis_are_refused() {
    let app = app();
    let why = hanging(
        &app,
        [1.0, 0.0, 0.0],
        "kind = \"hinge\"\naxes = [{ axis = \"rotation_x\" }, { axis = \"rotation_x\" }]",
    )
    .unwrap_err();
    assert!(format!("{why:#}").contains("two `axes` records"), "{why:#}");
}

#[test]
fn a_rope_takes_its_length_limit_from_max_length_alone() {
    let app = app();
    let why = hanging(
        &app,
        [1.0, 0.0, 0.0],
        "kind = \"rope\"\nmax_length = 1.0\naxes = [{ axis = \"x\", limits = [0.0, 2.0] }]",
    )
    .unwrap_err();
    assert!(format!("{why:#}").contains("max_length"), "{why:#}");
    let mut app = self::app();
    let ball = hanging(
        &app,
        [0.0, -1.0, 0.0],
        "kind = \"rope\"\nmax_length = 1.5\nanchor = [0.0, 0.0, 0.0]\naxes = [{ axis = \"x\", motor = \"velocity\", motor_target = 0.0 }]",
    )
    .unwrap();
    tick(&mut app, 120);
    let y = height(&app, ball);
    assert!(
        (-1.6..-1.4).contains(&y),
        "a motor on the rope's x lifted its max_length: the ball hangs at {y}"
    );
}

/// The spring's own model unless its record names one, read back as the
/// model rapier runs.
fn spring_model(record: &str) -> (MotorModel, String) {
    let app = app();
    let ball = hanging(
        &app,
        [0.0, -1.0, 0.0],
        &format!("kind = \"spring\"\nrest_length = 1.0\naxes = [{record}]"),
    )
    .unwrap();
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let balaur_physics::joint::JointHandle3d::Impulse(handle) = state.joints[&ball].handle else {
        panic!("the spring is not an impulse joint");
    };
    let data = state.world.impulse_joints.get(handle).unwrap().data;
    let ran = data.motor_model(JointAxis::LinX).unwrap();
    drop(state);
    let got = components::get(&app.engine, ball, "joint3d").unwrap();
    let read = got["axes"][0]["motor_model"].as_str().unwrap().to_string();
    (ran, read)
}

#[test]
fn a_spring_runs_rapiers_force_model_unless_its_record_names_another() {
    let (ran, read) = spring_model("{ axis = \"x\", stiffness = 50.0 }");
    assert_eq!(ran, MotorModel::ForceBased);
    assert_eq!(
        read, "force",
        "the spring reads back a model it does not run"
    );
    let (ran, read) =
        spring_model("{ axis = \"x\", stiffness = 50.0, motor_model = \"acceleration\" }");
    assert_eq!(
        ran,
        MotorModel::AccelerationBased,
        "motor_model never reached the spring"
    );
    assert_eq!(read, "acceleration");
}

#[test]
fn a_motor_on_a_springs_own_axis_is_refused() {
    let app = app();
    let why = hanging(
        &app,
        [0.0, -1.0, 0.0],
        "kind = \"spring\"\nrest_length = 1.0\naxes = [{ axis = \"x\", motor = \"velocity\" }]",
    )
    .unwrap_err();
    assert!(format!("{why:#}").contains("rest_length"), "{why:#}");
}

/// A ball held at `at` by a fixed joint with `threshold`: whether it is still
/// held after a second, at `substeps` solver iterations.
fn holds(at: [f32; 3], threshold: &str, substeps: usize) -> bool {
    let mut app = app();
    let ball = hanging(&app, at, &format!("kind = \"fixed\"\n{threshold}")).unwrap();
    tick(&mut app, 1);
    {
        let state = app.engine.resource::<PhysicsState3d>();
        state
            .borrow_mut()
            .world
            .integration_parameters
            .num_solver_iterations = substeps;
    }
    tick(&mut app, 60);
    joined(&app, ball)
}

/// The 2 kg ball weighs 19.6 N: a joint rated below that snaps, one rated
/// above holds, whatever the solver's substeps.
#[test]
fn break_force_is_a_force_in_newtons_whatever_the_substeps() {
    let below = [0.0, -1.0, 0.0];
    for substeps in [4, 8] {
        assert!(
            !holds(below, "break_force = 15.0", substeps),
            "a 15 N joint held 19.6 N at {substeps} substeps"
        );
        assert!(
            holds(below, "break_force = 25.0", substeps),
            "a 25 N joint snapped under 19.6 N at {substeps} substeps"
        );
    }
}

/// Held out level a metre away, the same ball twists its joint with 19.6 N m.
#[test]
fn break_torque_is_measured_apart_from_the_pull() {
    let level = [1.0, 0.0, 0.0];
    assert!(
        !holds(level, "break_torque = 15.0", 4),
        "a 15 N m joint held"
    );
    assert!(
        holds(level, "break_torque = 25.0", 4),
        "a 25 N m joint snapped"
    );
    assert!(
        holds(level, "break_force = 25.0", 4),
        "the torque was counted against break_force"
    );
}

#[test]
fn an_articulation_refuses_a_break_threshold() {
    let app = app();
    let why = hanging(
        &app,
        [0.0, -1.0, 0.0],
        "kind = \"fixed\"\narticulation = true\nbreak_force = 10.0",
    )
    .unwrap_err();
    assert!(format!("{why:#}").contains("articulation"), "{why:#}");
}

/// A script's limit and motor are what `get` reports and what a patch of
/// another key rebuilds the joint with: the ball stays held up after it.
#[test]
fn a_script_limit_and_motor_survive_a_patch_of_another_key() {
    crate::joints_and_characters::run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[[nodes]]
id = "n_hanging"
name = "Hanging"
parent = "n_world"
body3d = { kind = "dynamic" }
script = { source = "scripts/s.rn" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
axis = [0.0, 0.0, 1.0]
anchor = [-1.0, 0.0, 0.0]
"#,
        r#"pub fn init(this) {
    this.ticks = 0;
    let joint = this.node.joint3d;
    joint.set_joint_limits(physics3d::AXIS_ROTATION_X, -0.2, 0.2);
    joint.set_motor_velocity(physics3d::AXIS_ROTATION_X, 0.0, 0.5);
    joint.collide_connected = true;
    let axes = joint.axes;
    assert_eq!(axes.len(), 1, "the two calls wrote other than one record");
    assert_eq!(axes[0].axis, physics3d::AXIS_ROTATION_X, "the record names another axis");
    assert_eq!(axes[0].motor, physics3d::MOTOR_VELOCITY, "the motor call wrote no motor");
    assert!((axes[0].limits[1] - 0.2).abs() < 0.0001, "the motor call dropped the limit");
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 110 {
        let y = this.node.transform.position.y;
        assert!(y > -0.3, "the patch rebuilt the joint without the script's limit: y is {}", y);
    }
}
"#,
    );
}

/// How far a two-link arm, articulated, stops from a point it can reach, in
/// the dimension `scene` builds.
fn arm_reach(scene: &str, call: &str) -> f32 {
    let errors = crate::joints_and_characters::run(scene, &ARM_SCRIPT.replace("CALL", call));
    errors
        .iter()
        .find_map(|e| e.split("reach ").nth(1).and_then(|n| n.trim().parse().ok()))
        .unwrap_or_else(|| panic!("the arm never reported: {errors:#?}"))
}

/// The arm in either dimension: `DIM` is the components' suffix, `V` their
/// vector tail after x and y.
const ARM: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_base"
name = "Base"
parent = "n_world"
bodyDIM = { kind = "static" }

[[nodes]]
id = "n_upper"
name = "Upper"
parent = "n_world"
bodyDIM = { kind = "dynamic", gravity_scale = 0.0 }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.colliderDIM]
SHAPE

[nodes.jointDIM]
kind = "hinge"
connected_body = "/World/Base"
anchor = [-1.0, 0.0V]
articulation = true

[[nodes]]
id = "n_lower"
name = "Lower"
parent = "n_world"
bodyDIM = { kind = "dynamic", gravity_scale = 0.0 }
script = { source = "scripts/s.rn" }

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.colliderDIM]
SHAPE

[nodes.jointDIM]
kind = "hinge"
connected_body = "/World/Upper"
anchor = [-1.0, 0.0V]
articulation = true
"#;

fn arm_3d() -> String {
    ARM.replace("DIM", "3d")
        .replace("SHAPE", "kind = \"sphere\"\nradius = 0.1")
        .replace("V]", ", 0.0]\naxis = [0.0, 0.0, 1.0]")
}

fn arm_2d() -> String {
    ARM.replace("DIM", "2d")
        .replace("SHAPE", "kind = \"circle\"\nradius = 0.1")
        .replace("V]", "]")
}

/// Logs the distance left to the target as an error the harness collects.
const ARM_SCRIPT: &str = r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 2 {
        CALL;
    }
    if this.ticks == 3 {
        let p = this.node.transform.position;
        let dx = p.x - 1.2;
        let dy = p.y - 1.2;
        log::error(format!("reach {}", math::sqrt(dx * dx + dy * dy)));
    }
}
"#;

/// A position target leaves the end link's turn free; asking the solver to
/// hold every axis, as it did by default, trades the position for the turn.
#[test]
fn solving_for_a_position_leaves_the_end_links_turn_free() {
    let arm = arm_3d();
    let reach = arm_reach(
        &arm,
        "this.node.joint3d.solve_ik(1.2, 1.2, 0.0, #{ damping: 0.1, iterations: 100 })",
    );
    assert!(
        reach < 0.05,
        "the arm stopped {reach} short of a point it reaches"
    );
    let held = arm_reach(
        &arm,
        "this.node.joint3d.solve_ik(1.2, 1.2, 0.0, #{ damping: 0.1, iterations: 100, constrain: [\"x\", \"y\", \"z\", \"rotation_x\", \"rotation_y\", \"rotation_z\"] })",
    );
    assert!(
        held > reach,
        "holding the turn too reached as close: {held} against {reach}"
    );
}

#[test]
fn a_2d_chain_is_solved_for_a_position() {
    let reach = arm_reach(
        &arm_2d(),
        "this.node.joint2d.solve_ik(1.2, 1.2, #{ damping: 0.1, iterations: 100 })",
    );
    assert!(
        reach < 0.05,
        "the 2D arm stopped {reach} short of a point it reaches"
    );
}
