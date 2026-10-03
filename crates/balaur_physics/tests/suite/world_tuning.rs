//! `[physics.3d]` and `[physics.2d]` over the shared `[physics]` table, and
//! the soft-body rows with `[physics.soft_recovery]`.

use balaur_core::{App, AppConfig};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};

#[test]
fn each_world_reads_its_own_table_over_the_shared_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"tuned\"\nmain_scene = \"scenes/main.toml\"\n\n\
         [physics]\nlength_unit = 2.0\nsoft_max_extra_substeps = 7\n\n\
         [physics.soft_recovery]\noverlap_split = 4\noverlap_patch_constraints = \"stand_down\"\n\n\
         [physics.2d]\nlength_unit = 64.0\n\n\
         [physics.2d.soft_recovery]\noverlap_split = 6\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app.load_project().unwrap();
    app.tick(balaur_core::DEFAULT_FIXED_DT);
    let state = app.engine.resource::<PhysicsState3d>();
    let p = state.borrow().world.integration_parameters;
    let flat = app.engine.resource::<PhysicsState2d>();
    let q = flat.borrow().world.integration_parameters;
    assert!(
        (p.length_unit - 2.0).abs() < 1e-6,
        "3D length unit {}",
        p.length_unit
    );
    assert!(
        (q.length_unit - 64.0).abs() < 1e-6,
        "2D length unit {}",
        q.length_unit
    );
    assert_eq!(p.soft_bodies.max_extra_substeps, 7);
    assert_eq!(q.soft_bodies.max_extra_substeps, 7);
    assert_eq!(p.soft_bodies.recovery.overlap_split, 4);
    assert_eq!(q.soft_bodies.recovery.overlap_split, 6);
    assert_eq!(
        p.soft_bodies.recovery.overlap_patch_constraints,
        balaur_physics::rapier3d::dynamics::SoftPatchConstraints::StandDown
    );
}

/// A project whose `[physics]` table says `extra`, loaded but not yet ticked.
fn tuned(extra: &str) -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"tuned\"\nmain_scene = \"scenes/main.toml\"\n\n[physics]\n{extra}\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app.load_project().unwrap();
    (app, dir)
}

fn setup_entry<T>(
    app: &App,
    f: impl FnOnce(&balaur_core::replay::CaptureFn, &balaur_core::replay::RestoreFn) -> T,
) -> T {
    let registry = app
        .engine
        .resource::<balaur_core::replay::ReplaySetupRegistry>();
    let registry = registry.borrow();
    let (_, capture, restore) = registry
        .0
        .iter()
        .find(|(name, ..)| name == "physics_world")
        .expect("physics declares its world for a recording's header");
    f(capture, restore)
}

fn capture(app: &App) -> serde_json::Value {
    setup_entry(app, |capture, _| capture(&app.engine))
}

fn restore(app: &App, value: &serde_json::Value) {
    setup_entry(app, |_, restore| restore(&app.engine, value));
}

#[test]
fn a_recording_header_carries_the_tuning_and_gravity_and_a_replay_keeps_them() {
    // Recorded before the first tick, as a session starts: the manifest's value is in it.
    let (recorded_on, _dir) = tuned("solver_iterations = 7");
    let header = capture(&recorded_on);
    assert_eq!(header["tuning"]["3d"]["solver_iterations"], 7);
    assert_eq!(
        header["gravity_3d"][1].as_f64().map(f64::round),
        Some(-10.0)
    );

    // Replayed on a machine whose manifest says otherwise.
    let (mut replayed_on, _dir2) = tuned("solver_iterations = 2");
    let mut changed = header.clone();
    changed["gravity_2d"] = serde_json::json!([0.0, -3.0]);
    restore(&replayed_on, &changed);
    replayed_on.tick(balaur_core::DEFAULT_FIXED_DT);
    let state = replayed_on.engine.resource::<PhysicsState3d>();
    assert_eq!(
        state
            .borrow()
            .world
            .integration_parameters
            .num_solver_iterations,
        7
    );
    let flat = replayed_on.engine.resource::<PhysicsState2d>();
    assert!((flat.borrow().world.gravity.y + 3.0).abs() < 1e-6);
}
