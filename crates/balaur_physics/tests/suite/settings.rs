//! `[physics]` in `project.toml`, and the override a platform reads instead.
//!
//! The table reaches the solver through the settings registry, which is the
//! same path `physics/length_unit` takes from the settings screen. These
//! prove the file still lands, and that a tag redirects it.

use balaur_core::tags::Tags;
use balaur_core::{App, AppConfig};
use balaur_physics::{PhysicsPlugin, PhysicsState};

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
    app.tick(balaur_core::FIXED_DT);
    let state = app.engine.resource::<PhysicsState>();
    let unit = state.borrow().world.integration_parameters.length_unit;
    f32::from(unit)
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
