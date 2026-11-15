//! The small controls: a spinner, the radio a grouped checkbox is, and a
//! switch with its words.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

/// Every disc a pass painted, nested shapes included: centre and radius.
fn discs(out: &egui::FullOutput) -> Vec<(egui::Pos2, f32)> {
    fn walk(shape: &egui::Shape, into: &mut Vec<(egui::Pos2, f32)>) {
        match shape {
            egui::epaint::Shape::Circle(c) => into.push((c.center, c.radius)),
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|one| walk(one, into)),
            _ => {}
        }
    }
    let mut painted = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut painted);
    }
    painted
}

// A spinner is as big as its role says and turns on the engine's clock under
// a fixed step: the same frame twice is the same picture, a later one is not.
#[test]
fn a_spinner_turns_with_the_engine_clock_at_its_role_s_size() {
    let (dir, mut app) = app();
    app.set_fixed_dt(Some(1.0 / 60.0));
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/spin.toml"),
        "type = \"widget_theme\"\n\n[roles.small]\nheight = 16\n",
    )
    .unwrap();
    let plain = add_widget(
        &app,
        &toml::toml! { kind = "spinner" theme = "themes/spin.toml" x = 20.0 y = 20.0 }.into(),
    );
    let small = add_widget(
        &app,
        &toml::toml! { kind = "spinner" role = "small" theme = "themes/spin.toml" x = 80.0 y = 20.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let side = |node| balaur_ui::widget_rect(node).expect("it drew").size();
    assert_eq!(side(plain), egui::vec2(24.0, 24.0), "daisyUI's loading-md");
    assert_eq!(side(small), egui::vec2(16.0, 16.0));
    let ends = |out: &egui::FullOutput| {
        let mut ends = discs(out);
        ends.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
        ends
    };
    let first = ends(&pass(&app, &ctx, vec![]));
    assert_eq!(first.len(), 4, "two round ends each: {first:?}");
    assert_eq!(
        first,
        ends(&pass(&app, &ctx, vec![])),
        "the engine has not stepped"
    );
    app.tick(0.2);
    assert_ne!(first, ends(&pass(&app, &ctx, vec![])), "it turned");
}

// A checkbox in a radio group is a circle, and on, a dot inside a ring.
#[test]
fn a_checkbox_in_a_radio_group_is_round() {
    let (_dir, app) = app();
    let on = add_widget(
        &app,
        &toml::toml! { kind = "checkbox" text = "Small" x = 20.0 y = 20.0 widget_check = { radio_group = "size", checked = true } }
            .into(),
    );
    add_widget(
        &app,
        &toml::toml! { kind = "checkbox" text = "Ticked" x = 20.0 y = 80.0 widget_check = { checked = true } }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let row = balaur_ui::widget_rect(on).expect("it drew");
    let inside = discs(&out)
        .into_iter()
        .filter(|(centre, _)| row.contains(*centre))
        .count();
    assert_eq!(inside, 2, "a ring and a dot on the radio");
    assert!(
        discs(&out).iter().all(|(centre, _)| centre.y < 60.0),
        "and none on a plain ticked box"
    );
}

// A switch draws its caption after its track, as a checkbox does, and is
// measured with it.
#[test]
fn a_switch_draws_its_caption_after_its_track() {
    let (_dir, app) = app();
    let bare = add_widget(
        &app,
        &toml::toml! { kind = "switch" x = 20.0 y = 20.0 }.into(),
    );
    let worded = add_widget(
        &app,
        &toml::toml! { kind = "switch" text = "Sounds" x = 20.0 y = 80.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let width = |node| balaur_ui::widget_rect(node).expect("it drew").width();
    assert!(
        width(worded) > width(bare) + 30.0,
        "{} beside {}",
        width(worded),
        width(bare)
    );
    let (_, at) = texts(&out)
        .into_iter()
        .find(|(text, _)| text == "Sounds")
        .expect("the caption is drawn");
    assert!(at.x > balaur_ui::widget_rect(bare).expect("it drew").right() - 1.0);
}

// An icon is named in the theme's icon font: a button's, a row's and a
// mark's draw the glyph the font calls that, and a name the font has no
// glyph for draws nothing.
#[test]
fn icons_are_named_in_the_theme_s_icon_font() {
    let (dir, app) = app_with_editor_fonts();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/icons.toml"),
        "type = \"widget_theme\"\n\n[fonts]\nicon = \"fonts/Phosphor-Fill.ttf\"\n",
    )
    .unwrap();
    add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Add" theme = "themes/icons.toml" x = 20.0 y = 20.0 widget_button = { icon = "plus" } }
            .into(),
    );
    add_widget(
        &app,
        &toml::toml! {
            kind = "list" theme = "themes/icons.toml" x = 20.0 y = 80.0 width = 200.0 height = 60.0
            widget_rows = { row_height = 20.0, items = [{ label = "Assets", key = "a", icon = "folder", marks = [{ name = "lock", icon = "lock" }] }] }
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn: Vec<String> = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    for (name, glyph) in [
        ("plus", "\u{E3D4}"),
        ("folder", "\u{E24A}"),
        ("lock", "\u{E2FA}"),
    ] {
        assert!(
            drawn.iter().any(|text| text == glyph),
            "{name} did not draw: {drawn:?}"
        );
    }
}
