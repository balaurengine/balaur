//! The body keys and readers rapier has and `body3d`/`body2d` now reach:
//! island iterations, sleep thresholds, how a body starts, the inertia frame,
//! and the readers for the mass and the pose rapier works out.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, AppConfig, components};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};

use crate::boot::{self, entry, float, floats, number};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin).unwrap();
    app
}

/// A node with `body` (a component and its params) and a ball collider.
fn with_body(app: &App, name: &str, component: &str, params: &str) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, e, component, Some(&params)).unwrap();
    let (collider, shape) = if component == "body3d" {
        ("collider3d", "shape = { kind = \"sphere\", radius = 0.5 }")
    } else {
        ("collider2d", "shape = { kind = \"circle\", radius = 0.5 }")
    };
    let shape: toml::Value = toml::from_str(shape).unwrap();
    components::add(&app.engine, e, collider, Some(&shape)).unwrap();
    e
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn the_new_body_keys_reach_rapier_and_read_back() {
    let app = app();
    let e = with_body(
        &app,
        "Tuned",
        "body3d",
        r#"kind = "dynamic"
internal_iterations = 2
sleep_threshold = 0.2
sleep_angular_threshold = 0.3
dominance = -128
mass = 2.0
inertia = [1.0, 2.0, 3.0]
inertia_rotation = [0.0, 0.5, 0.0]"#,
    );
    {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        let body = &state.world.bodies[state.bodies[&e]];
        assert_eq!(body.additional_pgs_iterations(), 2);
        assert_eq!(body.dominance_group(), -128);
        assert!((body.activation().normalized_linear_threshold - 0.2).abs() < 1e-6);
        assert!((body.activation().angular_threshold - 0.3).abs() < 1e-6);
    }
    let back = components::get(&app.engine, e, "body3d").unwrap();
    assert_eq!(float(&back, "internal_iterations"), 2.0);
    assert_eq!(float(&back, "dominance"), -128.0);
    assert!((float(&back, "sleep_threshold") - 0.2).abs() < 1e-6);
    assert!((float(&back, "sleep_angular_threshold") - 0.3).abs() < 1e-6);
    let turn = floats(&back, "inertia_rotation");
    assert!(
        turn[0].abs() < 1e-5 && (turn[1] - 0.5).abs() < 1e-5 && turn[2].abs() < 1e-5,
        "the inertia frame read back as {turn:?}"
    );
    let inertia = floats(&back, "inertia");
    assert_eq!(inertia.len(), 3);
    assert!((inertia[1] - 2.0).abs() < 1e-5, "{inertia:?}");
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn a_2d_body_takes_the_same_keys() {
    let app = app();
    let e = with_body(
        &app,
        "Flat",
        "body2d",
        "kind = \"dynamic\"\ninternal_iterations = 3\nsleep_threshold = 0.4\ndominance = -128",
    );
    {
        let state = app.engine.resource::<PhysicsState2d>();
        let state = state.borrow();
        let body = &state.world.bodies[state.bodies[&e]];
        assert_eq!(body.additional_pgs_iterations(), 3);
        assert_eq!(body.dominance_group(), -128);
        assert!((body.activation().normalized_linear_threshold - 0.4).abs() < 1e-6);
    }
    let back = components::get(&app.engine, e, "body2d").unwrap();
    assert_eq!(float(&back, "internal_iterations"), 3.0);
    assert!((float(&back, "sleep_threshold") - 0.4).abs() < 1e-6);
}

// `initial_*` is a create-time key: a patch carries it, and must not throw a
// body that has since slowed back up to it.
#[test]
fn an_initial_velocity_is_given_once_and_never_by_a_patch() {
    let app = app();
    let e = with_body(
        &app,
        "Thrown",
        "body3d",
        "kind = \"dynamic\"\ngravity_scale = 0.0\ninitial_linear_velocity = [3.0, 0.0, 0.0]\ninitial_angular_velocity = [0.0, 1.0, 0.0]",
    );
    let velocity = |app: &App| {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        let body = &state.world.bodies[state.bodies[&e]];
        (body.linvel().x, body.angvel().y)
    };
    assert_eq!(velocity(&app), (3.0, 1.0), "the body was not made moving");
    {
        let state = app.engine.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let handle = state.bodies[&e];
        let still = balaur_physics::rapier3d::math::Vector::ZERO;
        state.world.bodies[handle].set_linvel(still, true);
        state.world.bodies[handle].set_angvel(still, true);
    }
    let patch: toml::Value = toml::from_str("linear_damping = 0.5").unwrap();
    components::patch(&app.engine, e, "body3d", &patch).unwrap();
    assert_eq!(velocity(&app), (0.0, 0.0), "a patch threw the body again");
    let back = components::get(&app.engine, e, "body3d").unwrap();
    assert_eq!(floats(&back, "initial_linear_velocity"), [3.0, 0.0, 0.0]);
}

#[test]
fn a_2d_initial_velocity_is_given_once_and_never_by_a_patch() {
    let app = app();
    let e = with_body(
        &app,
        "Thrown",
        "body2d",
        "kind = \"dynamic\"\ngravity_scale = 0.0\ninitial_linear_velocity = [0.0, 4.0]\ninitial_angular_velocity = 2.0",
    );
    let velocity = |app: &App| {
        let state = app.engine.resource::<PhysicsState2d>();
        let state = state.borrow();
        let body = &state.world.bodies[state.bodies[&e]];
        (body.linvel().y, body.angvel())
    };
    assert_eq!(velocity(&app), (4.0, 2.0));
    {
        let state = app.engine.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let handle = state.bodies[&e];
        let still = balaur_physics::rapier2d::math::Vector::ZERO;
        state.world.bodies[handle].set_linvel(still, true);
        state.world.bodies[handle].set_angvel(0.0, true);
    }
    let patch: toml::Value = toml::from_str("linear_damping = 0.5").unwrap();
    components::patch(&app.engine, e, "body2d", &patch).unwrap();
    assert_eq!(velocity(&app), (0.0, 0.0), "a patch threw the body again");
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn a_body_made_asleep_hangs_until_something_wakes_it() {
    let mut app = app();
    let e = with_body(
        &app,
        "Dozing",
        "body3d",
        "kind = \"dynamic\"\nstart_asleep = true",
    );
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    let y = app.engine.world().get::<&Transform>(e).unwrap().position.y;
    let asleep = {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]].is_sleeping()
    };
    assert!(asleep, "a body made asleep woke on its own");
    assert_eq!(y, 0.0, "a body made asleep fell");
    let back = components::get(&app.engine, e, "body3d").unwrap();
    assert_eq!(
        back.get("start_asleep").and_then(toml::Value::as_bool),
        Some(true)
    );
}

