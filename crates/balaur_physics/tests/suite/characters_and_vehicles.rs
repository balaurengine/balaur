//! What a character's sweep and a vehicle's wheel rays meet, the slope
//! angles' units, and the vehicle controller kept from one step to the next.

use balaur_core::components::{ComponentRegistry, StableId};
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, components, snapshot};
use balaur_physics::{PhysicsPlugin, PhysicsState3d};

use crate::joints_and_characters::run_clean;

/// A world with a wall two units along x, its collider table `wall`, and a
/// player whose `character3d` table is `character`, walking into it.
fn walk(wall: &str, collider: &str, character: &str, expect: &str) {
    run_clean(
        &format!(
            r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.collider3d]
kind = "box"
size = [1.0, 8.0, 16.0]
{wall}

[[nodes]]
id = "n_player"
name = "Player"
parent = "n_world"
script = {{ source = "scripts/s.rn" }}

[nodes.collider3d]
kind = "capsule"
radius = 0.4
height = 1.0
{collider}

[nodes.character3d]
floor_snap_length = 0.0
{character}
"#
        ),
        &format!(
            "pub fn init(this) {{ this.ticks = 0; }}

pub fn fixed_update(this, dt) {{
    this.ticks = this.ticks + 1;
    this.node.character3d.move_character(0.1, 0.0, 0.0);
    if this.ticks == 110 {{
        let x = this.node.transform.position.x;
        {expect}
    }}
}}
"
        ),
    );
}

const THROUGH: &str = r#"assert!(x > 3.0, "the character stopped at the wall: x is {}", x);"#;
const STOPPED: &str = r#"assert!(x < 1.2, "the character walked into the wall: x is {}", x);"#;

#[test]
fn a_character_walks_through_a_sensor_unless_told_not_to() {
    walk("sensor = true", "", "", THROUGH);
    walk("sensor = true", "", "ignore = []", STOPPED);
}

#[test]
fn a_character_passes_a_layer_its_mask_leaves_out() {
    walk(
        "collision_layer = [\"2\"]",
        "collision_mask = [\"1\"]",
        "",
        THROUGH,
    );
}

#[test]
fn a_character_passes_the_nodes_it_ignores() {
    walk("", "", "ignore_nodes = [\"/World/Wall\"]", THROUGH);
    walk("", "", "ignore = [\"static\"]", THROUGH);
}

/// A second collider on the character's own body, out in front of it, used
/// to stop every move it made.
#[test]
fn a_character_never_meets_its_own_body() {
    run_clean(
        r#"[[nodes]]
id = "n_player"
name = "Player"
body3d = { kind = "kinematic" }
script = { source = "scripts/s.rn" }

[nodes.collider3d]
kind = "capsule"
radius = 0.4
height = 1.0

[nodes.character3d]
floor_snap_length = 0.0

[[nodes]]
id = "n_nose"
name = "Nose"
parent = "n_player"

[nodes.transform]
position = [0.6, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.15
"#,
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    this.node.character3d.move_character(0.1, 0.0, 0.0);
    if this.ticks == 60 {
        let x = this.node.transform.position.x;
        assert!(x > 3.0, "the character's own collider stopped it: x is {}", x);
    }
}
"#,
    );
}

#[test]
fn a_2d_character_walks_through_a_sensor() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.collider2d]
kind = "rectangle"
size = [1.0, 8.0]
sensor = true

[[nodes]]
id = "n_player"
name = "Player"
parent = "n_world"
script = { source = "scripts/s.rn" }

[nodes.collider2d]
kind = "circle"
radius = 0.4

[nodes.character2d]
floor_snap_length = 0.0
"#,
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    this.node.character2d.move_character(0.1, 0.0);
    if this.ticks == 110 {
        let x = this.node.transform.position.x;
        assert!(x > 3.0, "the 2D character stopped at a sensor: x is {}", x);
    }
}
"#,
    );
}

