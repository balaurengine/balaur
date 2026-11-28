//! What the pointer meets over a widget: the shape its `cursor` word asks for.

use egui::pos2;

use crate::support::{
    add_widget, app, consume_input, focused, pass, press, press_with, settle, wheel,
};

// A widget's `cursor` word reaches egui's platform output while the pointer
// is over it, and a word past the first seventeen reaches it too.
#[test]
fn a_widget_s_cursor_word_shapes_the_pointer_over_it() {
    let (_dir, app) = app();
    add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "" x = 0.0 y = 0.0 width = 100.0 height = 100.0 cursor = "resize_row" }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let inside = pos2(50.0, 50.0);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(inside)]);
    let over = pass(&app, &ctx, vec![egui::Event::PointerMoved(inside)]);
    assert_eq!(
        over.platform_output.cursor_icon,
        egui::CursorIcon::ResizeRow
    );
    let away = pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(pos2(300.0, 300.0))],
    );
    assert_eq!(away.platform_output.cursor_icon, egui::CursorIcon::Default);
}

/// What the widget's node heard under `event` on the last tick.
fn heard(
    app: &balaur::App,
    entity: balaur_core::hecs::Entity,
    event: &str,
) -> Vec<balaur_script::Value> {
    balaur_core::events::delivered_from(&app.engine, entity, event)
}

// Any widget hears the pointer arrive, press and release a button over it
// by name, and leave, as a world node does.
#[test]
fn a_widget_hears_the_pointer_arrive_press_and_leave() {
    let (_dir, mut app) = app();
    let button = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Go" x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let over = balaur_ui::widget_rect(button)
        .expect("the button drew")
        .center();
    pass(&app, &ctx, vec![egui::Event::PointerMoved(over)]);
    consume_input(&mut app);
    assert_eq!(heard(&app, button, "pointer_enter").len(), 1, "no enter");
    pass(
        &app,
        &ctx,
        press_with(over, egui::PointerButton::Secondary, true),
    );
    consume_input(&mut app);
    assert_eq!(
        heard(&app, button, "pointer_down"),
        vec![balaur_script::Value::Str("right".into())]
    );
    pass(
        &app,
        &ctx,
        press_with(over, egui::PointerButton::Secondary, false),
    );
    consume_input(&mut app);
    assert_eq!(
        heard(&app, button, "pointer_up"),
        vec![balaur_script::Value::Str("right".into())]
    );
    pass(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(
            over.x + 400.0,
            over.y + 400.0,
        ))],
    );
    consume_input(&mut app);
    assert_eq!(heard(&app, button, "pointer_exit").len(), 1, "no exit");
}

// A click into a field is focus arriving, and a click away from it a blur.
#[test]
fn a_field_hears_focus_arrive_and_leave() {
    let (_dir, mut app) = app();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "" width = 200.0 x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let inside = balaur_ui::widget_rect(field)
        .expect("the field drew")
        .center();
    pass(&app, &ctx, press(inside, true));
    pass(&app, &ctx, press(inside, false));
    consume_input(&mut app);
    assert_eq!(
        focused(&app),
        Some(field),
        "the click did not focus the field"
    );
    assert_eq!(
        heard(&app, field, "focus_entered").len(),
        1,
        "no focus_entered event"
    );
    let away = egui::pos2(inside.x + 400.0, inside.y + 400.0);
    pass(&app, &ctx, press(away, true));
    pass(&app, &ctx, press(away, false));
    consume_input(&mut app);
    assert_eq!(
        heard(&app, field, "focus_exited").len(),
        1,
        "no focus_exited event"
    );
    assert_eq!(focused(&app), None, "focus stayed on the field");
}

/// A `view` that hears its `view_pointer`, `width` by 100 at `(x, y)` under
/// `holder`, or a root where it names none.
fn heard_view(
    app: &balaur_core::App,
    holder: Option<balaur_core::hecs::Entity>,
    (x, y, width): (f32, f32, f32),
) -> balaur_core::hecs::Entity {
    let view = toml::toml! {
        kind = "view" x = x y = y width = width height = 100.0
        bindings = { rows = [{ event = "emitted:view_pointer", action = "call", target = "..", value = "on_go" }] }
    };
    match holder {
        Some(holder) => crate::support::add_child_widget(app, holder, "View", &view.into()),
        None => add_widget(app, &view.into()),
    }
}

fn pointed(app: &balaur_core::App, view: balaur_core::hecs::Entity) -> Vec<toml::Value> {
    balaur_core::events::delivered_from(&app.engine, view, balaur_ui::VIEW_POINTER_EVENT)
        .iter()
        .map(|said| balaur_core::node_api::to_toml(said).expect("a value"))
        .collect()
}

