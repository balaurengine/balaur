//! A `screen_notifier2d` says when its box comes on screen and when it
//! leaves, measured against the project's window in a run with none.

use balaur_core::glamx::Vec3;
use balaur_core::{App, AppConfig, Transform, components, scene};
use balaur_render::RenderPlugin;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    app
}

fn move_to(app: &App, entity: balaur_core::hecs::Entity, x: f32) {
    app.engine
        .world_mut()
        .get::<&mut Transform>(entity)
        .unwrap()
        .position = Vec3::new(x, 0.0, 0.0);
}

/// The notifier events the node heard over two frames: one to notice, one
/// for the pump to hand it over.
fn heard(app: &mut App, entity: balaur_core::hecs::Entity) -> Vec<&'static str> {
    let mut out = Vec::new();
    for _ in 0..2 {
        app.tick(1.0 / 60.0);
        for event in ["screen_enter", "screen_exit"] {
            if !balaur_core::events::delivered_from(&app.engine, entity, event).is_empty() {
                out.push(event);
            }
        }
    }
    out
}

#[test]
fn a_notifier_says_when_its_box_comes_on_screen_and_when_it_leaves() {
    let mut app = app();
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "Coin", root);
    move_to(&app, node, 1000.0);
    let params: toml::Value = toml::from_str("size = [1.0, 1.0]").unwrap();
    components::add(&app.engine, node, "screen_notifier2d", Some(&params)).unwrap();
    assert_eq!(
        heard(&mut app, node),
        Vec::<&str>::new(),
        "far off to the right"
    );
    move_to(&app, node, 0.0);
    assert_eq!(heard(&mut app, node), vec!["screen_enter"]);
    assert_eq!(
        heard(&mut app, node),
        Vec::<&str>::new(),
        "once, not every frame"
    );
    move_to(&app, node, -1000.0);
    assert_eq!(heard(&mut app, node), vec!["screen_exit"]);
}
