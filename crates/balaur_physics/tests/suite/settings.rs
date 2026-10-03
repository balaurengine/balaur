//! `[physics]` in `project.toml`, and the override a platform reads instead.
//!
//! The table reaches the solver through the settings registry, which is the
//! same path `physics/length_unit` takes from the settings screen. These
//! prove the file still lands, and that a tag redirects it.

use balaur_core::tags::Tags;
use balaur_core::{App, AppConfig};
use balaur_physics::{PhysicsPlugin, PhysicsState3d};

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    std::fs::write(
        root.join("project.toml"),
        "[application]\nname = \"tuned\"\nmain_scene = \"scenes/main.toml\"\n\n\
         [physics]\nlength_unit = 64.0\n\n\
         [override.mobile.physics]\nlength_unit = 100.0\n",
    )
    .unwrap();
    std::fs::write(root.join("scenes/main.toml"), "").unwrap();
}

/// The tags a run holds decide which answer the solver gets. One project,
/// one tick, two machines.
fn length_unit_for(tags: Tags) -> f32 {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app.load_project().unwrap();
    app.engine.insert_resource(tags);
    app.tick(balaur_core::DEFAULT_FIXED_DT);
    let state = app.engine.resource::<PhysicsState3d>();
    state.borrow().world.integration_parameters.length_unit
}

#[test]
fn the_manifest_table_reaches_the_solver() {
    let unit = length_unit_for(Tags(vec!["desktop".into(), "linux".into()]));
    assert!((unit - 64.0).abs() < 1e-6, "length_unit read as {unit}");
}

#[test]
fn a_platform_override_outranks_the_table_it_overrides() {
    let unit = length_unit_for(Tags(vec!["mobile".into(), "android".into()]));
    assert!((unit - 100.0).abs() < 1e-6, "length_unit read as {unit}");
}

/// The contact and gravity rows: one file spelling each, read into the same
/// values `physics.tuning()` and `physics3d.gravity()` report.
#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn the_contact_and_gravity_rows_reach_both_worlds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"tuned\"\nmain_scene = \"scenes/main.toml\"\n\n\
         [physics]\nfriction_model = \"per_contact\"\ncontact_recycle_distance = 0.1\n\
         gravity_3d = [0.0, -5.0, 0.0]\ngravity_2d = [1.0, -2.0]\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app.load_project().unwrap();
    app.tick(balaur_core::DEFAULT_FIXED_DT);
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let p = &state.world.integration_parameters;
    assert_eq!(
        p.friction_model,
        balaur_physics::rapier3d::dynamics::FrictionModel::Coulomb
    );
    assert!((p.normalized_contact_recycle_distance - 0.1).abs() < 1e-6);
    assert_eq!(state.world.gravity.to_array(), [0.0, -5.0, 0.0]);
    let flat = app.engine.resource::<balaur_physics::PhysicsState2d>();
    let flat = flat.borrow();
    assert_eq!(flat.world.gravity.to_array(), [1.0, -2.0]);
    assert!(
        (flat
            .world
            .integration_parameters
            .normalized_contact_recycle_distance
            - 0.1)
            .abs()
            < 1e-6
    );
}
