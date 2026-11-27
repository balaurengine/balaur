//! A node's `offset` and `scale`, CSS's `translate` and `scale`: what it
//! draws moves and grows, its box and what sits beside it stay.

use crate::support::{add_widget, app, glyphs, painted, pass, settle};

// A node's `offset` draws it and what it holds that far from its place, as
// CSS's `translate`; its box, and what sits beside it, stay.
#[test]
fn an_offset_moves_what_is_drawn_not_the_box() {
    let (_dir, app) = app();
    let moved = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Bar" x = 0.0 y = 0.0 width = 100.0 height = 40.0 fill = "#ff0000" offset = [0.0, 30.0] }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let boxed = balaur_ui::widget_rect(moved).expect("it drew");
    let red = egui::Color32::from_rgb(255, 0, 0);
    let painted = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) if r.fill == red => Some(r.rect),
            _ => None,
        })
        .expect("the fill is painted");
    assert!((boxed.min.y - 0.0).abs() < 1.0, "the box stays: {boxed:?}");
    assert!(
        (painted.min.y - 30.0).abs() < 1.0,
        "the fill is drawn 30 down: {painted:?}"
    );
}

// A node's `scale` draws it that many times its size about its middle, as
// CSS's `scale`; its box stays.
#[test]
fn a_scale_draws_bigger_about_the_middle_not_the_box() {
    let (_dir, app) = app();
    let popped = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "7" x = 100.0 y = 100.0 width = 40.0 height = 40.0 fill = "#ff0000" scale = [1.5, 1.5] }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let boxed = balaur_ui::widget_rect(popped).expect("it drew");
    let red = egui::Color32::from_rgb(255, 0, 0);
    let painted = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) if r.fill == red => Some(r.rect),
            _ => None,
        })
        .expect("the fill is painted");
    assert!(
        (boxed.width() - 40.0).abs() < 1.0,
        "the box stays: {boxed:?}"
    );
    assert!(
        (painted.width() - 60.0).abs() < 1.0,
        "drawn half again as big: {painted:?}"
    );
    assert!(
        (painted.center() - boxed.center()).length() < 1.0,
        "about its middle: {painted:?} in {boxed:?}"
    );
}

// A `scale` of `[0.5, 1]` draws the widget half as wide about its middle
// and as tall as its box, its text with it, as CSS's `scale: 0.5 1`.
#[test]
fn a_scale_per_axis_draws_half_as_wide_and_as_tall() {
    let (_dir, app) = app();
    let red = egui::Color32::from_rgb(255, 0, 0);
    let green = egui::Color32::from_rgb(0, 255, 0);
    let whole = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "7" x = 100.0 y = 100.0 width = 40.0 height = 40.0 fill = "#ff0000" }
            .into(),
    );
    let narrow = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "7" x = 200.0 y = 100.0 width = 40.0 height = 40.0 fill = "#00ff00" scale = [0.5, 1.0] }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let boxed = balaur_ui::widget_rect(narrow).expect("it drew");
    let wide = balaur_ui::widget_rect(whole).expect("it drew");
    let plate = painted(&out, green)
        .into_iter()
        .next()
        .expect("the fill is painted");
    let unscaled = painted(&out, red)
        .into_iter()
        .next()
        .expect("the other fill is painted");
    assert!(
        (boxed.width() - 40.0).abs() < 1.0,
        "the box stays: {boxed:?}"
    );
    assert!(
        (plate.width() - unscaled.width() * 0.5).abs() < 1.0,
        "drawn half as wide: {plate:?} beside {unscaled:?}"
    );
    assert!(
        (plate.height() - unscaled.height()).abs() < 1.5,
        "and as tall: {plate:?} beside {unscaled:?}"
    );
    assert!(
        (plate.center() - boxed.center()).length() < 1.0,
        "about its middle: {plate:?} in {boxed:?}"
    );
    let mut letters = glyphs(&out);
    letters.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x));
    let &[(seven, _), (squeezed, _)] = letters.as_slice() else {
        panic!("a letter on each button: {letters:?}");
    };
    assert!(
        (squeezed.width() - seven.width() * 0.5).abs() < 0.5,
        "its text half as wide: {squeezed:?} beside {seven:?}"
    );
    assert!(
        (squeezed.height() - seven.height()).abs() < 0.5,
        "and as tall: {squeezed:?} beside {seven:?}"
    );
    let off = |letter: egui::Rect, at: egui::Rect| letter.center() - at.center();
    assert!(
        (off(squeezed, boxed) - off(seven, wide) * egui::vec2(0.5, 1.0)).length() < 0.5,
        "and drawn where its middle took it: {squeezed:?} in {boxed:?}"
    );
}
