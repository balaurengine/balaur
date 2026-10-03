//! What the egui widget behind each kind takes, from the `widget` component:
//! each option drawn, heard or read back, and the window, dialog, menu and
//! root options Balaur draws itself.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use balaur_core::hecs::Entity;
use egui::pos2;

fn number(app: &balaur_core::App, entity: Entity, key: &str) -> f64 {
    balaur_core::components::as_f64(&property(app, entity, key))
        .unwrap_or_else(|| panic!("`{key}` is not a number"))
}

/// One widget at the top-left corner, drawn until it settles, and the pass
/// after.
fn drawn(
    params: &toml::Value,
) -> (
    tempfile::TempDir,
    balaur_core::App,
    Entity,
    egui::Context,
    egui::FullOutput,
) {
    let (dir, app) = app();
    let entity = add_widget(&app, params);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    (dir, app, entity, ctx, out)
}

fn has_text(out: &egui::FullOutput, part: &str) -> bool {
    texts(out).iter().any(|(text, _)| text.contains(part))
}

/// Every mesh the pass painted, wherever a `Shape::Vec` nests it.
fn meshes(out: &egui::FullOutput) -> Vec<egui::epaint::Mesh> {
    fn walk(shape: &egui::epaint::Shape, into: &mut Vec<egui::epaint::Mesh>) {
        match shape {
            egui::epaint::Shape::Mesh(mesh) => into.push((**mesh).clone()),
            egui::epaint::Shape::Vec(parts) => parts.iter().for_each(|part| walk(part, into)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

fn picture(dir: &std::path::Path) {
    let mut red = image::RgbaImage::new(8, 8);
    for pixel in red.pixels_mut() {
        *pixel = image::Rgba([255, 255, 255, 255]);
    }
    red.save(dir.join("white.png")).unwrap();
}

#[test]
fn every_option_reads_back_as_it_was_written() {
    let (_dir, app) = app();
    let written = toml::toml! {
        kind = "slider" sense = "drag" show_tooltip_when_elided = false indeterminate = true show_value = true
        prefix = "x" logarithmic = true smallest_positive = 0.5 largest_finite = 900.0 clamp = "edits"
        clamp_existing = true smart_aim = false drag_speed = 0.25 decimals = 2 trailing_fill = true
        handle = "rect" handle_aspect = 0.75 number_format = "hex" update_while_editing = false
        show_percentage = true animate = true spacing = 9.0 overhang = -2.0 list_height = 120.0 alpha = "additive"
        inline = true editable = false tab_inserts = true caret_at_end = false clip_text = false
        submit_key = "cmd+enter" scrollbar = "always" stick_to_end = true scroll_offset = [3.0, 40.0]
        min_scrolled_width = 30.0 min_scrolled_height = 20.0 animated = false wheel_speed = [2.0, 0.5]
        drag_scroll = "never" wheel_scroll = false drag_cursor = "grabbing" tint = [0.5, 0.5, 1.0, 1.0]
        region = [1.0, 2.0, 3.0, 4.0] angle_degrees = 30.0 angle_origin = [0.0, 1.0] alt_text = "a cat"
        popup_gap = 2.0 popup_width = 140.0 placement_fallbacks = ["above", "right_end"] close_on = "never"
        backdrop_color = [1.0, 0.0, 0.0, 0.5] dismissable = false resizable = true collapsible = true
        closable = false movable = true constrain = true default_open = false fade_in = true
    };
    let entity = add_widget(&app, &written.clone().into());
    for (key, value) in &written {
        assert_eq!(
            &property(&app, entity, key),
            value,
            "`{key}` came back changed"
        );
    }
}

#[test]
fn a_checkbox_can_be_drawn_neither_ticked_nor_clear() {
    let line = |indeterminate: bool| {
        let (_dir, _app, _, _, out) = drawn(
            &toml::toml! { kind = "checkbox" text = "all" indeterminate = indeterminate x = 0.0 y = 0.0 }.into(),
        );
        out.shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::epaint::Shape::LineSegment { .. }))
    };
    assert!(line(true), "the mixed mark is a dash across the box");
    assert!(!line(false), "a clear box draws no mark");
}

#[test]
fn a_slider_shows_its_number_in_its_decimals_between_prefix_and_suffix() {
    let (_dir, _app, _, _, out) = drawn(
        &toml::toml! {
            kind = "slider" show_value = true decimals = 2 prefix = "at" suffix = "m"
            value = 3.0 min = 0.0 max = 10.0 width = 120.0 x = 0.0 y = 0.0
        }
        .into(),
    );
    assert!(has_text(&out, "3.00"), "{:?}", texts(&out));
    assert!(
        has_text(&out, "at") && has_text(&out, "m"),
        "{:?}",
        texts(&out)
    );
}

#[test]
fn a_vertical_slider_runs_up_its_box() {
    let (_dir, mut app) = app();
    let slider = add_widget(
        &app,
        &toml::toml! { kind = "slider" axis = "vertical" min = 0.0 max = 10.0 width = 30.0 height = 120.0 x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let near_top = pos2(15.0, 8.0);
    pass(&app, &ctx, press(near_top, true));
    pass(&app, &ctx, press(near_top, false));
    consume_input(&mut app);
    assert!(
        number(&app, slider, "value") > 8.0,
        "the top of its box is its top end"
    );
}

#[test]
fn a_number_field_writes_its_number_in_the_base_and_decimals_it_names() {
    let (_dir, _app, _, _, hex) = drawn(
        &toml::toml! { kind = "number_field" value = 255.0 number_format = "hex" width = 90.0 x = 0.0 y = 0.0 }.into(),
    );
    assert!(has_text(&hex, "ff"), "{:?}", texts(&hex));
    let (_dir, _app, _, _, fixed) = drawn(
        &toml::toml! { kind = "number_field" value = 2.0 decimals = 3 width = 90.0 x = 0.0 y = 0.0 }
            .into(),
    );
    assert!(has_text(&fixed, "2.000"), "{:?}", texts(&fixed));
}

#[test]
fn a_number_field_told_to_clamps_the_number_the_scene_gave_it() {
    let (_dir, mut app) = app();
    let field = add_widget(
        &app,
        &toml::toml! {
            kind = "number_field" value = -24.0 min = 0.0 max = 100.0 clamp_existing = true width = 90.0 x = 0.0 y = 0.0
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    consume_input(&mut app);
    assert!(number(&app, field, "value").abs() < 1e-6, "pulled to `min`");
}

#[test]
fn a_progress_bar_draws_its_percentage() {
    let (_dir, _app, _, _, out) = drawn(
        &toml::toml! { kind = "progress_bar" show_percentage = true value = 0.5 width = 160.0 x = 0.0 y = 0.0 }.into(),
    );
    assert!(has_text(&out, "50%"), "{:?}", texts(&out));
}

#[test]
fn an_animated_progress_bar_asks_for_frames_only_while_short_of_full() {
    let asks = |animate: bool, value: f64| {
        let (_dir, _app, _, _, out) = drawn(
            &toml::toml! { kind = "progress_bar" animate = animate value = value width = 160.0 x = 0.0 y = 0.0 }.into(),
        );
        out.viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|port| port.repaint_delay.is_zero())
    };
    assert!(
        !asks(false, 0.5),
        "a still bar asks for nothing, or this proves nothing"
    );
    assert!(
        asks(true, 0.5),
        "an animated bar short of full asks for the next frame"
    );
    assert!(!asks(true, 1.0), "a full one is done");
}

#[test]
fn a_progress_bar_with_no_font_size_is_as_tall_as_a_line_of_its_caption() {
    let (_dir, _app, bar, _, _) = drawn(
        &toml::toml! { kind = "progress_bar" text = "half" value = 0.5 x = 0.0 y = 0.0 }.into(),
    );
    let rect = balaur_ui::widget_rect(bar).unwrap();
    assert!(rect.height() >= 14.0, "{rect:?}");
}

#[test]
fn a_separator_takes_the_room_its_spacing_names() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 }.into(),
    );
    let rule = add_child_widget(
        &app,
        column,
        "r",
        &toml::toml! { kind = "separator" spacing = 20.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = balaur_ui::widget_rect(rule).unwrap();
    assert!((rect.height() - 20.0).abs() < 0.5, "{rect:?}");
}

#[test]
fn a_color_picker_drawn_inline_shows_its_picker_in_place() {
    let shapes = |inline: bool| {
        let (_dir, _app, _, _, out) =
            drawn(&toml::toml! { kind = "color_picker" inline = inline x = 0.0 y = 0.0 }.into());
        out.shapes.len()
    };
    assert!(
        shapes(true) > shapes(false) + 5,
        "the picker's planes and sliders are drawn"
    );
}

#[test]
fn a_text_field_draws_what_stands_either_side_of_its_text() {
    let (_dir, _app, _, _, out) = drawn(
        &toml::toml! { kind = "text_field" text = "12" prefix = "$" suffix = "kg" width = 160.0 x = 0.0 y = 0.0 }.into(),
    );
    assert!(
        has_text(&out, "$") && has_text(&out, "kg"),
        "{:?}",
        texts(&out)
    );
}

#[test]
fn a_text_field_that_is_not_editable_ignores_typing() {
    let (_dir, mut app) = app();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "keep" editable = false width = 160.0 x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let inside = pos2(40.0, 10.0);
    pass(&app, &ctx, press(inside, true));
    pass(&app, &ctx, press(inside, false));
    pass(&app, &ctx, vec![egui::Event::Text("x".into())]);
    consume_input(&mut app);
    assert_eq!(property(&app, field, "text").as_str(), Some("keep"));
}

#[test]
fn a_submit_key_takes_the_place_of_enter() {
    let submitted_by = |chord: egui::Modifiers| {
        let (_dir, mut app) = app();
        let field = add_widget(
            &app,
            &toml::toml! { kind = "text_field" text = "go" submit_key = "shift+enter" width = 160.0 x = 0.0 y = 0.0 }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let inside = pos2(40.0, 10.0);
        pass(&app, &ctx, press(inside, true));
        pass(&app, &ctx, press(inside, false));
        pass(
            &app,
            &ctx,
            vec![
                egui::Event::ModifiersChanged(chord),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: chord,
                },
            ],
        );
        consume_input(&mut app);
        balaur_core::events::delivered_from(&app.engine, field, "submit").len()
    };
    assert_eq!(
        submitted_by(egui::Modifiers::NONE),
        0,
        "Enter alone no longer submits"
    );
    assert_eq!(submitted_by(egui::Modifiers::SHIFT), 1, "the chord does");
}

#[test]
fn a_submit_key_that_is_no_chord_is_refused() {
    let (_dir, app) = app();
    let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), "F", app.engine.root());
    let err = balaur::components::add(
        &app.engine,
        node,
        "widget",
        Some(&toml::toml! { kind = "text_field" submit_key = "cmd+banana" }.into()),
    )
    .expect_err("no such key");
    assert!(format!("{err:#}").contains("submit_key"), "{err:#}");
}

/// A scroll of twenty 30 px lines in a 100 px box.
fn scroll_of_lines(
    params: &toml::Value,
) -> (
    tempfile::TempDir,
    balaur_core::App,
    Entity,
    Vec<Entity>,
    egui::Context,
) {
    let (dir, app) = app();
    let scroll = add_widget(&app, params);
    let lines = (0..20)
        .map(|i| {
            add_child_widget(
                &app,
                scroll,
                &format!("line{i}"),
                &toml::toml! { kind = "label" text = "a line" height = 30.0 }.into(),
            )
        })
        .collect();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    (dir, app, scroll, lines, ctx)
}

#[test]
fn a_written_scroll_offset_scrolls_there_and_the_reader_s_scroll_reads_back() {
    let (_dir, mut app, scroll, lines, ctx) = scroll_of_lines(
        &toml::toml! { kind = "scroll" scroll_offset = [-1.0, 60.0] x = 0.0 y = 0.0 width = 200.0 height = 100.0 gap = [0.0, 0.0] padding = 0.0 }.into(),
    );
    let first = balaur_ui::widget_rect(lines[0]).unwrap();
    assert!(
        (first.min.y + 60.0).abs() < 1.0,
        "the first line is scrolled 60 up: {first:?}"
    );
    let wheel = vec![
        egui::Event::PointerMoved(pos2(100.0, 50.0)),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -90.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    pass(&app, &ctx, wheel);
    for _ in 0..10 {
        pass(&app, &ctx, vec![]);
    }
    consume_input(&mut app);
    let read = property(&app, scroll, "scroll_offset");
    let down = read
        .as_array()
        .and_then(|pair| pair.get(1))
        .and_then(toml::Value::as_float);
    assert!(
        down.is_some_and(|y| y > 60.0),
        "the wheel's scroll reads back: {read:?}"
    );
    // The number the reader left is not sent back over the reader's scroll.
    pass(&app, &ctx, vec![]);
    let after = balaur_ui::widget_rect(lines[0]).unwrap();
    let down = down.unwrap_or_default() as f32;
    assert!(
        (after.min.y + down).abs() < 1.0,
        "still where the wheel put it: {after:?}"
    );
}

#[test]
fn a_scroll_that_sticks_to_its_end_shows_its_last_line() {
    let (_dir, _app, scroll, lines, _ctx) = scroll_of_lines(
        &toml::toml! { kind = "scroll" stick_to_end = true x = 0.0 y = 0.0 width = 200.0 height = 100.0 gap = [0.0, 0.0] padding = 0.0 }.into(),
    );
    let last = balaur_ui::widget_rect(lines[19]).unwrap();
    let held = balaur_ui::widget_rect(scroll).unwrap();
    assert!(
        (last.max.y - held.max.y).abs() < 2.0,
        "the last line at the bottom: {last:?} in {held:?}"
    );
}

#[test]
fn ui_scroll_takes_the_scroll_offset_the_widget_does() {
    // A mark 30 px down the scrolled content, in view either way.
    let mark_at = |offset: f64| {
        let body = format!(
            "ui::scroll(\"s\", #{{ scroll_offset: {offset:.1}, max_height: 60.0 }}, || {{ ui::add_space(200.0); ui::rect_stroke(0.0, 30.0, 10.0, 4.0, #{{ stroke: \"#ff0000\" }}); }});"
        );
        let (_dir, app, ctx, errors) = crate::pass::draw_with(&body);
        assert!(errors.is_empty(), "{errors:?}");
        ctx.begin_pass(egui::RawInput::default());
        balaur_ui::run_pass(&app.engine, &ctx);
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        out.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.stroke.color == egui::Color32::RED => {
                    Some(rect.rect.min.y)
                }
                _ => None,
            })
            .expect("the mark is in view")
    };
    let moved = mark_at(0.0) - mark_at(20.0);
    assert!(
        (moved - 20.0).abs() < 1.0,
        "20 px along moves it 20 up: {moved}"
    );
}

#[test]
fn an_image_is_tinted_and_cut_to_its_region() {
    let (dir, app) = app();
    picture(dir.path());
    add_widget(
        &app,
        &toml::toml! {
            kind = "image" image = "white.png" tint = [0.0, 1.0, 0.0, 1.0] region = [0.0, 0.0, 4.0, 4.0] x = 0.0 y = 0.0
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let (rect, uv) = out
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) => {
                rect.brush.as_ref().map(|brush| (rect.clone(), brush.uv))
            }
            _ => None,
        })
        .expect("the picture drew");
    assert_eq!(rect.fill, egui::Color32::GREEN, "tinted green");
    assert!(
        (uv.max.x - 0.5).abs() < 1e-3,
        "half the picture across: {uv:?}"
    );
    assert!(
        (rect.rect.width() - 4.0).abs() < 0.5,
        "drawn at the region's own size: {:?}",
        rect.rect
    );
}

