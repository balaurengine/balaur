//! What a freed node leaves behind in the physics world, which must be
//! nothing: a collider that answers raycasts forever, or a handle a script
//! call indexes rapier's arena with, are the same bug seen twice.

use balaur::{standard_app, AppConfig};
use balaur_core::components::StableId;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, snapshot, App};
use balaur_physics::{PhysicsPlugin, PhysicsState};

static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn app() -> App {
    let mut app = App::new(balaur_core::AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

/// A node at `y` with a ball collider, and a body only when `body` says so:
/// a collider without one is standalone world geometry, which is the case
/// nothing used to remove.
fn spawn(app: &App, name: &str, id: &str, y: f32, body: bool) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .world_mut()
        .insert_one(e, StableId(id.to_string()))
        .unwrap();
    app.engine.world().get::<&mut Transform>(e).unwrap().position.y = y;
    if body {
        components::add(
            &app.engine,
            e,
            "body3d",
            Some(&toml::from_str("kind = \"dynamic\"").unwrap()),
        )
        .unwrap();
    }
    components::add(
        &app.engine,
        e,
        "collider3d",
        Some(&toml::from_str("kind = \"ball\"\nradius = 0.5").unwrap()),
    )
    .unwrap();
    e
}

fn colliders_in_world(app: &App) -> usize {
    let state = app.engine.resource::<PhysicsState>();
    let count = state.borrow().world.colliders.len();
    count
}

/// `free_subtree` is the raw despawn every free path ends in, so a plugin
/// that only survives the component `remove` hooks fails here.
#[test]
fn freeing_a_node_takes_its_standalone_collider_out_of_the_world() {
    let mut app = app();
    let kept = spawn(&app, "Kept", "n_kept", 0.0, false);
    let doomed = spawn(&app, "Doomed", "n_doomed", 4.0, false);
    app.tick(1.0 / 60.0);
    assert_eq!(colliders_in_world(&app), 2);

    scene::free_subtree(&mut app.engine.world_mut(), doomed);
    app.tick(1.0 / 60.0);

    assert_eq!(
        colliders_in_world(&app),
        1,
        "the freed node's collider is still in the world"
    );
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    assert!(!state.colliders.contains_key(&doomed));
    assert!(!state.collider_params.contains_key(&doomed));
    assert!(state.colliders.contains_key(&kept), "the wrong one went");
}

/// Editors sit paused, so the prune cannot live behind the pause check.
#[test]
fn a_freed_node_is_pruned_while_the_simulation_is_paused() {
    let mut app = app();
    let doomed = spawn(&app, "Doomed", "n_doomed", 4.0, true);
    app.tick(1.0 / 60.0);
    {
        let state = app.engine.resource::<PhysicsState>();
        state.borrow_mut().paused = true;
    }

    scene::free_subtree(&mut app.engine.world_mut(), doomed);
    app.tick(1.0 / 60.0);

    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    assert_eq!(state.world.colliders.len(), 0, "paused, so nothing pruned");
    assert!(state.bodies.is_empty());
    assert!(state.colliders.is_empty());
}

/// Rapier drops a joint when either end's body goes; keeping the handle would
/// report a joint that is not there and never retry it.
#[test]
fn freeing_one_end_of_a_joint_drops_the_joint_too() {
    let mut app = app();
    let anchor = spawn(&app, "Anchor", "n_anchor", 0.0, true);
    let hanging = spawn(&app, "Hanging", "n_hanging", -1.0, true);
    components::add(
        &app.engine,
        hanging,
        "joint3d",
        Some(&toml::from_str("kind = \"revolute\"\nbody = \"/Anchor\"").unwrap()),
    )
    .unwrap();
    app.tick(1.0 / 60.0);
    {
        let state = app.engine.resource::<PhysicsState>();
        assert_eq!(state.borrow().joints.len(), 1, "the joint was never made");
    }

    scene::free_subtree(&mut app.engine.world_mut(), anchor);
    app.tick(1.0 / 60.0);

    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    assert!(
        state.joints.is_empty(),
        "the joint's handle outlived the body rapier dropped it with"
    );
    assert!(
        state.joint_params.contains_key(&hanging),
        "the authored joint should still be waiting for its other end"
    );
}

/// Core respawns a freed node as a *new* entity, so a frame keyed by entity
/// bits restores a map pointing at a dead one.
#[test]
fn a_body_whose_node_was_freed_and_restored_falls_again() {
    let mut app = app();
    let doomed = spawn(&app, "Faller", "n_faller", 4.0, true);
    for _ in 0..5 {
        app.tick(1.0 / 60.0);
    }
    let taken = snapshot::capture(&app.engine);

    scene::free_subtree(&mut app.engine.world_mut(), doomed);
    app.tick(1.0 / 60.0);
    snapshot::restore(&app.engine, &taken);

    let root = app.engine.root();
    let back = balaur_core::ids::find(&app.engine.world(), root, "n_faller")
        .expect("the nodes source put the node back");
    assert_ne!(back, doomed, "the respawn should mint a new entity");
    let start = {
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        let handle = *state
            .bodies
            .get(&back)
            .expect("physics re-resolved the body onto the respawned node");
        state.world.bodies[handle].translation().y
    };
    for _ in 0..20 {
        app.tick(1.0 / 60.0);
    }
    let now = {
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        let handle = state.bodies[&back];
        state.world.bodies[handle].translation().y
    };
    assert!(now < start - 0.001, "the restored body never fell: {now}");
}

/// A vehicle's engine force, brake, steering and wheel rotation are in no
/// component, so only the frame can carry them across a rollback.
#[test]
fn a_wheels_inputs_survive_a_snapshot() {
    let app = app();
    let chassis = spawn(&app, "Car", "n_car", 2.0, true);
    components::add(
        &app.engine,
        chassis,
        "vehicle3d",
        Some(&toml::from_str("forward_axis = 2.0").unwrap()),
    )
    .unwrap();
    let wheel = scene::spawn_node(&mut app.engine.world_mut(), "Wheel", chassis);
    app.engine
        .world_mut()
        .insert_one(wheel, StableId(String::from("n_wheel")))
        .unwrap();
    components::add(&app.engine, wheel, "wheel3d", None).unwrap();
    {
        let state = app.engine.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        let input = state.wheel_inputs.entry(wheel).or_default();
        input.engine_force = 250.0;
        input.brake = 3.0;
        input.steering = 0.25;
    }
    let taken = snapshot::capture(&app.engine);
    {
        let state = app.engine.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        let input = state.wheel_inputs.entry(wheel).or_default();
        input.engine_force = 0.0;
        input.brake = 0.0;
        input.steering = 0.0;
    }
    snapshot::restore(&app.engine, &taken);

    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    let input = state.wheel_inputs[&wheel];
    assert!((input.engine_force - 250.0).abs() < 1e-4, "engine force lost");
    assert!((input.brake - 3.0).abs() < 1e-4, "brake lost");
    assert!((input.steering - 0.25).abs() < 1e-4, "steering lost");
}

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
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

const TWO_BALLS: &str = r#"[[nodes]]
id = "n_watcher"
name = "Watcher"
script = "scripts/s.rn"

[[nodes]]
id = "n_target"
name = "Target"
position = [0.0, 2.0, 0.0]

[nodes.collider3d]
kind = "ball"
radius = 0.5
"#;

/// The whole point of the prune: a collider on a freed node used to keep
/// answering, because nothing ever took it out of the world.
#[test]
fn a_raycast_stops_finding_a_freed_node() {
    let errors = run(
        TWO_BALLS,
        r#"pub fn init(this) { this.ticks = 0; this.hit_before = false; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    let opts = #{ from: [0.0, 8.0, 0.0], dir: [0.0, -1.0, 0.0], max: 100.0 };
    if this.ticks == 2 {
        let hit = physics3d::raycast(opts);
        this.hit_before = !(hit is Tuple);
        let target = engine::find("/Target");
        target.queue_free();
    }
    if this.ticks == 8 {
        assert!(this.hit_before, "the ray never found the target to begin with");
        let hit = physics3d::raycast(opts);
        assert!(hit is Tuple, "the freed node still answers raycasts");
    }
}
"#,
    );
    assert!(errors.is_empty(), "the run logged errors: {errors:#?}");
}

/// A script that kept `other` from a collision event and calls into it a step
/// later used to panic inside rapier's arena.
#[test]
fn a_call_on_a_freed_node_errors_rather_than_panicking() {
    let errors = run(
        TWO_BALLS,
        r#"pub fn init(this) { this.ticks = 0; this.target = engine::find("/Target"); }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 2 {
        this.target.queue_free();
    }
    if this.ticks == 8 {
        let gone = this.target;
        let (aabb_ok, _) = script::attempt(|| physics3d::aabb(gone));
        let (mass_ok, _) = script::attempt(|| physics3d::collider_mass(gone));
        let (volume_ok, _) = script::attempt(|| physics3d::collider_volume(gone));
        let (swept_ok, _) = script::attempt(|| physics3d::swept_aabb(gone));
        assert!(!aabb_ok, "aabb answered for a freed node");
        assert!(!mass_ok, "collider_mass answered for a freed node");
        assert!(!volume_ok, "collider_volume answered for a freed node");
        assert!(!swept_ok, "swept_aabb answered for a freed node");
    }
}
"#,
    );
    assert!(errors.is_empty(), "the run logged errors: {errors:#?}");
}
