//! What a freed node leaves behind in the physics world, which must be
//! nothing: a collider that answers raycasts forever, or a handle a script
//! call indexes rapier's arena with, are the same bug seen twice.

use balaur::standard_app;
use balaur_core::components::StableId;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, components, snapshot};
use balaur_physics::{PhysicsPlugin, PhysicsState3d};

use crate::LOG;

fn app() -> App {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
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
    app.engine
        .world()
        .get::<&mut Transform>(e)
        .unwrap()
        .position
        .y = y;
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
        Some(&toml::from_str("kind = \"sphere\"\nradius = 0.5").unwrap()),
    )
    .unwrap();
    e
}

fn colliders_in_world(app: &App) -> usize {
    let state = app.engine.resource::<PhysicsState3d>();

    state.borrow().world.colliders.len()
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
    let state = app.engine.resource::<PhysicsState3d>();
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
        let state = app.engine.resource::<PhysicsState3d>();
        state.borrow_mut().paused = true;
    }

    scene::free_subtree(&mut app.engine.world_mut(), doomed);
    app.tick(1.0 / 60.0);

    let state = app.engine.resource::<PhysicsState3d>();
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
        Some(&toml::from_str("kind = \"hinge\"\nconnected_body = \"/Anchor\"").unwrap()),
    )
    .unwrap();
    app.tick(1.0 / 60.0);
    {
        let state = app.engine.resource::<PhysicsState3d>();
        assert_eq!(state.borrow().joints.len(), 1, "the joint was never made");
    }

    scene::free_subtree(&mut app.engine.world_mut(), anchor);
    app.tick(1.0 / 60.0);

    let state = app.engine.resource::<PhysicsState3d>();
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
        let state = app.engine.resource::<PhysicsState3d>();
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
        let state = app.engine.resource::<PhysicsState3d>();
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
        Some(&toml::from_str("forward_axis = \"z\"").unwrap()),
    )
    .unwrap();
    let wheel = scene::spawn_node(&mut app.engine.world_mut(), "Wheel", chassis);
    app.engine
        .world_mut()
        .insert_one(wheel, StableId(String::from("n_wheel")))
        .unwrap();
    components::add(&app.engine, wheel, "wheel3d", None).unwrap();
    {
        let state = app.engine.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let input = state.wheel_inputs.entry(wheel).or_default();
        input.engine_force = 250.0;
        input.brake = 3.0;
        input.steering = 0.25;
    }
    let taken = snapshot::capture(&app.engine);
    {
        let state = app.engine.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let input = state.wheel_inputs.entry(wheel).or_default();
        input.engine_force = 0.0;
        input.brake = 0.0;
        input.steering = 0.0;
    }
    snapshot::restore(&app.engine, &taken);

    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let input = state.wheel_inputs[&wheel];
    assert!(
        (input.engine_force - 250.0).abs() < 1e-4,
        "engine force lost"
    );
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
    std::fs::write(dir.path().join("scripts/s.bs"), script).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

const TWO_BALLS: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_watcher"
name = "Watcher"
parent = "n_world"
script = { source = "scripts/s.bs" }

[[nodes]]
id = "n_target"
name = "Target"
parent = "n_world"

[nodes.transform]
position = [0.0, 2.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.5
"#;

/// The whole point of the prune: a collider on a freed node used to keep
/// answering, because nothing ever took it out of the world.
#[test]
fn a_raycast_stops_finding_a_freed_node() {
    let errors = run(
        TWO_BALLS,
        r#"pub struct S { node: Node, ticks: int, hit_before: bool }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        let opts = map().with("origin", [0.0, 8.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0);
        if self.ticks == 2 {
            self.hit_before = !physics3d::raycast(opts).is_nil();
            scene::get_node("/World/Target").queue_free();
        }
        if self.ticks == 8 {
            if !self.hit_before { log::error("the ray never found the target to begin with"); }
            if !physics3d::raycast(opts).is_nil() { log::error("the freed node still answers raycasts"); }
        }
    }
}
"#,
    );
    assert!(errors.is_empty(), "the run logged errors: {errors:#?}");
}

/// A script that kept `other` from a collision event and calls into it a step
/// later used to panic inside rapier's arena. A failing engine call ends the
/// script call it is in, so each reader is a method of its own.
#[test]
fn a_call_on_a_freed_node_errors_rather_than_panicking() {
    let mut b = crate::boot::project(
        TWO_BALLS,
        r#"pub struct S { node: Node, ticks: int, target: Node }
impl S {
    fn init() {
        self.target = scene::get_node("/World/Target");
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 2 {
            self.target.queue_free();
        }
    }

    pub fn aabb() {
        self.target.collider3d.aabb();
        log::error("aabb answered for a freed node");
    }

    pub fn mass() {
        self.target.collider3d.collider_mass();
        log::error("collider_mass answered for a freed node");
    }

    pub fn volume() {
        self.target.collider3d.collider_volume();
        log::error("collider_volume answered for a freed node");
    }

    pub fn swept() {
        self.target.collider3d.swept_aabb();
        log::error("swept_aabb answered for a freed node");
    }
}
"#,
    );
    b.tick(8);
    let watcher = balaur_core::node_id_of(b.node("World/Watcher"));
    let host = b.app.engine.script_host().unwrap();
    for reader in ["aabb", "mass", "volume", "swept"] {
        assert_eq!(
            host.call_on(watcher, reader, &[]),
            None,
            "{reader} answered for a freed node"
        );
    }
    let errors = crate::boot::errors();
    assert_eq!(errors.len(), 4, "one error a reader: {errors:#?}");
    assert!(
        errors
            .iter()
            .all(|e| !e.contains("answered for a freed node")),
        "a reader answered for a freed node: {errors:#?}"
    );
}

/// A soft body names its node in `user_data`, which a tear event reads, so a
/// restore onto a respawned node has to point it at the new entity.
#[test]
fn a_restored_soft_body_names_the_respawned_node() {
    let mut app = app();
    let root = app.engine.root();
    let doomed = scene::spawn_node(&mut app.engine.world_mut(), "Blob", root);
    app.engine
        .world_mut()
        .insert_one(doomed, StableId("n_blob".to_string()))
        .unwrap();
    let cuboid = toml::from_str("kind = \"box\"\ncells = [2.0, 2.0, 2.0]").unwrap();
    components::add(&app.engine, doomed, "softbody3d", Some(&cuboid)).unwrap();
    app.tick(1.0 / 60.0);
    let taken = snapshot::capture(&app.engine);

    scene::free_subtree(&mut app.engine.world_mut(), doomed);
    app.tick(1.0 / 60.0);
    snapshot::restore(&app.engine, &taken);

    let back = balaur_core::ids::find(&app.engine.world(), root, "n_blob")
        .expect("the nodes source put the node back");
    assert_ne!(back, doomed, "the respawn should mint a new entity");
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let handle = *state
        .soft_bodies
        .get(&back)
        .expect("physics re-resolved the soft body onto the respawned node");
    assert_eq!(
        state.world.soft_bodies.get(handle).unwrap().user_data,
        u128::from(back.to_bits().get()),
        "the restored soft body still names the freed node"
    );
}