// A view says the pointer moving over it, the wheel with its unit and the
// keys held as it turned, and a pinch with its factor, each where it was.
#[test]
fn a_view_says_the_pointer_over_it_its_wheel_and_a_pinch() {
    let (_dir, mut app) = app();
    let view = heard_view(&app, None, (0.0, 0.0, 200.0));
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let here = pos2(30.0, 60.0);
    pass(&app, &ctx, vec![egui::Event::PointerMoved(here)]);
    pass(
        &app,
        &ctx,
        wheel(
            here,
            egui::MouseWheelUnit::Line,
            -3.0,
            egui::Modifiers::CTRL,
        ),
    );
    pass(&app, &ctx, vec![egui::Event::Zoom(1.25)]);
    consume_input(&mut app);
    let said = pointed(&app, view);
    let of = |what: &str| {
        said.iter()
            .find(|one| one.get("what").and_then(toml::Value::as_str) == Some(what))
            .unwrap_or_else(|| panic!("no {what} in {said:?}"))
            .clone()
    };
    let pair = |x: f64, y: f64| toml::Value::Array(vec![x.into(), y.into()]);
    let keys =
        |shift: bool, ctrl: bool| toml::toml! { shift = shift ctrl = ctrl alt = false cmd = false };
    let with = |what: &str, rest: toml::Table| {
        let mut want = toml::toml! { what = what at = (pair(30.0, 60.0)) };
        want.extend(rest);
        toml::Value::Table(want)
    };
    assert_eq!(of("move"), with("move", keys(false, false)));
    let mut turned = keys(false, true);
    turned.extend(toml::toml! { turn = (pair(0.0, -3.0)) unit = "line" });
    assert_eq!(of("wheel"), with("wheel", turned));
    let mut pinched = keys(false, false);
    pinched.extend(toml::toml! { scale = 1.25 });
    assert_eq!(of("pinch"), with("pinch", pinched));
}

// A view whose wheel is heard takes it from the scroll it sits in: the page
// scrolls by a turn beside the view and stays for one over it.
#[test]
fn a_view_that_hears_its_wheel_takes_it_from_the_page() {
    let (_dir, mut app) = app();
    let page = add_widget(
        &app,
        &toml::toml! { kind = "scroll" x = 0.0 y = 0.0 width = 300.0 height = 200.0 padding = 0.0 widget_layout = { gap = [0.0, 0.0] } }
            .into(),
    );
    heard_view(&app, Some(page), (0.0, 0.0, 150.0));
    let tall = crate::support::add_child_widget(
        &app,
        page,
        "Tall",
        &toml::toml! { kind = "panel" height = 900.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let mut scrolled_at = |y: f32| {
        let was = balaur_ui::widget_rect(tall).expect("it drew").min.y;
        pass(
            &app,
            &ctx,
            wheel(
                pos2(60.0, y),
                egui::MouseWheelUnit::Point,
                -90.0,
                egui::Modifiers::NONE,
            ),
        );
        for _ in 0..10 {
            pass(&app, &ctx, vec![]);
        }
        consume_input(&mut app);
        was - balaur_ui::widget_rect(tall).expect("it drew").min.y
    };
    assert!(
        scrolled_at(50.0).abs() < 0.5,
        "the page scrolled under the view"
    );
    assert!(
        scrolled_at(150.0) > 1.0,
        "the page scrolls by a turn off the view"
    );
}

// A point on the screen is found in a view's box with the scale and the turn
// it and what holds it are drawn with undone.
#[test]
fn a_point_drawn_moved_is_found_in_the_box_it_was_moved_from() {
    let (_dir, app) = app();
    let holder = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 100.0 padding = 0.0 scale = [2.0, 2.0] offset = [10.0, 0.0] }
            .into(),
    );
    let view = heard_view(&app, Some(holder), (0.0, 0.0, 200.0));
    let turned = add_widget(
        &app,
        &toml::toml! { kind = "view" x = 300.0 y = 0.0 width = 100.0 height = 100.0 rotation = 90.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    let near = |got: Option<[f32; 2]>, want: [f32; 2]| {
        got.is_some_and(|[x, y]| (x - want[0]).abs() < 0.01 && (y - want[1]).abs() < 0.01)
    };
    // Twice its size about (100, 50), 10 to the right: (120, 60) is drawn at
    // (150, 70).
    let found = balaur_ui::view_point_in(view, [150.0, 70.0]);
    assert!(near(found, [120.0, 60.0]), "{found:?}");
    // Turned a quarter about (350, 50): what was right of the middle is under it.
    let found = balaur_ui::view_point_in(turned, [350.0, 70.0]);
    assert!(near(found, [370.0, 50.0]), "{found:?}");
}

// The pointer reaches a moved view where it is drawn, not the box it was
// moved from, and a view the pointer passes through says nothing.
#[test]
fn a_view_drawn_moved_hears_the_pointer_where_it_is_drawn() {
    let (_dir, mut app) = app();
    let rows = toml::toml! { rows = [{ event = "emitted:view_pointer", action = "call", target = "..", value = "on_go" }] };
    let moved = add_widget(
        &app,
        &toml::toml! { kind = "view" x = 0.0 y = 0.0 width = 100.0 height = 100.0 offset = [100.0, 0.0] bindings = (rows.clone()) }
            .into(),
    );
    let through = add_widget(
        &app,
        &toml::toml! { kind = "view" x = 0.0 y = 200.0 width = 100.0 height = 100.0 interactive = false bindings = (rows) }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![]);
    let turn = |at| wheel(at, egui::MouseWheelUnit::Line, -1.0, egui::Modifiers::NONE);
    pass(&app, &ctx, turn(pos2(50.0, 50.0)));
    consume_input(&mut app);
    assert!(
        pointed(&app, moved).is_empty(),
        "heard where nothing of it is drawn"
    );
    pass(&app, &ctx, turn(pos2(150.0, 50.0)));
    pass(&app, &ctx, turn(pos2(50.0, 250.0)));
    consume_input(&mut app);
    assert!(
        !pointed(&app, moved).is_empty(),
        "not heard where it is drawn"
    );
    assert!(
        pointed(&app, through).is_empty(),
        "a view the pointer passes through said it"
    );
}
