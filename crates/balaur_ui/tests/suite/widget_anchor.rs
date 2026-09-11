//! Where a root goes on its surface, for the anchors that span one axis:
//! Godot's wide presets, which a HUD bar and a side panel are made of.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

fn rect(ctx: &egui::Context, entity: balaur_core::hecs::Entity) -> egui::Rect {
    ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
        .expect("the root drew, so its area has a rect")
}

/// The surface `pass` hands out is 640 by 480.
#[test]
fn a_wide_anchor_spans_one_axis_and_states_or_measures_the_other() {
    let (_dir, app) = app();
    let bar = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "top" anchor = "fill_top" height = 40.0 y = 0.0 }
            .into(),
    );
    let side = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "side" anchor = "fill_right" x = 0.0 }.into(),
    );
    let band = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "band" anchor = "fill_across" height = 60.0 y = 0.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);

    let top = rect(&ctx, bar);
    assert!(
        (top.width() - 640.0).abs() < 1.0,
        "fill_top spans the width: {top:?}"
    );
    assert!(
        (top.height() - 40.0).abs() < 1.0,
        "and keeps the height it states: {top:?}"
    );
    assert!(top.min.y.abs() < 1.0, "along the top edge: {top:?}");

    let right = rect(&ctx, side);
    assert!(
        (right.height() - 480.0).abs() < 1.0,
        "fill_right spans the height: {right:?}"
    );
    assert!(
        right.width() > 1.0 && right.width() < 320.0,
        "and measures its width, stating none: {right:?}"
    );
    assert!(
        (right.max.x - 640.0).abs() < 1.0,
        "against the right edge: {right:?}"
    );

    let middle = rect(&ctx, band);
    assert!(
        (middle.width() - 640.0).abs() < 1.0,
        "fill_across spans the width: {middle:?}"
    );
    assert!(
        (middle.center().y - 240.0).abs() < 1.0,
        "through the middle: {middle:?}"
    );
}
