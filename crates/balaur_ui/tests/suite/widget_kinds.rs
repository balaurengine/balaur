//! The controls and containers past the first nine kinds: what they write
//! back, where they put their children, and what a dialog keeps out.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use balaur_core::hecs::Entity;
use egui::pos2;

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
fn a_drag_value_shows_its_number_and_takes_a_drag() {
    let (_dir, mut app) = app();
    let params =
        toml::toml! { kind = "drag_value" x = 0.0 y = 0.0 width = 90.0 value = 2.0 step = 1.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let shown = texts(&pass(&app, &ctx, vec![]));
    assert!(
        shown.iter().any(|(t, _)| t.contains('2')),
        "the number is drawn: {shown:?}"
    );
    let at = pos2(40.0, 10.0);
    pass(&app, &ctx, press(at, true));
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(at.x + 40.0, at.y))],
    );
    pass(&app, &ctx, press(pos2(at.x + 40.0, at.y), false));
    consume_input(&mut app);
    let value = balaur_core::components::as_f64(&property(&app, entity, "value")).unwrap();
    assert!(value > 2.0, "dragging right raises it: {value}");
}

#[test]
fn a_text_area_keeps_the_newlines_a_field_would_drop() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "text_area" x = 0.0 y = 0.0 width = 200.0 height = 80.0 text = "one\ntwo" };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "text"),
        toml::Value::String("one\ntwo".into()),
        "the second line survives the pass"
    );
    let shown = texts(&pass(&app, &ctx, vec![]));
    assert!(
        shown.iter().any(|(t, _)| t.contains("two")),
        "both lines are drawn: {shown:?}"
    );
}

#[test]
fn a_color_swatch_keeps_what_the_scene_gave_it() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "color" x = 0.0 y = 0.0 color = [1.0, 0.0, 0.0, 1.0] };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    consume_input(&mut app);
    let held = property(&app, entity, "color");
    let red = held
        .as_array()
        .and_then(|a| a.first())
        .and_then(balaur_core::components::as_f64)
        .unwrap();
    assert!(
        (red - 1.0).abs() < f64::EPSILON,
        "the swatch holds its own colour, not the ink: {held:?}"
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

/// The roles the face tests read.
fn face_theme(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::fs::write(
        dir.join("themes/face.toml"),
        "type = \"widget_theme\"\n\n[roles.row]\nalign = \"left\"\n\n[roles.mark]\nplate = \"#ffffff\"\n",
    )
    .unwrap();
}

/// A role's `align = "left"` reaches a node button. It reached script pills
/// only, so a node `row` drew its caption in the middle of the row.
/// A widget inside a `scroll` keeps the theme its ancestor named. A scroll
/// is solved as a tree of its own, and measuring a leaf in it resolved only
/// that leaf's own `theme`, cached the themeless look it got, and the draw
/// answered out of that cache: the editor's transport drew in the near-white
/// a widget with no theme takes, which is invisible on a light theme.
#[test]
fn a_widget_under_a_scroll_keeps_its_ancestor_s_theme() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/ink.toml"),
        "type = \"widget_theme\"\n\n[colors]\nmark = \"#5b6670\"\n\n[roles.transport]\nd = 26\ncolor = \"mark\"\n",
    )
    .unwrap();
    let icon = "\u{e1dc}";
    let root = add_widget(
        &app,
        &toml::toml! { kind = "column" theme = "themes/ink.toml" x = 0.0 y = 0.0 width = 300.0 height = 120.0 }
            .into(),
    );
    let kid = |parent, name: &str, params: toml::Value| {
        let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), name, parent);
        balaur::components::add(&app.engine, node, "widget", Some(&params)).unwrap();
        node
    };
    let scroll = kid(
        root,
        "Scroll",
        toml::toml! { kind = "scroll" grow = 1 }.into(),
    );
    let strip = kid(
        scroll,
        "Strip",
        toml::toml! { kind = "row" grow = 1 }.into(),
    );
    kid(
        strip,
        "Mark",
        toml::toml! { kind = "button" text = "" icon = (icon) role = "transport" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let ink = out
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == icon => {
                Some(text.fallback_color)
            }
            _ => None,
        })
        .expect("the glyph drew");
    assert_eq!(
        ink,
        egui::Color32::from_rgb(0x5b, 0x66, 0x70),
        "the glyph took no theme"
    );
}

