//! A `scroll` under a finger and a mouse: the project's deadzone, the touch
//! class (or the mouse standing in for a finger) deciding a drag, a child
//! that takes the drag itself, and a row that slides sideways being as tall
//! as what it holds.

use crate::support::*;
use balaur_core::App;
use balaur_core::hecs::Entity;
use egui::pos2;

/// A project setting stated, as `project.toml` states it on a run.
fn set(app: &App, path: &str, value: toml::Value) {
    balaur_core::settings::set(&app.engine, path, value);
}

/// A 200 x 100 scroll of thirty lines, a button first; its `widget_scroll`
/// as given. Answers the scroll, the button and the sixth line.
fn list(app: &App, scroll: toml::Value) -> (Entity, Entity, Entity) {
    let mut params = toml::toml! { kind = "scroll" x = 0.0 y = 0.0 width = 200.0 height = 100.0 widget_layout = { gap = [0.0, 0.0] } };
    params.insert("widget_scroll".into(), scroll);
    let holder = add_widget(app, &params.into());
    let first = add_child_widget(
        app,
        holder,
        "first",
        &toml::toml! { kind = "button" text = "first" }.into(),
    );
    let mut sixth = first;
    for n in 0..30 {
        let text = format!("line {n}");
        let line = add_child_widget(
            app,
            holder,
            "line",
            &toml::toml! { kind = "label" text = text }.into(),
        );
        if n == 5 {
            sixth = line;
        }
    }
    (holder, first, sixth)
}

/// A drag of 72 down to up from (20, 80) in six steps, held a pass, let go;
/// answers how far the sixth line moved up while it was held.
fn drag_up(
    app: &App,
    ctx: &egui::Context,
    line: Entity,
    events: fn(egui::Pos2, bool) -> Vec<egui::Event>,
) -> f32 {
    let before = balaur_ui::widget_rect(line).unwrap().min.y;
    pass(app, ctx, events(pos2(20.0, 80.0), true));
    for step in 1..=6 {
        pass(
            app,
            ctx,
            vec![egui::Event::PointerMoved(pos2(
                20.0,
                80.0 - 12.0 * step as f32,
            ))],
        );
    }
    pass(app, ctx, vec![]);
    let after = balaur_ui::widget_rect(line).unwrap().min.y;
    pass(app, ctx, events(pos2(20.0, 8.0), false));
    before - after
}

fn as_touchscreen(app: &App) {
    let mut platform = balaur_core::facts::platform(&app.engine);
    platform.touchscreen = true;
    app.engine
        .resource::<balaur_core::facts::Facts>()
        .borrow_mut()
        .0 = Some(platform);
}

#[test]
fn a_mouse_drag_scrolls_nothing_and_a_finger_s_does_past_the_project_s_deadzone() {
    let (_dir, app) = app();
    let (_holder, _first, sixth) = list(&app, toml::toml! { axis = "vertical" }.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert!(
        drag_up(&app, &ctx, sixth, press).abs() < 0.5,
        "a mouse drag scrolled a list meant for fingers"
    );
    as_touchscreen(&app);
    settle(&app, &ctx);
    let moved = drag_up(&app, &ctx, sixth, touch);
    assert!(moved > 40.0, "a finger's drag did not scroll: {moved}");
}

#[test]
fn a_finger_s_wobble_inside_the_deadzone_is_a_tap() {
    let (_dir, mut app) = app();
    set(&app, "ui/scroll_deadzone", toml::Value::Float(12.0));
    as_touchscreen(&app);
    let (_holder, first, sixth) = list(&app, toml::toml! { axis = "vertical" }.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let before = balaur_ui::widget_rect(sixth).unwrap().min.y;
    pass(&app, &ctx, touch(pos2(20.0, 10.0), true));
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(24.0, 18.0))],
    );
    pass(&app, &ctx, touch(pos2(24.0, 18.0), false));
    consume_input(&mut app);
    assert!(
        clicked(&app, first),
        "a tap that wobbled 9 should still click"
    );
    let after = balaur_ui::widget_rect(sixth).unwrap().min.y;
    assert!(
        (before - after).abs() < 0.5,
        "the wobble scrolled the list: {before} -> {after}"
    );
}