/// The inspector clamps what it shows, degrees, so the bounds are written
/// in degrees while the file keeps radians.
#[test]
fn slope_angles_take_their_bounds_in_degrees() {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    let registry = app.engine.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    for component in ["character3d", "character2d"] {
        let schema = &registry.def(component).unwrap().schema;
        for key in ["floor_max_angle", "min_slide_angle"] {
            let spec = &schema[key];
            assert_eq!(spec["unit"].as_str(), Some("degrees"));
            assert_eq!(spec["max"].as_float(), Some(90.0), "{component}.{key}");
            let default = spec["default"].as_float().unwrap();
            assert!(
                default < 1.6,
                "{component}.{key} stores {default}, not radians"
            );
        }
    }
}

fn app() -> App {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

fn node(app: &App, name: &str, parent: Entity, at: [f32; 3]) -> Entity {
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, parent);
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

fn add(app: &App, e: Entity, component: &str, text: &str) {
    components::add(
        &app.engine,
        e,
        component,
        Some(&toml::from_str(text).unwrap()),
    )
    .unwrap();
}

/// A ground slab under z from -2 to 2 and a one-wheeled car over it, held
/// level; each table extends the slab's collider, the `vehicle3d` and the
/// `wheel3d`. Answers the car and its wheel.
fn car(app: &App, ground: &str, vehicle: &str, wheel: &str) -> (Entity, Entity) {
    let root = app.engine.root();
    let slab = node(app, "Ground", root, [0.0, -0.5, 0.0]);
    add(
        app,
        slab,
        "collider3d",
        &format!("kind = \"box\"\nsize = [8.0, 1.0, 4.0]\n{ground}"),
    );
    // The ray is rest_length 0.3 and radius 0.4 long, so it reaches 0.1 into the slab.
    let chassis = node(app, "Car", root, [0.0, 0.6, 0.0]);
    add(
        app,
        chassis,
        "body3d",
        "kind = \"dynamic\"\ngravity_scale = 0.0\nlock_translation = [\"y\"]\nlock_rotation = [\"x\", \"y\", \"z\"]",
    );
    add(
        app,
        chassis,
        "collider3d",
        "kind = \"sphere\"\nradius = 0.1",
    );
    add(app, chassis, "vehicle3d", vehicle);
    let tyre = node(app, "Wheel", chassis, [0.0, 0.0, 0.0]);
    add(app, tyre, "wheel3d", &format!("radius = 0.4\n{wheel}"));
    (chassis, tyre)
}

fn roll(app: &App, chassis: Entity) {
    let state = app.engine.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let handle = state.bodies[&chassis];
    let body = &mut state.world.bodies[handle];
    body.set_linvel(
        balaur_physics::rapier3d::math::Vector::new(0.0, 0.0, 4.0),
        true,
    );
}

fn wheel(app: &App, tyre: Entity) -> balaur_physics::vehicle::WheelInput3d {
    let state = app.engine.resource::<PhysicsState3d>();
    state.borrow().wheel_inputs[&tyre]
}

/// Off the end of the slab, a wheel keeps the spin it had and loses one
/// hundredth of it a step, and a snapshot carries that spin.
#[test]
fn an_airborne_wheel_keeps_spinning_and_slows() {
    let mut app = app();
    let (chassis, tyre) = car(&app, "", "", "");
    app.tick(1.0 / 60.0);
    roll(&app, chassis);
    let mut grounded = false;
    for _ in 0..60 {
        app.tick(1.0 / 60.0);
        grounded |= wheel(&app, tyre).grounded;
        if grounded && !wheel(&app, tyre).grounded {
            break;
        }
    }
    assert!(grounded, "the wheel never touched the slab");
    assert!(!wheel(&app, tyre).grounded, "the car never left the slab");
    let turned = |app: &mut App| {
        let before = wheel(app, tyre).rotation;
        app.tick(1.0 / 60.0);
        wheel(app, tyre).rotation - before
    };
    let first = turned(&mut app);
    let taken = snapshot::capture(&app.engine);
    let second = turned(&mut app);
    assert!(first.abs() > 1e-3, "an airborne wheel stopped dead");
    assert!(
        (second / first - 0.99).abs() < 1e-3,
        "the spin went from {first} to {second}, not down by a hundredth"
    );
    snapshot::restore(&app.engine, &taken);
    let again = turned(&mut app);
    assert!(
        (again - second).abs() < 1e-6,
        "after a restore the wheel turned {again}, not {second}"
    );
}

#[test]
fn wheel_rays_pass_through_sensors_and_layers_off_the_mask() {
    let grounded = |ground: &str, vehicle: &str| {
        let mut app = app();
        let (_, tyre) = car(&app, ground, vehicle, "");
        app.tick(1.0 / 60.0);
        app.tick(1.0 / 60.0);
        wheel(&app, tyre).grounded
    };
    assert!(grounded("", ""), "the wheel missed solid ground");
    assert!(
        !grounded("sensor = true", ""),
        "the wheel stood on a sensor"
    );
    assert!(
        grounded("sensor = true", "ignore = []"),
        "ignore = [] still passed the sensor"
    );
    assert!(
        !grounded("collision_layer = [\"2\"]", "collision_mask = [\"1\"]"),
        "the wheel stood on a layer its mask leaves out"
    );
}

#[test]
fn a_wheels_suspension_force_is_reported_after_its_cap() {
    let mut app = app();
    let (_, tyre) = car(
        &app,
        "",
        "",
        "suspension_max_force = 0.5\nrest_length = 0.6",
    );
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let read = wheel(&app, tyre);
    assert!(read.grounded, "the wheel never touched the slab");
    assert!(
        read.suspension_force > 0.0 && read.suspension_force <= 0.5,
        "the suspension pushed with {} past its 0.5 cap",
        read.suspension_force
    );
}

/// Rapier wakes a chassis for a forward drive alone.
#[test]
fn reversing_or_steering_wakes_a_parked_car() {
    for (what, set) in [
        (
            "reversing",
            (|input: &mut balaur_physics::vehicle::WheelInput3d| input.engine_force = -50.0)
                as fn(&mut _),
        ),
        ("steering", |input| input.steering = 0.3),
        ("braking", |input| input.brake = 1.0),
    ] {
        let mut app = app();
        let (chassis, tyre) = car(&app, "", "", "");
        app.tick(1.0 / 60.0);
        {
            let state = app.engine.resource::<PhysicsState3d>();
            let mut state = state.borrow_mut();
            let handle = state.bodies[&chassis];
            state.world.bodies[handle].sleep();
            set(state.wheel_inputs.entry(tyre).or_default());
        }
        app.tick(1.0 / 60.0);
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        let asleep = state.world.bodies[state.bodies[&chassis]].is_sleeping();
        assert!(!asleep, "{what} left the chassis asleep");
    }
}

/// A project of `scene` and one script, loaded and ticked `ticks` times, for a
/// test that reads the world after; the errors it logged come back too.
fn boot(scene: &str, script: &str, ticks: u32) -> (tempfile::TempDir, App, Vec<String>) {
    let _guard = crate::LOG
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
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    for _ in 0..ticks {
        app.tick(1.0 / 60.0);
    }
    let errors = balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (dir, app, errors)
}

/// Where the node at `path` stands.
fn position_of(app: &App, path: &str) -> glamx::Vec3 {
    let world = app.engine.world();
    let e = scene::find_node(&world, app.engine.root(), path).expect("the node is in the scene");
    world.get::<&Transform>(e).unwrap().position
}

/// A wall whose face is at x = 1.5 and a player walking into it, with the
/// player's own collider rows `player` and its `character3d` rows `character`.
fn wall_and_player(player: &str, character: &str) -> String {
    format!(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.collider3d]
kind = "box"
size = [1.0, 8.0, 16.0]

[[nodes]]
id = "n_player"
name = "Player"
parent = "n_world"
script = {{ source = "scripts/s.rn" }}
{player}

[nodes.character3d]
floor_snap_length = 0.0
{character}
"#
    )
}