#[test]
fn a_role_puts_a_button_s_face_at_its_left_edge() {
    let (dir, app) = app();
    face_theme(dir.path());
    let icon = "\u{e1dc}";
    let left = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "go" icon = (icon) width = 200.0 role = "row" theme = "themes/face.toml" x = 0.0 y = 0.0 }
            .into(),
    );
    let _middle = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "go" icon = (icon) width = 200.0 theme = "themes/face.toml" x = 0.0 y = 100.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let at = |upper: bool| {
        texts(&out)
            .into_iter()
            .find(|(t, p)| t == icon && (p.y < 50.0) == upper)
            .map(|(_, p)| p.x)
            .expect("the icon drew")
    };
    let edge = root_rect(&ctx, left).min.x;
    assert!(
        at(true) - edge < 20.0,
        "the row's face is not at its edge: {}",
        at(true)
    );
    assert!(
        at(false) > at(true) + 40.0,
        "the plain button is not centred"
    );
}

/// Trailing text sits after the caption and widens a button that hugs its
/// content: a menu's caret, or a row's shortcut against its far edge.
#[test]
fn trailing_text_widens_a_button_and_draws() {
    let (_dir, app) = app();
    let plain = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Open" x = 0.0 y = 0.0 }.into(),
    );
    let tailed = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Open" trailing = "⌘K" x = 0.0 y = 100.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let (narrow, wide) = (root_rect(&ctx, plain), root_rect(&ctx, tailed));
    assert!(
        wide.width() > narrow.width() + 10.0,
        "{narrow:?} vs {wide:?}"
    );
    let shortcut = texts(&out).into_iter().find(|(t, _)| t == "⌘K");
    let (_, pos) = shortcut.expect("the trailing text drew");
    assert!(
        pos.x > wide.center().x,
        "the trailing text is not on the far side"
    );
}

/// A button's picture draws on the disc its role asks for, and the button is
/// measured wide enough to hold it.
#[test]
fn a_picture_sits_on_its_role_s_plate() {
    let (dir, app) = app();
    face_theme(dir.path());
    let mut mark = image::RgbaImage::new(8, 8);
    for pixel in mark.pixels_mut() {
        *pixel = image::Rgba([0, 0, 0, 255]);
    }
    mark.save(dir.path().join("mark.png")).unwrap();
    let bare = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "B" theme = "themes/face.toml" x = 0.0 y = 0.0 }
            .into(),
    );
    let pictured = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "B" source = "mark.png" role = "mark" theme = "themes/face.toml" x = 0.0 y = 100.0 }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let (without, with) = (root_rect(&ctx, bare), root_rect(&ctx, pictured));
    assert!(
        with.width() > without.width() + 8.0,
        "{without:?} vs {with:?}"
    );
    let plate = out.shapes.iter().any(|s| match &s.shape {
        egui::epaint::Shape::Rect(r) => {
            r.fill == egui::Color32::WHITE && with.contains_rect(r.rect)
        }
        _ => false,
    });
    assert!(plate, "no white disc under the picture");
}

