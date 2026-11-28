//! A node's `offset`, `scale` and `rotation`, CSS's `translate`, `scale` and
//! `rotate`, and what a tween eases beside them: a colour over its fill, a
//! shadow of its own, a box that cuts what it holds. What it draws moves,
//! grows and turns; its box and what sits beside it stay.

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

/// The box `fill` is painted in by a 100 by 20 button drawn with `turned`.
fn turned_plate(turned: toml::Table) -> (egui::Rect, egui::Rect) {
    let (_dir, app) = app();
    let mut params = toml::toml! { kind = "button" text = "" x = 100.0 y = 100.0 width = 100.0 height = 20.0 fill = "#ff0000" };
    params.extend(turned);
    let bar = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let plate = painted(&out, egui::Color32::from_rgb(255, 0, 0))
        .into_iter()
        .next()
        .expect("the fill is painted");
    (plate, balaur_ui::widget_rect(bar).expect("it drew"))
}

#[test]
fn a_rotation_turns_what_is_drawn_about_its_middle_not_the_box() {
    let (plate, boxed) = turned_plate(toml::toml! { rotation = 90.0 });
    assert!(
        (boxed.width() - 100.0).abs() < 1.0,
        "the box stays: {boxed:?}"
    );
    assert!(
        (plate.width() - 20.0).abs() < 1.5 && (plate.height() - 100.0).abs() < 1.5,
        "{plate:?}"
    );
    assert!(
        (plate.center() - boxed.center()).length() < 1.0,
        "about its middle: {plate:?}"
    );
}

#[test]
fn a_turn_about_the_upright_axis_narrows_what_is_drawn() {
    let (plate, _) = turned_plate(toml::toml! { rotation_y = 60.0 });
    assert!(
        (plate.width() - 50.0).abs() < 1.5 && (plate.height() - 20.0).abs() < 1.5,
        "{plate:?}"
    );
}

/// One pass over a 40 by 40 button with a black fill and `own`.
fn plain_with(own: toml::Table) -> egui::FullOutput {
    let (_dir, app) = app();
    let mut params = toml::toml! { kind = "button" text = "" x = 100.0 y = 100.0 width = 40.0 height = 40.0 fill = "#000000" };
    params.extend(own);
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![])
}

/// Every colour a pass fills a box or a mesh with.
fn colours(out: &egui::FullOutput) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, into: &mut Vec<egui::Color32>) {
        match shape {
            egui::epaint::Shape::Rect(rect) => into.push(rect.fill),
            egui::epaint::Shape::Mesh(mesh) => into.extend(mesh.vertices.iter().map(|v| v.color)),
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|one| walk(one, into)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

#[test]
fn a_fill_over_is_laid_on_the_fill_at_its_alpha() {
    let half_red = colours(&plain_with(
        toml::toml! { fill_over = [1.0, 0.0, 0.0, 0.5] },
    ));
    assert!(
        half_red.iter().any(|c| c.r() > 100
            && c.r() < 255
            && c.g() < 10
            && c.b() < 10
            && c.a() == 255),
        "black half under red: {half_red:?}"
    );
    let none = colours(&plain_with(
        toml::toml! { fill_over = [1.0, 0.0, 0.0, 0.0] },
    ));
    assert!(
        !none.iter().any(|c| c.r() > 100 && c.g() < 10),
        "nothing laid: {none:?}"
    );
}

#[test]
fn a_widget_s_own_shadow_spreads_past_its_box() {
    let out = plain_with(
        toml::toml! { shadow = [0.0, 0.0, 0.0, 12.0] shadow_color = [1.0, 0.0, 0.0, 1.0] },
    );
    let ring = painted(&out, egui::Color32::from_rgb(255, 0, 0))
        .into_iter()
        .reduce(egui::Rect::union)
        .expect("the shadow is painted");
    assert!(ring.width() >= 40.0 + 2.0 * 12.0 - 2.0, "{ring:?}");
}

/// The clip the red panel `held` is painted under, in a 30 tall column
/// with 10 of padding that clips at its padding box.
fn clipped_red(held: toml::Table) -> egui::Rect {
    let (_dir, app) = app();
    let root = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 100.0 height = 200.0 }.into(),
    );
    let column = crate::support::add_child_widget(
        &app,
        root,
        "Fold",
        &toml::toml! { kind = "column" height = 30.0 padding = 10.0 widget_box = { overflow = "clip" } }
            .into(),
    );
    let mut tall = toml::toml! { kind = "panel" height = 100.0 fill = "#ff0000" };
    tall.extend(held);
    crate::support::add_child_widget(&app, column, "Tall", &tall.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let red = egui::Color32::from_rgb(255, 0, 0);
    let painted_red = |shape: &egui::Shape| match shape {
        egui::epaint::Shape::Rect(r) => r.fill == red,
        egui::epaint::Shape::Mesh(mesh) => mesh.vertices.iter().any(|v| v.color == red),
        _ => false,
    };
    out.shapes
        .iter()
        .find(|clipped| painted_red(&clipped.shape))
        .map(|clipped| clipped.clip_rect)
        .expect("the panel is painted")
}

// CSS's `overflow: clip` cuts at the padding box: the padding shows what
// runs into it, the edge cuts it.
#[test]
fn a_box_that_clips_cuts_what_it_holds_at_its_edge() {
    let clip = clipped_red(toml::Table::new());
    assert!(
        clip.max.y <= 30.5 && clip.max.y >= 29.5,
        "cut at the column's edge, past its padding: {clip:?}"
    );
}

// A turned card keeps its ancestors' clip where it is: it draws no further
// past the box than it would upright.
#[test]
fn a_turned_widget_keeps_the_clip_it_is_drawn_under() {
    let clip = clipped_red(toml::toml! { rotation = 30.0 });
    assert!(
        clip.max.y <= 30.5 && clip.min.y >= -0.5,
        "the column's clip, not one turned with the panel: {clip:?}"
    );
}