const WALK: &str =
    "pub fn fixed_update(this, dt) { this.node.character3d.move_character(0.05, 0.0, 0.0); }\n";

const CAPSULE: &str = "\n[nodes.collider3d]\nkind = \"capsule\"\nradius = 0.4\nheight = 2.0\n";

#[test]
fn each_character_length_reads_its_own_mode() {
    let stop = |character: &str| {
        let (_dir, app, errors) = boot(&wall_and_player(CAPSULE, character), WALK, 60);
        assert!(errors.is_empty(), "{errors:#?}");
        position_of(&app, "World/Player").x
    };
    let absolute = stop("safe_margin = 0.2");
    let relative = stop("safe_margin = 0.2\nsafe_margin_lengths = \"relative\"");
    // Relative, the margin is a fifth of the capsule's height of 2.
    assert!(
        (absolute - 0.9).abs() < 0.05,
        "an absolute margin of 0.2 stopped the player at {absolute}"
    );
    assert!(
        (relative - 0.7).abs() < 0.05,
        "a relative margin of 0.2 stopped the player at {relative}"
    );
    let snapped = stop("safe_margin = 0.2\nfloor_snap_lengths = \"relative\"");
    assert!(
        (snapped - absolute).abs() < 1e-3,
        "another length's mode moved the margin: {snapped}"
    );
}