#[test]
fn the_mouse_stands_in_for_a_finger_when_the_project_says_so() {
    let (_dir, app) = app();
    set(
        &app,
        "input/emulate_touch_from_mouse",
        toml::Value::Boolean(true),
    );
    let (_holder, _first, sixth) = list(&app, toml::toml! { axis = "vertical" }.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let moved = drag_up(&app, &ctx, sixth, press);
    assert!(moved > 40.0, "the emulated finger did not scroll: {moved}");
}

#[test]
fn a_child_that_takes_the_drag_keeps_it_from_the_scroll() {
    let (_dir, app) = app();
    as_touchscreen(&app);
    let holder = add_widget(
        &app,
        &toml::toml! { kind = "scroll" x = 0.0 y = 0.0 width = 200.0 height = 100.0 widget_layout = { gap = [0.0, 0.0] } widget_scroll = { axis = "vertical" } }.into(),
    );
    add_child_widget(&app, holder, "grip", &toml::toml! { kind = "button" text = "grip" height = 90.0 widget_button = { sense = "drag" } }.into());
    let mut sixth = holder;
    for n in 0..30 {
        let text = format!("line {n}");
        let line = add_child_widget(
            &app,
            holder,
            "line",
            &toml::toml! { kind = "label" text = text }.into(),
        );
        if n == 5 {
            sixth = line;
        }
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let moved = drag_up(&app, &ctx, sixth, touch);
    assert!(
        moved.abs() < 0.5,
        "a drag the grip took scrolled the list: {moved}"
    );
}

#[test]
fn a_row_that_slides_sideways_is_as_tall_as_what_it_holds() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 300.0 widget_layout = { gap = [0.0, 0.0] } }.into(),
    );
    let slide = add_child_widget(
        &app,
        column,
        "slide",
        &toml::toml! { kind = "scroll" widget_scroll = { axis = "horizontal" } }.into(),
    );
    let row = add_child_widget(
        &app,
        slide,
        "row",
        &toml::toml! { kind = "row" widget_layout = { gap = [8.0, 0.0] } }.into(),
    );
    for n in 0..8 {
        let text = format!("card {n}");
        add_child_widget(
            &app,
            row,
            "card",
            &toml::toml! { kind = "button" text = text width = 80.0 height = 40.0 }.into(),
        );
    }
    let after = add_child_widget(
        &app,
        column,
        "after",
        &toml::toml! { kind = "label" text = "after" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let box_ = balaur_ui::widget_rect(slide).unwrap();
    assert!(
        (box_.height() - 40.0).abs() < 1.0,
        "the sliding row was {} tall",
        box_.height()
    );
    assert!(
        box_.width() <= 200.5,
        "the sliding row ran past its column: {}",
        box_.width()
    );
    let below = balaur_ui::widget_rect(after).unwrap().min.y;
    assert!(
        below >= box_.max.y - 0.5,
        "what follows lies over the row: {below} < {}",
        box_.max.y
    );
}

/// A 200 x 300 column holding a caption, a column whose `overflow` is
/// `scroll` with `lines` buttons 40 high, and a caption after it. Answers
/// the box, its sixth line (or its last, with fewer) and the caption after.
fn boxed(app: &App, lines: usize, scroll: toml::Value) -> (Entity, Entity, Entity) {
    let column = add_widget(
        app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 300.0 widget_layout = { gap = [0.0, 0.0] } }.into(),
    );
    add_child_widget(
        app,
        column,
        "head",
        &toml::toml! { kind = "label" text = "head" }.into(),
    );
    let mut params = toml::toml! { kind = "column" widget_layout = { gap = [0.0, 0.0] } widget_box = { overflow = "scroll" } };
    params.insert("widget_scroll".into(), scroll);
    let held = add_child_widget(app, column, "held", &params.into());
    let mut sixth = held;
    for n in 0..lines {
        let text = format!("line {n}");
        let line = add_child_widget(
            app,
            held,
            "line",
            &toml::toml! { kind = "button" text = text height = 40.0 }.into(),
        );
        if n <= 5 {
            sixth = line;
        }
    }
    let after = add_child_widget(
        app,
        column,
        "after",
        &toml::toml! { kind = "label" text = "after" }.into(),
    );
    (held, sixth, after)
}