// A body held awake carries negative thresholds; what the author wrote has
// to survive that and come back when sleep is allowed again.
#[test]
fn a_body_held_awake_keeps_its_sleep_thresholds() {
    let app = app();
    let e = with_body(
        &app,
        "Awake",
        "body3d",
        "kind = \"dynamic\"\ncan_sleep = false\nsleep_threshold = 0.25",
    );
    let back = components::get(&app.engine, e, "body3d").unwrap();
    assert!((float(&back, "sleep_threshold") - 0.25).abs() < 1e-6);
    balaur_physics::set_sleeping_allowed(&app.engine, false);
    let patch: toml::Value = toml::from_str("can_sleep = true").unwrap();
    components::patch(&app.engine, e, "body3d", &patch).unwrap();
    balaur_physics::set_sleeping_allowed(&app.engine, true);
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let threshold = state.world.bodies[state.bodies[&e]]
        .activation()
        .normalized_linear_threshold;
    assert!(
        (threshold - 0.25).abs() < 1e-6,
        "allowing sleep again put back {threshold}, not the authored 0.25"
    );
}

const SPINNER_3D: &str = r#"[[nodes]]
id = "n_spinner"
name = "Spinner"
script = { source = "scripts/scenario.bs" }
body3d = { kind = "dynamic", gravity_scale = 0.0, mass = 2.0, center_of_mass = [0.0, 0.5, 0.0], inertia = [1.0, 2.0, 3.0], inertia_rotation = [0.0, 0.5, 0.0], lock_translation = ["x"], initial_angular_velocity = [0.0, 0.0, 1.0] }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }
"#;

