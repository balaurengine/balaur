//! What the pointer meets over a widget: the shape its `cursor` word asks for.

use egui::pos2;

use crate::support::{add_widget, app, pass, settle};

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