#[test]
fn a_turned_image_is_drawn_turned() {
    let (dir, app) = app();
    picture(dir.path());
    add_widget(
        &app,
        &toml::toml! { kind = "image" image = "white.png" width = 40.0 height = 40.0 angle_degrees = 45.0 x = 50.0 y = 50.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let mesh = meshes(&out)
        .into_iter()
        .find(|mesh| mesh.texture_id != egui::TextureId::default())
        .expect("the picture drew");
    let bounds = mesh.calc_bounds();
    assert!(
        bounds.width() > 50.0,
        "a square turned 45 degrees is wider: {bounds:?}"
    );
}

#[test]
fn an_image_that_will_not_load_draws_its_alt_text() {
    let (_dir, _app, _, _, out) = drawn(
        &toml::toml! { kind = "image" image = "missing.png" alt_text = "no picture" x = 0.0 y = 0.0 }.into(),
    );
    assert!(has_text(&out, "no picture"), "{:?}", texts(&out));
}

#[test]
fn a_menu_opens_to_the_side_its_placement_names() {
    let (_dir, _app, menu, _, out) = drawn(
        &toml::toml! {
            kind = "menu" text = "File" options = ["Open", "Save"] placement = "right" showing = true x = 20.0 y = 20.0
        }
        .into(),
    );
    let button = balaur_ui::widget_rect(menu).unwrap();
    let row = texts(&out)
        .into_iter()
        .find(|(text, _)| text == "Open")
        .map(|(_, at)| at)
        .expect("the rows drew");
    assert!(
        row.x >= button.max.x,
        "the rows open to the right: {row:?} beside {button:?}"
    );
}

#[test]
fn a_dialog_dims_with_its_backdrop_and_stays_when_it_is_not_dismissable() {
    let (_dir, mut app) = app();
    let dialog = add_widget(
        &app,
        &toml::toml! {
            kind = "dialog" text = "Sure?" width = 200.0 height = 100.0 dismissable = false backdrop_color = [1.0, 0.0, 0.0, 0.5]
        }
        .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, key(egui::Key::Escape));
    let dim = egui::Color32::from_rgba_unmultiplied(255, 0, 0, 128);
    assert!(
        out.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::epaint::Shape::Rect(rect) if rect.fill == dim)
        ),
        "the backdrop is its colour"
    );
    consume_input(&mut app);
    assert_eq!(
        property(&app, dialog, "open").as_bool(),
        Some(true),
        "Escape left it open"
    );
}