#[test]
fn a_box_that_scrolls_is_as_tall_as_what_it_holds_where_it_has_the_room() {
    let (_dir, app) = app();
    let (held, _sixth, after) = boxed(&app, 4, toml::toml! { axis = "vertical" }.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let box_ = balaur_ui::widget_rect(held).unwrap();
    assert!(
        (box_.height() - 160.0).abs() < 1.0,
        "four lines of 40 made a box {} tall",
        box_.height()
    );
    let below = balaur_ui::widget_rect(after).unwrap().min.y;
    assert!(
        (below - box_.max.y).abs() < 1.0,
        "what follows stands at {below}, the box ends at {}",
        box_.max.y
    );
}

#[test]
fn a_box_that_scrolls_gives_way_where_its_container_leaves_it_less() {
    let (_dir, app) = app();
    let (held, sixth, after) = boxed(&app, 30, toml::toml! { axis = "vertical" }.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let box_ = balaur_ui::widget_rect(held).unwrap();
    let last = balaur_ui::widget_rect(after).unwrap();
    assert!(
        last.max.y <= 300.5,
        "what follows the box ran past the column: {}",
        last.max.y
    );
    assert!(
        box_.height() > 200.0 && box_.height() < 300.0,
        "thirty lines left the box {} tall in a column of 300",
        box_.height()
    );
    // What it holds keeps its size and runs on past the box.
    let line = balaur_ui::widget_rect(sixth).unwrap();
    assert!(
        (line.height() - 40.0).abs() < 1.0,
        "a line of 40 was drawn {} tall",
        line.height()
    );
}

#[test]
fn what_a_scrolled_box_holds_moves_with_it_under_a_finger() {
    let (_dir, app) = app();
    as_touchscreen(&app);
    let (_held, sixth, _after) = boxed(&app, 30, toml::toml! { axis = "vertical" }.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let moved = drag_up(&app, &ctx, sixth, touch);
    assert!(
        moved > 40.0,
        "a finger's drag did not scroll the box: {moved}"
    );
}

#[test]
fn a_box_that_scrolls_goes_where_its_offset_sends_it_and_takes_all_it_holds() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 200.0 widget_layout = { gap = [0.0, 0.0] } }.into(),
    );
    let held = add_child_widget(
        &app,
        column,
        "held",
        &toml::toml! { kind = "column" widget_layout = { gap = [0.0, 0.0] } widget_box = { overflow = "scroll" } widget_scroll = { axis = "vertical", animated = false } }.into(),
    );
    let mut deep = held;
    for n in 0..20 {
        let row = add_child_widget(&app, held, "row", &toml::toml! { kind = "row" }.into());
        let text = format!("line {n}");
        let line = add_child_widget(
            &app,
            row,
            "line",
            &toml::toml! { kind = "button" text = text height = 40.0 }.into(),
        );
        if n == 3 {
            deep = line;
        }
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let before = balaur_ui::widget_rect(deep).unwrap().min.y;
    balaur::components::add(
        &app.engine,
        held,
        "widget_scroll",
        Some(
            &toml::toml! { axis = "vertical" animated = false scroll_offset = [-1.0, 80.0] }.into(),
        ),
    )
    .unwrap();
    settle(&app, &ctx);
    let after = balaur_ui::widget_rect(deep).unwrap().min.y;
    assert!(
        (before - after - 80.0).abs() < 1.0,
        "a button two boxes down moved {} for an offset of 80",
        before - after
    );
}

#[test]
fn a_grid_of_stated_tracks_is_as_large_as_them_inside_a_scroll() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 300.0 height = 400.0 widget_layout = { gap = [0.0, 0.0], align_items = "start" } }.into(),
    );
    let slide = add_child_widget(
        &app,
        column,
        "slide",
        &toml::toml! { kind = "scroll" max_height = 300.0 widget_scroll = { axis = "vertical" } }
            .into(),
    );
    let grid = add_child_widget(
        &app,
        slide,
        "grid",
        &toml::toml! { kind = "grid" widget_grid = { grid_columns = "repeat(3, 20)", grid_rows = "repeat(4, 20)" } widget_layout = { gap = [0.0, 0.0] } }.into(),
    );
    for _ in 0..12 {
        add_child_widget(&app, grid, "square", &toml::toml! { kind = "stack" }.into());
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let box_ = balaur_ui::widget_rect(slide).unwrap();
    assert!(
        (box_.height() - 80.0).abs() < 1.0,
        "four rows of 20 made a scroll {} tall",
        box_.height()
    );
}

