//! The `particles` component is a visual observer: it round-trips its
//! settings and never moves the engine's random stream.

use balaur_core::rng::{Pcg32, RngState};
use balaur_core::{components, scene, App, AppConfig};
use balaur_render::Particles;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).expect("App::new builds headless");
    balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin::default())
        .expect("the render plugin builds headless");
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

/// Seed the engine stream, tick a few frames, then read some draws — the
/// sequence an emitter must never disturb.
fn seeded_draws(with_emitter: bool) -> Vec<u32> {
    let mut app = app();
    app.engine.insert_resource(RngState(Pcg32::new(7)));
    let entity = node(&app);
    if with_emitter {
        let params: toml::Value = toml::from_str(
            "rate = 500.0\nlifetime = 0.5\nspeed = 3.0\nangle = 45.0\nspread = 180.0",
        )
        .expect("the emitter params are valid TOML");
        components::add(&app.engine, entity, "particles", Some(&params))
            .expect("a valid emitter applies");
        // Control: the component is really there while the frames tick.
        assert!(
            app.engine.world().get::<&Particles>(entity).is_ok(),
            "apply writes a Particles on the node"
        );
    }
    for _ in 0..8 {
        app.tick(1.0 / 60.0);
    }
    (0..8)
        .map(|_| balaur_core::rng::with_rng(&app.engine, Pcg32::next_u32))
        .collect()
}

#[test]
fn a_particles_component_round_trips_and_stays_out_of_the_simulation() {
    let app = app();
    let entity = node(&app);
    let params: toml::Value = toml::from_str(
        "emitting = false\nrate = 5.0\nlifetime = 2.0\nspeed = 1.5\nangle = 45.0\nspread = 10.0\nsize = 2.0\ngravity = [1.0, -2.0]",
    )
    .expect("the emitter params are valid TOML");
    components::add(&app.engine, entity, "particles", Some(&params))
        .expect("a valid emitter applies");

    let saved = components::get(&app.engine, entity, "particles").expect("the emitter reads back");
    let table = saved.as_table().expect("get returns a property table");
    assert!(!table["emitting"].as_bool().expect("emitting reads back"));
    for (key, expected) in [
        ("rate", 5.0),
        ("lifetime", 2.0),
        ("speed", 1.5),
        ("angle", 45.0),
        ("spread", 10.0),
        ("size", 2.0),
    ] {
        let got = table[key].as_float().expect("a float property reads back");
        assert!(
            (got - expected).abs() < 1e-6,
            "{key}: expected {expected}, got {got}"
        );
    }
    let gravity = table["gravity"]
        .as_array()
        .expect("gravity reads back as an array");
    assert!((gravity[0].as_float().expect("gravity x") - 1.0).abs() < 1e-6);
    assert!((gravity[1].as_float().expect("gravity y") + 2.0).abs() < 1e-6);

    // The determinism control: the engine stream with and without a live
    // emitter is one and the same sequence.
    assert_eq!(
        seeded_draws(false),
        seeded_draws(true),
        "an emitter moved the engine's rng stream"
    );
}

#[test]
fn a_burst_and_a_ramp_round_trip() {
    let app = app();
    let entity = node(&app);
    let params: toml::Value = toml::from_str(
        "one_shot = true\nexplosiveness = 0.5\nsize_end = 0.0\ntexture = \"art/spark.png\"\ncolor_end = [1.0, 0.5, 0.0, 0.0]",
    )
    .expect("the emitter params are valid TOML");
    components::add(&app.engine, entity, "particles", Some(&params)).expect("applies");
    let saved = components::get(&app.engine, entity, "particles").expect("reads back");
    let table = saved.as_table().unwrap();
    assert!(table["one_shot"].as_bool().unwrap());
    assert!((table["explosiveness"].as_float().unwrap() - 0.5).abs() < 1e-6);
    assert!(table["size_end"].as_float().unwrap().abs() < 1e-6);
    assert_eq!(table["texture"].as_str().unwrap(), "art/spark.png");
    let end = table["color_end"].as_array().unwrap();
    assert!((end[1].as_float().unwrap() - 0.5).abs() < 1e-6);
    assert!(end[3].as_float().unwrap().abs() < 1e-6);
}