/// A window at (100, 100), 200 by 120, with the extra keys given.
fn window(extra: toml::Table) -> (tempfile::TempDir, balaur_core::App, Entity, egui::Context) {
    let (dir, app) = app();
    let mut params = toml::toml! { kind = "window" text = "Debug" x = 100.0 y = 100.0 width = 200.0 height = 120.0 };
    params.extend(extra);
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    (dir, app, entity, ctx)
}

fn drag(app: &balaur_core::App, ctx: &egui::Context, from: egui::Pos2, by: egui::Vec2) {
    pass(app, ctx, vec![egui::Event::PointerMoved(from)]);
    pass(app, ctx, press(from, true));
    pass(app, ctx, vec![egui::Event::PointerMoved(from + by / 2.0)]);
    pass(app, ctx, vec![egui::Event::PointerMoved(from + by)]);
    pass(app, ctx, press(from + by, false));
}

#[test]
fn a_window_that_is_not_movable_stays_where_it_is() {
    let (_dir, mut app, entity, ctx) = window(toml::toml! { movable = false });
    drag(&app, &ctx, pos2(118.0, 112.0), egui::vec2(30.0, 20.0));
    consume_input(&mut app);
    assert!((number(&app, entity, "x") - 100.0).abs() < 0.5);
}

#[test]
fn a_constrained_window_stays_on_its_surface() {
    let (_dir, mut app, entity, ctx) = window(toml::toml! { movable = true constrain = true });
    // A tick after every pass, as a frame has: each step is held against
    // where the last one left the window.
    let from = pos2(118.0, 112.0);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(from)]);
    pass(&app, &ctx, press(from, true));
    for step in 1..=4 {
        pass(
            &app,
            &ctx,
            vec![egui::Event::PointerMoved(
                from - egui::vec2(100.0 * step as f32, 0.0),
            )],
        );
        consume_input(&mut app);
    }
    pass(&app, &ctx, press(from - egui::vec2(400.0, 0.0), false));
    consume_input(&mut app);
    assert!(
        number(&app, entity, "x") >= -0.5,
        "held at the left edge: {}",
        number(&app, entity, "x")
    );
}

