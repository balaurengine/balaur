//! The controls and containers past the first nine kinds: what they write
//! back, where they put their children, and what a dialog keeps out.

mod support;

use balaur_core::hecs::Entity;
use egui::pos2;
#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use support::*;

fn property(app: &balaur_core::App, entity: Entity, key: &str) -> toml::Value {
    balaur::components::get(&app.engine, entity, "widget")
        .expect("the widget component is still on the node")
        .get(key)
        .cloned()
        .unwrap_or_else(|| panic!("the widget has no `{key}`"))
}

/// Every text shape's caption and top-left corner, for finding a child.
fn texts(out: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    out.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some((text.galley.text().to_string(), text.pos)),
            _ => None,
        })
        .collect()
}

fn root_rect(ctx: &egui::Context, entity: Entity) -> egui::Rect {
    ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
        .expect("the root drew")
}

#[test]
fn a_check_flips_on_click_and_reads_back() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "check" text = "Music" x = 0.0 y = 0.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert_eq!(
        property(&app, entity, "checked"),
        toml::Value::Boolean(false)
    );
    let target = pos2(8.0, 8.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "checked"),
        toml::Value::Boolean(true),
        "the click did not tick the box"
    );
    assert!(
        clicked(&app, entity),
        "a check reports its click like a button"
    );
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "checked"),
        toml::Value::Boolean(false)
    );
}

#[test]
fn a_slider_click_writes_where_it_landed() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "slider" x = 0.0 y = 0.0 width = 200.0 min = 0.0 max = 10.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let target = pos2(150.0, 10.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    let value = balaur_core::components::as_f64(&property(&app, entity, "value")).unwrap();
    assert!(
        value > 5.0 && value <= 10.0,
        "a click past the middle lands high: {value}"
    );
}

#[test]
fn a_dropdown_takes_the_option_that_was_clicked() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "dropdown" text = "One" options = ["One", "Two", "Three"] x = 0.0 y = 0.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let head = pos2(20.0, 10.0);
    pass(&app, &ctx, press(head, true));
    pass(&app, &ctx, press(head, false));
    let open = pass(&app, &ctx, vec![]);
    let (_, at) = texts(&open)
        .into_iter()
        .find(|(text, _)| text == "Two")
        .expect("the list opened with its options");
    let target = at + egui::vec2(6.0, 6.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "text"),
        toml::Value::String("Two".into())
    );
}

#[test]
fn a_grid_places_children_in_rows_of_columns() {
    let (_dir, app) = app();
    let grid = add_widget(
        &app,
        &toml::toml! { kind = "grid" columns = 2 gap = 0.0 x = 0.0 y = 0.0 }.into(),
    );
    let cells: Vec<Entity> = ["a", "b", "c", "d", "e"]
        .into_iter()
        .map(|label| {
            add_child_widget(
                &app,
                grid,
                label,
                &toml::toml! { kind = "label" text = label }.into(),
            )
        })
        .collect();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at: Vec<egui::Rect> = cells
        .iter()
        .map(|cell| balaur_ui::widget_rect(*cell).expect("every cell drew"))
        .collect();
    assert!(
        (at[0].min.y - at[1].min.y).abs() < 0.5,
        "a and b share a row: {at:?}"
    );
    assert!(at[1].min.x > at[0].min.x, "b is to the right of a");
    assert!(at[2].min.y > at[0].min.y, "c starts the second row");
    assert!((at[2].min.x - at[0].min.x).abs() < 0.5, "c is under a");
    assert!(at[4].min.y > at[2].min.y, "the fifth starts a third row");
    assert!(
        (at[0].size() - at[3].size()).length() < 0.5,
        "every cell is the same size"
    );
}

#[test]
fn a_flow_wraps_when_the_row_is_full() {
    let (_dir, app) = app();
    let flow = add_widget(
        &app,
        &toml::toml! { kind = "flow" width = 120.0 gap = 4.0 x = 0.0 y = 0.0 }.into(),
    );
    let buttons: Vec<Entity> = ["alpha", "beta", "gamma", "delta", "epsilon"]
        .into_iter()
        .map(|label| {
            add_child_widget(
                &app,
                flow,
                label,
                &toml::toml! { kind = "button" text = label }.into(),
            )
        })
        .collect();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at: Vec<egui::Rect> = buttons
        .iter()
        .map(|b| balaur_ui::widget_rect(*b).expect("every button drew"))
        .collect();
    let rows: std::collections::BTreeSet<i32> = at.iter().map(|r| r.min.y.round() as i32).collect();
    assert!(
        rows.len() >= 2,
        "five buttons in 120 px should wrap: {at:?}"
    );
    assert!(
        at.iter().all(|r| r.max.x <= 121.0),
        "nothing runs past the width: {at:?}"
    );
}

