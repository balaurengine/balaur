//! One layout owner: every kind is drawn in the box it measured, so a row
//! sized from the measure holds what is painted in it.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

/// Each leaf kind, as a scene writes it with nothing sizing it.
fn kinds() -> Vec<toml::Value> {
    vec![
        toml::toml! { kind = "label" text = "Hello there" }.into(),
        toml::toml! { kind = "button" text = "Press me" }.into(),
        toml::toml! { kind = "text_field" text = "typed" }.into(),
        toml::toml! { kind = "text_field" text = "a longer line than a dozen letters" widget_field = { clip_text = false } }.into(),
        toml::toml! { kind = "text_field" text = "12" widget_field = { prefix = "$", suffix = "kg" } }.into(),
        toml::toml! { kind = "text_area" text = "one\ntwo" }.into(),
        toml::toml! { kind = "checkbox" text = "Music" }.into(),
        toml::toml! { kind = "switch" }.into(),
        toml::toml! { kind = "dropdown" text = "One" widget_dropdown = { options = ["One", "Two", "Three"] } }.into(),
        toml::toml! { kind = "menu" text = "File" widget_menu = { options = ["Open", "Save"] } }.into(),
        toml::toml! { kind = "slider" widget_range = { min = 0.0, max = 10.0 } }.into(),
        toml::toml! { kind = "number_field" widget_range = { value = 2.0 } }.into(),
        toml::toml! { kind = "number_field" widget_range = { value = 11.0, min = 0.0, max = 12.0, arrows = true, prefix = "W", suffix = "px" } }.into(),
        toml::toml! { kind = "progress_bar" text = "Loading" widget_range = { value = 0.5 } }.into(),
        toml::toml! { kind = "separator" }.into(),
        toml::toml! { kind = "color_picker" }.into(),
        toml::toml! { kind = "code" text = "fn go() {\n    x = 1;\n}" }.into(),
        toml::toml! { kind = "fold" text = "Advanced" widget_fold = { open = false } }.into(),
        toml::toml! { kind = "image" widget_image = { alt_text = "no picture", image = "missing.png" } }.into(),
        toml::toml! { kind = "color_picker" widget_color_picker = { inline = true } }.into(),
        toml::toml! { kind = "list" width = 160.0 height = 120.0 widget_rows = { items = [{ label = "first" }, { label = "second" }] } }.into(),
        toml::toml! { kind = "panel" text = "Title" }.into(),
        toml::toml! { kind = "window" text = "Debug" width = 200.0 height = 120.0 }.into(),
    ]
}