#[test]
fn every_collider_of_a_character_is_swept() {
    let text = format!(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.collider3d]
kind = "box"
size = [1.0, 8.0, 16.0]

[[nodes]]
id = "n_player"
name = "Player"
parent = "n_world"
script = {{ source = "scripts/s.rn" }}
body3d = {{ kind = "kinematic" }}
character3d = {{ floor_snap_length = 0.0 }}
{CAPSULE}
[[nodes]]
id = "n_nose"
name = "Nose"
parent = "n_player"

[nodes.transform]
position = [0.8, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.3
"#
    );
    let (_dir, app, errors) = boot(&text, WALK, 60);
    assert!(errors.is_empty(), "{errors:#?}");
    let x = position_of(&app, "World/Player").x;
    // The nose reaches 1.1 ahead of the node, so it meets the wall first.
    assert!(
        x < 0.45,
        "the nose passed into the wall: the player is at {x}"
    );
}

#[test]
fn a_collision_reports_where_the_sweep_met_the_obstacle() {
    let (_dir, _app, errors) = boot(
        &wall_and_player(CAPSULE, ""),
        r#"pub fn init(this) { this.done = false; }

pub fn fixed_update(this, dt) {
    let moved = this.node.character3d.move_character(0.2, 0.0, 0.0);
    if !this.done && moved.collisions.len() > 0 {
        let hit = moved.collisions[0];
        assert!(hit.point.x > 1.4 && hit.point.x < 1.6, "the wall was met at {}", hit.point.x);
        assert!(hit.own_point.x > 1.3 && hit.own_point.x < 1.6, "the capsule was met at {}", hit.own_point.x);
        assert!(hit.own_normal.x > 0.9, "the capsule's normal points {}", hit.own_normal.x);
        assert!(hit.distance >= 0.0 && hit.distance <= 0.2, "it swept {}", hit.distance);
        assert!(hit.position.x + 0.4 < 1.6, "the character stood at {}", hit.position.x);
        assert!(hit.applied.x >= 0.0, "applied {}", hit.applied.x);
        assert!(hit.status == physics3d::SWEEP_CONVERGED || hit.status == physics3d::SWEEP_OUT_OF_ITERATIONS, "status {}", hit.status);
        assert!(hit.subshape == 0, "a box has no parts: {}", hit.subshape);
        this.done = true;
        log::error("checked: collision record");
    }
}
"#,
        30,
    );
    assert!(
        errors.len() == 1 && errors[0].contains("checked: collision record"),
        "the check did not run clean: {errors:#?}"
    );
}