#[test]
fn the_body_readers_answer_what_rapier_works_out() {
    let booted = boot::project(
        SPINNER_3D,
        r"pub class Scenario { node: Node }
impl Scenario {
    pub fn probe() -> [any] {
        let n = self.node;
        [n.body3d.is_ccd_active(), n.body3d.world_center_of_mass(), n.body3d.local_center_of_mass(), n.body3d.total_inertia(),
         n.body3d.total_inertia_rotation(), n.body3d.effective_mass(), n.body3d.effective_angular_inertia(),
         n.body3d.predict_position(0.5), true]
    }
}
",
    );
    let spinner = booted.node("Spinner");
    let balaur_script::Value::List(found) = booted.call(spinner, "probe") else {
        panic!("the probe answered no list");
    };
    let vec3 = |value: &balaur_script::Value| {
        let v = boot::vector(value);
        [v[0] as f32, v[1] as f32, v[2] as f32]
    };
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4);
    assert_eq!(found[8], balaur_script::Value::Bool(true), "the probe ran");
    assert_eq!(
        found[0],
        balaur_script::Value::Bool(false),
        "a resting body swept"
    );
    assert!(near(vec3(&found[1]), [0.0, 0.5, 0.0]), "{:?}", found[1]);
    assert!(near(vec3(&found[2]), [0.0, 0.5, 0.0]), "{:?}", found[2]);
    assert!(near(vec3(&found[3]), [1.0, 2.0, 3.0]), "{:?}", found[3]);
    assert!(near(vec3(&found[4]), [0.0, 0.5, 0.0]), "{:?}", found[4]);
    assert!(
        near(vec3(&found[5]), [0.0, 2.0, 2.0]),
        "a locked axis has mass: {:?}",
        found[5]
    );
    let balaur_script::Value::List(rows) = &found[6] else {
        panic!("the inertia is not three rows: {:?}", found[6]);
    };
    let trace: f32 = (0..3).map(|i| vec3(&rows[i])[i]).sum();
    assert!(
        (trace - 6.0).abs() < 1e-3,
        "the world inertia's trace is {trace}"
    );
    let turned = vec3(entry(&found[7], "rotation"));
    assert!(
        near(turned, [0.0, 0.0, 0.5]),
        "half a second at one radian a second turned it {turned:?}"
    );
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn teleport_can_turn_a_body_too() {
    let mut booted = boot::project(
        r#"[[nodes]]
id = "n_crate"
name = "Crate"
script = { source = "scripts/scenario.bs" }
body3d = { kind = "dynamic", gravity_scale = 0.0 }
collider3d = { shape = { kind = "box" } }
"#,
        r"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        self.node.body3d.teleport(vec3(1.0, 2.0, 3.0), physics3d::TeleportOptions { rotation: vec3(0.0, 0.5, 0.0) });
    }
}
",
    );
    booted.tick(1);
    let crate_node = booted.node("Crate");
    assert_eq!(booted.position(crate_node), [1.0, 2.0, 3.0]);
    let world = booted.app.engine.world();
    let rotation = world.get::<&Transform>(crate_node).unwrap().rotation;
    let (z, y, x) = rotation.to_euler(glamx::EulerRot::ZYX);
    assert!(
        x.abs() < 1e-5 && (y - 0.5).abs() < 1e-5 && z.abs() < 1e-5,
        "teleport left the crate turned ({x}, {y}, {z})"
    );
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn the_2d_body_readers_answer_what_rapier_works_out() {
    let mut booted = boot::project(
        r#"[[nodes]]
id = "n_puck"
name = "Puck"
script = { source = "scripts/scenario.bs" }
body2d = { kind = "dynamic", gravity_scale = 0.0, mass = 2.0, center_of_mass = [0.5, 0.0], inertia = 3.0, lock_translation = ["y"], initial_angular_velocity = 1.0 }
collider2d = { shape = { kind = "circle", radius = 0.5 } }
"#,
        r"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        self.node.body2d.add_constant_force(vec2(4.0, 0.0));
    }

    pub fn probe() -> [any] {
        let n = self.node;
        [n.body2d.is_ccd_active(), n.body2d.world_center_of_mass(), n.body2d.local_center_of_mass(), n.body2d.total_inertia(),
         n.body2d.effective_mass(), n.body2d.effective_angular_inertia(),
         n.body2d.predict_position(0.5), n.body2d.predict_position_with_forces(0.5), true]
    }
}
",
    );
    booted.tick(1);
    let puck = booted.node("Puck");
    let balaur_script::Value::List(found) = booted.call(puck, "probe") else {
        panic!("the probe answered no list");
    };
    let vec2 = |value: &balaur_script::Value| {
        let v = boot::vector(value);
        [v[0] as f32, v[1] as f32]
    };
    assert_eq!(found[8], balaur_script::Value::Bool(true), "the probe ran");
    assert_eq!(found[0], balaur_script::Value::Bool(false));
    let centre = vec2(&found[1]);
    assert!(
        (centre[0] - 0.5).abs() < 0.01 && centre[1].abs() < 0.01,
        "{centre:?}"
    );
    assert_eq!(vec2(&found[2]), [0.5, 0.0]);
    assert!((number(&found[3]) - 3.0).abs() < 1e-4);
    assert_eq!(vec2(&found[4]), [2.0, 0.0], "a locked axis has mass");
    assert!(number(&found[5]) > 0.0);
    let coasting = vec2(entry(&found[6], "position"));
    let pushed = vec2(entry(&found[7], "position"));
    assert!(
        pushed[0] > coasting[0] + 1e-3,
        "the force moved nothing: {pushed:?} against {coasting:?}"
    );
    // One tick at one radian a second, then half a second more.
    let turned = number(entry(&found[6], "rotation"));
    assert!((turned - (0.5 + 1.0 / 60.0)).abs() < 1e-3, "{turned}");
}