#[test]
fn a_resizable_window_grows_by_its_grip() {
    let (_dir, mut app, entity, ctx) = window(toml::toml! { resizable = true max_width = 230.0 });
    let rect = root_rect(&ctx, entity);
    drag(
        &app,
        &ctx,
        rect.max - egui::vec2(4.0, 4.0),
        egui::vec2(60.0, 30.0),
    );
    consume_input(&mut app);
    assert!(
        (number(&app, entity, "width") - 230.0).abs() < 0.5,
        "held at `max_width`"
    );
    assert!(number(&app, entity, "height") > 130.0, "and taller");
}

#[test]
fn a_collapsible_window_folds_to_its_bar_and_back() {
    let (_dir, app, entity, ctx) = window(toml::toml! { collapsible = true });
    let child = add_child_widget(
        &app,
        entity,
        "c",
        &toml::toml! { kind = "label" text = "inside" }.into(),
    );
    settle(&app, &ctx);
    let open = root_rect(&ctx, entity);
    let drawn = pass(&app, &ctx, vec![]);
    let arrow = texts(&drawn)
        .into_iter()
        .find(|(text, _)| text == "▾")
        .map(|(_, at)| at)
        .expect("the arrow drew");
    let at = arrow + egui::vec2(4.0, 6.0);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    settle(&app, &ctx);
    let shut = root_rect(&ctx, entity);
    assert!(
        shut.height() < open.height() / 2.0,
        "folded to its bar: {shut:?} from {open:?}"
    );
    assert!(
        balaur_ui::widget_rect(child).is_none(),
        "nothing under the bar is drawn"
    );
    let drawn = pass(&app, &ctx, vec![]);
    let arrow = texts(&drawn)
        .into_iter()
        .find(|(text, _)| text == "▸")
        .map(|(_, at)| at)
        .expect("the arrow points the way it folded");
    let at = arrow + egui::vec2(4.0, 6.0);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    settle(&app, &ctx);
    assert!(
        (root_rect(&ctx, entity).height() - open.height()).abs() < 1.0,
        "and back"
    );
}

