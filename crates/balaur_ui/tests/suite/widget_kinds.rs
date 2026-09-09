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

use balaur_script::BindingsExt as _;

struct StubPlugin {
    manifest: balaur_plugin::Manifest,
    name: &'static str,
}

impl balaur_plugin::Plugin for StubPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> anyhow::Result<()> {
        let mut m = reg.script_module(self.name)?;
        let m = &mut *m;
        m.function("targets", |_: &balaur::Engine, ()| {
            Ok(balaur_script::Value::List(vec![]))
        });
        m.function(
            "listen",
            |_: &balaur::Engine, (_a, _b): (balaur_script::Value, Option<balaur_script::Value>)| {
                Ok(balaur_script::Value::Nil)
            },
        );
        m.function(
            "start",
            |_: &balaur::Engine, (_a, _b): (String, Option<balaur_script::Value>)| Ok(false),
        );
        m.function("output", |_: &balaur::Engine, _t: String| {
            Ok(balaur_script::Value::Str(String::new()))
        });
        m.function("running", |_: &balaur::Engine, ()| Ok(0i64));
        m.function("file", |_: &balaur::Engine, _p: String| {
            Ok(balaur_script::Value::Nil)
        });
        m.function("kind_of", |_: &balaur::Engine, _p: String| {
            Ok(balaur_script::Value::Nil)
        });
        Ok(())
    }
}

fn stub(name: &'static str) -> StubPlugin {
    StubPlugin {
        manifest: balaur_plugin::Manifest::new(name, "0.0.0"),
        name,
    }
}

fn shell_pass(
    app: &balaur_core::App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            pos2(0.0, 0.0),
            egui::vec2(1000.0, 470.0),
        )),
        events,
        ..Default::default()
    };
    ctx.begin_pass(input);
    balaur_ui::run_pass(&app.engine, ctx);
    let mut out = ctx.end_pass();
    out.textures_delta.clear();
    out
}

fn editor() -> (balaur_core::App, egui::Context) {
    balaur_core::logbuf::capture_for_test();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor");
    let game = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let mut config = balaur::AppConfig::dev(root.to_string_lossy().as_ref());
    config.watch = false;
    config.script_args = vec![game.to_string_lossy().into_owned()];
    let mut app = balaur::standard_app(config).unwrap();
    balaur_plugin::load(&mut app, &mut stub("export")).unwrap();
    balaur_plugin::load(&mut app, &mut stub("import")).unwrap();
    balaur::file_api::add_root(&app.engine, &game);
    app.load_project().unwrap();
    let ctx = egui::Context::default();
    for _ in 0..24 {
        app.tick(1.0 / 60.0);
        shell_pass(&app, &ctx, vec![]);
    }
    (app, ctx)
}

#[test]
fn zz_probe_shell() {
    let (mut app, ctx) = editor();
    let fingerprint = |out: &egui::FullOutput| {
        let mut rows: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| {
                let b = s.shape.visual_bounding_rect();
                b.is_finite().then(|| {
                    let paint = match &s.shape {
                        egui::epaint::Shape::Rect(r) => format!("{:?}/{:?}", r.fill, r.stroke.color),
                        egui::epaint::Shape::Text(t) => format!("{:?}", t.fallback_color),
                        _ => String::new(),
                    };
                    format!("{:.0},{:.0},{:.0},{:.0} {paint}", b.min.x, b.min.y, b.width(), b.height())
                })
            })
            .collect();
        rows.sort();
        rows
    };
    let shot = |app: &mut balaur_core::App, at: egui::Pos2| {
        for _ in 0..3 {
            app.tick(1.0 / 60.0);
            shell_pass(app, &ctx, vec![egui::Event::PointerMoved(at)]);
        }
        app.tick(1.0 / 60.0);
        fingerprint(&shell_pass(app, &ctx, vec![egui::Event::PointerMoved(at)]))
    };
    let at = pos2(748.0, 90.0);
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
        shell_pass(&app, &ctx, vec![egui::Event::PointerMoved(at)]);
    }
    app.tick(1.0 / 60.0);
    shell_pass(&app, &ctx, vec![egui::Event::PointerMoved(at)]);
    for entry in balaur_core::logbuf::recent(600) {
        if entry.message.contains("PROBE stage") {
            println!("LOG {}", entry.message);
        }
    }
    ctx.memory(|m| {
        for layer in m.layer_ids() {
            if let Some(r) = m.area_rect(layer.id)
                && r.contains(at)
            {
                println!("AREA {:?} {:?} {:.0}..{:.0} y{:.0}..{:.0}", layer.order, layer.id, r.min.x, r.max.x, r.min.y, r.max.y);
            }
        }
    });
    panic!("probe");
}