/// A `stack` lays its children over one another, each placed in the box by
/// its own `anchor`: Godot's MarginContainer, and the top and bottom bars a
/// phone game pins to the edges of one screen.
#[test]
fn a_stack_lays_its_children_over_one_another() {
    let (_dir, app) = app();
    let stack = add_widget(&app, &toml::toml! { kind = "stack" anchor = "fill" }.into());
    let child = |name: &str, anchor: &str| {
        let params = toml::toml! {
            kind = "panel" text = "" width = 100.0 height = 40.0 anchor = anchor
        };
        add_child_widget(&app, stack, name, &params.into())
    };
    let top = child("Top", "center_top");
    let bottom = child("Bottom", "center_bottom");
    let whole = child("Whole", "fill");
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity| balaur_ui::widget_rect(entity).expect("the widget was placed");
    let (top, bottom, whole) = (rect(top), rect(bottom), rect(whole));
    assert!(
        top.bottom() < bottom.top(),
        "one at each edge: {top:?} {bottom:?}"
    );
    assert!(
        (top.center().x - bottom.center().x).abs() < 1.0,
        "both centred across: {top:?} {bottom:?}"
    );
    assert!(
        whole.height() > top.height() * 4.0,
        "a child anchored `fill` takes the whole box: {whole:?}"
    );
}

/// `fit` is Godot's expand and stretch modes: a picture with one is sized by
/// the box it was given rather than sizing that box itself.
#[test]
fn a_picture_with_a_fit_takes_the_box_it_was_given() {
    let (dir, app) = app();
    let mut picture = image::RgbaImage::new(200, 40);
    for pixel in picture.pixels_mut() {
        *pixel = image::Rgba([0, 128, 255, 255]);
    }
    picture.save(dir.path().join("wide.png")).unwrap();
    let row = add_widget(
        &app,
        &toml::toml! { kind = "row" width = 400.0 height = 80.0 }.into(),
    );
    let params = toml::toml! { kind = "image" source = "wide.png" fit = "contain" width = 40.0 };
    let fitted = add_child_widget(&app, row, "Fitted", &params.into());
    let own = toml::toml! { kind = "image" source = "wide.png" };
    let native = add_child_widget(&app, row, "Native", &own.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity| balaur_ui::widget_rect(entity).expect("the widget was placed");
    let (fitted, native) = (rect(fitted), rect(native));
    assert!(
        fitted.width() < native.width(),
        "the fitted picture does not ask for the image's own width: {fitted:?} {native:?}"
    );
}

/// `padding` takes one number for every side or four for left, top, right
/// and bottom, which is what a Godot MarginContainer's margins convert to.
#[test]
fn padding_takes_one_number_or_four() {
    let (_dir, app) = app();
    let sided = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "" width = 200.0 height = 200.0 padding = [40.0, 0.0, 0.0, 0.0] }
            .into(),
    );
    let child = add_child_widget(
        &app,
        sided,
        "Inside",
        &toml::toml! { kind = "panel" text = "" height = 20.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = balaur_ui::widget_rect(child).expect("the child was placed");
    let outer = balaur_ui::widget_rect(sided).expect("the panel was placed");
    assert!(
        (rect.left() - outer.left() - 40.0).abs() < 1.0,
        "the left side pads by 40: {rect:?} in {outer:?}"
    );
    assert!(
        (rect.top() - outer.top()).abs() < 1.0,
        "and the top by nothing: {rect:?} in {outer:?}"
    );
}

/// A theme entry's `gap` spaces a container's children and its `icon_color`
/// dresses a button's picture: Godot's separations and icon colours.
#[test]
fn a_theme_spaces_a_column_and_inks_an_icon() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/game.toml"),
        "type = \"widget_theme\"\n\n[column]\ngap = 24.0\n\n[button]\nicon_color = \"#ff8800\"\n",
    )
    .unwrap();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" theme = "themes/game.toml" width = 200.0 }.into(),
    );
    let rows: Vec<_> = ["One", "Two"]
        .into_iter()
        .map(|name| {
            let params = toml::toml! { kind = "button" text = name height = 30.0 };
            add_child_widget(&app, column, name, &params.into())
        })
        .collect();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity| balaur_ui::widget_rect(entity).expect("the row was placed");
    let (first, second) = (rect(rows[0]), rect(rows[1]));
    assert!(
        (second.top() - first.bottom() - 24.0).abs() < 1.0,
        "the theme's gap is between them: {first:?} {second:?}"
    );
}