#[test]
fn a_window_without_a_header_or_a_cross_draws_neither() {
    let (_dir, app, _, ctx) = window(toml::toml! { header = false });
    let bare = pass(&app, &ctx, vec![]);
    assert!(!has_text(&bare, "Debug") && !has_text(&bare, "×"));
    let (_dir, app, _, ctx) = window(toml::toml! { closable = false });
    let titled = pass(&app, &ctx, vec![]);
    assert!(has_text(&titled, "Debug") && !has_text(&titled, "×"));
}

#[test]
fn a_movable_root_moves_with_a_drag_on_its_box() {
    let (_dir, mut app) = app();
    let panel = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "drag me" movable = true x = 50.0 y = 50.0 width = 120.0 height = 60.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    drag(&app, &ctx, pos2(100.0, 95.0), egui::vec2(20.0, 10.0));
    consume_input(&mut app);
    assert!(
        (number(&app, panel, "x") - 70.0).abs() < 2.0,
        "{}",
        number(&app, panel, "x")
    );
    assert!(
        (number(&app, panel, "y") - 60.0).abs() < 2.0,
        "{}",
        number(&app, panel, "y")
    );
}

#[test]
fn a_none_cursor_hides_the_pointer_over_the_widget() {
    let (_dir, app, _, ctx, _) =
        drawn(&toml::toml! { kind = "button" text = "go" cursor = "none" x = 0.0 y = 0.0 }.into());
    let out = pass(&app, &ctx, vec![egui::Event::PointerMoved(pos2(10.0, 8.0))]);
    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::None);
}

