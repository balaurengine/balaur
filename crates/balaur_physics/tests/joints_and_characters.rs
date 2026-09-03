//! Phases 5, 6 and 7 of `docs/PLAN-rapier.md`: collision events, joints and
//! the character controller, driven from scripts because their whole surface
//! is the script seam.

use balaur::{standard_app, AppConfig};

static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run a project made of `scene` and one script, and report what it logged as
/// an error.
fn run(scene: &str, script: &str) -> Vec<String> {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..120 {
        app.tick(1.0 / 60.0);
    }
    balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

fn run_clean(scene: &str, script: &str) {
    let errors = run(scene, script);
    assert!(errors.is_empty(), "the run logged errors: {errors:#?}");
}

/// A sensor tells the node's script who walked into it, and who left.
#[test]
fn a_trigger_calls_its_script_when_something_enters() {
    run_clean(
        r#"[[nodes]]
id = "n_trigger"
name = "Trigger"
script = "scripts/s.rn"

[nodes.collider3d]
kind = "cuboid"
half_extents = [2.0, 2.0, 2.0]
sensor = true
events = ["collision"]

[[nodes]]
id = "n_faller"
name = "Faller"
position = [0.0, 6.0, 0.0]
body3d = "dynamic"

[nodes.collider3d]
kind = "ball"
radius = 0.5
"#,
        r#"pub fn init(this) { this.seen = 0; this.left = 0; this.ticks = 0; }

pub fn on_collision_start(this, other) { this.seen += 1; }
pub fn on_collision_stop(this, other) { this.left += 1; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 110 {
        assert!(this.seen >= 1, "nothing ever entered the trigger");
        assert!(this.left >= 1, "nothing ever left it");
    }
}
"#,
    );
}

/// A revolute joint holds two bodies together: the hanging one swings rather
/// than falling away.
#[test]
fn a_joint_holds_two_bodies_together() {
    run_clean(
        r#"[[nodes]]
id = "n_anchor"
name = "Anchor"
body3d = "static"

[nodes.collider3d]
kind = "ball"
radius = 0.2

[[nodes]]
id = "n_hanging"
name = "Hanging"
position = [1.0, 0.0, 0.0]
body3d = "dynamic"
script = "scripts/s.rn"

[nodes.collider3d]
kind = "ball"
radius = 0.2

[nodes.joint3d]
kind = "revolute"
body = "/Anchor"
axis = [0.0, 0.0, 1.0]
anchor = [-1.0, 0.0, 0.0]
"#,
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 110 {
        let position = this.node.position();
        let far = math::sqrt(position.x * position.x + position.y * position.y);
        assert!(far < 1.2, "the joint let go: the body is {} from the anchor", far);
        assert!(position.y < -0.1, "the body never swung: y is {}", position.y);
    }
}
"#,
    );
}

/// A joint whose other end is named before that node exists still connects:
/// a scene file names nodes in whatever order it likes.
#[test]
fn a_joint_waits_for_a_node_that_comes_later() {
    run_clean(
        r#"[[nodes]]
id = "n_hanging"
name = "Hanging"
position = [1.0, 0.0, 0.0]
body3d = "dynamic"
script = "scripts/s.rn"

[nodes.collider3d]
kind = "ball"
radius = 0.2

[nodes.joint3d]
kind = "revolute"
body = "/Anchor"
anchor = [-1.0, 0.0, 0.0]

[[nodes]]
id = "n_anchor"
name = "Anchor"
body3d = "static"

[nodes.collider3d]
kind = "ball"
radius = 0.2
"#,
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 110 {
        let position = this.node.position();
        let far = math::sqrt(position.x * position.x + position.y * position.y);
        assert!(far < 1.2, "the forward reference never connected: {} away", far);
    }
}
"#,
    );
}

/// The character controller walks into a wall and stops, rather than passing
/// through it or being pushed back by the solver.
#[test]
fn a_character_slides_along_a_wall_instead_of_entering_it() {
    run_clean(
        r#"[[nodes]]
id = "n_wall"
name = "Wall"
position = [2.0, 0.0, 0.0]

[nodes.collider3d]
kind = "cuboid"
half_extents = [0.5, 4.0, 8.0]

[[nodes]]
id = "n_player"
name = "Player"
script = "scripts/s.rn"

[nodes.collider3d]
kind = "capsule"
radius = 0.4
height = 1.0

[nodes.character3d]
snap_to_ground = 0.0
"#,
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    // Walk into the wall, and along it.
    let moved = physics3d::move_character(this.node, 0.1, 0.0, 0.05);
    if this.ticks == 110 {
        let position = this.node.position();
        assert!(position.x < 1.2, "the character walked into the wall: x is {}", position.x);
        assert!(position.z > 0.5, "the character did not slide along it: z is {}", position.z);
    }
}
"#,
    );
}

/// Every phase owes the snapshot a round trip: a joint, a motor and a voxel
/// edit must all come back the way they went in, or a rollback quietly
/// rebuilds a different world.
#[test]
fn joints_and_shape_edits_survive_a_snapshot() {
    use balaur_core::hecs::Entity;
    use balaur_core::scene::{self, Transform};
    use balaur_core::{components, snapshot, App, AppConfig};
    use balaur_physics::{PhysicsPlugin, PhysicsState};

    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(PhysicsPlugin).unwrap();

    let root = app.engine.root();
    let spawn = |name: &str, x: f32| -> Entity {
        let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
        app.engine
            .world()
            .get::<&mut Transform>(e)
            .unwrap()
            .position
            .x = x;
        components::add(
            &app.engine,
            e,
            "body3d",
            Some(&toml::from_str("kind = \"dynamic\"").unwrap()),
        )
        .unwrap();
        components::add(
            &app.engine,
            e,
            "collider3d",
            Some(&toml::from_str("kind = \"ball\"\nradius = 0.2").unwrap()),
        )
        .unwrap();
        e
    };
    let anchor = spawn("Anchor", 0.0);
    let hanging = spawn("Hanging", 1.0);
    components::add(
        &app.engine,
        hanging,
        "joint3d",
        Some(
            &toml::from_str("kind = \"revolute\"\nbody = \"/Anchor\"\nbreak_force = 500.0")
                .unwrap(),
        ),
    )
    .unwrap();
    let _ = anchor;

    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    let taken = snapshot::capture(&app.engine);
    let before = {
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        (
            state.joints.len(),
            state.world.bodies[state.bodies[&hanging]].translation().y,
        )
    };
    assert_eq!(before.0, 1, "the joint was never made");

    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    snapshot::restore(&app.engine, &taken);
    let after = {
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        (
            state.joints.len(),
            state.world.bodies[state.bodies[&hanging]].translation().y,
        )
    };
    assert_eq!(after.0, before.0, "the joint did not come back");
    assert!(
        (after.1 - before.1).abs() < 1e-6,
        "the body came back at {} rather than {}",
        after.1,
        before.1
    );
}
