//! What the pointer meets over a widget: the shape its `cursor` word asks for.

use egui::pos2;

use crate::support::{add_widget, app, consume_input, focused, pass, press, press_with, settle};

/// A widget's `cursor` word reaches egui's platform output while the pointer
/// is over it, and a word past the first seventeen reaches it too.
#[test]
fn a_widget_s_cursor_word_shapes_the_pointer_over_it() {
    let (_dir, app) = app();
    add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "" x = 0.0 y = 0.0 width = 100.0 height = 100.0 cursor = "resize_row" }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let inside = pos2(50.0, 50.0);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(inside)]);
    let over = pass(&app, &ctx, vec![egui::Event::PointerMoved(inside)]);
    assert_eq!(
        over.platform_output.cursor_icon,
        egui::CursorIcon::ResizeRow
    );
    let away = pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(300.0, 300.0))],
    );
    assert_eq!(away.platform_output.cursor_icon, egui::CursorIcon::Default);
}

/// What the widget's node heard under `event` on the last tick.
fn heard(
    app: &balaur::App,
    entity: balaur_core::hecs::Entity,
    event: &str,
) -> Vec<balaur_script::Value> {
    balaur_core::events::delivered_from(&app.engine, entity, event)
}

/// Any widget hears the pointer arrive, press and release a button over it
/// by name, and leave, as a world node does.
#[test]
fn a_widget_hears_the_pointer_arrive_press_and_leave() {
    let (_dir, mut app) = app();
    let button = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Go" x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let over = balaur_ui::widget_rect(button)
        .expect("the button drew")
        .center();
    pass(&app, &ctx, vec![egui::Event::PointerMoved(over)]);
    consume_input(&mut app);
    assert_eq!(heard(&app, button, "pointer_enter").len(), 1, "no enter");
    pass(
        &app,
        &ctx,
        press_with(over, egui::PointerButton::Secondary, true),
    );
    consume_input(&mut app);
    assert_eq!(
        heard(&app, button, "pointer_down"),
        vec![balaur_script::Value::Str("right".into())]
    );
    pass(
        &app,
        &ctx,
        press_with(over, egui::PointerButton::Secondary, false),
    );
    consume_input(&mut app);
    assert_eq!(
        heard(&app, button, "pointer_up"),
        vec![balaur_script::Value::Str("right".into())]
    );
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(
            over.x + 400.0,
            over.y + 400.0,
        ))],
    );
    consume_input(&mut app);
    assert_eq!(heard(&app, button, "pointer_exit").len(), 1, "no exit");
}

/// A click into a field is focus arriving, and a click away from it a blur.
#[test]
fn a_field_hears_focus_arrive_and_leave() {
    let (_dir, mut app) = app();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "" width = 200.0 x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let inside = balaur_ui::widget_rect(field)
        .expect("the field drew")
        .center();
    pass(&app, &ctx, press(inside, true));
    pass(&app, &ctx, press(inside, false));
    consume_input(&mut app);
    assert_eq!(
        focused(&app),
        Some(field),
        "the click did not focus the field"
    );
    assert_eq!(heard(&app, field, "focus").len(), 1, "no focus event");
    let away = egui::pos2(inside.x + 400.0, inside.y + 400.0);
    pass(&app, &ctx, press(away, true));
    pass(&app, &ctx, press(away, false));
    consume_input(&mut app);
    assert_eq!(heard(&app, field, "blur").len(), 1, "no blur event");
    assert_eq!(focused(&app), None, "focus stayed on the field");
}