#[test]
fn every_kind_draws_in_the_box_it_measured() {
    let mut wrong = Vec::new();
    for params in kinds() {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
        );
        let child = add_child_widget(&app, column, "Kind", &params);
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        pass(&app, &ctx, vec![]);
        let given = balaur_ui::widget_rect(child).expect("the kind drew").size();
        let drawn = balaur_ui::widget_drawn_size(child).expect("the kind drew");
        if (given - drawn).abs().max_elem() > 0.5 {
            wrong.push(format!("{params}: measured {given:?}, drew {drawn:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// A container with what it holds, as a scene writes it with nothing sizing
// it: its box is what it measured, children and all.
#[test]
fn every_container_draws_in_the_box_it_measured() {
    let label = || -> toml::Value { toml::toml! { kind = "label" text = "Inside" }.into() };
    let holders: Vec<(toml::Value, Vec<toml::Value>)> = vec![
        (
            toml::toml! { kind = "fold" text = "Advanced" widget_fold = { open = true } }.into(),
            vec![label()],
        ),
        (
            toml::toml! { kind = "row" }.into(),
            vec![label(), toml::toml! { kind = "button" text = "Go" }.into()],
        ),
        (
            toml::toml! { kind = "panel" text = "Title" }.into(),
            vec![label()],
        ),
        (toml::toml! { kind = "tabs" }.into(), vec![label(), label()]),
        (toml::toml! { kind = "stack" }.into(), vec![label()]),
        (
            toml::toml! { kind = "grid" }.into(),
            vec![label(), label(), label()],
        ),
        (
            toml::toml! { kind = "row" widget_layout = { wrap_children = "wrap" } }.into(),
            vec![label(), label()],
        ),
        (
            toml::toml! { kind = "window" text = "Debug" }.into(),
            vec![label()],
        ),
        (
            toml::toml! { kind = "scroll" width = 120.0 height = 40.0 }.into(),
            vec![label(), label(), label()],
        ),
        (
            toml::toml! { kind = "menu" text = "File" }.into(),
            vec![toml::toml! { kind = "button" text = "Open" }.into()],
        ),
        (
            toml::toml! { kind = "panel" text = "A title wider than what it holds" }.into(),
            vec![label()],
        ),
    ];
    let mut wrong = Vec::new();
    for (params, children) in holders {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
        );
        let holder = add_child_widget(&app, column, "Holder", &params);
        for (slot, child) in children.iter().enumerate() {
            add_child_widget(&app, holder, &format!("Child{slot}"), child);
        }
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        pass(&app, &ctx, vec![]);
        let given = balaur_ui::widget_rect(holder).map(|rect| rect.size());
        let drawn = balaur_ui::widget_drawn_size(holder);
        let agree = given
            .zip(drawn)
            .is_some_and(|(given, drawn)| (given - drawn).abs().max_elem() <= 0.5);
        if !agree {
            wrong.push(format!("{params}: measured {given:?}, drew {drawn:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// A button's picture is as big as its icon would be: a stated `icon_size`
// sizes a brand's mark as it sizes a glyph, else the caption's height.
#[test]
fn a_button_s_picture_takes_its_icon_s_size() {
    let (dir, app) = app();
    let mut mark = image::RgbaImage::new(8, 8);
    for pixel in mark.pixels_mut() {
        *pixel = image::Rgba([0, 0, 0, 255]);
    }
    mark.save(dir.path().join("mark.png")).unwrap();
    let caption = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "B" x = 0.0 y = 0.0 widget_button = { image = "mark.png" } }
            .into(),
    );
    let sized = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "B" x = 0.0 y = 100.0 widget_button = { image = "mark.png", icon_size = 40.0 } }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    let (small, big) = (root_rect(&ctx, caption), root_rect(&ctx, sized));
    assert!(big.height() >= 40.0, "{big:?}");
    assert!(big.width() >= small.width() + 20.0, "{small:?} vs {big:?}");
}

/// A theme beside the project, `themes/roles.toml`, holding `roles`.
fn theme_in(dir: &std::path::Path, roles: &str) {
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::fs::write(
        dir.join("themes/roles.toml"),
        format!("type = \"widget_theme\"\n\n{roles}"),
    )
    .unwrap();
}

#[test]
fn a_role_holds_a_box_between_its_least_and_greatest_size() {
    let (dir, app) = app();
    theme_in(
        dir.path(),
        "[roles.calendar]\nmin_width = 300\nmax_width = 384\n\n[roles.clue]\nmin_height = 44\n",
    );
    let sheet = |width: f64| {
        add_widget(
            &app,
            &toml::toml! { kind = "column" theme = "themes/roles.toml" x = 0.0 y = 0.0 width = (width) height = 400.0 }
                .into(),
        )
    };
    let (wide, narrow) = (sheet(1000.0), sheet(200.0));
    let calendar = |parent| {
        add_child_widget(
            &app,
            parent,
            "Calendar",
            &toml::toml! { kind = "row" role = "calendar" height = 20.0 }.into(),
        )
    };
    let (capped, floored) = (calendar(wide), calendar(narrow));
    let clue = add_child_widget(
        &app,
        wide,
        "Clue",
        &toml::toml! { kind = "row" role = "clue" }.into(),
    );
    add_child_widget(
        &app,
        clue,
        "Word",
        &toml::toml! { kind = "label" text = "Hi" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity| balaur_ui::widget_rect(entity).expect("it drew");
    assert!(
        (rect(capped).width() - 384.0).abs() < 1.0,
        "{:?}",
        rect(capped)
    );
    assert!(
        (rect(floored).width() - 300.0).abs() < 1.0,
        "{:?}",
        rect(floored)
    );
    assert!((rect(clue).height() - 44.0).abs() < 1.0, "{:?}", rect(clue));
}

#[test]
fn a_checked_role_s_weight_sizes_the_widget_it_draws() {
    let (dir, app) = app_with_editor_fonts();
    theme_in(dir.path(), "[roles.pick.checked]\nfont_weight = 700\n");
    let pick = |checked: bool| {
        add_widget(
            &app,
            &toml::toml! { kind = "button" theme = "themes/roles.toml" role = "pick" text = "Snap every node to the grid" x = 0.0 y = 0.0 widget_check = { checked = (checked) } }
                .into(),
        )
    };
    let (rest, picked) = (pick(false), pick(true));
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let wide = |entity| balaur_ui::widget_rect(entity).expect("it drew").width();
    assert!(
        wide(picked) > wide(rest) + 2.0,
        "the checked weight did not size the button: {} against {}",
        wide(picked),
        wide(rest)
    );
}

/// Whether `out` strokes anything in `colour`.
fn stroked_in(out: &egui::FullOutput, colour: egui::Color32) -> bool {
    out.shapes.iter().any(|clipped| match &clipped.shape {
        egui::epaint::Shape::Rect(rect) => rect.stroke.color == colour && rect.stroke.width > 0.0,
        _ => false,
    })
}

/// One pass over a `pick` button, checked or not, under a theme whose
/// checked state names a red outline.
fn picked_pass(checked: bool) -> egui::FullOutput {
    let (dir, app) = app();
    theme_in(
        dir.path(),
        "[roles.pick.checked]\noutline = \"#ff0000\"\noutline_width = 2\n",
    );
    add_widget(
        &app,
        &toml::toml! { kind = "button" theme = "themes/roles.toml" role = "pick" text = "Pick" x = 0.0 y = 0.0 widget_check = { checked = (checked) } }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![])
}

#[test]
fn a_checked_role_s_outline_rings_the_checked_widget_alone() {
    let red = egui::Color32::from_rgb(255, 0, 0);
    assert!(
        stroked_in(&picked_pass(true), red),
        "the checked widget is ringed"
    );
    assert!(
        !stroked_in(&picked_pass(false), red),
        "an unchecked one is not"
    );
}

// A role's `icon_size = "50%"` draws the glyph at half the width inside the
// padding, as CSS's `w-1/2` sizes a tile's icon: a tile twice as wide draws
// it twice as big, and the tile is as tall as the glyph it holds.
#[test]
fn an_icon_size_as_a_share_follows_the_width_it_is_given() {
    let (dir, app) = app_with_editor_fonts();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/tile.toml"),
        "type = \"widget_theme\"\n\n[fonts]\nicon = \"fonts/Phosphor.ttf\"\n\n[roles.tile]\nicon_size = \"50%\"\npadding = 10\n",
    )
    .unwrap();
    let icon = "\u{e1dc}";
    let tile = |(x, y): (f32, f32), width: f32, parts: toml::Table| {
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" theme = "themes/tile.toml" x = x y = y width = width widget_layout = { align_items = "stretch" } }
                .into(),
        );
        crate::support::add_child_widget(
            &app,
            column,
            "Tile",
            &{
                let mut button = toml::toml! { kind = "button" role = "tile" text = "" };
                button.extend(parts);
                button
            }
            .into(),
        )
    };
    let narrow = tile(
        (0.0, 0.0),
        100.0,
        toml::toml! { widget_button = { icon = "cursor" } },
    );
    let wide = tile(
        (0.0, 200.0),
        200.0,
        toml::toml! { widget_button = { icon = "cursor" } },
    );
    // A trailing glyph takes the share too, and the box is as tall.
    let trailing = tile(
        (300.0, 0.0),
        200.0,
        toml::toml! { text = "go" widget_button = { trailing_icon = "cursor" } },
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let mut sizes: Vec<f32> = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == icon => {
                Some(text.galley.job.sections[0].format.font_id.size)
            }
            _ => None,
        })
        .collect();
    sizes.sort_by(f32::total_cmp);
    assert_eq!(sizes.len(), 3, "every glyph drew: {sizes:?}");
    assert!(
        (sizes[0] - 40.0).abs() < 0.5
            && (sizes[1] - 90.0).abs() < 0.5
            && (sizes[2] - 90.0).abs() < 0.5,
        "half of 80 and of 180: {sizes:?}"
    );
    let boxed = |node| balaur_ui::widget_rect(node).expect("it drew");
    assert!(
        boxed(narrow).height() >= 40.0 + 20.0 - 0.5
            && boxed(wide).height() >= 90.0 + 20.0 - 0.5
            && boxed(trailing).height() >= 90.0 + 20.0 - 0.5,
        "as tall as the glyph and its padding: {:?} {:?}",
        boxed(narrow),
        boxed(wide)
    );
}