const STILL_AND_SPINNING: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_still3d"
name = "Still3d"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
body3d = { kind = "dynamic", gravity_scale = 0.0 }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }

[[nodes]]
id = "n_spinning3d"
name = "Spinning3d"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
transform = { position = [4.0, 0.0, 0.0] }
body3d = { kind = "dynamic", gravity_scale = 0.0, initial_angular_velocity = [0.0, 0.0, 1.0] }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }

[[nodes]]
id = "n_still2d"
name = "Still2d"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
body2d = { kind = "dynamic", gravity_scale = 0.0 }
collider2d = { shape = { kind = "circle", radius = 0.5 } }

[[nodes]]
id = "n_spinning2d"
name = "Spinning2d"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
transform = { position = [4.0, 0.0, 0.0] }
body2d = { kind = "dynamic", gravity_scale = 0.0, initial_angular_velocity = 1.0 }
collider2d = { shape = { kind = "circle", radius = 0.5 } }
"#;

/// A quarter of a second of steps, short of the half-second `time_to_sleep`
/// after which a still body sleeps and its clock stops.
const STEPS: usize = 15;

// rapier's clock runs while a body stays below its sleep threshold and goes
// back to zero while it moves; the next pose is where the last step left it.
#[test]
fn a_still_body_counts_the_time_it_could_sleep_and_a_spinning_one_turns() {
    let mut booted = boot::project(
        STILL_AND_SPINNING,
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn probe() -> [any] {
        let n = self.node;
        if n.has_component("body3d") { return [n.body3d.time_since_can_sleep(), n.body3d.next_position().rotation]; }
        return [n.body2d.time_since_can_sleep(), n.body2d.next_position().rotation];
    }
}
"#,
    );
    booted.tick(STEPS);
    let elapsed = STEPS as f64 / 60.0;
    let probe = |name: &str| {
        let node = booted.node(&format!("World/{name}"));
        let balaur_script::Value::List(found) = booted.call(node, "probe") else {
            panic!("{name}'s probe answered no list");
        };
        found
    };
    for still in ["Still3d", "Still2d"] {
        let clock = number(&probe(still)[0]);
        assert!(
            (clock - elapsed).abs() <= 1.0 / 60.0 + 1e-4,
            "{still} could sleep for {clock} s of the {elapsed} s it sat still"
        );
    }
    let spun = probe("Spinning3d");
    let clock = number(&spun[0]);
    assert!(
        clock.abs() < 1e-6,
        "a spinning body counted {clock} s to sleep"
    );
    let turned = boot::vector(&spun[1]);
    assert!(
        turned[0].abs() < 1e-4 && turned[1].abs() < 1e-4 && (turned[2] - elapsed).abs() < 1e-3,
        "{STEPS} steps at one radian a second left the next pose turned {turned:?}"
    );
    let flat = probe("Spinning2d");
    let clock = number(&flat[0]);
    assert!(
        clock.abs() < 1e-6,
        "a spinning 2D body counted {clock} s to sleep"
    );
    let flat_turn = number(&flat[1]);
    assert!(
        (flat_turn - elapsed).abs() < 1e-3,
        "{STEPS} steps at one radian a second left the next 2D pose turned {flat_turn}"
    );
}