#[test]
fn a_row_that_scrolls_keeps_what_it_holds_as_wide_as_it_is() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 100.0 widget_layout = { gap = [0.0, 0.0] } }.into(),
    );
    let held = add_child_widget(
        &app,
        column,
        "held",
        &toml::toml! { kind = "row" widget_layout = { gap = [0.0, 0.0] } widget_box = { overflow = "scroll" } widget_scroll = { axis = "horizontal", animated = false, scroll_offset = [120.0, -1.0] } }.into(),
    );
    let mut cards = Vec::new();
    for n in 0..8 {
        let text = format!("card {n}");
        cards.push(add_child_widget(
            &app,
            held,
            "card",
            &toml::toml! { kind = "button" text = text width = 80.0 height = 40.0 }.into(),
        ));
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let box_ = balaur_ui::widget_rect(held).unwrap();
    assert!(
        box_.width() <= 200.5,
        "the row ran past its column: {}",
        box_.width()
    );
    let first = balaur_ui::widget_rect(cards[0]).unwrap();
    assert!(
        (first.width() - 80.0).abs() < 1.0,
        "a card of 80 was drawn {} wide",
        first.width()
    );
    assert!(
        (first.min.x + 120.0).abs() < 1.0,
        "sent 120 across, the first card stands at {}",
        first.min.x
    );
}

#[test]
fn a_panel_that_scrolls_moves_what_it_holds_under_its_caption() {
    let (_dir, app) = app();
    let panel = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "Log" x = 0.0 y = 0.0 width = 200.0 height = 160.0 widget_layout = { gap = [0.0, 0.0] } widget_box = { overflow = "scroll" } widget_scroll = { axis = "vertical", animated = false } }.into(),
    );
    let mut third = panel;
    for n in 0..12 {
        let text = format!("line {n}");
        let line = add_child_widget(
            &app,
            panel,
            "line",
            &toml::toml! { kind = "button" text = text height = 40.0 }.into(),
        );
        if n == 2 {
            third = line;
        }
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let before = balaur_ui::widget_rect(third).unwrap().min.y;
    balaur::components::add(
        &app.engine,
        panel,
        "widget_scroll",
        Some(
            &toml::toml! { axis = "vertical" animated = false scroll_offset = [-1.0, 60.0] }.into(),
        ),
    )
    .unwrap();
    settle(&app, &ctx);
    let after = balaur_ui::widget_rect(third).unwrap().min.y;
    assert!(
        (before - after - 60.0).abs() < 1.0,
        "a panel's line moved {} for an offset of 60",
        before - after
    );
    assert!(
        (balaur_ui::widget_rect(panel).unwrap().height() - 160.0).abs() < 1.0,
        "the panel is no longer the 160 it states"
    );
}