#[test]
fn a_button_that_only_senses_hover_takes_no_click() {
    let (_dir, mut app) = app();
    let button = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "go" sense = "hover" x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, press(pos2(10.0, 8.0), true));
    pass(&app, &ctx, press(pos2(10.0, 8.0), false));
    consume_input(&mut app);
    assert!(!clicked(&app, button));
}

#[test]
fn a_button_s_gap_spaces_its_icon_from_its_caption() {
    let wide = |gap: f64| {
        let (_dir, _app, button, _, _) = drawn(
            &toml::toml! { kind = "button" text = "go" icon = "+" gap = [gap, gap] x = 0.0 y = 0.0 }
                .into(),
        );
        balaur_ui::widget_rect(button).unwrap().width()
    };
    assert!(
        (wide(30.0) - wide(10.0) - 20.0).abs() < 0.6,
        "{} against {}",
        wide(30.0),
        wide(10.0)
    );
}

#[test]
fn a_fixed_width_button_cuts_a_caption_too_long_for_it_at_its_edge() {
    let (_dir, _app, button, _, out) = drawn(
        &toml::toml! { kind = "button" text = "Save every scene in the project" width = 80.0 x = 0.0 y = 0.0 }.into(),
    );
    let rect = balaur_ui::widget_rect(button).unwrap();
    let caption = out
        .shapes
        .iter()
        .filter(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => text.galley.text().contains("Save"),
            egui::epaint::Shape::Mesh(mesh) => mesh.texture_id != egui::TextureId::default(),
            _ => false,
        })
        .map(|shape| shape.clip_rect)
        .next()
        .expect("the caption drew");
    assert!(
        caption.max.x <= rect.max.x + 0.5,
        "clipped to {rect:?}: {caption:?}"
    );
}

