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
