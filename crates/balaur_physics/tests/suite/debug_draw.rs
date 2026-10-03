//! The physics debug draw: which modes it takes, which nodes it is limited
//! to, and the `[physics.debug]` colours it draws in.

use balaur_core::App;
use balaur_core::debug_lines::DebugLineBuffer3d;
use balaur_core::scene::{self, Transform};
use balaur_physics::PhysicsPlugin;
use balaur_physics::debug::PhysicsDebugConfig;
use balaur_physics::rapier3d::pipeline::DebugRenderMode;

/// Two boxes, `Near` and `Far`, drawn with `config`; answers the colours of
/// the lines one frame drew, and how many lie near x = 5.
fn draw(config: PhysicsDebugConfig, settings: &[(&str, toml::Value)]) -> (Vec<[f32; 4]>, usize) {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app.engine.insert_resource(DebugLineBuffer3d::default());
    let root = app.engine.root();
    let mut nodes = Vec::new();
    for (name, x) in [("Near", 0.0), ("Far", 5.0)] {
        let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
        app.engine
            .world()
            .get::<&mut Transform>(e)
            .unwrap()
            .position = [x, 0.0, 0.0].into();
        balaur_core::components::add(
            &app.engine,
            e,
            "body3d",
            Some(&toml::from_str("kind = \"static\"").unwrap()),
        )
        .unwrap();
        balaur_core::components::add(
            &app.engine,
            e,
            "collider3d",
            Some(&toml::from_str("kind = \"box\"").unwrap()),
        )
        .unwrap();
        nodes.push(e.to_bits().get());
    }
    for (path, value) in settings {
        balaur_core::settings::set(&app.engine, path, value.clone());
    }
    let mut config = config;
    if config.nodes == [0] {
        config.nodes = vec![nodes[0]];
    }
    *app.engine.resource::<PhysicsDebugConfig>().borrow_mut() = config;
    app.tick(1.0 / 60.0);
    let buffer = app.engine.resource::<DebugLineBuffer3d>();
    let lines = &buffer.borrow().lines;
    let far = lines
        .iter()
        .filter(|(a, b, ..)| a[0] > 4.0 && b[0] > 4.0)
        .count();
    (lines.iter().map(|line| line.2).collect(), far)
}

fn shapes() -> PhysicsDebugConfig {
    PhysicsDebugConfig {
        enabled: true,
        mode: DebugRenderMode::COLLIDER_SHAPES,
        nodes: Vec::new(),
    }
}

#[test]
fn a_debug_draw_limited_to_some_nodes_draws_only_theirs() {
    let (all, far) = draw(shapes(), &[]);
    assert!(
        !all.is_empty() && far > 0,
        "the two boxes drew {} lines, {far} far",
        all.len()
    );
    let limited = PhysicsDebugConfig {
        nodes: vec![0],
        ..shapes()
    };
    let (some, far) = draw(limited, &[]);
    assert!(!some.is_empty(), "the listed node drew nothing");
    assert_eq!(far, 0, "a node left off the list was drawn");
}

#[test]
fn the_static_colour_comes_from_physics_debug() {
    let red = toml::Value::Array(vec![1.0.into(), 0.0.into(), 0.0.into(), 1.0.into()]);
    let (colors, _) = draw(shapes(), &[("physics/debug/static_color", red)]);
    assert!(!colors.is_empty(), "nothing was drawn");
    for [r, g, b, a] in colors {
        assert!(
            (r - 1.0).abs() < 1e-3 && g.abs() < 1e-3 && b.abs() < 1e-3 && (a - 1.0).abs() < 1e-3,
            "a static box drew in {r} {g} {b} {a}"
        );
    }
}

#[test]
fn a_debug_colour_keeps_its_alpha() {
    let faint = toml::Value::Array(vec![1.0.into(), 0.0.into(), 0.0.into(), 0.25.into()]);
    let (colors, _) = draw(shapes(), &[("physics/debug/static_color", faint)]);
    assert!(!colors.is_empty(), "nothing was drawn");
    for [.., a] in colors {
        assert!((a - 0.25).abs() < 1e-3, "a static box drew at alpha {a}");
    }
}
