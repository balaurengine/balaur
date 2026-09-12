//! The controls and containers past the first nine kinds: what they write
//! back, where they put their children, and what a dialog keeps out.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use balaur_core::hecs::Entity;
use egui::pos2;

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
fn a_list_draws_only_the_rows_that_fit() {
    let (_dir, app) = app();
    let many: Vec<String> = (0..2000).map(|i| format!("Row{i}")).collect();
    let params =
        toml::toml! { kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 120.0 options = (many) };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let rows = drawn.iter().filter(|(t, _)| t.starts_with("Row")).count();
    assert!(
        rows > 0 && rows < 60,
        "2000 items, only a screenful built: {rows}"
    );
    assert!(
        drawn.iter().any(|(t, _)| t == "Row0"),
        "and it starts at the top: {drawn:?}"
    );
}

#[test]
fn a_tree_indents_a_row_by_its_leading_tabs() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 120.0
        options = ["Root", "\tChild", "\t\tLeaf"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let at = |name: &str| drawn.iter().find(|(t, _)| t == name).map(|(_, p)| p.x);
    let (root, child, leaf) = (
        at("Root").expect("root drawn"),
        at("Child").expect("child drawn"),
        at("Leaf").expect("leaf drawn"),
    );
    assert!(
        child > root && leaf > child,
        "each level steps right: {root} {child} {leaf}"
    );
}

#[test]
fn a_list_row_splits_into_icon_label_and_note() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 240.0 height = 120.0
        options = ["*\u{1f}Named\u{1f}12 KB\u{1f}#ff0000", "Plain"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn: Vec<String> = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    for want in ["*", "Named", "12 KB", "Plain"] {
        assert!(drawn.iter().any(|t| t == want), "{want} drawn: {drawn:?}");
    }
}

#[test]
fn picking_a_row_leaves_the_rows_under_it_where_they_were() {
    let ctx = egui::Context::default();
    // The theme a shell brings: a resting stroke is what egui takes out of a
    // button's margin and, on the hovered or picked one, puts back.
    ctx.all_styles_mut(|style| {
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::GRAY);
    });
    let rows = |chosen: &str| {
        let (_dir, app) = app();
        let params = toml::toml! {
            kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 200.0
            text = chosen
            options = ["One", "Two", "Three", "Four", "Five", "Six"]
        };
        add_widget(&app, &params.into());
        settle(&app, &ctx);
        texts(&pass(&app, &ctx, vec![]))
    };
    let resting = rows("");
    assert!(
        resting.iter().any(|(t, _)| t == "Six"),
        "every row drew: {resting:?}"
    );
    assert_eq!(
        resting,
        rows("Two"),
        "a picked row must not move itself or the rows under it"
    );
}

#[test]
fn a_tree_caret_folds_the_branch_under_it() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 200.0
        options = ["Root", "\tChild", "\t\tLeaf", "After"]
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let shown = |out: &egui::FullOutput| {
        texts(out)
            .into_iter()
            .map(|(t, _)| t)
            .filter(|t| t != "▾" && t != "▸")
            .collect::<Vec<_>>()
    };
    let open = shown(&pass(&app, &ctx, vec![]));
    assert!(
        open.iter().any(|t| t == "Leaf"),
        "the branch starts open: {open:?}"
    );
    // The caret sits left of the row it folds.
    let (_, at) = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .find(|(t, _)| t == "▾")
        .expect("a parent row draws a caret");
    let hit = pos2(at.x + 3.0, at.y + 3.0);
    pass(&app, &ctx, press(hit, true));
    pass(&app, &ctx, press(hit, false));
    let folded = shown(&pass(&app, &ctx, vec![]));
    assert!(
        !folded.iter().any(|t| t == "Child") && folded.iter().any(|t| t == "After"),
        "the branch folds and its sibling stays: {folded:?}"
    );
}