/// A button is as wide as the box the layout gave it, not as wide as its
/// caption: a row of them in a column lines up.
#[test]
fn a_button_fills_the_box_the_layout_gave_it() {
    let (_dir, app) = app();
    let column = add_widget(&app, &toml::toml! { kind = "column" width = 300.0 }.into());
    let short = add_child_widget(
        &app,
        column,
        "Short",
        &toml::toml! { kind = "button" text = "Go" }.into(),
    );
    let long = add_child_widget(
        &app,
        column,
        "Long",
        &toml::toml! { kind = "button" text = "A much longer caption" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity| balaur_ui::widget_rect(entity).expect("the button was placed");
    let (short, long) = (rect(short), rect(long));
    assert!(
        (short.width() - long.width()).abs() < 1.0,
        "both take the column's width: {short:?} {long:?}"
    );
}

/// A dialog is shut by Escape or by a click on the dim behind it, and says
/// so through `open` and `on_change`, the way a window's cross does.
#[test]
fn a_dialog_closes_on_escape_and_on_the_dim_behind_it() {
    for by_escape in [true, false] {
        let (_dir, mut app) = app();
        let dialog = add_widget(
            &app,
            &toml::toml! { kind = "dialog" text = "Sure?" width = 200.0 height = 100.0 }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let outside = pos2(10.0, 10.0);
        if by_escape {
            pass(&app, &ctx, key(egui::Key::Escape));
        } else {
            pass(&app, &ctx, press(outside, true));
            pass(&app, &ctx, press(outside, false));
        }
        consume_input(&mut app);
        assert_eq!(
            property(&app, dialog, "open"),
            toml::Value::Boolean(false),
            "by_escape = {by_escape}: the dialog is still open"
        );
        let after = pass(&app, &ctx, vec![]);
        assert!(
            !texts(&after).iter().any(|(text, _)| text == "Sure?"),
            "by_escape = {by_escape}: a shut dialog is still drawn"
        );
    }
}

/// Toasts at one anchor stack rather than cover each other, and a toast
/// leaves on its own once its `duration` is up.
#[test]
fn toasts_stack_at_their_anchor_and_leave_when_their_time_is_up() {
    let (_dir, mut app) = app();
    let one = toml::toml! {
        kind = "toast" text = "Saved" anchor = "top_right" duration = 0.2
        width = 120.0 height = 30.0
    };
    let first = add_widget(&app, &one.into());
    let two = toml::toml! {
        kind = "toast" text = "Copied" anchor = "top_right" duration = 0.2
        width = 120.0 height = 30.0
    };
    let second = add_widget(&app, &two.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let above = balaur_ui::widget_rect(first).expect("the first toast drew");
    let below = balaur_ui::widget_rect(second).expect("the second toast drew");
    assert!(
        below.min.y >= above.max.y,
        "the second toast covers the first: {above:?} against {below:?}"
    );
    // Time is the engine's, so the toast goes when the ticks say so.
    for _ in 0..20 {
        app.tick(1.0 / 60.0);
        pass(&app, &ctx, vec![]);
    }
    assert!(
        !app.engine.world().contains(first),
        "the toast outstayed its duration"
    );
    assert!(
        !app.engine.world().contains(second),
        "the second toast outstayed its duration"
    );
}

/// A toast is read, not used: a click goes through it to whatever it covers.
#[test]
fn a_toast_takes_no_click_from_what_is_under_it() {
    let (_dir, mut app) = app();
    let behind = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "behind" x = 0.0 y = 0.0 width = 200.0 height = 60.0 }
            .into(),
    );
    add_widget(
        &app,
        &toml::toml! {
            kind = "toast" text = "Saved" anchor = "top_left" duration = 0.0
            width = 200.0 height = 60.0
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = pos2(40.0, 20.0);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    consume_input(&mut app);
    assert!(
        clicked(&app, behind),
        "the toast swallowed the click meant for the button under it"
    );
}