#[test]
fn a_collapsible_window_that_does_not_start_open_starts_folded() {
    let (_dir, _app, entity, ctx) = window(toml::toml! { collapsible = true default_open = false });
    assert!(
        root_rect(&ctx, entity).height() < 60.0,
        "{:?}",
        root_rect(&ctx, entity)
    );
}

#[test]
fn a_number_field_moves_by_its_drag_speed() {
    let (_dir, mut app) = app();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "number_field" value = 0.0 drag_speed = 0.5 width = 90.0 x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    drag(&app, &ctx, pos2(40.0, 10.0), egui::vec2(40.0, 0.0));
    consume_input(&mut app);
    assert!(
        (number(&app, field, "value") - 20.0).abs() < 1.0,
        "{}",
        number(&app, field, "value")
    );
}

#[test]
fn a_scroll_the_wheel_may_not_scroll_stays_put() {
    let (_dir, mut app, scroll, _lines, ctx) = scroll_of_lines(
        &toml::toml! { kind = "scroll" wheel_scroll = false x = 0.0 y = 0.0 width = 200.0 height = 100.0 gap = [0.0, 0.0] padding = 0.0 }.into(),
    );
    let wheel = vec![
        egui::Event::PointerMoved(pos2(100.0, 50.0)),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -90.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    pass(&app, &ctx, wheel);
    for _ in 0..5 {
        pass(&app, &ctx, vec![]);
    }
    consume_input(&mut app);
    assert!(balaur_core::events::delivered_from(&app.engine, scroll, "scrolled").is_empty());
}

#[test]
fn a_separator_s_overhang_runs_its_line_past_its_room() {
    let span = |overhang: f64| {
        let (_dir, app) = app();
        let column = add_widget(&app, &toml::toml! { kind = "column" width = 100.0 gap = [0.0, 0.0] padding = 0.0 x = 0.0 y = 0.0 }.into());
        add_child_widget(
            &app,
            column,
            "r",
            &toml::toml! { kind = "separator" overhang = overhang }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let out = pass(&app, &ctx, vec![]);
        out.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::LineSegment { points, .. } => {
                    Some((points[1].x - points[0].x).abs())
                }
                _ => None,
            })
            .expect("the rule drew")
    };
    assert!(
        (span(10.0) - span(0.0) - 20.0).abs() < 0.6,
        "ten past each end"
    );
}

#[test]
fn a_slider_s_shown_number_stays_inside_its_box() {
    let (_dir, _app, _, _, out) = drawn(
        &toml::toml! {
            kind = "slider" show_value = true prefix = "at" suffix = "metres" value = 3.0 max = 10.0 width = 200.0 x = 0.0 y = 0.0
        }
        .into(),
    );
    let right = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text.pos.x + text.galley.size().x),
            _ => None,
        })
        .fold(0.0f32, f32::max);
    assert!(right > 0.0 && right <= 200.5, "the number ends at {right}");
}
