//! The UI scale is egui's zoom factor and nothing else.
//!
//! Before it was, balaur multiplied its own sizes by a scale of its own while
//! egui's built-in controls took only the display's, so the two drifted apart
//! as the scale rose. These two tests are that drift, and its absence.

use egui::vec2;

use crate::support::{add_child_widget, add_widget, app, set_scale, settle};

/// The rect a widget drew at, in design pixels. A child has no `Area` of its
/// own, so this is where the layer says it put it.
fn drew_at(entity: balaur_core::hecs::Entity) -> egui::Rect {
    balaur_ui::widget_rect(entity).expect("the widget drew")
}

/// The height of a `button` and of a `drag_value`, drawn side by side with
/// neither given a size, at one scale.
fn heights(scale: f32) -> (f32, f32) {
    let (_dir, app) = app();
    let row = add_widget(&app, &toml::toml! { kind = "row" x = 0.0 y = 0.0 }.into());
    let button = add_child_widget(
        &app,
        row,
        "b",
        &toml::toml! { kind = "button" text = "ok" }.into(),
    );
    let number = add_child_widget(
        &app,
        row,
        "n",
        &toml::toml! { kind = "drag_value" value = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    set_scale(&app, &ctx, scale);
    settle(&app, &ctx);
    (drew_at(button).height(), drew_at(number).height())
}

/// egui's own control and balaur's answer to the same scale. The `drag_value`
/// is egui's `DragValue`; the `button` is drawn by this crate.
#[test]
fn a_button_and_a_drag_value_are_one_height_at_any_scale() {
    for scale in [1.0, 2.0] {
        let (button, number) = heights(scale);
        assert!(
            (button - number).abs() < 1.0,
            "at scale {scale} the button is {button} high and the drag value {number}"
        );
    }
}

/// A design pixel is a point, so a stated size is the same number at every
/// scale; what the scale changes is how many screen pixels a point costs.
#[test]
fn a_stated_size_is_the_same_points_at_every_scale_and_more_pixels() {
    let measure = |scale: f32| {
        let (_dir, app) = app();
        let panel = add_widget(
            &app,
            &toml::toml! { kind = "panel" x = 0.0 y = 0.0 width = 120.0 height = 40.0 }.into(),
        );
        let ctx = egui::Context::default();
        set_scale(&app, &ctx, scale);
        settle(&app, &ctx);
        (drew_at(panel).size(), ctx.pixels_per_point())
    };
    let (single, one_point) = measure(1.0);
    let (double, two_points) = measure(2.0);
    assert!(
        (single - vec2(120.0, 40.0)).length() < 1.0,
        "the panel drew {single} where it stated 120 by 40"
    );
    assert!(
        (single - double).length() < 1.0,
        "the scale moved the design size: {single} then {double}"
    );
    assert!(
        (two_points - one_point * 2.0).abs() < f32::EPSILON,
        "a point should cost twice the pixels: {one_point} then {two_points}"
    );
}