#[test]
fn a_heavier_push_mass_shoves_a_crate_further() {
    let shove = |character: &str| {
        let scene = format!(
            r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_floor"
name = "Floor"
parent = "n_world"

[nodes.transform]
position = [0.0, -1.5, 0.0]

[nodes.collider3d]
kind = "box"
size = [40.0, 1.0, 40.0]
friction = 0.0

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
body3d = {{ kind = "dynamic", mass = 20.0 }}

[nodes.transform]
position = [1.2, -0.5, 0.0]

[nodes.collider3d]
kind = "box"
size = [1.0, 1.0, 1.0]
friction = 0.0

[[nodes]]
id = "n_player"
name = "Player"
parent = "n_world"
script = {{ source = "scripts/s.rn" }}
body3d = {{ kind = "kinematic" }}

[nodes.transform]
position = [0.0, -0.4, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.4

[nodes.character3d]
floor_snap_length = 0.0
{character}
"#
        );
        let (_dir, app, errors) = boot(&scene, WALK, 40);
        assert!(errors.is_empty(), "{errors:#?}");
        position_of(&app, "World/Crate").x
    };
    let light = shove("push_mass = 0.01");
    let heavy = shove("push_mass = 1000.0");
    assert!(
        heavy > light + 0.2,
        "a 1000 kg push moved the crate to {heavy}, a 0.01 kg one to {light}"
    );
}

#[test]
fn a_car_facing_down_its_negative_axis_drives_that_way() {
    let mut app = app();
    let (chassis, tyre) = car(&app, "", "forward_axis = \"-z\"", "");
    app.tick(1.0 / 60.0);
    {
        let state = app.engine.resource::<PhysicsState3d>();
        state
            .borrow_mut()
            .wheel_inputs
            .entry(tyre)
            .or_default()
            .engine_force = 200.0;
    }
    for _ in 0..20 {
        app.tick(1.0 / 60.0);
    }
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let vz = state.world.bodies[state.bodies[&chassis]].linvel().z;
    assert!(vz < -0.1, "a forward drive moved the car along +z at {vz}");
    // rapier's own index names +z, so its reading runs against the car.
    let kept = state.vehicles[&chassis].controller.current_vehicle_speed;
    assert!(
        kept < 0.0,
        "rapier read the speed along the car's -z: {kept}"
    );
}

const DRIVEWAY: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"

[nodes.transform]
position = [0.0, -0.5, 0.0]

[nodes.collider3d]
kind = "box"
size = [8.0, 1.0, 40.0]

[[nodes]]
id = "n_car"
name = "Car"
parent = "n_world"
script = { source = "scripts/s.rn" }
body3d = { kind = "dynamic", gravity_scale = 0.0, lock_translation = ["y"], lock_rotation = ["x", "y", "z"] }
vehicle3d = { forward_axis = "-z" }

[nodes.transform]
position = [0.0, 0.6, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.1

[[nodes]]
id = "n_wheel"
name = "Wheel"
parent = "n_car"
wheel3d = { radius = 0.4 }
"#;

#[test]
fn speed_and_wheel_state_read_the_kept_controller() {
    let (_dir, _app, errors) = boot(
        DRIVEWAY,
        r#"pub fn init(this) {
    this.ticks = 0;
    this.node.body3d.set_linear_velocity(0.0, 0.0, 4.0);
}

pub fn fixed_update(this, dt) {
    this.ticks += 1;
    let wheel = this.node.get_node("Wheel");
    if this.ticks == 3 {
        wheel.wheel3d.set_wheel_rotation(1.5);
        assert!((wheel.wheel3d.wheel_state().rotation - 1.5).abs() < 0.0001, "the rotation was not set");
    }
    if this.ticks == 10 {
        let speed = this.node.vehicle3d.speed();
        assert!((speed + 4.0).abs() < 0.5, "a car rolling backwards read {}", speed);
        let state = wheel.wheel3d.wheel_state();
        assert!(state.in_contact, "the wheel is off the ground");
        assert!(state.ground == scene::get_node("World/Ground"), "the ray hit {:?}", state.ground);
        assert!(state.contact_point.y.abs() < 0.05, "the ray met the ground at {}", state.contact_point.y);
        assert!(state.contact_normal.y > 0.9, "the ground's normal points {}", state.contact_normal.y);
        assert!((state.ray_origin.y - 0.6).abs() < 0.05, "the ray started at {}", state.ray_origin.y);
        assert!(state.suspension_length > 0.0 && state.suspension_length < 0.31, "suspension {}", state.suspension_length);
        assert!(state.suspension.y < -0.9, "the suspension points {}", state.suspension.y);
        assert!(state.axle.x.abs() > 0.9, "the axle points {}", state.axle.x);
        assert!(state.center.y < 0.6, "the wheel's centre is at {}", state.center.y);
        assert!(state.rotation.abs() > 1.6, "the wheel stopped turning after the set: {}", state.rotation);
        log::error("checked: wheel state");
    }
}
"#,
        12,
    );
    assert!(
        errors.len() == 1 && errors[0].contains("checked: wheel state"),
        "the check did not run clean: {errors:#?}"
    );
}