#[test]
fn a_menu_reports_the_item_that_was_picked() {
    let (_dir, mut app) = app();
    let params =
        toml::toml! { kind = "menu" text = "File" options = ["Open", "Save"] x = 0.0 y = 0.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let head = pos2(20.0, 10.0);
    pass(&app, &ctx, press(head, true));
    pass(&app, &ctx, press(head, false));
    let open = pass(&app, &ctx, vec![]);
    let (_, at) = texts(&open)
        .into_iter()
        .find(|(t, _)| t == "Save")
        .expect("the list is open and holds its items");
    let item = pos2(at.x + 4.0, at.y + 4.0);
    pass(&app, &ctx, press(item, true));
    pass(&app, &ctx, press(item, false));
    consume_input(&mut app);
    assert_eq!(
        property(&app, entity, "text"),
        toml::Value::String("Save".into()),
        "the pick lands on the widget"
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

/// A menu's rows can be nodes: an icon, a shortcut and a tick are widgets
/// like any other, which a flat list of strings cannot carry.
#[test]
fn a_menu_opens_its_child_rows() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "menu" text = "Menu" x = 10.0 y = 10.0 };
    let host = add_widget(&app, &params.into());
    let mut rows = Vec::new();
    for name in ["R0", "R1"] {
        let row = toml::toml! { kind = "button" text = "row" width = 173.0 height = 22.0 };
        rows.push(add_child_widget(&app, host, name, &row.into()));
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    // Counted by the boxes they take: a node caption shapes through the text
    // renderer, not an egui galley, so it is not a Text shape to find.
    let boxes = |out: &egui::FullOutput| {
        out.shapes
            .iter()
            .filter(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => (r.rect.width() - 173.0).abs() < 1.0,
                _ => false,
            })
            .map(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => r.rect,
                _ => unreachable!(),
            })
            .fold(Vec::<egui::Rect>::new(), |mut seen, rect| {
                // A button paints its frame and its fill over the same box.
                if !seen.contains(&rect) {
                    seen.push(rect);
                }
                seen
            })
    };
    assert!(
        boxes(&pass(&app, &ctx, vec![])).is_empty(),
        "a shut menu drew its rows"
    );
    let at = root_rect(&ctx, host).center();
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    let open = boxes(&pass(&app, &ctx, vec![]));
    assert_eq!(open.len(), 2, "the open menu drew {open:?}");
    assert!(open[1].min.y >= open[0].max.y, "the rows overlap: {open:?}");
    let second = open[1].center();
    pass(&app, &ctx, press(second, true));
    pass(&app, &ctx, press(second, false));
    consume_input(&mut app);
    assert!(
        clicked(&app, rows[1]),
        "a click on a row did not reach its node"
    );
    assert!(!clicked(&app, rows[0]), "the click reached the wrong row");
}

/// An action row closes its menu; a toggle row says `keep_open` and stays,
/// which is what a grid of panel ticks needs.
#[test]
fn a_row_closes_its_menu_unless_it_keeps_it_open() {
    for (keep, open_after) in [(false, false), (true, true)] {
        let (_dir, mut app) = app();
        let params = toml::toml! { kind = "menu" text = "Menu" x = 10.0 y = 10.0 };
        let host = add_widget(&app, &params.into());
        let row = toml::toml! {
            kind = "button" text = "row" width = 173.0 height = 22.0 keep_open = keep
        };
        add_child_widget(&app, host, "R0", &row.into());
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let at = root_rect(&ctx, host).center();
        pass(&app, &ctx, press(at, true));
        pass(&app, &ctx, press(at, false));
        // Shown the pass after the click, and hit-tested against that pass.
        pass(&app, &ctx, vec![]);
        let row_at = pos2(100.0, 51.0);
        pass(&app, &ctx, press(row_at, true));
        pass(&app, &ctx, press(row_at, false));
        consume_input(&mut app);
        let drawn = pass(&app, &ctx, vec![])
            .shapes
            .iter()
            .any(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => (r.rect.width() - 173.0).abs() < 1.0,
                _ => false,
            });
        assert_eq!(
            drawn, open_after,
            "keep_open = {keep}: menu open after the click = {drawn}"
        );
    }
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

/// `showing` holds a menu's rows up with no click, which is the only way an
/// offscreen run or a tutorial can show one.
#[test]
fn a_showing_menu_is_open_without_a_click() {
    let (_dir, app) = app();
    let params = toml::toml! { kind = "menu" text = "Menu" showing = true x = 10.0 y = 10.0 };
    let host = add_widget(&app, &params.into());
    let row = toml::toml! { kind = "button" text = "row" width = 173.0 height = 22.0 };
    add_child_widget(&app, host, "R0", &row.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = pass(&app, &ctx, vec![])
        .shapes
        .iter()
        .any(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) => (r.rect.width() - 173.0).abs() < 1.0,
            _ => false,
        });
    assert!(drawn, "a showing menu drew no rows");
}

/// A menu whose rows are nodes is measured as the button it draws: its picture
/// and caret included, or whatever sits after it in a row draws over it.
#[test]
fn a_menu_button_holds_its_room_in_a_row() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/row.toml"),
        "type = \"widget_theme\"\n\n[roles.m]\nfill = \"#ff0000\"\n\n[roles.n]\nfill = \"#00ff00\"\n",
    )
    .unwrap();
    let strip = add_widget(
        &app,
        &toml::toml! { kind = "row" gap = 0.0 theme = "themes/row.toml" x = 0.0 y = 0.0 }.into(),
    );
    let menu = add_child_widget(
        &app,
        strip,
        "M",
        &toml::toml! { kind = "menu" role = "m" text = "Balaur" trailing = "▾" }.into(),
    );
    let row = toml::toml! { kind = "button" text = "row" };
    add_child_widget(&app, menu, "R0", &row.into());
    add_child_widget(
        &app,
        strip,
        "N",
        &toml::toml! { kind = "button" role = "n" text = "next" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let boxed = |fill: egui::Color32| {
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
                _ => None,
            })
            .expect("the button drew")
    };
    let (mine, next) = (boxed(egui::Color32::RED), boxed(egui::Color32::GREEN));
    assert!(
        mine.max.x <= next.min.x + 0.5,
        "the next button draws over the menu: {mine:?} {next:?}"
    );
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
