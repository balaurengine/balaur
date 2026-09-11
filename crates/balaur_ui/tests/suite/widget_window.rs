//! The `window` kind: dragged by its title bar, shut by its cross.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use egui::pos2;

fn property(app: &balaur_core::App, entity: balaur_core::hecs::Entity, key: &str) -> toml::Value {
    balaur::components::get(&app.engine, entity, "widget")
        .expect("the widget component is still on the node")
        .get(key)
        .cloned()
        .unwrap_or_else(|| panic!("the widget has no `{key}`"))
}

fn number(app: &balaur_core::App, entity: balaur_core::hecs::Entity, key: &str) -> f64 {
    property(app, entity, key).as_float().unwrap_or_else(|| panic!("{key} is not a number"))
}

#[test]
fn a_window_moves_with_its_title_bar_and_shuts_on_its_cross() {
    let (_dir, mut app) = app();
    let window = add_widget(
        &app,
        &toml::toml! { kind = "window" text = "Debug" x = 100.0 y = 100.0 width = 200.0 height = 120.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);

    // Drag the title 30 right and 20 down.
    let grab = pos2(118.0, 112.0);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(grab)]);
    pass(&app, &ctx, press(grab, true));
    pass(&app, &ctx, vec![egui::Event::PointerMoved(pos2(grab.x + 15.0, grab.y + 10.0))]);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(pos2(grab.x + 30.0, grab.y + 20.0))]);
    pass(&app, &ctx, press(pos2(grab.x + 30.0, grab.y + 20.0), false));
    consume_input(&mut app);
    assert!(
        (number(&app, window, "x") - 130.0).abs() < 2.0 && (number(&app, window, "y") - 120.0).abs() < 2.0,
        "the drag moved the window with the pointer: {}, {}",
        number(&app, window, "x"),
        number(&app, window, "y")
    );

    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![]);
    let cross = drawn
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "×" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .expect("the title bar draws its cross");
    pass(&app, &ctx, press(cross, true));
    pass(&app, &ctx, press(cross, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, window, "open").as_bool(),
        Some(false),
        "the cross shuts the window"
    );
}