#[test]
fn a_fold_hides_its_children_until_its_header_is_clicked() {
    let (_dir, mut app) = app();
    let fold = add_widget(
        &app,
        &toml::toml! { kind = "fold" text = "Advanced" open = false x = 0.0 y = 0.0 }.into(),
    );
    let inner = add_child_widget(
        &app,
        fold,
        "inner",
        &toml::toml! { kind = "label" text = "hidden line" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert!(
        balaur_ui::widget_rect(inner).is_none(),
        "a closed fold drew its child"
    );
    let head = pos2(10.0, 8.0);
    pass(&app, &ctx, press(head, true));
    pass(&app, &ctx, press(head, false));
    consume_input(&mut app);
    assert_eq!(property(&app, fold, "open"), toml::Value::Boolean(true));
    settle(&app, &ctx);
    assert!(
        balaur_ui::widget_rect(inner).is_some(),
        "an open fold shows its child"
    );
}

#[test]
fn a_dialog_dims_the_screen_and_keeps_clicks_from_what_is_behind() {
    let (_dir, mut app) = app();
    let behind = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "behind" x = 0.0 y = 0.0 }.into(),
    );
    let dialog = add_widget(
        &app,
        &toml::toml! { kind = "dialog" text = "Sure?" width = 200.0 height = 100.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = root_rect(&ctx, dialog);
    assert!(
        (rect.center() - pos2(320.0, 240.0)).length() < 2.0,
        "a dialog sits in the middle of the surface: {rect:?}"
    );
    let target = pos2(10.0, 10.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    assert!(
        !clicked(&app, behind),
        "the button behind the dialog took a click"
    );
}

#[test]
fn a_fill_root_takes_the_surface_less_its_inset() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" anchor = "fill" inset = [10.0, 20.0, 30.0, 40.0] }.into(),
    );
    add_child_widget(
        &app,
        column,
        "line",
        &toml::toml! { kind = "label" text = "one line" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = root_rect(&ctx, column);
    assert!(
        (rect.min.x - 10.0).abs() < 1.0 && (rect.min.y - 20.0).abs() < 1.0,
        "{rect:?}"
    );
    assert!(
        (rect.width() - 600.0).abs() < 1.0,
        "640 less 10 and 30: {rect:?}"
    );
    assert!(
        (rect.height() - 420.0).abs() < 1.0,
        "480 less 20 and 40: {rect:?}"
    );
}

#[test]
fn a_sliced_image_keeps_its_corners_at_their_own_size() {
    let (dir, app) = app();
    let mut picture = image::RgbaImage::new(8, 8);
    for pixel in picture.pixels_mut() {
        *pixel = image::Rgba([255, 0, 0, 255]);
    }
    picture.save(dir.path().join("frame.png")).unwrap();
    let entity = add_widget(
        &app,
        &toml::toml! { kind = "image" source = "frame.png" slice = [2.0, 2.0, 2.0, 2.0] width = 100.0 height = 50.0 x = 0.0 y = 0.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let mut pieces = Vec::new();
    for shape in &out.shapes {
        if let egui::epaint::Shape::Vec(parts) = &shape.shape {
            for part in parts {
                if let egui::epaint::Shape::Mesh(mesh) = part {
                    pieces.push(mesh.calc_bounds());
                }
            }
        }
    }
    assert_eq!(pieces.len(), 9, "a nine-patch is nine quads");
    let corner = pieces
        .iter()
        .find(|r| (r.min.x).abs() < 0.5 && (r.min.y).abs() < 0.5)
        .expect("a top-left corner");
    assert!((corner.width() - 2.0).abs() < 0.5 && (corner.height() - 2.0).abs() < 0.5);
    let whole = pieces.iter().fold(egui::Rect::NOTHING, |a, b| a.union(*b));
    assert!((whole.width() - 100.0).abs() < 0.5 && (whole.height() - 50.0).abs() < 0.5);
    let _ = entity;
}

#[test]
fn a_scroll_deadzone_lets_a_short_drag_click_and_a_long_one_scroll() {
    let (_dir, mut app) = app();
    let holder = add_widget(
        &app,
        &toml::toml! { kind = "scroll" deadzone = 30.0 x = 0.0 y = 0.0 width = 200.0 height = 100.0 gap = 0.0 }
            .into(),
    );
    let first = add_child_widget(
        &app,
        holder,
        "first",
        &toml::toml! { kind = "button" text = "first" }.into(),
    );
    let mut lines = Vec::new();
    for n in 0..30 {
        let text = format!("line {n}");
        lines.push(add_child_widget(
            &app,
            holder,
            "line",
            &toml::toml! { kind = "label" text = text }.into(),
        ));
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let before = balaur_ui::widget_rect(lines[5]).unwrap().min.y;
    // A tap with a wobble inside the deadzone is a click.
    pass(&app, &ctx, press(pos2(20.0, 10.0), true));
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(22.0, 14.0))],
    );
    pass(&app, &ctx, press(pos2(22.0, 14.0), false));
    consume_input(&mut app);
    assert!(
        clicked(&app, first),
        "a short drag should still land as a click"
    );
    // A drag past it moves the contents.
    pass(&app, &ctx, press(pos2(20.0, 80.0), true));
    for step in 1..=6 {
        pass(
            &app,
            &ctx,
            vec![egui::Event::PointerMoved(pos2(
                20.0,
                80.0 - 12.0 * step as f32,
            ))],
        );
    }
    pass(&app, &ctx, vec![]);
    let after = balaur_ui::widget_rect(lines[5]).unwrap().min.y;
    pass(&app, &ctx, press(pos2(20.0, 8.0), false));
    assert!(
        after < before - 20.0,
        "the contents did not follow the finger: {before} -> {after}"
    );
}
